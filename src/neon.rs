//! The aarch64 codec
//!
//! A block is a whole number of groups: `ld3` reads 48 bytes deinterleaved,
//! one `tbl` spells them, and `st4` writes them back interleaved.

use core::arch::aarch64::{
    uint8x16_t, uint8x16x3_t, uint8x16x4_t, uint8x8_t, uint8x8x3_t, uint8x8x4_t, vaddq_u8, vand_u8,
    vandq_u32, vandq_u8, vbslq_u8, vceqq_u8, vcombine_u8, vdup_n_u32, vdup_n_u8, vdupq_n_u32,
    vdupq_n_u8, vget_high_u8, vget_low_u8, vgetq_lane_u32, vld1_u8, vld1q_u8, vld1q_u8_x4, vld3_u8,
    vld3q_u8, vld4_u8, vld4q_u8, vmaxvq_u8, vmulq_u16, vorr_u8, vorrq_u16, vorrq_u32, vorrq_u8,
    vqtbl1q_u8, vqtbl4q_u8, vreinterpret_u8_u32, vreinterpretq_u16_u32, vreinterpretq_u16_u8,
    vreinterpretq_u32_u16, vreinterpretq_u32_u8, vreinterpretq_u8_u16, vreinterpretq_u8_u32,
    vshl_n_u8, vshlq_n_u32, vshlq_n_u8, vshr_n_u8, vshrq_n_u16, vshrq_n_u32, vshrq_n_u8, vsli_n_u8,
    vsliq_n_u8, vsri_n_u8, vsriq_n_u8, vst1_u8, vst1q_u8, vst3_u8, vst3q_u8, vst4_u8, vst4q_u8,
};

use crate::alphabet::{Alphabet, ODD_VALUE};

/// Groups a full-width block converts
const WIDE_GROUPS: usize = 16;

/// Groups a half-width block converts
const NARROW_GROUPS: usize = 8;

/// Groups a single-register block converts
const SHORT_GROUPS: usize = 4;

/// Gather 3 bytes into each 32-bit lane, as `[b1 b0 b2 b1]`
const GATHER: [u8; 16] = [1, 0, 2, 1, 4, 3, 5, 4, 7, 6, 8, 7, 10, 9, 11, 10];

/// Take the 3 bytes each 32-bit lane ends up holding
const COMPACT: [u8; 16] = [2, 1, 0, 6, 5, 4, 10, 9, 8, 14, 13, 12, 0, 0, 0, 0];

/// Blocks decoded between two readings of the invalid-character accumulator
const CHECK_EVERY: usize = 4;

/// Convert `groups` whole groups, returning how many were converted
pub(crate) fn encode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    if groups < SHORT_GROUPS || groups * 3 > input.len() || groups * 4 > out.len() {
        return 0;
    }
    // SAFETY: every block below ends at or before group `groups`, checked above.
    unsafe {
        let table = vld1q_u8_x4(A::ENCODE.as_ptr());
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        let mut done = 0;
        while groups - done >= WIDE_GROUPS {
            encode_wide(&table, source.add(done * 3), destination.add(done * 4));
            done += WIDE_GROUPS;
        }
        // The leftover is covered by a block moved back to end with the groups, redoing a few.
        match groups - done {
            0 => {}
            1..=4 => {
                let start = groups - SHORT_GROUPS;
                encode_short(&table, source.add(start * 3), destination.add(start * 4));
            }
            5..=8 if groups >= NARROW_GROUPS => {
                let start = groups - NARROW_GROUPS;
                encode_narrow(&table, source.add(start * 3), destination.add(start * 4));
            }
            5..=7 => {
                encode_short(&table, source.add(done * 3), destination.add(done * 4));
                let start = groups - SHORT_GROUPS;
                encode_short(&table, source.add(start * 3), destination.add(start * 4));
            }
            _ if groups >= WIDE_GROUPS => {
                let start = groups - WIDE_GROUPS;
                encode_wide(&table, source.add(start * 3), destination.add(start * 4));
            }
            9..=12 => {
                encode_narrow(&table, source.add(done * 3), destination.add(done * 4));
                let start = groups - SHORT_GROUPS;
                encode_short(&table, source.add(start * 3), destination.add(start * 4));
            }
            _ => {
                encode_narrow(&table, source.add(done * 3), destination.add(done * 4));
                let start = groups - NARROW_GROUPS;
                encode_narrow(&table, source.add(start * 3), destination.add(start * 4));
            }
        }
    }
    groups
}

