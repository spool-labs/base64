//! The AVX-512 codec, where VBMI is present
//!
//! `vpermb` spells a value in one lookup and `vpmultishiftqb` splits a group
//! in one instruction, so a block is a load, three instructions and a store.
//! Decoding indexes the whole of ASCII with `vpermi2b`. Masked loads and
//! stores cover the last partial block, so nothing is left for the walk.

use core::arch::x86_64::*;

use crate::alphabet::Alphabet;
use crate::wide::{expand, field_starts, gather_bytes};

/// Groups a block converts
const BLOCK_GROUPS: usize = 16;

/// Blocks decoded between two readings of the bad character accumulator
///
/// The run is written out rather than looped, so this cannot change alone
///
/// A constant, so a run unrolls. Reading the accumulator moves a mask register
/// into a general one and branches, which is a barrier between blocks.
const CHECK_EVERY: usize = 4;
const _: () = assert!(CHECK_EVERY == 4);

static EXPAND: [i8; 64] = expand();
static FIELD_STARTS: [i8; 64] = field_starts();
static GATHER: [i8; 64] = gather_bytes();

/// Convert `groups` whole groups, returning how many were converted
///
/// # Safety
///
/// The running machine must have AVX-512F, BW and VBMI.
#[target_feature(enable = "avx512f,avx512bw,avx512vbmi")]
pub(crate) unsafe fn encode_bulk<A: Alphabet>(
    input: &[u8],
    out: &mut [u8],
    groups: usize,
) -> usize {
    if groups == 0 || groups * 3 > input.len() || groups * 4 > out.len() {
        return 0;
    }
    // SAFETY: every block converts groups `start` through `start + n` with
    // `start + n <= groups`, reading `n * 3` bytes and writing `n * 4`, and
    // the caller has room for `groups` of each. The last block narrows both
    // its load and its store to however many groups are left.
    unsafe {
        let alphabet = _mm512_loadu_si512(A::ENCODE.as_ptr().cast());
        let expand = _mm512_loadu_si512(EXPAND.as_ptr().cast());
        let starts = _mm512_loadu_si512(FIELD_STARTS.as_ptr().cast());
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        // A whole block reads a constant 48 bytes and writes a constant 64, so
        // its load mask is lifted out of the loop and its store needs none. A
        // mask derived from a varying block size costs two shifts and two
        // moves into mask registers per iteration, and stops the unrolling.
        let whole = mask_for(BLOCK_GROUPS * 3);
        let mut done = 0;
        while groups - done >= BLOCK_GROUPS {
            let data = _mm512_maskz_loadu_epi8(whole, source.add(done * 3).cast());
            let spread = _mm512_permutexvar_epi8(expand, data);
            let fields = _mm512_multishift_epi64_epi8(starts, spread);
            // Only the low six bits of an index are read, so the two bits
            // above each field never need masking off.
            let spelled = _mm512_permutexvar_epi8(fields, alphabet);
            _mm512_storeu_si512(destination.add(done * 4).cast(), spelled);
            done += BLOCK_GROUPS;
        }
        // At most one of these, and the only place a mask is computed.
        if done < groups {
            let taking = groups - done;
            let data = _mm512_maskz_loadu_epi8(mask_for(taking * 3), source.add(done * 3).cast());
            let spread = _mm512_permutexvar_epi8(expand, data);
            let fields = _mm512_multishift_epi64_epi8(starts, spread);
            let spelled = _mm512_permutexvar_epi8(fields, alphabet);
            _mm512_mask_storeu_epi8(
                destination.add(done * 4).cast(),
                mask_for(taking * 4),
                spelled,
            );
        }
        groups
    }
}

/// A mask over the low `count` bytes of a register, for `count` up to 64
#[inline(always)]
fn mask_for(count: usize) -> __mmask64 {
    if count >= 64 {
        !0
    } else {
        (1u64 << count) - 1
    }
}

