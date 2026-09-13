# tape-base64

Fast base64. One kernel per architecture, written for the shape of that
architecture's registers rather than ported from another's, and a portable
walk everywhere else. `no_std`, no allocation, no dependencies.

## API

```rust
// The four RFC 4648 combinations, as constants.
STANDARD           // +/ with padding
STANDARD_NO_PAD    // +/ without
URL_SAFE           // -_ with padding
URL_SAFE_NO_PAD    // -_ without

codec.encode(&[u8], &mut [u8]) -> Result<usize, EncodeError>
codec.decode(&[u8], &mut [u8]) -> Result<usize, DecodeError>
codec.encoded_len(usize) -> Option<usize>
codec.decoded_len(&[u8]) -> Result<usize, DecodeError>
codec.decoded_len_max(usize) -> usize

// The padded standard alphabet, which is what most callers want.
encode(&[u8], &mut [u8]) -> Result<usize, EncodeError>
decode(&[u8], &mut [u8]) -> Result<usize, DecodeError>
encoded_len(usize) -> Option<usize>

// The same conversions taken a piece at a time.
codec.encoder() -> Encoder
codec.decoder() -> Decoder
encoder.push(&[u8], &mut [u8]) -> Result<usize, EncodeError>
encoder.finish(&mut [u8]) -> Result<usize, EncodeError>
decoder.push(&[u8], &mut [u8]) -> Result<usize, DecodeError>
decoder.finish(&mut [u8]) -> Result<usize, DecodeError>
```

Every conversion writes into a buffer the caller owns and returns how much of
it was written. A buffer longer than the conversion needs keeps whatever was
past the end.

## Pieces

A long input can go through as it arrives, without being joined first. Each
piece gets its own call into whatever room the caller hands over, and the
final call completes what a piece boundary left held: the bytes short of a
group on the encode side, the padded ending on the decode side.

```rust
let mut encoder = STANDARD.encoder();
let written = encoder.push(chunk, &mut out)?;
let written = encoder.finish(&mut out[written..])?;

let mut decoder = STANDARD.decoder();
let written = decoder.push(chunk, &mut out)?;
let written = decoder.finish(&mut out[written..])?;
```

The pieces convert to exactly what the pieces joined convert to, refusals
included: a streamed decode refuses where, and only where, one whole-input
call would, pointing at the same character at the same offset. What was converted
before a refusal stays converted. An encode push that finds the buffer short
of the whole piece refuses instead of converting part of it, so a refusal
consumes nothing.

## How it is fast

A base64 group is 3 bytes, and 3 does not divide any register width. Every
SIMD base64 in the wild answers this the x86 way: read 16 bytes, use 12,
shuffle them into place, and pull the six-bit fields out with multiplies.
aarch64 does not need that, because its loads deinterleave.

`ld3` reads 48 bytes as three registers holding the first, second and third
byte of every group, which is already the arrangement the six-bit fields want.
Four shifts and inserts split them. One `tbl` against all 64 characters spells
them, because aarch64 can index four registers in a single lookup, so the
compare-and-offset chain a 16-byte lookup forces is not needed. `st4` writes
them back interleaved. 48 bytes a block, against 12 for a ported kernel, and
nothing is shuffled into place.

Decoding runs the same way through `ld4` and `st3`, classifying characters by
their two nibbles: one lookup says which rows of the ASCII table a low nibble
is absent from, another says which row a character sits in, and a character
is in the alphabet exactly when those two answers share no bit.

A block is a whole number of groups, so a block that would run past the end is
moved back to end where the groups end rather than padded or staged: it
converts a few groups twice and writes the same characters over the ones
already there, which costs one block and leaves the portable walk nothing but
the one or two bytes that do not fill a group. Each aarch64 kernel also reads
and writes exactly the range it converts, so none of them needs readable slack
past the end of a buffer. The 4 bytes of slack the ported kernels want is the
reason they leave a scalar tail behind.

On x86 one kernel does read outside: gathering 3 bytes into every 32-bit lane
wants the 12 bytes each half of a register will use at the same offset within
it, and one load 4 bytes before the block puts them there where two
overlapping loads and an insert cost two more instructions. That form is used
only for blocks with 4 readable bytes on each side, counted over the whole
unrolled run rather than its first block, and the blocks at either end take
the two-load form instead.

The x86 decoder does the same on the writing side. A block with another block
behind it lands its 24 bytes as two 16-byte stores that overlap, which is three
fewer shuffles than cutting them apart, and the block behind overwrites the 4
bytes it ran over. Only a block that has one behind it takes that form, so
nothing is written past what the call returns. Every other kernel, in both
directions, touches exactly what it converts.

Above the size where the input and its larger output stop fitting together in
the last level cache, the x86 encoder writes past the cache rather than through
it, because keeping the output there only evicts what the next block is about
to read. That size is read from cpuid rather than fixed, since the threshold is
a fraction of a particular machine's cache and not a constant.

## Paths

| Architecture | Path |
|---|---|
| aarch64 | `ld3`/`st4` blocks of 48 bytes, alphabet through one four-register lookup |
| x86-64 with AVX2 | 3 bytes gathered into each lane, fields split by a pair of multiplies |
| x86-64 with AVX-512VBMI | `vpermb` for the alphabet and `vpmultishiftqb` for the fields, masked at both ends |
| anything else | portable, two groups per wide load |

