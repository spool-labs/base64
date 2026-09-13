//! The AVX2 codec
//!
//! A shuffle indexes at most 16 bytes, so the alphabet is reached as a
//! 16 entry table of offsets picked by where a value falls.

#[cfg(not(miri))]
use core::arch::asm;
use core::arch::x86_64::*;

use crate::alphabet::{Alphabet, ODD_VALUE};

// The explicit import shadows the glob above, so the kernel reads the same either way.
#[cfg(miri)]
use shims::{_mm_sfence, _mm_stream_si128};

/// Models of the two stores the interpreter has no version of
#[cfg(miri)]
mod shims {
    use core::arch::x86_64::{__m128i, _mm_storeu_si128};

    /// `movntdq`: the same 16 bytes as an ordinary store, faulting unless 16 byte aligned
    #[inline]
    pub unsafe fn _mm_stream_si128(destination: *mut __m128i, value: __m128i) {
        assert!(
            destination.addr().is_multiple_of(16),
            "a streaming store wants a 16 byte aligned destination"
        );
        // SAFETY: the caller promised 16 writable bytes.
        unsafe { _mm_storeu_si128(destination, value) };
    }

    /// `sfence`: the stores before it land before any store after it
    #[inline]
    pub fn _mm_sfence() {
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
    }
}

/// Bytes a block converts
const BLOCK_IN: usize = 24;

/// Bytes a half block converts
const HALF_IN: usize = 12;

/// Blocks converted at a time in the steady state
const UNROLL: usize = 8;

/// Gather 3 bytes into each 32 bit lane, each half of the register from its own 12 bytes
const SPLIT_LOW: [i8; 16] = [1, 0, 2, 1, 4, 3, 5, 4, 7, 6, 8, 7, 10, 9, 11, 10];
const SPLIT_HIGH: [i8; 16] = [5, 4, 6, 5, 8, 7, 9, 8, 11, 10, 12, 11, 14, 13, 15, 14];

/// Compact 3 bytes out of every 4, then the 4 lanes into 12 bytes
const PACK: [i8; 16] = [2, 1, 0, 6, 5, 4, 10, 9, 8, 14, 13, 12, -1, -1, -1, -1];

/// The alphabet and the two field multipliers, in one width
struct Wide {
    table: __m256i,
    align: __m256i,
    field: __m256i,
}

struct Narrow {
    table: __m128i,
    align: __m128i,
    field: __m128i,
}

impl Wide {
    /// # Safety
    ///
    /// AVX2. `A::SPELLING` is read in full, which is 16 bytes.
    #[inline(always)]
    unsafe fn load<A: Alphabet>() -> Self {
        // SAFETY: `SPELLING` is a 16 byte array, read in full.
        unsafe {
            Wide {
                table: _mm256_broadcastsi128_si256(_mm_loadu_si128(A::SPELLING.as_ptr().cast())),
                align: _mm256_set1_epi32(0x0010_0001),
                field: _mm256_set1_epi32(0x0100_0010),
            }
        }
    }
}

impl Narrow {
    /// The low half of the wide keys, taken by cast rather than made again
    ///
    /// # Safety
    ///
    /// The caller must have AVX2 enabled.
    #[inline(always)]
    unsafe fn of(keys: &Wide) -> Self {
        // SAFETY: no memory is touched, these are register casts.
        unsafe {
            Narrow {
                table: _mm256_castsi256_si128(keys.table),
                align: _mm256_castsi256_si128(keys.align),
                field: _mm256_castsi256_si128(keys.field),
            }
        }
    }
}

