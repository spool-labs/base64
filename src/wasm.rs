//! The wasm simd128 codec, the AVX2 form on one 16 byte register

use core::arch::wasm32::*;

use crate::alphabet::{Alphabet, ODD_VALUE};

/// Blocks an encode converts at a time in the steady state
const UNROLL: usize = 4;

/// Convert `groups` whole groups, returning how many
pub(crate) fn encode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    if groups < 4 || groups * 3 > input.len() || groups * 4 > out.len() {
        return 0;
    }
    // SAFETY: `SPELLING` is 16 bytes, and every block below is inside both buffers.
    unsafe {
        let table = v128_load(A::SPELLING.as_ptr().cast());
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        let mut done = encode_run::<UNROLL>(table, source, destination, input, groups, 0);
        while groups - done >= 4 {
            let slack = input.len() - done * 3 >= 16;
            encode_block(
                table,
                source.add(done * 3),
                destination.add(done * 4),
                slack,
            );
            done += 4;
        }
        // The leftover is covered by a block moved back to end with the groups, redoing a few.
        if done < groups {
            let start = groups - 4;
            let slack = input.len() - start * 3 >= 16;
            encode_block(
                table,
                source.add(start * 3),
                destination.add(start * 4),
                slack,
            );
        }
        groups
    }
}

/// Convert runs of `UNROLL` blocks, every load issued before any of the arithmetic
///
/// # Safety
/// Every block from `done` on is inside both buffers.
#[inline(always)]
unsafe fn encode_run<const UNROLL: usize>(
    table: v128,
    source: *const u8,
    destination: *mut u8,
    input: &[u8],
    groups: usize,
    mut done: usize,
) -> usize {
    // SAFETY: the length test leaves 16 readable bytes under the last load of a run.
    unsafe {
        while groups - done >= UNROLL * 4 && input.len() - done * 3 >= (UNROLL - 1) * 12 + 16 {
            let base = source.add(done * 3);
            let mut raw = [u8x16_splat(0); UNROLL];
            for (step, slot) in raw.iter_mut().enumerate() {
                *slot = v128_load(base.add(step * 12).cast());
            }
            let target = destination.add(done * 4);
            for (step, held) in raw.into_iter().enumerate() {
                v128_store(target.add(step * 16).cast(), encode_vector(table, held));
            }
            done += UNROLL * 4;
        }
        done
    }
}

/// Convert 4 groups: 12 bytes in, 16 characters out
///
/// # Safety
/// `source` readable for 12 bytes, or 16 with `slack`, and `destination` writable for 16.
#[inline(always)]
unsafe fn encode_block(table: v128, source: *const u8, destination: *mut u8, slack: bool) {
    // SAFETY: one 16 byte load, or 8 and 4, and a store of exactly 16.
    unsafe {
        let raw = if slack {
            v128_load(source.cast())
        } else {
            v128_load32_lane::<2>(v128_load64_zero(source.cast()), source.add(8).cast())
        };
        v128_store(destination.cast(), encode_vector(table, raw));
    }
}

/// Spell 12 bytes as 16 characters
#[inline(always)]
fn encode_vector(table: v128, raw: v128) -> v128 {
    // Each 32 bit lane holds a group as [b1, b0, b2, b1], one 6 bit field per output byte.
    let gathered = i8x16_shuffle::<1, 0, 2, 1, 4, 3, 5, 4, 7, 6, 8, 7, 10, 9, 11, 10>(raw, raw);
    // A multiply by differing lane constants is scalarized here, so each field moves on its own.
    let first = u16x8_shr(v128_and(gathered, i32x4_splat(0x0000_FC00)), 10);
    let second = i16x8_shl(v128_and(gathered, i32x4_splat(0x0000_03F0)), 4);
    let third = u16x8_shr(v128_and(gathered, i32x4_splat(0x0FC0_0000)), 6);
    let fourth = i16x8_shl(v128_and(gathered, i32x4_splat(0x003F_0000)), 8);
    let values = v128_or(v128_or(first, second), v128_or(third, fourth));
    // The run a value falls in indexes the offset that spells it.
    let index = i8x16_sub(
        u8x16_sub_sat(values, u8x16_splat(51)),
        i8x16_gt(values, i8x16_splat(25)),
    );
    i8x16_add(values, u8x16_swizzle(table, index))
}