/// Four 6-bit fields out of 3 bytes, one shifted insert per field
macro_rules! split_bits {
    ($group:expr, $low_six:expr, $shr:ident, $shl:ident, $sri:ident, $and:ident) => {{
        let group = $group;
        (
            $shr::<2>(group.0),
            $and($sri::<4>($shl::<4>(group.0), group.1), $low_six),
            $and($sri::<6>($shl::<2>(group.1), group.2), $low_six),
            $and(group.2, $low_six),
        )
    }};
}

/// Convert 16 groups: 48 bytes in, 64 characters out
///
/// # Safety
///
/// `source` must be readable for 48 bytes and `destination` writable for 64.
#[inline(always)]
unsafe fn encode_wide(table: &uint8x16x4_t, source: *const u8, destination: *mut u8) {
    // SAFETY: exactly the 48 bytes one `ld3` reads and the 64 one `st4` writes.
    unsafe {
        let low_six = vdupq_n_u8(0x3F);
        let (first, second, third, fourth) = split_bits!(
            vld3q_u8(source),
            low_six,
            vshrq_n_u8,
            vshlq_n_u8,
            vsriq_n_u8,
            vandq_u8
        );
        vst4q_u8(
            destination,
            uint8x16x4_t(
                vqtbl4q_u8(*table, first),
                vqtbl4q_u8(*table, second),
                vqtbl4q_u8(*table, third),
                vqtbl4q_u8(*table, fourth),
            ),
        );
    }
}

/// Convert 8 groups: 24 bytes in, 32 characters out
///
/// # Safety
///
/// `source` must be readable for 24 bytes and `destination` writable for 32.
#[inline(always)]
unsafe fn encode_narrow(table: &uint8x16x4_t, source: *const u8, destination: *mut u8) {
    // SAFETY: exactly what one half-width `ld3` reads and one half-width `st4` writes.
    unsafe {
        let low_six = vdup_n_u8(0x3F);
        let (first, second, third, fourth) = split_bits!(
            vld3_u8(source),
            low_six,
            vshr_n_u8,
            vshl_n_u8,
            vsri_n_u8,
            vand_u8
        );
        let front = vqtbl4q_u8(*table, vcombine_u8(first, second));
        let back = vqtbl4q_u8(*table, vcombine_u8(third, fourth));
        vst4_u8(
            destination,
            uint8x8x4_t(
                vget_low_u8(front),
                vget_high_u8(front),
                vget_low_u8(back),
                vget_high_u8(back),
            ),
        );
    }
}

/// A half-width accumulator seen as a full-width one
///
/// # Safety
///
/// NEON is the baseline on aarch64, so the intrinsic is always there.
#[inline(always)]
unsafe fn widen(bad: uint8x8_t) -> uint8x16_t {
    // SAFETY: NEON is the baseline on this architecture.
    unsafe { vcombine_u8(bad, vdup_n_u8(0)) }
}

/// Convert fewer groups than one full block, in one or two blocks
///
/// # Safety
///
/// `quads` groups readable and writable, and in `SHORT_GROUPS..WIDE_GROUPS`.
#[inline(always)]
unsafe fn decode_small(
    tables: &Tables,
    source: *const u8,
    destination: *mut u8,
    quads: usize,
) -> uint8x16_t {
    // SAFETY: each block covers groups ending at or before group `quads`.
    unsafe {
        if quads >= NARROW_GROUPS {
            let mut bad = widen(decode_narrow(tables, source, destination));
            if quads > NARROW_GROUPS + SHORT_GROUPS {
                let start = quads - NARROW_GROUPS;
                bad = vorrq_u8(
                    bad,
                    widen(decode_narrow(
                        tables,
                        source.add(start * 4),
                        destination.add(start * 3),
                    )),
                );
            } else if quads > NARROW_GROUPS {
                let start = quads - SHORT_GROUPS;
                bad = vorrq_u8(
                    bad,
                    decode_short(tables, source.add(start * 4), destination.add(start * 3)),
                );
            }
            bad
        } else {
            let mut bad = decode_short(tables, source, destination);
            if quads > SHORT_GROUPS {
                let start = quads - SHORT_GROUPS;
                bad = vorrq_u8(
                    bad,
                    decode_short(tables, source.add(start * 4), destination.add(start * 3)),
                );
            }
            bad
        }
    }
}

