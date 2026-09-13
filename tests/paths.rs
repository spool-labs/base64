//! The vector stage and the portable walk have to agree, byte for byte

#![cfg(feature = "internals")]

mod common;

use common::Rng;
use tape_base64::testing;

/// Say which path the tests below are exercising, since without a vector one they prove nothing
#[test]
fn the_vector_path_is_the_one_being_tested() {
    if cfg!(target_arch = "aarch64") {
        assert!(
            testing::has_vector_path(),
            "aarch64 always has NEON, so something is wrong with the wiring"
        );
    } else if !testing::has_vector_path() {
        eprintln!(
            "no vector path on this machine: the differential tests are \
             checking the portable walk against itself"
        );
    }
}

/// The vector encode stage writes what the portable walk would have written
#[test]
fn encode_stages_agree() {
    let mut rng = Rng::new(0xB10C);
    let mut input = [0u8; 1024];
    for round in 0..64 {
        rng.fill(&mut input);
        let len = 48 + rng.below(976);
        let url_safe = round % 2 == 1;

        let mut vector = [0u8; 1400];
        let (read, written) = testing::vector_encode_bulk(&input[..len], &mut vector, url_safe);
        if read == 0 {
            continue;
        }
        assert_eq!(read % 3, 0, "a block that is not whole groups");
        assert_eq!(written, read / 3 * 4);

        let mut portable = [0u8; 1400];
        if url_safe {
            let count = tape_base64::URL_SAFE_NO_PAD
                .encode(&input[..read], &mut portable)
                .unwrap();
            assert_eq!(count, written);
        } else {
            testing::portable_encode(&input[..read], &mut portable[..written], false);
        }
        assert_eq!(vector[..written], portable[..written], "round {round}");
    }
}

/// The same for decoding, over characters the encoder just produced
#[test]
fn decode_stages_agree() {
    let mut rng = Rng::new(0xDEC0DE);
    let mut input = [0u8; 768];
    for round in 0..64 {
        rng.fill(&mut input);
        let len = 48 + rng.below(720);
        let url_safe = round % 2 == 1;
        let codec = if url_safe {
            tape_base64::URL_SAFE_NO_PAD
        } else {
            tape_base64::STANDARD_NO_PAD
        };

        let mut encoded = [0u8; 1024];
        let chars = codec.encode(&input[..len], &mut encoded).unwrap();

        let mut vector = [0u8; 768];
        let (read, written) = testing::vector_decode_bulk(&encoded[..chars], &mut vector, url_safe);
        if read == 0 {
            continue;
        }
        assert_eq!(read % 4, 0, "a block that is not whole groups");
        assert_eq!(written, read / 4 * 3);
        assert_eq!(vector[..written], input[..written], "round {round}");
    }
}

/// A character outside the alphabet is never inside what the vector stage claims
#[test]
fn the_vector_stage_never_claims_a_bad_character() {
    for position in 0..512 {
        let mut encoded = [b'A'; 512];
        encoded[position] = b'\n';
        let mut out = [0xCCu8; 384];
        let (read, written) = testing::vector_decode_bulk(&encoded, &mut out, false);
        assert!(read <= position, "claimed the character at {position}");
        assert_eq!(read % 4, 0);
        assert_eq!(written, read / 4 * 3);
        // Everything it did claim was spelled `A`, which is the value zero
        assert!(
            out[..written].iter().all(|&byte| byte == 0),
            "wrote something it did not read, bad byte at {position}"
        );

        let mut decoded = [0u8; 384];
        assert_eq!(
            tape_base64::STANDARD_NO_PAD
                .decode(&encoded, &mut decoded)
                .unwrap_err(),
            tape_base64::DecodeError::InvalidByte {
                position,
                byte: b'\n'
            }
        );
    }
}