/// Convert `groups` whole groups, returning how many, writing past the cache with `STREAM`
///
/// # Safety
///
/// The running machine must have AVX2, and with `STREAM` the output must be 16 byte aligned.
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn encode_bulk<A: Alphabet, const STREAM: bool>(
    input: &[u8],
    out: &mut [u8],
    groups: usize,
) -> usize {
    if groups < HALF_IN / 3 || groups * 3 > input.len() || groups * 4 > out.len() {
        return 0;
    }
    // SAFETY: every block stays inside both buffers, and one loading 4 bytes early follows another.
    unsafe {
        let wide = Wide::load::<A>();
        let narrow = Narrow::of(&wide);
        let split_low = _mm_loadu_si128(SPLIT_LOW.as_ptr().cast());
        let split_high = _mm_loadu_si128(SPLIT_HIGH.as_ptr().cast());
        // Both halves want their 12 bytes at the same offset: one load 4 bytes early, or two loads.
        let early = _mm256_inserti128_si256(_mm256_castsi128_si256(split_high), split_low, 1);
        let here = _mm256_inserti128_si256(_mm256_castsi128_si256(split_low), split_high, 1);
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        let mut done = 0;
        // The first block has nothing 4 bytes to its left, so a lane slide leaves a copy there.
        if groups >= BLOCK_IN / 3 && input.len() >= 32 {
            let raw = _mm256_permutevar8x32_epi32(
                _mm256_loadu_si256(source.cast()),
                _mm256_setr_epi32(0, 0, 1, 2, 3, 4, 5, 6),
            );
            _mm256_storeu_si256(destination.cast(), encode_vector(&wide, early, raw));
            done = BLOCK_IN / 3;

            done = encode_early::<STREAM>(&wide, early, source, destination, input, groups, done);
        }

        // What is left has no spare 4 bytes past it, so it takes the two load form.
        while groups - done >= BLOCK_IN / 3 {
            encode_block(&wide, here, source.add(done * 3), destination.add(done * 4));
            done += BLOCK_IN / 3;
        }
        while groups - done >= HALF_IN / 3 {
            encode_half(
                &narrow,
                split_low,
                source.add(done * 3),
                destination.add(done * 4),
            );
            done += HALF_IN / 3;
        }
        if done < groups {
            let start = groups - HALF_IN / 3;
            encode_half(
                &narrow,
                split_low,
                source.add(start * 3),
                destination.add(start * 4),
            );
        }
        if STREAM {
            // A streaming store is not ordered against the caller's later reads.
            _mm_sfence();
        }
        groups
    }
}

/// Convert the blocks that can load 4 bytes early, from `done` onward
///
/// # Safety
///
/// AVX2, every block from `done` inside both buffers, and with `STREAM` an aligned destination.
#[inline(always)]
unsafe fn encode_early<const STREAM: bool>(
    keys: &Wide,
    split: __m256i,
    source: *const u8,
    destination: *mut u8,
    input: &[u8],
    groups: usize,
    mut done: usize,
) -> usize {
    // SAFETY: both conditions keep the block and its 4 early bytes inside the caller's buffers.
    unsafe {
        // Every load of a run issues before any of its arithmetic, so one block covers the next.
        while groups - done >= UNROLL * (BLOCK_IN / 3)
            && done * 3 + (UNROLL - 1) * BLOCK_IN + BLOCK_IN + 4 <= input.len()
        {
            let base = done * 3 - 4;
            let mut raw = [_mm256_setzero_si256(); UNROLL];
            for (step, slot) in raw.iter_mut().enumerate() {
                *slot = _mm256_loadu_si256(source.add(base + step * BLOCK_IN).cast());
            }
            for (step, held) in raw.into_iter().enumerate() {
                put::<STREAM>(
                    destination.add((done + step * (BLOCK_IN / 3)) * 4),
                    encode_vector(keys, split, held),
                );
            }
            done += UNROLL * (BLOCK_IN / 3);
        }
        while groups - done >= BLOCK_IN / 3 && done * 3 + BLOCK_IN + 4 <= input.len() {
            let raw = _mm256_loadu_si256(source.add(done * 3 - 4).cast());
            put::<STREAM>(destination.add(done * 4), encode_vector(keys, split, raw));
            done += BLOCK_IN / 3;
        }
        done
    }
}

/// Write 32 characters, past the cache when `STREAM`
///
/// # Safety
///
/// AVX2, `destination` writable for 32 bytes, and 16 byte aligned with `STREAM`.
#[inline(always)]
unsafe fn put<const STREAM: bool>(destination: *mut u8, chars: __m256i) {
    // SAFETY: both forms write exactly the 32 bytes the caller promised.
    unsafe {
        if STREAM {
            // Two halves rather than one whole: the allocator hands out 16 byte alignment, not 32.
            let half = destination.cast::<__m128i>();
            _mm_stream_si128(half, _mm256_castsi256_si128(chars));
            _mm_stream_si128(half.add(1), _mm256_extracti128_si256(chars, 1));
        } else {
            _mm256_storeu_si256(destination.cast(), chars);
        }
    }
}

