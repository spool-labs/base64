//! The two alphabets, and the tables each path reads them through

#[cfg(target_arch = "x86_64")]
use crate::wide::ascii_values;

/// The characters and the tables each path reads them through
#[cfg_attr(
    not(any(
        all(target_arch = "aarch64", target_feature = "neon"),
        all(target_arch = "wasm32", target_feature = "simd128"),
        target_arch = "x86_64"
    )),
    allow(dead_code)
)]
pub(crate) trait Alphabet {
    /// The 64 characters, in value order
    const ENCODE: &'static [u8; 64];

    /// Value of every byte, or `INVALID` for the ones that have none
    const DECODE: &'static [u8; 256];

    /// Rows of the ASCII table a low nibble is *not* a character in
    const LUT_LO: &'static [u8; 16];

    /// The bit that stands for each row of the ASCII table
    const LUT_HI: &'static [u8; 16];

    /// What to add to a character in each row to reach its value
    const LUT_ROLL: &'static [u8; 16];

    /// The character `LUT_ROLL` cannot place, its row needing two offsets
    const ODD_CHAR: u8;

    /// Every byte below 128 and what it is worth, low half
    #[cfg(target_arch = "x86_64")]
    const ASCII_LOW: &'static [i8; 64];

    /// Every byte below 128 and what it is worth, high half
    #[cfg(target_arch = "x86_64")]
    const ASCII_HIGH: &'static [i8; 64];

    /// What to add to a value to spell it, by which run it falls in
    #[cfg(any(
        target_arch = "x86_64",
        all(target_arch = "wasm32", target_feature = "simd128")
    ))]
    const SPELLING: &'static [i8; 16];
}

/// What to add to a value to spell it, by which run it falls in
#[cfg(any(
    target_arch = "x86_64",
    all(target_arch = "wasm32", target_feature = "simd128")
))]
const fn spelling(chars: &[u8; 64]) -> [i8; 16] {
    let mut table = [0i8; 16];
    table[0] = chars[0] as i8;
    table[1] = chars[26] as i8 - 26;
    let mut digit = 0;
    while digit < 10 {
        table[2 + digit] = chars[52 + digit] as i8 - (52 + digit) as i8;
        digit += 1;
    }
    table[12] = chars[62] as i8 - 62;
    table[13] = chars[63] as i8 - 63;
    table
}

/// The value `Alphabet::DECODE` holds for a byte outside the alphabet
pub(crate) const INVALID: u8 = 0xFF;

/// The value `Alphabet::ODD_CHAR` stands for, in both alphabets
#[cfg_attr(
    not(any(
        all(target_arch = "aarch64", target_feature = "neon"),
        all(target_arch = "wasm32", target_feature = "simd128"),
        target_arch = "x86_64"
    )),
    allow(dead_code)
)]
pub(crate) const ODD_VALUE: u8 = 63;

/// RFC 4648 section 4, the alphabet ending `+/`
pub(crate) struct Standard;

/// RFC 4648 section 5, the alphabet ending `-_`, safe in a URL or a path
pub(crate) struct UrlSafe;

const STANDARD_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const URL_SAFE_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

const fn invert(chars: &[u8; 64]) -> [u8; 256] {
    let mut table = [INVALID; 256];
    let mut value = 0;
    while value < 64 {
        table[chars[value] as usize] = value as u8;
        value += 1;
    }
    table
}

static STANDARD_DECODE: [u8; 256] = invert(STANDARD_CHARS);
static URL_SAFE_DECODE: [u8; 256] = invert(URL_SAFE_CHARS);

#[cfg(target_arch = "x86_64")]
static STANDARD_ASCII_LOW: [i8; 64] = ascii_values(STANDARD_CHARS, 0);
#[cfg(target_arch = "x86_64")]
static STANDARD_ASCII_HIGH: [i8; 64] = ascii_values(STANDARD_CHARS, 1);
#[cfg(target_arch = "x86_64")]
static URL_SAFE_ASCII_LOW: [i8; 64] = ascii_values(URL_SAFE_CHARS, 0);
#[cfg(target_arch = "x86_64")]
static URL_SAFE_ASCII_HIGH: [i8; 64] = ascii_values(URL_SAFE_CHARS, 1);
#[cfg(any(
    target_arch = "x86_64",
    all(target_arch = "wasm32", target_feature = "simd128")
))]
static STANDARD_SPELLING: [i8; 16] = spelling(STANDARD_CHARS);
#[cfg(any(
    target_arch = "x86_64",
    all(target_arch = "wasm32", target_feature = "simd128")
))]
static URL_SAFE_SPELLING: [i8; 16] = spelling(URL_SAFE_CHARS);

impl Alphabet for Standard {
    const ENCODE: &'static [u8; 64] = STANDARD_CHARS;
    const DECODE: &'static [u8; 256] = &STANDARD_DECODE;

