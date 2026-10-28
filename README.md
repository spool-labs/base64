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

A group is 3 bytes, which no register width divides, so each kernel is
written for the shape of its own registers.

| | a block |
|---|---|
| aarch64 | `ld3` reads 48 bytes as the first, second and third byte of 16 groups, shifts split the fields, one `tbl` over all 64 characters spells them, `st4` writes them back interleaved |
| x86-64 AVX2 | 3 bytes gathered into each 32-bit lane, fields split by a pair of multiplies, alphabet through a 16-entry offset table |
| x86-64 AVX-512VBMI | `vpermb` indexes all 64 characters at once, `vpmultishiftqb` pulls every field out of a lane, masked loads and stores cover the last partial block |
| wasm32 simd128 | the AVX2 form on one 16-byte register, lookups through `i8x16.swizzle` |
| anything else | portable, two groups per wide load |

Decoding runs the same way backwards, classifying each character by its two
nibbles. Which x86 path runs is asked of the processor at first use. Every
kernel reads and writes exactly what it converts, except where the x86 ones
overlap a block a later block overwrites, so nothing lands past what a call
returns.

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