/// Cover the groups past the last whole block, with the cheapest block
///
/// # Safety
///
/// `quads` groups readable and writable, and `quads` at least `WIDE_GROUPS`.
#[inline(always)]
unsafe fn decode_tail(
    tables: &Tables,
    source: *const u8,
    destination: *mut u8,
    quads: usize,
) -> uint8x16_t {
    // SAFETY: both branches convert groups ending at group `quads`.
    unsafe {
        if quads % WIDE_GROUPS <= SHORT_GROUPS {
            let start = quads - SHORT_GROUPS;
            decode_short(tables, source.add(start * 4), destination.add(start * 3))
        } else if quads % WIDE_GROUPS <= NARROW_GROUPS {
            let start = quads - NARROW_GROUPS;
            widen(decode_narrow(
                tables,
                source.add(start * 4),
                destination.add(start * 3),
            ))
        } else {
            let start = quads - WIDE_GROUPS;
            decode_wide(tables, source.add(start * 4), destination.add(start * 3))
        }
    }
}

/// Convert 4 groups: 12 bytes in, 16 characters out
///
/// # Safety
///
/// `source` must be readable for 12 bytes and `destination` writable for 16.
#[inline(always)]
unsafe fn encode_short(table: &uint8x16x4_t, source: *const u8, destination: *mut u8) {
    // SAFETY: loads of 8 and 4 cover the 12 promised, and the store writes 16.
    unsafe {
        let front = vld1_u8(source);
        // Read as a value, since `vld1_dup_u32` wants a `*const u32` the source need not suit.
        let back = vreinterpret_u8_u32(vdup_n_u32(source.add(8).cast::<u32>().read_unaligned()));
        let gathered = vqtbl1q_u8(vcombine_u8(front, back), vld1q_u8(GATHER.as_ptr()));

        // Every field is 6 bits inside one 16-bit half, so a multiply does two shifts at once.
        let halves = vreinterpretq_u16_u8(gathered);
        let aligned = vshrq_n_u16::<10>(vmulq_u16(
            halves,
            vreinterpretq_u16_u32(vdupq_n_u32(0x0010_0001)),
        ));
        let masked = vmulq_u16(
            vreinterpretq_u16_u32(vandq_u32(
                vreinterpretq_u32_u16(halves),
                vdupq_n_u32(0x003F_03F0),
            )),
            vreinterpretq_u16_u32(vdupq_n_u32(0x0100_0010)),
        );
        let values = vreinterpretq_u8_u16(vorrq_u16(aligned, masked));

        vst1q_u8(destination, vqtbl4q_u8(*table, values));
    }
}

/// The tables a decode block reads, loaded once for a whole call
struct Tables {
    low: uint8x16_t,
    high: uint8x16_t,
    roll: uint8x16_t,
    odd_char: uint8x16_t,
    odd_value: uint8x16_t,
}

impl Tables {
    /// # Safety
    ///
    /// Always sound: each `LUT_*` is a 16 byte array, read in full.
    #[inline]
    unsafe fn load<A: Alphabet>() -> Self {
        // SAFETY: each `LUT_*` is a 16 byte array, read in full.
        unsafe {
            Tables {
                low: vld1q_u8(A::LUT_LO.as_ptr()),
                high: vld1q_u8(A::LUT_HI.as_ptr()),
                roll: vld1q_u8(A::LUT_ROLL.as_ptr()),
                odd_char: vdupq_n_u8(A::ODD_CHAR),
                odd_value: vdupq_n_u8(ODD_VALUE),
            }
        }
    }
}

