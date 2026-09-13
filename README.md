# tape-base64

Fast base64. One kernel per architecture, written for the shape of that
architecture's registers rather than ported from another's, and a portable
walk everywhere else. `no_std`, no allocation, no dependencies.

## API

```rust
STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD   // the four RFC 4648 codecs

codec.encode(&[u8], &mut [u8]) -> Result<usize, EncodeError>
codec.decode(&[u8], &mut [u8]) -> Result<usize, DecodeError>
codec.encoded_len(usize) -> Option<usize>
codec.decoded_len(&[u8]) -> Result<usize, DecodeError>
codec.decoded_len_max(usize) -> usize

encode, decode, encoded_len        // the same on the padded standard alphabet

codec.encoder() -> Encoder         // push(&[u8], &mut [u8]), finish(&mut [u8])
codec.decoder() -> Decoder         // push(&[u8], &mut [u8]), finish(&mut [u8])
```

Every call writes into a buffer the caller owns and returns how much it wrote.
The encoder and decoder take input a piece at a time and produce exactly what
one call over the joined input would, refusals included: the same error at the
same offset. What was converted before a refusal stays converted, and an encode
push short of room refuses the whole piece rather than part of it.

## How it is fast

A group is 3 bytes, which no register width divides. On aarch64, `ld3` reads
48 bytes as three registers holding the first, second and third byte of every
group, four shifts and inserts split the six-bit fields, one `tbl` over all 64
characters spells them, and `st4` writes them back interleaved. Decoding runs
the other way through `ld4` and `st3`, classifying each character by its two
nibbles. 48 bytes a block, and nothing shuffled into place.

On x86, AVX2 gathers 3 bytes into each 32-bit lane and splits the fields with
a pair of multiplies. AVX-512 with VBMI does it in three instructions: `vpermb`
indexes all 64 characters at once and `vpmultishiftqb` pulls every six-bit
field out of a lane, with masked loads and stores covering the last partial
block. Which x86 path runs is asked of the processor at first use, not fixed
when the crate is built.

A block that would run past the end is moved back to end where the groups end,
converting a few groups twice, so the portable walk is left only the one or two
bytes short of a group. The aarch64 kernels touch exactly what they convert.
The x86 encoder reads 4 bytes before a block only where both sides have the
room, and the x86 decoder lands 24 bytes as two overlapping 16-byte stores only
where the next block overwrites the overhang, so nothing is written past what a
call returns. Above the size where input and output stop fitting in the last
level cache, read from cpuid, the x86 encoder streams its stores past the cache.

## Paths

| Architecture | Path |
|---|---|
| aarch64 | `ld3`/`st4` blocks of 48 bytes, alphabet through one four-register lookup |
| x86-64 with AVX2 | 3 bytes gathered into each lane, fields split by a pair of multiplies |
| x86-64 with AVX-512VBMI | `vpermb` for the alphabet, `vpmultishiftqb` for the fields, masked at both ends |
| anything else | portable, two groups per wide load |

The AVX2 path has run on EPYC Milan and Xeon Skylake-SP, the AVX-512VBMI path
on EPYC Turin under valgrind, and Skylake-SP, which has AVX-512 without VBMI,
shows dispatch declining the wide path rather than faulting.

## Verification

Every kernel is held against the portable walk at every length, in both
alphabets, and every refusal has to match character and offset. `tests/forced.rs`
pins each path the running processor can execute in turn:

```sh
cargo test --features internals
```

The x86 kernels also run on Apple silicon under Miri, with the instructions the
interpreter lacks modelled beside them and the target features in `RUSTFLAGS`
standing in for cpuid, so what runs is the kernel itself:

```sh
RUSTFLAGS="-C target-feature=+avx512f,+avx512bw,+avx512vbmi" \
  cargo +nightly miri test --target x86_64-unknown-linux-gnu \
  --features internals --test forced
```

Over-reads are invisible to `cargo test`, so valgrind finds them:

```sh
cargo test --release --features internals --test forced --no-run
valgrind --error-exitcode=9 --quiet target/release/deps/forced-*
```

The aarch64 kernel cannot run under Miri, which has no `vqtbl4q_u8`.

`fuzz/` holds three cargo-fuzz targets, `decode`, `roundtrip` and `streaming`,
each against the reference crate:

```sh
cd fuzz && cargo +nightly fuzz run decode
```

CI runs the test suite on x86-64 and aarch64, Miri over the x86 kernels,
valgrind on both architectures, and a minute of each fuzz target.

## Features

| feature | what it does |
|---|---|
| `std` | `std::error::Error` for the two error types. Off by default |
| `internals` | the `testing` module that pins a path, for the test suite. Off by default |

## Refusals

Decoding is strict and says where it went wrong:

* a byte outside the alphabet, with its offset
* a character count no encoder produces
* padding that is absent, too long, or not at the end
* a final character with bits the output has no room for

The vector stage never reports these. It stops before a bad block and the
portable walk finds the offset, so an error costs only when there is one.

## Benchmarks

[BENCHMARKS.md](BENCHMARKS.md) holds the numbers, the machines, and how to
reproduce them.

## Attribution

The decoder's nibble classification is Wojciech Muła's, from his and Daniel
Lemire's work on vectorised base64. The aarch64 block structure, `ld3` in,
`st4` out and the alphabet through `tbl`, follows Alfred Klomp's
`aklomp/base64`. The x86 field split is the same authors', in the variant that
reaches its shifts through a low multiply. All of these are widely
reimplemented; this is an independent implementation of the same ideas, with
the block sizes, remainder handling, exact bounds and error handling worked out
here.
