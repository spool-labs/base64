//! Held against the `base64` crate, over inputs nobody would think to write
//!
//! Encoding is checked character for character. Decoding is checked both
//! ways: that the two agree on what valid characters mean, and that they
//! agree on which inputs are valid at all, including inputs built to sit
//! just outside the alphabet, at the padding, or at the trailing bits.

mod common;

use base64::engine::general_purpose::{GeneralPurposeConfig, STANDARD, URL_SAFE};
use base64::engine::{DecodePaddingMode, GeneralPurpose};
use base64::Engine as _;
use common::Rng;

/// The `base64` crate's unpadded engines, spelled the way its own docs do
fn standard_no_pad() -> GeneralPurpose {
    GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_encode_padding(false)
            .with_decode_padding_mode(DecodePaddingMode::RequireNone),
    )
}

/// Encoding matches, at every length and in both alphabets
#[test]
fn encoding_matches_the_reference() {
    let mut rng = Rng::new(0xA11CE);
    let mut input = [0u8; 600];
    for _ in 0..200 {
        rng.fill(&mut input);
        let len = rng.below(601);
        let input = &input[..len];

        let mut ours = [0u8; 808];
        let written = tape_base64::STANDARD.encode(input, &mut ours).unwrap();
        assert_eq!(
            core::str::from_utf8(&ours[..written]).unwrap(),
            STANDARD.encode(input),
            "standard, length {len}"
        );

        let written = tape_base64::URL_SAFE.encode(input, &mut ours).unwrap();
        assert_eq!(
            core::str::from_utf8(&ours[..written]).unwrap(),
            URL_SAFE.encode(input),
            "url-safe, length {len}"
        );

        let written = tape_base64::STANDARD_NO_PAD.encode(input, &mut ours).unwrap();
        assert_eq!(
            core::str::from_utf8(&ours[..written]).unwrap(),
            standard_no_pad().encode(input),
            "standard unpadded, length {len}"
        );
    }
}

/// Decoding matches on characters an encoder produced
#[test]
fn decoding_matches_the_reference() {
    let mut rng = Rng::new(0xBEEF);
    let mut input = [0u8; 600];
    for _ in 0..200 {
        rng.fill(&mut input);
        let len = rng.below(601);
        let encoded = STANDARD.encode(&input[..len]);

        let mut ours = [0u8; 600];
        let written = tape_base64::STANDARD
            .decode(encoded.as_bytes(), &mut ours)
            .unwrap();
        assert_eq!(&ours[..written], &input[..len]);
        assert_eq!(&ours[..written], &STANDARD.decode(&encoded).unwrap()[..]);
    }
}

/// The two agree on which inputs are valid, over characters chosen to land
/// on every boundary the classification has
#[test]
fn validity_matches_the_reference() {
    // Characters at the edges of the alphabet's rows, the padding, and a few
    // bytes that are in no row at all
    const INTERESTING: [u8; 24] = [
        b'A', b'Z', b'a', b'z', b'0', b'9', b'+', b'/', b'=', b'-', b'_', b'.', b',', b':', b';',
        b'@', b'[', b'`', b'{', 0x00, 0x1F, 0x7F, 0x80, 0xFF,
    ];

    let mut rng = Rng::new(0x0DDBA11);
    for _ in 0..20_000 {
        let len = rng.below(80);
        let mut encoded = [0u8; 80];
        for slot in encoded[..len].iter_mut() {
            *slot = INTERESTING[rng.below(INTERESTING.len())];
        }
        let encoded = &encoded[..len];

        let mut ours = [0u8; 80];
        let mine = tape_base64::STANDARD.decode(encoded, &mut ours);
        let theirs = STANDARD.decode(encoded);

        match (mine, theirs) {
            (Ok(written), Ok(expected)) => {
                assert_eq!(&ours[..written], &expected[..], "input {encoded:?}");
            }
            (Err(_), Err(_)) => {}
            (mine, theirs) => panic!("disagreed about {encoded:?}: {mine:?} against {theirs:?}"),
        }
    }
}

/// One byte of an encoding changed to something else, at every offset: the
/// two have to agree about whether the result still decodes, and to what
#[test]
fn a_single_changed_character_matches_the_reference() {
    let mut rng = Rng::new(0xF00D);
    let mut input = [0u8; 200];
    for _ in 0..400 {
        rng.fill(&mut input);
        let len = rng.below(201);
        let mut encoded = STANDARD.encode(&input[..len]).into_bytes();
        if encoded.is_empty() {
            continue;
        }
        let position = rng.below(encoded.len());
        let original = encoded[position];
        encoded[position] = rng.next_u64() as u8;
        if encoded[position] == original {
            continue;
        }

        let mut ours = [0u8; 200];
        let mine = tape_base64::STANDARD.decode(&encoded, &mut ours);
        let theirs = STANDARD.decode(&encoded);
        match (mine, theirs) {
            (Ok(written), Ok(expected)) => assert_eq!(&ours[..written], &expected[..]),
            (Err(_), Err(_)) => {}
            (mine, theirs) => panic!(
                "disagreed about byte {position} of {}: {mine:?} against {theirs:?}",
                String::from_utf8_lossy(&encoded)
            ),
        }
    }
}