/// `vpmullw`, written out so LLVM cannot rewrite the multiply into two shifts and a blend
///
/// # Safety
///
/// The caller must have AVX2 enabled.
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn mul_wide(value: __m256i, multiplier: __m256i) -> __m256i {
    let product: __m256i;
    #[cfg(miri)]
    {
        // The interpreter has no assembler.
        product = _mm256_mullo_epi16(value, multiplier);
    }
    #[cfg(not(miri))]
    // SAFETY: one register to register instruction, no memory and no flags.
    unsafe {
        asm!(
            "vpmullw {product}, {value}, {multiplier}",
            product = lateout(ymm_reg) product,
            value = in(ymm_reg) value,
            multiplier = in(ymm_reg) multiplier,
            options(pure, nomem, nostack, preserves_flags),
        );
    }
    product
}

/// `vpmullw` on half a register, for the same reason as `mul_wide`
///
/// # Safety
///
/// The caller must have AVX2 enabled.
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn mul_narrow(value: __m128i, multiplier: __m128i) -> __m128i {
    let product: __m128i;
    #[cfg(miri)]
    {
        // The same stand-in as in `mul_wide`.
        product = _mm_mullo_epi16(value, multiplier);
    }
    #[cfg(not(miri))]
    // SAFETY: one register to register instruction, no memory and no flags.
    unsafe {
        asm!(
            "vpmullw {product}, {value}, {multiplier}",
            product = lateout(xmm_reg) product,
            value = in(xmm_reg) value,
            multiplier = in(xmm_reg) multiplier,
            options(pure, nomem, nostack, preserves_flags),
        );
    }
    product
}

/// Split 3 bytes into four 6 bit fields in every 32 bit lane, a multiply doing two shifts at once
macro_rules! split_bits {
    ($gathered:expr, $align:expr, $field:expr, $and:ident, $mul:ident, $shr:ident, $or:ident,
     $set:ident) => {{
        let gathered = $gathered;
        let aligned = $shr($mul(gathered, $align), 10);
        let masked = $mul($and(gathered, $set(0x003F_03F0)), $field);
        $or(aligned, masked)
    }};
}

/// Turn 6 bit values into characters, indexing the 16 entry table by a saturate and a compare
macro_rules! spell {
    ($values:expr, $table:expr, $subs:ident, $cmpgt:ident, $sub:ident, $add:ident,
     $shuffle:ident, $set:ident) => {{
        let values = $values;
        let index = $sub($subs(values, $set(51)), $cmpgt(values, $set(25)));
        $add(values, $shuffle($table, index))
    }};
}

/// Spell one register's worth: 24 bytes gathered in, 32 characters out
///
/// # Safety
///
/// The caller must have AVX2 enabled.
#[inline(always)]
unsafe fn encode_vector(keys: &Wide, split: __m256i, raw: __m256i) -> __m256i {
    // SAFETY: no memory is touched here at all.
    unsafe {
        let gathered = _mm256_shuffle_epi8(raw, split);
        let values = split_bits!(
            gathered,
            keys.align,
            keys.field,
            _mm256_and_si256,
            mul_wide,
            _mm256_srli_epi16,
            _mm256_or_si256,
            _mm256_set1_epi32
        );
        spell!(
            values,
            keys.table,
            _mm256_subs_epu8,
            _mm256_cmpgt_epi8,
            _mm256_sub_epi8,
            _mm256_add_epi8,
            _mm256_shuffle_epi8,
            _mm256_set1_epi8
        )
    }
}

/// Convert 8 groups: 24 bytes in, 32 characters out
///
/// # Safety
///
/// AVX2, `source` readable for 24 bytes, `destination` writable for 32.
#[inline(always)]
unsafe fn encode_block(keys: &Wide, split: __m256i, source: *const u8, destination: *mut u8) {
    // SAFETY: the two loads cover bytes 0 to 16 and 8 to 24, and the store writes exactly 32.
    unsafe {
        let low = _mm_loadu_si128(source.cast());
        let high = _mm_loadu_si128(source.add(BLOCK_IN - 16).cast());
        let pair = _mm256_inserti128_si256(_mm256_castsi128_si256(low), high, 1);
        _mm256_storeu_si256(destination.cast(), encode_vector(keys, split, pair));
    }
}

