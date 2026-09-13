//! The sixty-four characters, and the two hundred and fifty-six bytes around them
//!
//! A vector path classifies a character by its two nibbles rather than by a
//! table over every byte, so the bytes it lets through are the ones worth
//! checking one at a time: every value a byte can take, at every offset of a
//! block, against what the reference crate makes of it.

mod common;

use base64::Engine as _;

use common::{CODECS, STANDARD_ALPHABET};
use tape_base64::{STANDARD, URL_SAFE};

/// The bytes whose encoding spells all sixty-four characters in value order
fn every_value_in_order() -> Vec<u8> {
    let mut packed = Vec::with_capacity(48);
    for group in 0..16u32 {
        let first = group * 4;
        let value = (first << 18) | ((first + 1) << 12) | ((first + 2) << 6) | (first + 3);
        packed.extend([(value >> 16) as u8, (value >> 8) as u8, value as u8]);
    }
    packed
}

/// Whether a codec takes a character, from the two alphabets alone
fn is_in_alphabet(entry: &common::Compared, byte: u8) -> bool {
    entry.alphabet.contains(&byte)
}

/// Every byte, at every offset of a block, classified the way the reference does
#[test]
fn every_byte_is_accepted_exactly_where_the_reference_accepts_it() {
    for entry in &CODECS {
        let name = entry.name;
        let clean = vec![entry.alphabet[0]; 128];

        for position in 0..clean.len() {
            for byte in 0..=u8::MAX {
                let mut encoded = clean.clone();
                encoded[position] = byte;

                let mine = common::decode_to_vec(entry.codec, &encoded);
                let theirs = entry.reference.decode(&encoded);
                match (mine, theirs) {
                    (Ok(mine), Ok(theirs)) => {
                        // The one byte outside the alphabet an encoding may
                        // end with is the padding of a codec that has it
                        let is_takeable = is_in_alphabet(entry, byte)
                            || (entry.is_padded && byte == b'=' && position + 1 == clean.len());
                        assert!(is_takeable, "{name} took {byte:#04x} at {position}");
                        assert_eq!(mine, theirs, "{name} byte {byte:#04x} at {position}");
                    }
                    (Err(_), Err(_)) => {}
                    (mine, theirs) => panic!(
                        "{name} disagreed about {byte:#04x} at {position}: \
                         {mine:?} against {theirs:?}"
                    ),
                }
            }
        }
    }
}

/// Every value is spelled by its own character, and no two share one
#[test]
fn every_value_gets_its_own_character() {
    let packed = every_value_in_order();

    for entry in &CODECS {
        let encoded = common::encode_to_vec(entry.codec, &packed);
        assert_eq!(encoded, entry.alphabet.to_vec(), "{}", entry.name);

        let decoded = common::decode_to_vec(entry.codec, &encoded).expect("our own encoding");
        assert_eq!(decoded, packed, "{}", entry.name);
    }

    let mut seen = [false; 256];
    for &character in STANDARD_ALPHABET.iter() {
        assert!(!seen[character as usize], "{character:#04x} twice");
        seen[character as usize] = true;
    }
}

/// The two alphabets take the same bytes but for the two each ends with
#[test]
fn the_two_alphabets_differ_in_two_characters() {
    for byte in 0..=u8::MAX {
        let encoded = [b'A', b'A', b'A', byte];
        let mut room = [0u8; 3];
        let is_standard = STANDARD.decode(&encoded, &mut room).is_ok();
        let is_url_safe = URL_SAFE.decode(&encoded, &mut room).is_ok();

        match byte {
            b'+' | b'/' => assert!(is_standard && !is_url_safe, "{byte:#04x}"),
            b'-' | b'_' => assert!(!is_standard && is_url_safe, "{byte:#04x}"),
            _ => assert_eq!(is_standard, is_url_safe, "{byte:#04x}"),
        }
    }
}

/// The two spellings of an encoding differ at exactly the characters the
/// alphabets differ at, at every length
#[test]
fn the_two_spellings_differ_only_where_the_alphabets_do() {
    let mut rng = common::Rng::new(0xE1F0_C0DE);
    let mut source = vec![0u8; 512];
    rng.fill(&mut source);

    for length in 0..=512 {
        let input = &source[..length];
        let standard = common::encode_to_vec(STANDARD, input);
        let url_safe = common::encode_to_vec(URL_SAFE, input);
        assert_eq!(standard.len(), url_safe.len(), "length {length}");

        for (place, (&one, &other)) in standard.iter().zip(url_safe.iter()).enumerate() {
            let translated = match one {
                b'+' => b'-',
                b'/' => b'_',
                _ => one,
            };
            assert_eq!(
                translated, other,
                "length {length} character {place} spelled {one} and {other}"
            );
        }
    }
}

/// A codec never spells a character its own alphabet does not have
#[test]
fn nothing_outside_the_alphabet_is_ever_written() {
    let mut rng = common::Rng::new(0x0FF1_CE55);
    let mut source = vec![0u8; 600];

    for _ in 0..64 {
        rng.fill(&mut source);
        let length = rng.below(601);
        for entry in &CODECS {
            let encoded = common::encode_to_vec(entry.codec, &source[..length]);
            for &character in encoded.iter() {
                assert!(
                    entry.alphabet.contains(&character) || character == b'=',
                    "{} wrote {character:#04x}",
                    entry.name
                );
            }
        }
    }
}

/// The free functions convert with the standard codec and nothing else
#[test]
fn the_free_functions_are_the_standard_codec() {
    let mut rng = common::Rng::new(0x1CE0_B0BA);
    let mut source = vec![0u8; 300];

    for _ in 0..64 {
        rng.fill(&mut source);
        let length = rng.below(301);
        let input = &source[..length];

        assert_eq!(
            tape_base64::encoded_len(length),
            STANDARD.encoded_len(length)
        );

        let needed = tape_base64::encoded_len(length).expect("a length this small");
        let mut theirs = vec![0u8; needed];
        let written = tape_base64::encode(input, &mut theirs).expect("exactly enough");
        let ours = common::encode_to_vec(STANDARD, input);
        assert_eq!(theirs[..written], ours[..]);

        let mut back = vec![0u8; length];
        let read = tape_base64::decode(&theirs[..written], &mut back).expect("our own encoding");
        assert_eq!(&back[..read], input);
    }
}