Which x86 path runs is settled by asking the processor, not by what the crate
was built for, so a build carrying no target flags still reaches the wide one.

AVX-512 with VBMI is where base64 stops being a bit-twiddling problem.
`vpermb` indexes all 64 characters in one instruction, the way aarch64's
four-register `tbl` does, and `vpmultishiftqb` pulls all 8 six-bit fields out
of a 64-bit lane at once: a block is a masked load, three instructions and a
masked store. The masks also mean the last
partial block is handled like any other, so that path has no scalar tail.

The AVX2 path has been run: on an AMD EPYC Milan and on an Intel Xeon
Skylake-SP, where every length and both alphabets produce the same bytes as
the portable walk and the same refusals byte for byte.

The AVX-512VBMI path has been run too, on an AMD EPYC Turin, where
`tests/forced.rs` reports `portable, avx2, avx512vbmi` and every length and
both alphabets match the portable walk with valgrind clean. It is on by
default, and `--no-default-features` stops dispatch at AVX2.

The tables it indexes by are also checked on any machine, VBMI or not, by a
scalar model of `vpermb`, `vpmultishiftqb` and `vpermi2b` held against the
portable codec. That model checks the tables and not the calls, so the kernel
itself is run under Miri, where the argument order is checked with it.

What has been checked on hardware is that the guard works: on Skylake-SP,
which has AVX-512F, BW and VL but not VBMI, dispatch declines the wide path
and runs AVX2, rather than reaching `vpermb` and faulting.

## Verification

Every kernel is held against the portable walk at every length, in both
alphabets, and every refusal has to name the same character at the same
offset. `tests/forced.rs` does that on each path the running processor can
execute, pinned in turn, which needs the `internals` feature:

```sh
cargo test --features internals
```

The x86 kernels also run on a machine that has no x86 in it, under Miri and a
foreign target, because the instructions the interpreter has no version of are
modelled beside them: `vpermb`, `vpmultishiftqb`, the streaming store and its
fence, and the `vpmullw` the AVX2 encoder writes as assembly. What runs is the
kernel, addressing and argument order included.

```sh
RUSTFLAGS="-C target-feature=+avx512f,+avx512bw,+avx512vbmi" \
  cargo +nightly miri test --target x86_64-unknown-linux-gnu \
  --features internals --test forced
```

`cpuid` is not interpreted either, so under Miri the target features the build
was given stand in for what a processor would report: the command above pins
and runs the portable walk, AVX2 and AVX-512VBMI in turn, and without those
`RUSTFLAGS` it runs the portable walk alone. The sweeps thin themselves under
Miri, since a block is interpreted rather than executed, and the size below
which the encoder will not stream drops from 64 KiB to 1 KiB so that an
interpreted run reaches the streaming store at all.

The aarch64 kernel cannot be run this way. Miri has no `vqtbl4q_u8`, and
modelling the whole of `ld3`, `st4` and the structured loads beside it would
be a second kernel to keep right.

Reads past the end of a buffer are invisible to `cargo test`, since an
over-read still produces the right answer, and valgrind is what finds them:

```sh
cargo test --release --features internals --test forced --no-run
valgrind --error-exitcode=9 --quiet target/release/deps/forced-*
```

## Block sizes

A block is a whole number of groups, and there are three of them: 16 groups
through a deinterleaving load, 8 through a half-width one, and 4 through a
single register with no deinterleaving at all. Which one covers a remainder is
not simply the smallest that fits. A deinterleaving load costs about the same
at either width, so the 8-group block is worth more than the 4-group one
wherever there are 8 groups to land it on.

## Features

| | |
|---|---|
| `std` | `impl std::error::Error` for the two error types. Nothing else changes, the codec is `no_std` either way. Off by default |
| `avx512` | lets dispatch choose the AVX-512VBMI path on a processor reporting AVX-512F, BW and VBMI, which is what the kernel is written in. On by default |

## Refusals

Decoding is strict, and says where it went wrong:

* a byte outside the alphabet, with its offset
* a character count no encoder produces
* padding that is absent, too long, or not at the end
* a final character carrying bits the output has no room for, which would
  otherwise let two spellings decode alike

The vector stage never reports these. It stops before any block holding a
character outside the alphabet and lets the portable walk find the offset,
so the cost of locating an error is paid only when there is one.

## Benchmarks

See [BENCHMARKS.md](BENCHMARKS.md), which holds the numbers, the machine,
and how to reproduce them.

## Attribution

The nibble classification the decoder uses is Wojciech Muła's, from his and
Daniel Lemire's work on vectorised base64. The aarch64 block structure,
`ld3` in, `st4` out and alphabet through `tbl`, follows Alfred Klomp's
`aklomp/base64`. The x86 field split is the same authors', in the variant
that reaches its two right shifts through a low multiply and one shared
shift rather than a high multiply.

All of these are widely reimplemented. This is an independent
implementation of the same ideas: the block sizes and how a remainder is
covered, the overlapping tail, the exact bounds, and the error handling were
worked out here and are measured in [BENCHMARKS.md](BENCHMARKS.md).