/// The portable decode and the whole codec see the same bytes
#[test]
fn portable_decode_matches_the_codec() {
    let mut rng = Rng::new(0x1234_5678);
    let mut input = [0u8; 512];
    for _ in 0..64 {
        rng.fill(&mut input);
        let len = rng.below(512);
        let mut encoded = [0u8; 700];
        let chars = tape_base64::STANDARD_NO_PAD
            .encode(&input[..len], &mut encoded)
            .unwrap();

        let mut portable = [0u8; 512];
        let count = testing::portable_decode(&encoded[..chars], &mut portable).unwrap();
        assert_eq!(count, len);
        assert_eq!(portable[..count], input[..len]);
    }
}

/// The characters a url-safe alphabet spells, given the standard ones
fn to_url_safe(encoded: &mut [u8]) {
    for slot in encoded.iter_mut() {
        *slot = match *slot {
            b'+' => b'-',
            b'/' => b'_',
            other => other,
        };
    }
}

/// Whatever the codec picks, it produces what the portable walk would, at every length
#[test]
fn every_length_of_the_codec_agrees_with_the_portable_walk() {
    let mut rng = Rng::new(0x5A17_C0DE);
    let mut source = vec![0u8; 512 + 64];
    rng.fill(&mut source);

    for length in 0..=512 {
        let start = (length * 7) % 64;
        let input = &source[start..start + length];

        for is_padded in [false, true] {
            let codec = if is_padded {
                tape_base64::STANDARD
            } else {
                tape_base64::STANDARD_NO_PAD
            };
            let needed = codec.encoded_len(length).expect("a length this small");

            let mut walked = vec![0u8; needed];
            testing::portable_encode(input, &mut walked, is_padded);

            let mut converted = vec![0u8; needed];
            assert_eq!(
                codec.encode(input, &mut converted).expect("exactly enough"),
                needed
            );
            assert_eq!(converted, walked, "length {length}, padded {is_padded}");

            let mut back = vec![0u8; length];
            let read = codec.decode(&converted, &mut back).expect("our own");
            assert_eq!(read, length, "length {length}, padded {is_padded}");
            assert_eq!(back, input, "length {length}, padded {is_padded}");
        }

        let whole = tape_base64::STANDARD_NO_PAD
            .encoded_len(length)
            .expect("a length this small");
        let mut payload = vec![0u8; whole];
        testing::portable_encode(input, &mut payload, false);

        let mut walked = vec![0u8; length];
        let count = testing::portable_decode(&payload, &mut walked).expect("our own encoding");
        assert_eq!(count, length, "length {length}");
        assert_eq!(walked, input, "length {length}");
    }
}

/// The vector stage at every length, in both alphabets, against the portable walk
#[test]
fn every_length_of_the_vector_stage_agrees_with_the_portable_walk() {
    let mut rng = Rng::new(0x71C0_FFEE);
    let mut source = vec![0u8; 512 + 64];
    rng.fill(&mut source);

    for length in 0..=512 {
        let start = (length * 7) % 64;
        let input = &source[start..start + length];
        let whole = tape_base64::STANDARD_NO_PAD
            .encoded_len(length)
            .expect("a length this small");

        for is_url_safe in [false, true] {
            let mut walked = vec![0u8; whole];
            testing::portable_encode(input, &mut walked, false);
            if is_url_safe {
                to_url_safe(&mut walked);
            }

            let mut vector = vec![0u8; whole + 64];
            let (read, written) = testing::vector_encode_bulk(input, &mut vector, is_url_safe);
            assert_eq!(read % 3, 0, "a block that is not whole groups");
            assert_eq!(written, read / 3 * 4);
            assert!(read <= length, "the stage took more than it was given");
            assert_eq!(
                vector[..written],
                walked[..written],
                "encode length {length}, url-safe {is_url_safe}"
            );

            let mut bytes = vec![0u8; length + 64];
            let (taken, put) = testing::vector_decode_bulk(&walked, &mut bytes, is_url_safe);
            assert_eq!(taken % 4, 0, "a block that is not whole groups");
            assert_eq!(put, taken / 4 * 3);
            assert!(
                taken <= walked.len(),
                "the stage took more than it was given"
            );
            assert_eq!(
                bytes[..put],
                input[..put],
                "decode length {length}, url-safe {is_url_safe}"
            );
        }
    }
}
