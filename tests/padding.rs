//! Padding, the lengths it makes possible, and the bits a short group cannot hold
//!
//! A short group holds more bits than its bytes have room for, so every one of the
//! sixty-four values the last character can take is put through both short groups.

mod common;

use base64::Engine as _;

use common::{Compared, CODECS};
use tape_base64::DecodeError;

/// An encoding of `groups` whole groups of zero, to put a short group after a kernel block
fn prefix(entry: &Compared, groups: usize) -> Vec<u8> {
    vec![entry.alphabet[0]; groups * 4]
}

/// The characters a codec ends an encoding with, given a short group
fn padding(entry: &Compared, count: usize) -> Vec<u8> {
    if entry.is_padded {
        vec![b'='; count]
    } else {
        Vec::new()
    }
}

/// Short groups hold four spare bits or two, and all of them have to be zero
#[test]
fn every_trailing_bit_pattern_is_judged_like_the_reference() {
    for groups in [0usize, 1, 4, 8, 16, 17] {
        for entry in &CODECS {
            let (name, reference) = (entry.name, &entry.reference);
            let head = prefix(entry, groups);

            for value in 0..64usize {
                let last = entry.alphabet[value];

                let mut two = head.clone();
                two.extend([entry.alphabet[0], last]);
                two.extend(padding(entry, 2));
                let outcome = common::decode_to_vec(entry.codec, &two);
                if value & 0x0F == 0 {
                    let mine = outcome.expect("the spare bits are clear");
                    let theirs = reference.decode(&two).expect("the reference agrees");
                    assert_eq!(mine, theirs, "{name} groups {groups} value {value}");
                } else {
                    assert_eq!(
                        outcome.unwrap_err(),
                        DecodeError::InvalidLastSymbol {
                            position: head.len() + 1,
                            byte: last
                        },
                        "{name} groups {groups} value {value}"
                    );
                    assert!(reference.decode(&two).is_err());
                }

                let mut three = head.clone();
                three.extend([entry.alphabet[0], entry.alphabet[0], last]);
                three.extend(padding(entry, 1));
                let outcome = common::decode_to_vec(entry.codec, &three);
                if value & 0x03 == 0 {
                    let mine = outcome.expect("the spare bits are clear");
                    let theirs = entry
                        .reference
                        .decode(&three)
                        .expect("the reference agrees");
                    assert_eq!(mine, theirs, "{name} groups {groups} value {value}");
                } else {
                    assert_eq!(
                        outcome.unwrap_err(),
                        DecodeError::InvalidLastSymbol {
                            position: head.len() + 2,
                            byte: last
                        },
                        "{name} groups {groups} value {value}"
                    );
                    assert!(reference.decode(&three).is_err());
                }
            }
        }
    }
}

/// Padding taken off, doubled, or written where an encoder never puts it
#[test]
fn the_padding_matrix() {
    for entry in &CODECS {
        let name = entry.name;
        for length in 1..=9usize {
            let input = vec![0x5Au8; length];
            let encoded = common::encode_to_vec(entry.codec, &input);
            let payload = common::payload_len(&encoded);

            assert_eq!(
                common::decode_to_vec(entry.codec, &encoded).expect("our own encoding"),
                input,
                "{name} length {length}"
            );

            // Padding taken off a codec that requires it
            if entry.is_padded && payload < encoded.len() {
                assert_eq!(
                    common::decode_to_vec(entry.codec, &encoded[..payload]).unwrap_err(),
                    DecodeError::InvalidLength { length: payload },
                    "{name} accepted length {length} unpadded"
                );
            }

            // Padding written by a codec that refuses it
            if !entry.is_padded && length % 3 != 0 {
                let mut padded = encoded.clone();
                padded.extend(vec![b'='; 4 - encoded.len() % 4]);
                assert_eq!(
                    common::decode_to_vec(entry.codec, &padded).unwrap_err(),
                    DecodeError::InvalidByte {
                        position: encoded.len(),
                        byte: b'='
                    },
                    "{name} accepted padding at length {length}"
                );
            }

            // One character of padding past what the encoding needs
            let mut extra = encoded.clone();
            extra.push(b'=');
            assert!(
                common::decode_to_vec(entry.codec, &extra).is_err(),
                "{name} accepted an extra pad at length {length}"
            );

            // A whole group of padding, three pad characters however the length falls
            let mut group = encoded.clone();
            group.extend_from_slice(b"====");
            assert!(
                common::decode_to_vec(entry.codec, &group).is_err(),
                "{name} accepted a group of padding at length {length}"
            );
        }
    }
}

/// Two encodings joined put padding in the middle, which no decode may take
#[test]
fn padding_in_the_middle_is_named_where_it_sits() {
    for entry in &CODECS {
        if !entry.is_padded {
            continue;
        }
        for length in [1usize, 2, 4, 5, 7, 8] {
            let input = vec![0xA3u8; length];
            let encoded = common::encode_to_vec(entry.codec, &input);
            let payload = common::payload_len(&encoded);
            if payload == encoded.len() {
                continue;
            }

            let mut joined = encoded.clone();
            joined.extend_from_slice(&encoded);
            assert_eq!(
                common::decode_to_vec(entry.codec, &joined).unwrap_err(),
                DecodeError::InvalidByte {
                    position: payload,
                    byte: b'='
                },
                "{} length {length}",
                entry.name
            );
            assert!(entry.reference.decode(&joined).is_err());
        }
    }
}

/// A pad character written over every offset in turn
#[test]
fn a_pad_at_every_position_matches_the_reference() {
    let mut lengths: Vec<usize> = (0..=48).collect();
    lengths.extend([96, 128, 192]);

    for length in lengths {
        let input = vec![0x3Cu8; length];
        for entry in &CODECS {
            let encoded = common::encode_to_vec(entry.codec, &input);
            for position in 0..encoded.len() {
                let mut corrupted = encoded.clone();
                corrupted[position] = b'=';

                let mine = common::decode_to_vec(entry.codec, &corrupted);
                let theirs = entry.reference.decode(&corrupted);
                match (mine, theirs) {
                    (Ok(mine), Ok(theirs)) => assert_eq!(
                        mine, theirs,
                        "{} length {length} offset {position}",
                        entry.name
                    ),
                    (Err(_), Err(_)) => {}
                    (mine, theirs) => panic!(
                        "{} disagreed at length {length} offset {position}: \
                         {mine:?} against {theirs:?}",
                        entry.name
                    ),
                }
            }
        }
    }
}

/// Which character counts an encoding can have, decided by the length alone
#[test]
fn every_character_count_is_accepted_or_refused_like_the_reference() {
    for length in 0..=260usize {
        for entry in &CODECS {
            let name = entry.name;
            // Every character is the value zero, so only the length and the padding rule decide
            let encoded = vec![entry.alphabet[0]; length];

            let is_valid_length = if entry.is_padded {
                length % 4 == 0
            } else {
                length % 4 != 1
            };
            let mine = common::decode_to_vec(entry.codec, &encoded);
            assert_eq!(mine.is_ok(), is_valid_length, "{name} length {length}");

            match mine {
                Ok(decoded) => {
                    let theirs = entry
                        .reference
                        .decode(&encoded)
                        .expect("the reference agrees");
                    assert_eq!(decoded, theirs, "{name} length {length}");
                }
                Err(error) => {
                    assert_eq!(error, DecodeError::InvalidLength { length });
                    assert!(entry.reference.decode(&encoded).is_err());
                }
            }
        }
    }
}