/// Convert `quads` whole groups, returning how many, stopping at a foreign character
pub(crate) fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    if quads < SHORT_GROUPS || quads * 4 > input.len() || quads * 3 > out.len() {
        return 0;
    }
    // SAFETY: every block below ends at or before group `quads`, checked above.
    unsafe {
        let tables = Tables::load::<A>();
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        let full = quads / WIDE_GROUPS;
        let has_tail = quads - full * WIDE_GROUPS > 0;

        if full > 0 {
            // Reading the accumulator back is a branch, so it happens once per several blocks.
            let mut taken = 0;
            let mut done = 0;
            // A constant count, so the run unrolls and the loads issue together.
            while full - taken >= CHECK_EVERY {
                let mut bad = vdupq_n_u8(0);
                for step in 0..CHECK_EVERY {
                    let start = (taken + step) * WIDE_GROUPS;
                    bad = vorrq_u8(
                        bad,
                        decode_wide(&tables, source.add(start * 4), destination.add(start * 3)),
                    );
                }
                if vmaxvq_u8(bad) != 0 {
                    return done;
                }
                taken += CHECK_EVERY;
                done = taken * WIDE_GROUPS;
            }
            let mut bad = vdupq_n_u8(0);
            for step in 0..full - taken {
                let start = (taken + step) * WIDE_GROUPS;
                bad = vorrq_u8(
                    bad,
                    decode_wide(&tables, source.add(start * 4), destination.add(start * 3)),
                );
            }
            if has_tail {
                bad = vorrq_u8(bad, decode_tail(&tables, source, destination, quads));
            }
            if vmaxvq_u8(bad) == 0 {
                done = quads;
            }
            done
        } else if vmaxvq_u8(decode_small(&tables, source, destination, quads)) == 0 {
            quads
        } else {
            0
        }
    }
}

/// Turn characters into values, gathering the ones outside the alphabet
macro_rules! values_of {
    ($chars:expr, $bad:expr, $tables:expr, $shr:ident, $and:ident, $or:ident, $tbl:ident,
     $add:ident, $ceq:ident, $bsl:ident, $low_four:expr, $odd_char:expr, $odd_value:expr) => {{
        let chars = $chars;
        let high = $shr::<4>(chars);
        let low = $and(chars, $low_four);
        $bad = $or($bad, $and($tbl($tables.high, high), $tbl($tables.low, low)));
        let rolled = $add(chars, $tbl($tables.roll, high));
        $bsl($ceq(chars, $odd_char), $odd_value, rolled)
    }};
}

/// Three bytes out of four 6-bit fields, one insert per field
macro_rules! merge_bits {
    ($fields:expr, $shr:ident, $sli:ident) => {{
        let (first, second, third, fourth) = $fields;
        (
            $sli::<2>($shr::<4>(second), first),
            $sli::<4>($shr::<2>(third), second),
            $sli::<6>(fourth, third),
        )
    }};
}

/// Convert 4 groups: 16 characters in, 12 bytes out
///
/// # Safety
///
/// `source` must be readable for 16 bytes and `destination` writable for 12.
#[inline(always)]
unsafe fn decode_short(tables: &Tables, source: *const u8, destination: *mut u8) -> uint8x16_t {
    // SAFETY: one 16-byte load, and stores of exactly 8 and 4 bytes.
    unsafe {
        let low_four = vdupq_n_u8(0x0F);
        let odd_char = tables.odd_char;
        let odd_value = tables.odd_value;
        let mut bad = vdupq_n_u8(0);
        let values = values_of!(
            vld1q_u8(source),
            bad,
            tables,
            vshrq_n_u8,
            vandq_u8,
            vorrq_u8,
            vqtbl1q_u8,
            vaddq_u8,
            vceqq_u8,
            vbslq_u8,
            low_four,
            odd_char,
            odd_value
        );

        // Sliding the outer pair up and the inner pair down puts the group's 24 bits in a row.
        let lanes = vreinterpretq_u32_u8(values);
        let outer = vandq_u32(lanes, vdupq_n_u32(0x003F_003F));
        let inner = vandq_u32(lanes, vdupq_n_u32(0x3F00_3F00));
        let merged = vorrq_u32(
            vorrq_u32(vshlq_n_u32::<18>(outer), vshrq_n_u32::<10>(outer)),
            vorrq_u32(vshlq_n_u32::<4>(inner), vshrq_n_u32::<24>(inner)),
        );
        let packed = vqtbl1q_u8(vreinterpretq_u8_u32(merged), vld1q_u8(COMPACT.as_ptr()));

        vst1_u8(destination, vget_low_u8(packed));
        destination
            .add(8)
            .cast::<u32>()
            .write_unaligned(vgetq_lane_u32::<2>(vreinterpretq_u32_u8(packed)));
        bad
    }
}