/// Convert `quads` whole groups, returning how many, stopping at a foreign character
pub(crate) fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    if quads < 4 || quads * 4 > input.len() || quads * 3 > out.len() {
        return 0;
    }
    // SAFETY: each table is 16 bytes, and every block below is inside both buffers.
    unsafe {
        let tables = Tables::load::<A>();
        let source = input.as_ptr();
        let destination = out.as_mut_ptr();

        // Two blocks at a time, both loads before either chain, while both 16 byte stores fit.
        let mut done = 0;
        while quads - done >= 12 {
            let base = source.add(done * 4);
            let front = v128_load(base.cast());
            let back = v128_load(base.add(16).cast());
            let target = destination.add(done * 3);
            let (first, bad_first) = decode_vector(&tables, front);
            let (second, bad_second) = decode_vector(&tables, back);
            v128_store(target.cast(), first);
            v128_store(target.add(12).cast(), second);
            // A foreign character in either one sends the loop below over the same two blocks.
            if v128_any_true(v128_or(bad_first, bad_second)) {
                break;
            }
            done += 8;
        }
        while quads - done >= 4 {
            // A block with another behind it stores 16 bytes, since the next block rewrites 4.
            let spare = quads - done >= 8;
            let bad = decode_block(
                &tables,
                source.add(done * 4),
                destination.add(done * 3),
                spare,
            );
            if v128_any_true(bad) {
                return done;
            }
            done += 4;
        }
        if done < quads {
            let start = quads - 4;
            let bad = decode_block(
                &tables,
                source.add(start * 4),
                destination.add(start * 3),
                false,
            );
            if !v128_any_true(bad) {
                done = quads;
            }
        }
        done
    }
}

/// The nibble tables of one alphabet
struct Tables {
    low: v128,
    high: v128,
    roll: v128,
    odd: v128,
}

impl Tables {
    /// # Safety
    /// Each `LUT_*` is a 16 byte array, read in full.
    #[inline(always)]
    unsafe fn load<A: Alphabet>() -> Self {
        // SAFETY: three 16 byte reads of 16 byte arrays.
        unsafe {
            Tables {
                low: v128_load(A::LUT_LO.as_ptr().cast()),
                high: v128_load(A::LUT_HI.as_ptr().cast()),
                roll: v128_load(A::LUT_ROLL.as_ptr().cast()),
                odd: u8x16_splat(A::ODD_CHAR),
            }
        }
    }
}

/// Convert 4 groups: 16 characters in, 12 bytes out, saying whether any character was foreign
///
/// # Safety
/// `source` readable for 16 bytes, `destination` writable for 12, or 16 with `spare`.
#[inline(always)]
unsafe fn decode_block(
    tables: &Tables,
    source: *const u8,
    destination: *mut u8,
    spare: bool,
) -> v128 {
    // SAFETY: one 16 byte load, and stores of 16, or exactly 8 and 4.
    unsafe {
        let (packed, bad) = decode_vector(tables, v128_load(source.cast()));
        if spare {
            v128_store(destination.cast(), packed);
        } else {
            v128_store64_lane::<0>(packed, destination.cast());
            v128_store32_lane::<2>(packed, destination.add(8).cast());
        }
        bad
    }
}

/// Turn 16 characters into 12 bytes at the bottom of the register, and the foreign ones into a mask
#[inline(always)]
fn decode_vector(tables: &Tables, chars: v128) -> (v128, v128) {
    let high_nibble = v128_and(u32x4_shr(chars, 4), u8x16_splat(0x0F));
    let low_nibble = v128_and(chars, u8x16_splat(0x0F));
    // A character is in the alphabet exactly when its two nibbles' rows share no bit.
    let bad = v128_and(
        u8x16_swizzle(tables.high, high_nibble),
        u8x16_swizzle(tables.low, low_nibble),
    );
    let rolled = i8x16_add(chars, u8x16_swizzle(tables.roll, high_nibble));
    let values = v128_bitselect(u8x16_splat(ODD_VALUE), rolled, i8x16_eq(chars, tables.odd));
    // Two values to a 16 bit lane, then two lanes to a 24 bit value, then its 3 bytes gathered.
    let paired = v128_or(
        i16x8_shl(v128_and(values, u16x8_splat(0x00FF)), 6),
        u16x8_shr(values, 8),
    );
    let merged = i32x4_dot_i16x8(paired, i32x4_splat(0x0001_1000));
    let packed =
        i8x16_shuffle::<2, 1, 0, 6, 5, 4, 10, 9, 8, 14, 13, 12, 12, 13, 14, 15>(merged, merged);
    (packed, bad)
}
