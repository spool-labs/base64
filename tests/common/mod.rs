//! A deterministic byte source, so a failing case is the same one next run

#![allow(dead_code)]

use base64::engine::general_purpose::{
    STANDARD as REFERENCE_STANDARD, STANDARD_NO_PAD as REFERENCE_STANDARD_NO_PAD,
    URL_SAFE as REFERENCE_URL_SAFE, URL_SAFE_NO_PAD as REFERENCE_URL_SAFE_NO_PAD,
};
use base64::engine::GeneralPurpose;

use tape_base64::{Codec, DecodeError, STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

/// xorshift64*, seeded away from zero
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for slot in buf.iter_mut() {
            *slot = self.next_u64() as u8;
        }
    }

    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    /// One of the given items
    pub fn pick(&mut self, items: &[u8]) -> u8 {
        items[self.below(items.len())]
    }
}

/// The characters of RFC 4648 section 4, in value order
pub const STANDARD_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The characters of RFC 4648 section 5, in value order
pub const URL_SAFE_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// A codec beside the reference engine that has to agree with it
pub struct Compared {
    pub name: &'static str,
    pub codec: Codec,
    pub reference: GeneralPurpose,

    pub alphabet: &'static [u8; 64],
    pub is_padded: bool,
}

/// The four codecs RFC 4648 describes, each against its reference engine
pub const CODECS: [Compared; 4] = [
    Compared {
        name: "standard",
        codec: STANDARD,
        reference: REFERENCE_STANDARD,
        alphabet: STANDARD_ALPHABET,
        is_padded: true,
    },
    Compared {
        name: "standard-no-pad",
        codec: STANDARD_NO_PAD,
        reference: REFERENCE_STANDARD_NO_PAD,
        alphabet: STANDARD_ALPHABET,
        is_padded: false,
    },
    Compared {
        name: "url-safe",
        codec: URL_SAFE,
        reference: REFERENCE_URL_SAFE,
        alphabet: URL_SAFE_ALPHABET,
        is_padded: true,
    },
    Compared {
        name: "url-safe-no-pad",
        codec: URL_SAFE_NO_PAD,
        reference: REFERENCE_URL_SAFE_NO_PAD,
        alphabet: URL_SAFE_ALPHABET,
        is_padded: false,
    },
];

/// Encode into a buffer of exactly the length the codec asks for
pub fn encode_to_vec(codec: Codec, input: &[u8]) -> Vec<u8> {
    let needed = codec.encoded_len(input.len()).expect("a length with an encoding");
    let mut encoded = vec![0u8; needed];
    let written = codec
        .encode(input, &mut encoded)
        .expect("an exactly sized buffer is enough");
    assert_eq!(written, needed, "encode wrote past what it asked for");
    encoded
}

/// Decode into a buffer sized from the character count alone
pub fn decode_to_vec(codec: Codec, encoded: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut decoded = vec![0u8; codec.decoded_len_max(encoded.len())];
    let written = codec.decode(encoded, &mut decoded)?;
    decoded.truncate(written);
    Ok(decoded)
}

/// Characters of an encoding that carry bits, padding left off
pub fn payload_len(encoded: &[u8]) -> usize {
    let mut len = encoded.len();
    while len > 0 && encoded[len - 1] == b'=' {
        len -= 1;
    }
    len
}