/// Convert 4 groups: 12 bytes in, 16 characters out
///
/// # Safety
///
/// AVX2, `source` readable for 12 bytes, `destination` writable for 16.
#[inline(always)]
unsafe fn encode_half(keys: &Narrow, split: __m128i, source: *const u8, destination: *mut u8) {
    // SAFETY: the 12 bytes are read as 8 and 4, and the store writes exactly 16.
    unsafe {
        let front = _mm_loadl_epi64(source.cast());
        let back = _mm_cvtsi32_si128(source.add(8).cast::<u32>().read_unaligned() as i32);
        let twelve = _mm_unpacklo_epi64(front, back);
        let gathered = _mm_shuffle_epi8(twelve, split);
        let values = split_bits!(
            gathered,
            keys.align,
            keys.field,
            _mm_and_si128,
            mul_narrow,
            _mm_srli_epi16,
            _mm_or_si128,
            _mm_set1_epi32
        );
        let spelled = spell!(
            values,
            keys.table,
            _mm_subs_epu8,
            _mm_cmpgt_epi8,
            _mm_sub_epi8,
            _mm_add_epi8,
            _mm_shuffle_epi8,
            _mm_set1_epi8
        );
        _mm_storeu_si128(destination.cast(), spelled);
    }
}

/// Convert `quads` whole groups, returning how many were converted
///
/// # Safety
///
/// The running machine must have AVX2.
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    if quads < 4 || quads * 4 > input.len() || quads * 3 > out.len() {
        return 0;
    }
    // SAFETY: as in `encode_bulk`, every block is inside both buffers.
    unsafe {
        let tables = Tables::load::<A>();
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        // One reading of the accumulator per block: `vptest` is cheap and the branch predicts.
        let mut done = 0;
        // A block with another behind it stores its 24 bytes as two overlapping 16 byte
        // stores, since the next block overwrites the 4 it ran over.
        while quads - done >= 16 {
            let found =
                decode_block::<true>(&tables, source.add(done * 4), destination.add(done * 3));
            if _mm256_testz_si256(found, found) == 0 {
                return done;
            }
            done += 8;
        }
        while quads - done >= 8 {
            let found =
                decode_block::<false>(&tables, source.add(done * 4), destination.add(done * 3));
            if _mm256_testz_si256(found, found) == 0 {
                return done;
            }
            done += 8;
        }
        while quads - done >= 4 {
            let found = decode_half(&tables, source.add(done * 4), destination.add(done * 3));
            if _mm_testz_si128(found, found) == 0 {
                return done;
            }
            done += 4;
        }
        if done < quads {
            let start = quads - 4;
            let found = decode_half(&tables, source.add(start * 4), destination.add(start * 3));
            if _mm_testz_si128(found, found) != 0 {
                done = quads;
            }
        }
        done
    }
}

/// The tables a decode block reads, in both widths
struct Tables {
    wide_low: __m256i,
    wide_high: __m256i,
    wide_roll: __m256i,
    wide_odd: __m256i,
    narrow_low: __m128i,
    narrow_high: __m128i,
    narrow_roll: __m128i,
    narrow_odd: __m128i,
    wide_pack: __m256i,
    narrow_pack: __m128i,
}

impl Tables {
    /// # Safety
    ///
    /// AVX2. Each table read is 16 bytes, which is their length.
    #[inline(always)]
    unsafe fn load<A: Alphabet>() -> Self {
        // SAFETY: each `LUT_*` is a 16 byte array, read in full.
        unsafe {
            let low = _mm_loadu_si128(A::LUT_LO.as_ptr().cast());
            let high = _mm_loadu_si128(A::LUT_HI.as_ptr().cast());
            let roll = _mm_loadu_si128(A::LUT_ROLL.as_ptr().cast());
            let pack = _mm_loadu_si128(PACK.as_ptr().cast());
            Tables {
                wide_low: _mm256_broadcastsi128_si256(low),
                wide_high: _mm256_broadcastsi128_si256(high),
                wide_roll: _mm256_broadcastsi128_si256(roll),
                wide_odd: _mm256_set1_epi8(A::ODD_CHAR as i8),
                narrow_low: low,
                narrow_high: high,
                narrow_roll: roll,
                narrow_odd: _mm_set1_epi8(A::ODD_CHAR as i8),
                wide_pack: _mm256_broadcastsi128_si256(pack),
                narrow_pack: pack,
            }
        }
    }
}

