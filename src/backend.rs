//! The vector stage, or nothing where the architecture has none

use crate::alphabet::Alphabet;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
use crate::neon;

#[cfg(target_arch = "x86_64")]
use crate::x86;

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
use crate::wasm;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
pub(crate) fn encode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    neon::encode_bulk::<A>(input, out, groups)
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
pub(crate) fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    neon::decode_bulk::<A>(input, out, quads)
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn encode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    x86::encode_bulk::<A>(input, out, groups)
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    x86::decode_bulk::<A>(input, out, quads)
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub(crate) fn encode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    wasm::encode_bulk::<A>(input, out, groups)
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub(crate) fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    wasm::decode_bulk::<A>(input, out, quads)
}

#[cfg(not(any(
    all(target_arch = "aarch64", target_feature = "neon"),
    all(target_arch = "wasm32", target_feature = "simd128"),
    target_arch = "x86_64"
)))]
pub(crate) fn encode_bulk<A: Alphabet>(_input: &[u8], _out: &mut [u8], _groups: usize) -> usize {
    0
}

#[cfg(not(any(
    all(target_arch = "aarch64", target_feature = "neon"),
    all(target_arch = "wasm32", target_feature = "simd128"),
    target_arch = "x86_64"
)))]
pub(crate) fn decode_bulk<A: Alphabet>(_input: &[u8], _out: &mut [u8], _quads: usize) -> usize {
    0
}
