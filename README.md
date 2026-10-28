# tape-base64

[![Crates.io](https://img.shields.io/crates/v/tape-base64.svg)](https://crates.io/crates/tape-base64)
[![Documentation](https://docs.rs/tape-base64/badge.svg)](https://docs.rs/tape-base64)
[![License](https://img.shields.io/crates/l/tape-base64.svg)](LICENSE)

A fast base64 implementation for Rust. It's `no_std`, never allocates, and has
no dependencies.

## Why it's fast

The fastest way to do base64 is different on each instruction set, so we wrote
a separate kernel for each one instead of translating a single design across
them. We did try porting: the AVX-512 decoder's lookup table, moved to NEON,
came out 4 to 13% slower, because the instruction it leans on is one uop on x86
and four on ARM.

So each kernel is built around what its hardware is good at:

- **ARM (NEON)** can load 48 bytes and split them into the first, second and
  third byte of every group in one instruction (`ld3`), and store four
  registers back interleaved (`st4`). That rearranging is the awkward part of
  base64, and here the loads and stores do it. One table lookup covers the
  whole alphabet.
- **AVX2** has neither, and its shuffles only look up 16 entries at a time. So
  it moves each 3-byte group into its own 32-bit lane, splits out the fields
  with multiplies, and maps them to characters through a 16-entry offset table.
  The multiplies are written in inline asm, because LLVM was quietly turning
  them into shifts and blends and losing a third of the encode speed.
- **AVX-512 VBMI** can look up all 64 characters at once (`vpermb`) and pull
  every field out of a lane in one instruction (`vpmultishiftqb`), so that
  kernel is short. Masked loads and stores handle the leftover bytes without
  a separate tail loop.
- **wasm simd128** looks like AVX2 with 16-byte registers, so it reuses that
  approach. Everything else gets a portable version.

On x86, the crate asks the processor which of these it supports the first time
you call it. Very large encodes on AVX2 skip the cache, since writing that much
output through it only evicts everything else. The cutoff is scaled from the
cache size the processor reports, because a fixed number tuned on one machine
is wrong on the next.

On an Apple M4 that comes to 1.6 to 1.9x the next fastest crate. On x86 it's
level with `base64-turbo`. [BENCHMARKS.md](BENCHMARKS.md) has the full
numbers, the machines, and how to reproduce them.

## Using it

```toml
[dependencies]
tape-base64 = "0.1"
```

```rust
STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD   // the four RFC 4648 codecs

codec.encode(&[u8], &mut [u8]) -> Result<usize, EncodeError>
codec.decode(&[u8], &mut [u8]) -> Result<usize, DecodeError>
codec.encoded_len(usize) -> Option<usize>
codec.decoded_len(&[u8]) -> Result<usize, DecodeError>
codec.decoded_len_max(usize) -> usize

codec.encoder() -> Encoder         // push(&[u8], &mut [u8]), finish(&mut [u8])
codec.decoder() -> Decoder         // push(&[u8], &mut [u8]), finish(&mut [u8])
```

You give it the output buffer and it tells you how many bytes it wrote, so
nothing allocates and the same code works in `no_std` and in a hot loop. If
your input arrives in pieces, `encoder()` and `decoder()` handle the
boundaries and produce exactly what one call on the whole input would, errors
and error offsets included.

Decoding is strict. Whitespace, bad padding and nonzero trailing bits are all
errors, so each byte string has only one valid encoding per codec.

Nothing is ever written past the length a call returns. CI runs the kernels
under Miri and valgrind as well as the tests, because an out-of-bounds read
usually still gives the right answer. Valgrind caught two that the tests
missed.

## Credits

The decoder's character check, which looks at each character's high and low
nibble, comes from Wojciech Muła and Daniel Lemire's work on vectorised base64.
So does the multiply trick in the AVX2 encoder. The NEON structure follows
Alfred Klomp's [aklomp/base64](https://github.com/aklomp/base64). We didn't
copy code from either. The block sizes, tail handling, bounds and errors are
our own.