/// Convert 16 groups: 64 characters in, 48 bytes out, written even where a character was bad
///
/// # Safety
///
/// `source` must be readable for 64 bytes and `destination` writable for 48.
#[inline(always)]
unsafe fn decode_wide(tables: &Tables, source: *const u8, destination: *mut u8) -> uint8x16_t {
    // SAFETY: exactly the 64 bytes one `ld4` reads and the 48 one `st3` writes.
    unsafe {
        let low_four = vdupq_n_u8(0x0F);
        let odd_char = tables.odd_char;
        let odd_value = tables.odd_value;
        let quad = vld4q_u8(source);
        let mut bad = vdupq_n_u8(0);
        let fields = (
            values_of!(
                quad.0, bad, tables, vshrq_n_u8, vandq_u8, vorrq_u8, vqtbl1q_u8, vaddq_u8,
                vceqq_u8, vbslq_u8, low_four, odd_char, odd_value
            ),
            values_of!(
                quad.1, bad, tables, vshrq_n_u8, vandq_u8, vorrq_u8, vqtbl1q_u8, vaddq_u8,
                vceqq_u8, vbslq_u8, low_four, odd_char, odd_value
            ),
            values_of!(
                quad.2, bad, tables, vshrq_n_u8, vandq_u8, vorrq_u8, vqtbl1q_u8, vaddq_u8,
                vceqq_u8, vbslq_u8, low_four, odd_char, odd_value
            ),
            values_of!(
                quad.3, bad, tables, vshrq_n_u8, vandq_u8, vorrq_u8, vqtbl1q_u8, vaddq_u8,
                vceqq_u8, vbslq_u8, low_four, odd_char, odd_value
            ),
        );
        let (first, second, third) = merge_bits!(fields, vshrq_n_u8, vsliq_n_u8);
        vst3q_u8(destination, uint8x16x3_t(first, second, third));
        bad
    }
}

/// Convert 8 groups: 32 characters in, 24 bytes out
///
/// # Safety
///
/// `source` must be readable for 32 bytes and `destination` writable for 24.
#[inline(always)]
unsafe fn decode_narrow(tables: &Tables, source: *const u8, destination: *mut u8) -> uint8x8_t {
    // SAFETY: exactly what one half-width `ld4` reads and one half-width `st3` writes.
    unsafe {
        let low_four = vdupq_n_u8(0x0F);
        let odd_char = tables.odd_char;
        let odd_value = tables.odd_value;
        let quad = vld4_u8(source);
        let mut bad = vdupq_n_u8(0);
        let front = values_of!(
            vcombine_u8(quad.0, quad.1),
            bad,
            tables,
            vshrq_n_u8,
            vandq_u8,
            vorrq_u8,
            vqtbl1q_u8,
            vaddq_u8,
            vceqq_u8,
            vbslq_u8,
            low_four,
            odd_char,
            odd_value
        );
        let back = values_of!(
            vcombine_u8(quad.2, quad.3),
            bad,
            tables,
            vshrq_n_u8,
            vandq_u8,
            vorrq_u8,
            vqtbl1q_u8,
            vaddq_u8,
            vceqq_u8,
            vbslq_u8,
            low_four,
            odd_char,
            odd_value
        );
        let fields = (
            vget_low_u8(front),
            vget_high_u8(front),
            vget_low_u8(back),
            vget_high_u8(back),
        );
        let (first, second, third) = merge_bits!(fields, vshr_n_u8, vsli_n_u8);
        vst3_u8(destination, uint8x8x3_t(first, second, third));
        vorr_u8(vget_low_u8(bad), vget_high_u8(bad))
    }
}