/// Convert `quads` whole groups, returning how many were converted
///
/// # Safety
///
/// The running machine must have AVX-512F, BW and VBMI.
#[target_feature(enable = "avx512f,avx512bw,avx512vbmi")]
pub(crate) unsafe fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    if quads == 0 || quads * 4 > input.len() || quads * 3 > out.len() {
        return 0;
    }
    // SAFETY: a whole block reads the 64 characters and writes the 48 bytes of
    // groups `done` through `done + 16`, which the caller has room for. The
    // wide store is only reached where at least 6 groups follow, whose own
    // store rewrites the 16 bytes it overhangs by. The last block narrows
    // both ends with masks.
    unsafe {
        let values_low = _mm512_loadu_si512(A::ASCII_LOW.as_ptr().cast());
        let values_high = _mm512_loadu_si512(A::ASCII_HIGH.as_ptr().cast());
        let gather = _mm512_loadu_si512(GATHER.as_ptr().cast());
        let whole = mask_for(BLOCK_GROUPS * 3);
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        // A character outside the alphabet sets bit 7 of either the character
        // or the value the table gave back, so gathering both is one
        // three-input OR and reading it is one test per run.
        let mut bad = _mm512_setzero_si512();
        let mut done = 0;

        /// The arithmetic of one block, on characters already loaded, leaving
        /// them and their values for a caller folding several at once
        macro_rules! convert_loaded {
            ($chars:expr) => {{
                let chars = $chars;
                let values = _mm512_permutex2var_epi8(values_low, chars, values_high);
                let paired = _mm512_maddubs_epi16(values, _mm512_set1_epi32(0x0140_0140));
                let merged = _mm512_madd_epi16(paired, _mm512_set1_epi32(0x0001_1000));
                (_mm512_permutexvar_epi8(gather, merged), chars, values)
            }};
        }

        /// One block: 64 characters to 48 bytes
        macro_rules! convert {
            ($at:expr) => {{
                let chars = _mm512_loadu_si512(source.add($at * 4).cast());
                let values = _mm512_permutex2var_epi8(values_low, chars, values_high);
                bad = _mm512_ternarylogic_epi32::<0xFE>(bad, chars, values);
                let paired = _mm512_maddubs_epi16(values, _mm512_set1_epi32(0x0140_0140));
                let merged = _mm512_madd_epi16(paired, _mm512_set1_epi32(0x0001_1000));
                _mm512_permutexvar_epi8(gather, merged)
            }};
        }

        // 48 bytes is an awkward width to store and 64 is not. Where 6 or more
        // groups follow, that is 18 bytes against the 16 this overhangs by,
        // the next store rewrites the overhang and the width can be ignored.
        while quads - done >= CHECK_EVERY * BLOCK_GROUPS + 6 {
            // Every load of the run issued before any of its arithmetic, so
            // four chains are in flight rather than each waiting on the one
            // before it. This is what the AVX2 encoder does for the same
            // reason.
            let mut raw = [_mm512_setzero_si512(); CHECK_EVERY];
            for (step, slot) in raw.iter_mut().enumerate() {
                *slot = _mm512_loadu_si512(source.add((done + step * BLOCK_GROUPS) * 4).cast());
            }
            let (first, first_chars, first_values) = convert_loaded!(raw[0]);
            let (second, second_chars, second_values) = convert_loaded!(raw[1]);
            let (third, third_chars, third_values) = convert_loaded!(raw[2]);
            let (fourth, fourth_chars, fourth_values) = convert_loaded!(raw[3]);
            _mm512_storeu_si512(destination.add(done * 3).cast(), first);
            _mm512_storeu_si512(destination.add((done + BLOCK_GROUPS) * 3).cast(), second);
            _mm512_storeu_si512(destination.add((done + 2 * BLOCK_GROUPS) * 3).cast(), third);
            _mm512_storeu_si512(
                destination.add((done + 3 * BLOCK_GROUPS) * 3).cast(),
                fourth,
            );

            // A tree rather than a chain: none of these waits on the one
            // before it, and only the last waits on `bad`.
            let left = _mm512_ternarylogic_epi32::<0xFE>(first_chars, first_values, second_chars);
            let right = _mm512_ternarylogic_epi32::<0xFE>(second_values, third_chars, third_values);
            let rest = _mm512_ternarylogic_epi32::<0xFE>(fourth_chars, fourth_values, left);
            bad = _mm512_ternarylogic_epi32::<0xFE>(bad, right, rest);
            if _mm512_movepi8_mask(bad) != 0 {
                return done;
            }
            done += CHECK_EVERY * BLOCK_GROUPS;
        }
        // Read per block outside the run, so a bad character costs one block.
        while quads - done >= BLOCK_GROUPS + 6 {
            let packed = convert!(done);
            _mm512_storeu_si512(destination.add(done * 3).cast(), packed);
            if _mm512_movepi8_mask(bad) != 0 {
                return done;
            }
            done += BLOCK_GROUPS;
        }
        while quads - done >= BLOCK_GROUPS {
            let packed = convert!(done);
            _mm512_mask_storeu_epi8(destination.add(done * 3).cast(), whole, packed);
            if _mm512_movepi8_mask(bad) != 0 {
                return done;
            }
            done += BLOCK_GROUPS;
        }

        if done < quads {
            let taking = quads - done;
            let read = mask_for(taking * 4);
            let chars = _mm512_maskz_loadu_epi8(read, source.add(done * 4).cast());
            let values = _mm512_permutex2var_epi8(values_low, chars, values_high);
            if (_mm512_movepi8_mask(chars) | _mm512_movepi8_mask(values)) & read != 0 {
                return done;
            }
            let paired = _mm512_maddubs_epi16(values, _mm512_set1_epi32(0x0140_0140));
            let merged = _mm512_madd_epi16(paired, _mm512_set1_epi32(0x0001_1000));
            let packed = _mm512_permutexvar_epi8(gather, merged);
            _mm512_mask_storeu_epi8(
                destination.add(done * 3).cast(),
                mask_for(taking * 3),
                packed,
            );
        }
        quads
    }
}