    // Rows 2 through 7 hold every character, and bit 4 marks the rows that hold none.
    const LUT_LO: &'static [u8; 16] = &[
        0x15, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, //
        0x11, 0x11, 0x13, 0x1A, 0x1B, 0x1B, 0x1B, 0x1A,
    ];
    const LUT_HI: &'static [u8; 16] = &[
        0x10, 0x10, 0x01, 0x02, 0x04, 0x08, 0x04, 0x08, //
        0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10,
    ];
    // `+` at 43 wants 62, so row 2 adds 19, and `/` in the same row is odd.
    const LUT_ROLL: &'static [u8; 16] = &[
        0, 0, 19, 4, 0xBF, 0xBF, 0xB9, 0xB9, //
        0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const ODD_CHAR: u8 = b'/';

    #[cfg(target_arch = "x86_64")]
    const ASCII_LOW: &'static [i8; 64] = &STANDARD_ASCII_LOW;
    #[cfg(target_arch = "x86_64")]
    const ASCII_HIGH: &'static [i8; 64] = &STANDARD_ASCII_HIGH;
    #[cfg(any(
        target_arch = "x86_64",
        all(target_arch = "wasm32", target_feature = "simd128")
    ))]
    const SPELLING: &'static [i8; 16] = &STANDARD_SPELLING;
}

impl Alphabet for UrlSafe {
    const ENCODE: &'static [u8; 64] = URL_SAFE_CHARS;
    const DECODE: &'static [u8; 256] = &URL_SAFE_DECODE;

    // Row 5 now holds `_` too, so it can no longer share a bit with row 7.
    const LUT_LO: &'static [u8; 16] = &[
        0x15, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, //
        0x11, 0x11, 0x13, 0x3B, 0x3B, 0x3A, 0x3B, 0x33,
    ];
    const LUT_HI: &'static [u8; 16] = &[
        0x10, 0x10, 0x01, 0x02, 0x04, 0x08, 0x04, 0x20, //
        0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10,
    ];
    // `-` at 45 wants 62, so row 2 adds 17, and `_` is the odd one out of row 5.
    const LUT_ROLL: &'static [u8; 16] = &[
        0, 0, 17, 4, 0xBF, 0xBF, 0xB9, 0xB9, //
        0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const ODD_CHAR: u8 = b'_';

    #[cfg(target_arch = "x86_64")]
    const ASCII_LOW: &'static [i8; 64] = &URL_SAFE_ASCII_LOW;
    #[cfg(target_arch = "x86_64")]
    const ASCII_HIGH: &'static [i8; 64] = &URL_SAFE_ASCII_HIGH;
    #[cfg(any(
        target_arch = "x86_64",
        all(target_arch = "wasm32", target_feature = "simd128")
    ))]
    const SPELLING: &'static [i8; 16] = &URL_SAFE_SPELLING;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nibble_decode<A: Alphabet>(byte: u8) -> Option<u8> {
        let high = (byte >> 4) as usize;
        let low = (byte & 0x0F) as usize;
        if A::LUT_HI[high] & A::LUT_LO[low] != 0 {
            return None;
        }
        if byte == A::ODD_CHAR {
            return Some(ODD_VALUE);
        }
        Some(byte.wrapping_add(A::LUT_ROLL[high]))
    }

    fn tables_agree<A: Alphabet>() {
        for byte in 0..=u8::MAX {
            let table = A::DECODE[byte as usize];
            let nibbles = nibble_decode::<A>(byte);
            match nibbles {
                None => assert_eq!(
                    table, INVALID,
                    "byte {byte:#04x} accepted by one, not the other"
                ),
                Some(value) => assert_eq!(table, value, "byte {byte:#04x} decoded two ways"),
            }
        }
    }

    fn is_a_bijection<A: Alphabet>() {
        let mut is_spelled = [false; 64];
        for (value, &byte) in A::ENCODE.iter().enumerate() {
            assert_eq!(A::DECODE[byte as usize], value as u8);
            assert!(!is_spelled[value], "value {value} spelled twice");
            is_spelled[value] = true;
        }
        assert!(is_spelled.iter().all(|&hit| hit));
        assert_eq!(A::DECODE[A::ODD_CHAR as usize], ODD_VALUE);
    }

    // the nibble tables and the 256-entry table answer alike for every byte
    #[test]
    fn standard_tables() {
        tables_agree::<Standard>();
    }

    #[test]
    fn url_safe_tables() {
        tables_agree::<UrlSafe>();
    }

    // every character decodes to its own value, and every value has one
    #[test]
    fn standard_bijection() {
        is_a_bijection::<Standard>();
    }

    #[test]
    fn url_safe_bijection() {
        is_a_bijection::<UrlSafe>();
    }

    // padding is not a character, in either alphabet
    #[test]
    fn padding_refused() {
        assert_eq!(Standard::DECODE[b'=' as usize], INVALID);
        assert_eq!(UrlSafe::DECODE[b'=' as usize], INVALID);
        assert!(nibble_decode::<Standard>(b'=').is_none());
        assert!(nibble_decode::<UrlSafe>(b'=').is_none());
    }
}
