//! The x86-64 codec: AVX2 or AVX-512VBMI, chosen by asking the processor

use crate::alphabet::Alphabet;

mod avx2;
mod avx512;
mod dispatch;

pub(crate) use dispatch::Path;

#[cfg(feature = "internals")]
pub use dispatch::{available, force, force_stream_floor, AVX2, PORTABLE, UNKNOWN, WIDE};

/// Below this no cache is small enough for streaming to be the right answer,
/// so a short buffer settles the question with one compare
const STREAM_CANDIDATE: usize = 1 << 16;

/// Convert `groups` whole groups, returning how many were converted
pub(crate) fn encode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    match dispatch::path() {
        // SAFETY: cpuid reported the path, so the instructions are present.
        Path::Wide => unsafe { avx512::encode_bulk::<A>(input, out, groups) },
        Path::Avx2 => {
            // Asked here rather than inside the kernel, so the ordinary form
            // carries neither the test nor a branch that would give it a frame
            // it does not otherwise need.
            let is_streaming = input.len() >= STREAM_CANDIDATE
                && input.len() >= dispatch::stream_floor()
                && out.as_ptr().align_offset(16) == 0;
            // SAFETY: cpuid reported the path, and streaming is only chosen
            // once the output is known to be aligned for it.
            unsafe {
                if is_streaming {
                    avx2::encode_bulk::<A, true>(input, out, groups)
                } else {
                    avx2::encode_bulk::<A, false>(input, out, groups)
                }
            }
        }
        Path::Portable => 0,
    }
}

/// Convert `quads` whole groups, returning how many were converted
pub(crate) fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    match dispatch::path() {
        // SAFETY: cpuid reported the path, so the instructions are present.
        Path::Wide => unsafe { avx512::decode_bulk::<A>(input, out, quads) },
        Path::Avx2 => unsafe { avx2::decode_bulk::<A>(input, out, quads) },
        Path::Portable => 0,
    }
}
