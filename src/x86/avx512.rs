//! The AVX-512 codec, where VBMI is present
//!
//! `vpermb` spells a value in one lookup and `vpmultishiftqb` splits a group in one
//! instruction. Masked loads and stores cover the last partial block, so the walk gets nothing.

use core::arch::x86_64::*;

use crate::alphabet::Alphabet;
use crate::wide::{expand, field_starts, gather_bytes};

// The explicit import shadows the glob above, so the kernel reads the same either way.
#[cfg(miri)]
use shims::{_mm512_multishift_epi64_epi8, _mm512_permutexvar_epi8};

/// Scalar models of the byte permutes the interpreter has no version of
#[cfg(miri)]
mod shims {
    use core::arch::x86_64::__m512i;
    use core::mem::transmute;

    /// `vpermb`: every result byte is the `data` byte its index's low 6 bits pick
    #[inline]
    pub unsafe fn _mm512_permutexvar_epi8(index: __m512i, data: __m512i) -> __m512i {
        // SAFETY: both are 64 bytes, and so is the array either becomes.
        unsafe {
            let index: [u8; 64] = transmute(index);
            let data: [u8; 64] = transmute(data);
            let mut out = [0u8; 64];
            for (slot, &which) in out.iter_mut().zip(index.iter()) {
                *slot = data[usize::from(which & 63)];
            }
            transmute(out)
        }
    }

    /// `vpmultishiftqb`: every result byte is 8 bits of its own 64-bit lane, wrapping at the top
    #[inline]
    pub unsafe fn _mm512_multishift_epi64_epi8(control: __m512i, data: __m512i) -> __m512i {
        // SAFETY: 64 bytes either way, whether read as bytes or as lanes.
        unsafe {
            let control: [u8; 64] = transmute(control);
            let lanes: [u64; 8] = transmute(data);
            let mut out = [0u8; 64];
            for (slot, (&start, lane)) in out
                .iter_mut()
                .zip(control.iter().zip(lanes.iter().flat_map(|lane| [lane; 8])))
            {
                *slot = lane.rotate_right(u32::from(start & 63)) as u8;
            }
            transmute(out)
        }
    }
}

/// Groups a block converts
const BLOCK_GROUPS: usize = 16;

/// Blocks decoded between two readings of the bad character accumulator, unrolled by hand below
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
    // SAFETY: every block stays inside both buffers, the last masking its load and its store.
    unsafe {
        let alphabet = _mm512_loadu_si512(A::ENCODE.as_ptr().cast());
        let expand = _mm512_loadu_si512(EXPAND.as_ptr().cast());
        let starts = _mm512_loadu_si512(FIELD_STARTS.as_ptr().cast());
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        // A whole block reads 48 bytes and writes 64, so its load mask lifts out of the loop.
        let whole = mask_for(BLOCK_GROUPS * 3);
        let mut done = 0;
        while groups - done >= BLOCK_GROUPS {
            let data = _mm512_maskz_loadu_epi8(whole, source.add(done * 3).cast());
            let spread = _mm512_permutexvar_epi8(expand, data);
            let fields = _mm512_multishift_epi64_epi8(starts, spread);
            // Only the low six bits of an index are read, so the bits above need no masking.
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
    // SAFETY: every block stays inside both buffers, the wide store's 16 byte overhang is
    // rewritten by the next store, and the last block masks both ends.
    unsafe {
        let values_low = _mm512_loadu_si512(A::ASCII_LOW.as_ptr().cast());
        let values_high = _mm512_loadu_si512(A::ASCII_HIGH.as_ptr().cast());
        let gather = _mm512_loadu_si512(GATHER.as_ptr().cast());
        let whole = mask_for(BLOCK_GROUPS * 3);
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        // A foreign character sets bit 7 of itself or of its value, so one OR gathers both.
        let mut bad = _mm512_setzero_si512();
        let mut done = 0;

        /// The arithmetic of one block, on characters already loaded
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

        // 48 is an awkward store width, so with 6 groups behind the next store covers the overhang.
        while quads - done >= CHECK_EVERY * BLOCK_GROUPS + 6 {
            // Every load of the run issues before any arithmetic, so four chains are in flight.
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

            // A tree rather than a chain, so only the last of these waits on `bad`.
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
