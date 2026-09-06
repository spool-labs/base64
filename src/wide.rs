//! The tables the AVX-512 kernels index by, and a model of what indexes them

use crate::alphabet::INVALID;

/// Gather 6 input bytes into the top of each 64-bit lane, most significant
/// first, which is the order the bit stream runs in
pub(crate) const fn expand() -> [i8; 64] {
    let mut table = [0i8; 64];
    let mut lane = 0;
    while lane < 8 {
        let mut slot = 2;
        while slot < 8 {
            table[lane * 8 + slot] = (lane * 6 + 7 - slot) as i8;
            slot += 1;
        }
        lane += 1;
    }
    table
}

/// The bottom bit of each of the 8 six-bit fields of a lane
pub(crate) const fn field_starts() -> [i8; 64] {
    let mut table = [0i8; 64];
    let mut lane = 0;
    while lane < 8 {
        let mut field = 0;
        while field < 8 {
            table[lane * 8 + field] = (58 - 6 * field) as i8;
            field += 1;
        }
        lane += 1;
    }
    table
}

/// Take 3 bytes out of every 4, the arrangement the packing leaves
pub(crate) const fn gather_bytes() -> [i8; 64] {
    let mut table = [0i8; 64];
    let mut byte = 0;
    while byte < 48 {
        table[byte] = (byte / 3 * 4 + 2 - byte % 3) as i8;
        byte += 1;
    }
    table
}

/// Every byte below 128 and what it is worth, in the half `vpermi2b` reads
///
/// A byte with no value keeps its top bit set, which is what the kernel tests.
pub(crate) const fn ascii_values(chars: &[u8; 64], half: usize) -> [i8; 64] {
    let mut table = [INVALID as i8; 64];
    let mut value = 0;
    while value < 64 {
        let character = chars[value] as usize;
        if character >= half * 64 && character < half * 64 + 64 {
            table[character - half * 64] = value as i8;
        }
        value += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::{Alphabet, Standard, UrlSafe};

    // `vpermb`: each result byte is the `data` byte its index's low 6 bits name
    fn permute(index: &[i8; 64], data: &[u8; 64]) -> [u8; 64] {
        let mut out = [0u8; 64];
        for (slot, &which) in index.iter().enumerate() {
            out[slot] = data[(which as u8 & 63) as usize];
        }
        out
    }

    // `vpermi2b`: the index reaches across both registers, 7 bits, 128 bytes
    fn permute_pair(low: &[i8; 64], index: &[u8; 64], high: &[i8; 64]) -> [u8; 64] {
        let mut out = [0u8; 64];
        for (slot, &which) in index.iter().enumerate() {
            let which = (which & 127) as usize;
            out[slot] = if which < 64 {
                low[which] as u8
            } else {
                high[which - 64] as u8
            };
        }
        out
    }

    // `vpmultishiftqb`: each result byte is 8 bits of its lane, wrapping
    fn multishift(control: &[i8; 64], data: &[u8; 64]) -> [u8; 64] {
        let mut out = [0u8; 64];
        for lane in 0..8 {
            let mut word = 0u64;
            for byte in 0..8 {
                word |= (data[lane * 8 + byte] as u64) << (8 * byte);
            }
            for byte in 0..8 {
                let start = (control[lane * 8 + byte] as u8 & 63) as u32;
                out[lane * 8 + byte] = word.rotate_right(start) as u8;
            }
        }
        out
    }

    fn encode_block<A: Alphabet>(input: &[u8; 48]) -> [u8; 64] {
        let mut data = [0u8; 64];
        data[..48].copy_from_slice(input);
        let spread = permute(&expand(), &data);
        let fields = multishift(&field_starts(), &spread);
        let mut alphabet = [0u8; 64];
        alphabet.copy_from_slice(A::ENCODE);
        permute(&fields.map(|field| field as i8), &alphabet)
    }

    fn decode_block<A: Alphabet>(chars: &[u8; 64]) -> ([u8; 48], bool) {
        let low = ascii_values(A::ENCODE, 0);
        let high = ascii_values(A::ENCODE, 1);
        let values = permute_pair(&low, chars, &high);
        let has_outsider = chars.iter().any(|&c| c >= 0x80) || values.iter().any(|&v| v >= 0x80);

        // `vpmaddubsw` then `vpmaddwd`, which leave a lane holding its three
        // bytes in the order 2, 1, 0
        let mut merged = [0u8; 64];
        for lane in 0..16 {
            let value = |slot: usize| values[lane * 4 + slot] as u32;
            let word = (value(0) << 18) | (value(1) << 12) | (value(2) << 6) | value(3);
            merged[lane * 4] = word as u8;
            merged[lane * 4 + 1] = (word >> 8) as u8;
            merged[lane * 4 + 2] = (word >> 16) as u8;
        }
        let packed = permute(&gather_bytes(), &merged);
        let mut out = [0u8; 48];
        out.copy_from_slice(&packed[..48]);
        (out, has_outsider)
    }

    fn round_trips<A: Alphabet>() {
        let mut input = [0u8; 48];
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..2_000 {
            for slot in input.iter_mut() {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                *slot = state.wrapping_mul(0x2545_F491_4F6C_DD1D) as u8;
            }

            let spelled = encode_block::<A>(&input);
            let mut expected = [0u8; 64];
            crate::scalar::encode_bulk::<A>(&input, &mut expected, 16);
            assert_eq!(
                spelled, expected,
                "the expansion or the field starts are wrong"
            );

            let (bytes, has_outsider) = decode_block::<A>(&spelled);
            assert!(!has_outsider, "its own characters were called invalid");
            assert_eq!(bytes, input, "the value table or the gather is wrong");
        }
    }

    // a block encodes and decodes back to itself, standard alphabet
    #[test]
    fn standard_block() {
        round_trips::<Standard>();
    }

    // the same, for the url-safe alphabet
    #[test]
    fn url_safe_block() {
        round_trips::<UrlSafe>();
    }

    // every byte outside the alphabet leaves its top bit set
    #[test]
    fn invalid_marked() {
        for byte in 0..=u8::MAX {
            let expected = Standard::DECODE[byte as usize];
            let found = if byte < 64 {
                ascii_values(Standard::ENCODE, 0)[byte as usize] as u8
            } else if byte < 128 {
                ascii_values(Standard::ENCODE, 1)[byte as usize - 64] as u8
            } else {
                // Caught by its own top bit before it reaches the table
                continue;
            };
            assert_eq!(found, expected, "byte {byte:#04x}");
            assert_eq!(found >= 0x80, expected == INVALID);
        }
    }
}
