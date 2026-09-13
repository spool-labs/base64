//! Where a decode says the trouble is, at every offset it could be
//!
//! The offset a refusal reports is put together from a block count and a walk inside it,
//! so every offset of every length is corrupted in turn and has to be reported exactly.

mod common;

use base64::Engine as _;

use common::{Rng, CODECS};
use tape_base64::{DecodeError, STANDARD_NO_PAD};

/// Lengths that put every block size against every remainder, some long enough to loop
fn lengths() -> Vec<usize> {
    let mut lengths: Vec<usize> = (0..=96).collect();
    lengths.extend([127, 128, 129, 191, 192, 193, 255, 256, 257, 383, 384, 385]);
    lengths
}

/// Hold one decode against the reference, on value as well as validity
fn agrees_with_reference(entry: &common::Compared, encoded: &[u8]) {
    let mine = common::decode_to_vec(entry.codec, encoded);
    let theirs = entry.reference.decode(encoded);
    if let Ok(mine) = &mine {
        // What the length says before the characters are read has to be what reading them writes
        assert_eq!(entry.codec.decoded_len(encoded), Ok(mine.len()));
    }
    match (mine, theirs) {
        (Ok(mine), Ok(theirs)) => assert_eq!(
            mine,
            theirs,
            "{} read {} as different bytes",
            entry.name,
            String::from_utf8_lossy(encoded)
        ),
        (Err(_), Err(_)) => {}
        (mine, theirs) => panic!(
            "{} disagreed about {}: {mine:?} against {theirs:?}",
            entry.name,
            String::from_utf8_lossy(encoded)
        ),
    }
}

/// A character outside the alphabet is reported at the offset it sits at
#[test]
fn a_byte_outside_the_alphabet_is_reported_at_its_own_offset() {
    let mut rng = Rng::new(0x3F00_1D0C);
    let mut source = vec![0u8; 386];
    rng.fill(&mut source);

    for length in lengths() {
        for entry in &CODECS {
            let encoded = common::encode_to_vec(entry.codec, &source[..length]);
            let payload = common::payload_len(&encoded);

            for position in 0..encoded.len() {
                let mut corrupted = encoded.clone();
                corrupted[position] = b'*';

                let error = common::decode_to_vec(entry.codec, &corrupted).unwrap_err();
                if position < payload {
                    assert_eq!(
                        error,
                        DecodeError::InvalidByte {
                            position,
                            byte: b'*'
                        },
                        "{} length {length} offset {position}",
                        entry.name
                    );
                }
                assert!(entry.reference.decode(&corrupted).is_err());
            }
        }
    }
}

/// A character outside the alphabet is reported at its own offset after whole runs of blocks
#[test]
fn a_bad_byte_late_in_a_long_input_is_reported_at_its_own_offset() {
    let mut rng = Rng::new(0x2545_F491);
    let mut source = vec![0u8; 1536];
    rng.fill(&mut source);

    for length in [768usize, 1023, 1536] {
        for entry in &CODECS {
            let encoded = common::encode_to_vec(entry.codec, &source[..length]);
            let payload = common::payload_len(&encoded);

            for position in 0..payload {
                let mut corrupted = encoded.clone();
                corrupted[position] = b'*';
                assert_eq!(
                    common::decode_to_vec(entry.codec, &corrupted).unwrap_err(),
                    DecodeError::InvalidByte {
                        position,
                        byte: b'*'
                    },
                    "{} length {length} offset {position}",
                    entry.name
                );
            }
        }
    }
}

/// The offset reported is the first one that is wrong, not the one a block reaches first
#[test]
fn the_earliest_bad_byte_is_the_one_reported() {
    let mut rng = Rng::new(0x6A09_E667);
    let mut source = vec![0u8; 300];
    rng.fill(&mut source);

    let encoded = common::encode_to_vec(STANDARD_NO_PAD, &source);
    for first in [0usize, 1, 3, 15, 16, 31, 32, 47, 63, 64, 95, 191] {
        for second in [first + 1, first + 2, first + 5, first + 33, first + 64] {
            if second >= encoded.len() {
                continue;
            }
            let mut corrupted = encoded.clone();
            corrupted[first] = b'*';
            corrupted[second] = b'#';

            assert_eq!(
                common::decode_to_vec(STANDARD_NO_PAD, &corrupted).unwrap_err(),
                DecodeError::InvalidByte {
                    position: first,
                    byte: b'*'
                },
                "with a second bad byte at {second}"
            );
        }
    }
}

/// One character replaced by another, at every offset, on every edge of the classification
#[test]
fn a_character_swapped_in_matches_the_reference() {
    const REPLACEMENTS: [u8; 16] = [
        b'A', b'B', b'Z', b'a', b'z', b'0', b'9', b'+', b'/', b'-', b'_', b'=', b'*', b'\n', 0x00,
        0xFF,
    ];

    let mut rng = Rng::new(0x8B00_5AF3);
    let mut source = vec![0u8; 66];
    rng.fill(&mut source);

    let mut lengths: Vec<usize> = (0..=32).collect();
    lengths.extend([47, 48, 49, 63, 64, 65]);

    for length in lengths {
        for entry in &CODECS {
            let encoded = common::encode_to_vec(entry.codec, &source[..length]);
            for position in 0..encoded.len() {
                for replacement in REPLACEMENTS {
                    let mut corrupted = encoded.clone();
                    corrupted[position] = replacement;
                    agrees_with_reference(entry, &corrupted);
                }
            }
        }
    }
}

/// An encoding cut short at every offset is read, or refused, the same way
#[test]
fn every_truncation_matches_the_reference() {
    let mut rng = Rng::new(0xB7E1_5162);
    let mut source = vec![0u8; 96];
    rng.fill(&mut source);

    for length in 0..=96 {
        for entry in &CODECS {
            let encoded = common::encode_to_vec(entry.codec, &source[..length]);
            for kept in 0..=encoded.len() {
                agrees_with_reference(entry, &encoded[..kept]);
            }
        }
    }
}

/// A character outside the alphabet is refused either way, and a short buffer says so first
#[test]
fn a_buffer_check_comes_before_the_characters() {
    let encoded = b"Zm9vYmFy****";
    let mut room = [0u8; 4];
    assert_eq!(
        STANDARD_NO_PAD.decode(encoded, &mut room).unwrap_err(),
        DecodeError::BufferTooSmall { needed: 9 }
    );

    let mut room = [0u8; 9];
    assert_eq!(
        STANDARD_NO_PAD.decode(encoded, &mut room).unwrap_err(),
        DecodeError::InvalidByte {
            position: 8,
            byte: b'*'
        }
    );
}
