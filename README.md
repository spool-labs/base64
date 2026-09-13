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
On wasm32 built with `-C target-feature=+simd128`, the same form runs on one
16-byte register through `i8x16.swizzle`.

A block that would run past the end is moved back to end where the groups end,
converting a few groups twice, so the portable walk is left only the one or two
bytes short of a group. The aarch64 kernels touch exactly what they convert.
The x86 encoder reads 4 bytes before a block only where both sides have the
room, and the x86 decoder lands 24 bytes as two overlapping 16-byte stores only
where the next block overwrites the overhang, so nothing is written past what a
call returns. Above the size where input and output stop fitting in the last
level cache, read from cpuid, the x86 encoder streams its stores past the cache.

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