/// Turn characters into values, and say whether any was outside the alphabet
macro_rules! values_of {
    ($chars:expr, $low:expr, $high:expr, $roll:expr, $odd:expr, $bad:ident, $and:ident,
     $or:ident, $srli:ident, $shuffle:ident, $cmpeq:ident, $add:ident, $blend:ident,
     $set8:ident, $set32:ident) => {{
        let chars = $chars;
        let high_nibble = $and($srli(chars, 4), $set32(0x0f0f_0f0f));
        let low_nibble = $and(chars, $set32(0x0f0f_0f0f));
        $bad = $or(
            $bad,
            $and($shuffle($high, high_nibble), $shuffle($low, low_nibble)),
        );
        let rolled = $add(chars, $shuffle($roll, high_nibble));
        $blend(rolled, $set8(ODD_VALUE as i8), $cmpeq(chars, $odd))
    }};
}

/// Pack four 6 bit values into 3 bytes, in every 32 bit lane
macro_rules! merge_bits {
    ($values:expr, $maddubs:ident, $madd:ident, $shuffle:ident, $set32:ident, $pack:expr) => {{
        let paired = $maddubs($values, $set32(0x0140_0140));
        $shuffle($madd(paired, $set32(0x0001_1000)), $pack)
    }};
}

/// Convert 8 groups: 32 characters in, 24 bytes out
///
/// # Safety
///
/// AVX2, `source` readable for 32 bytes, `destination` writable for 24, or 28 with `SPARE`.
#[inline(always)]
unsafe fn decode_block<const SPARE: bool>(
    tables: &Tables,
    source: *const u8,
    destination: *mut u8,
) -> __m256i {
    // SAFETY: one 32 byte load, and stores inside what the caller promised.
    unsafe {
        let chars = _mm256_loadu_si256(source.cast());
        let mut bad = _mm256_setzero_si256();
        let values = values_of!(
            chars,
            tables.wide_low,
            tables.wide_high,
            tables.wide_roll,
            tables.wide_odd,
            bad,
            _mm256_and_si256,
            _mm256_or_si256,
            _mm256_srli_epi32,
            _mm256_shuffle_epi8,
            _mm256_cmpeq_epi8,
            _mm256_add_epi8,
            _mm256_blendv_epi8,
            _mm256_set1_epi8,
            _mm256_set1_epi32
        );
        let packed = merge_bits!(
            values,
            _mm256_maddubs_epi16,
            _mm256_madd_epi16,
            _mm256_shuffle_epi8,
            _mm256_set1_epi32,
            tables.wide_pack
        );
        // 12 bytes live at the bottom of each half.
        let low = _mm256_castsi256_si128(packed);
        let high = _mm256_extracti128_si256(packed, 1);
        if SPARE {
            _mm_storeu_si128(destination.cast(), low);
            _mm_storeu_si128(destination.add(12).cast(), high);
        } else {
            _mm_storeu_si128(
                destination.cast(),
                _mm_or_si128(low, _mm_slli_si128(high, 12)),
            );
            _mm_storel_epi64(destination.add(16).cast(), _mm_srli_si128(high, 4));
        }
        bad
    }
}

/// Convert 4 groups: 16 characters in, 12 bytes out
///
/// # Safety
///
/// AVX2, `source` readable for 16 bytes, `destination` writable for 12.
#[inline(always)]
unsafe fn decode_half(tables: &Tables, source: *const u8, destination: *mut u8) -> __m128i {
    // SAFETY: one 16 byte load, and stores of exactly 8 and 4 bytes.
    unsafe {
        let chars = _mm_loadu_si128(source.cast());
        let mut bad = _mm_setzero_si128();
        let values = values_of!(
            chars,
            tables.narrow_low,
            tables.narrow_high,
            tables.narrow_roll,
            tables.narrow_odd,
            bad,
            _mm_and_si128,
            _mm_or_si128,
            _mm_srli_epi32,
            _mm_shuffle_epi8,
            _mm_cmpeq_epi8,
            _mm_add_epi8,
            _mm_blendv_epi8,
            _mm_set1_epi8,
            _mm_set1_epi32
        );
        let packed = merge_bits!(
            values,
            _mm_maddubs_epi16,
            _mm_madd_epi16,
            _mm_shuffle_epi8,
            _mm_set1_epi32,
            tables.narrow_pack
        );
        _mm_storel_epi64(destination.cast(), packed);
        destination
            .add(8)
            .cast::<u32>()
            .write_unaligned(_mm_extract_epi32(packed, 2) as u32);
        bad
    }
}
