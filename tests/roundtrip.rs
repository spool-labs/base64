//! What goes in comes back out, at every length the block sizes care about

mod common;

use common::Rng;
use tape_base64::{Codec, STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

const CODECS: [(&str, Codec); 4] = [
    ("standard", STANDARD),
    ("standard-no-pad", STANDARD_NO_PAD),
    ("url-safe", URL_SAFE),
    ("url-safe-no-pad", URL_SAFE_NO_PAD),
];

fn roundtrip(codec: Codec, name: &str, input: &[u8]) {
    let mut encoded = [0u8; 1024];
    let written = codec.encode(input, &mut encoded).unwrap();
    assert_eq!(written, codec.encoded_len(input.len()).unwrap(), "{name}");

    let encoded = &encoded[..written];
    assert_eq!(codec.decoded_len(encoded).unwrap(), input.len(), "{name}");

    let mut decoded = [0u8; 768];
    let back = codec.decode(encoded, &mut decoded).unwrap();
    assert_eq!(back, input.len(), "{name} length {}", input.len());
    assert_eq!(&decoded[..back], input, "{name} length {}", input.len());
}

/// Every length from nothing through five blocks of both kernels
#[test]
fn every_length_up_to_five_blocks() {
    let mut rng = Rng::new(0x5EED);
    let mut input = [0u8; 400];
    rng.fill(&mut input);
    for len in 0..=320 {
        for (name, codec) in CODECS {
            roundtrip(codec, name, &input[..len]);
        }
    }
}

/// The same lengths, but with bytes that spell the ends of the alphabet
#[test]
fn extreme_bytes() {
    for filler in [0x00u8, 0xFF, 0xAA, 0x55, 0xFC, 0x03] {
        let input = [filler; 200];
        for len in 0..=200 {
            for (name, codec) in CODECS {
                roundtrip(codec, name, &input[..len]);
            }
        }
    }
}

/// Longer inputs, where the vector stage does nearly all of the work
#[test]
fn long_inputs() {
    let mut rng = Rng::new(0xC0FFEE);
    let mut input = [0u8; 700];
    for _ in 0..64 {
        rng.fill(&mut input);
        let len = 320 + rng.below(380);
        for (name, codec) in CODECS {
            roundtrip(codec, name, &input[..len]);
        }
    }
}

/// RFC 4648 section 10
#[test]
fn rfc_test_vectors() {
    const VECTORS: [(&[u8], &str); 7] = [
        (b"", ""),
        (b"f", "Zg=="),
        (b"fo", "Zm8="),
        (b"foo", "Zm9v"),
        (b"foob", "Zm9vYg=="),
        (b"fooba", "Zm9vYmE="),
        (b"foobar", "Zm9vYmFy"),
    ];
    for (input, expected) in VECTORS {
        let mut encoded = [0u8; 16];
        let written = STANDARD.encode(input, &mut encoded).unwrap();
        assert_eq!(core::str::from_utf8(&encoded[..written]).unwrap(), expected);

        let mut decoded = [0u8; 16];
        let back = STANDARD.decode(expected.as_bytes(), &mut decoded).unwrap();
        assert_eq!(&decoded[..back], input);
    }
}

/// The alphabets differ only in the last two characters, so each refuses the other's spelling
#[test]
fn alphabets_differ_where_they_should() {
    let input = [0xFBu8, 0xFF, 0xBF];
    let mut standard = [0u8; 4];
    let mut url_safe = [0u8; 4];
    STANDARD.encode(&input, &mut standard).unwrap();
    URL_SAFE.encode(&input, &mut url_safe).unwrap();
    assert_eq!(&standard, b"+/+/");
    assert_eq!(&url_safe, b"-_-_");

    let mut out = [0u8; 8];
    assert!(URL_SAFE.decode(&standard, &mut out).is_err());
    assert!(STANDARD.decode(&url_safe, &mut out).is_err());
}

/// A buffer with room to spare keeps whatever was past the encoding
#[test]
fn writes_only_what_it_returns() {
    let input = [1u8, 2, 3, 4, 5];
    let mut out = [0xCCu8; 32];
    let written = STANDARD.encode(&input, &mut out).unwrap();
    assert_eq!(written, 8);
    assert!(out[written..].iter().all(|&byte| byte == 0xCC));

    let mut decoded = [0xCCu8; 32];
    let back = STANDARD.decode(&out[..written], &mut decoded).unwrap();
    assert_eq!(back, 5);
    assert!(decoded[back..].iter().all(|&byte| byte == 0xCC));
}

/// A source off a word boundary, which the aarch64 short block reads at `source + 8`
#[test]
fn sources_at_every_offset() {
    #[repr(align(16))]
    struct Aligned([u8; 160]);

    let mut room = Aligned([0u8; 160]);
    Rng::new(0xA11B_0FF5).fill(&mut room.0);
    for offset in 0..8 {
        for len in 0..=96 {
            for (name, codec) in CODECS {
                roundtrip(codec, name, &room.0[offset..offset + len]);
            }
        }
    }
}
