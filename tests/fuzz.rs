//! Thousands of inputs nobody would think to write, held against the reference
//!
//! No fuzzer and no dependency: a fixed xorshift and a handful of seeds, so a
//! failure is the same failure the next run and on the next machine.

mod common;

use base64::Engine as _;

use common::{Compared, Rng, CODECS};

/// Characters an encoding is built from, the padding, and bytes that sit just
/// outside every row of the alphabet
const SOUP: [u8; 32] = [
    b'A', b'B', b'M', b'Z', b'a', b'b', b'm', b'z', b'0', b'5', b'9', b'+', b'/', b'-', b'_', b'=',
    b'@', b'[', b'`', b'{', b':', b';', b'<', b'>', b'^', b'.', b',', 0x00, 0x0A, 0x7F, 0x80, 0xFF,
];

/// Hold one decode against the reference, on value as well as validity
fn check_decode(entry: &Compared, encoded: &[u8]) {
    let mine = common::decode_to_vec(entry.codec, encoded);
    let theirs = entry.reference.decode(encoded);
    if let Ok(mine) = &mine {
        // What the length says before the characters are read has to be what
        // reading them writes
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

/// A length that is usually short and occasionally long enough for a kernel
/// loop to run many times
fn length(rng: &mut Rng) -> usize {
    match rng.below(4) {
        0 => rng.below(16),
        1 => rng.below(200),
        2 => rng.below(1000),
        _ => rng.below(4096),
    }
}

/// Random bytes encode and decode the way the reference does
#[test]
fn random_bytes_convert_like_the_reference() {
    for seed in [0x243F_6A88u64, 0x85A3_08D3, 0x1319_8A2E, 0x0370_7344] {
        let mut rng = Rng::new(seed);
        let mut source = vec![0u8; 4096];

        for _ in 0..500 {
            rng.fill(&mut source);
            let taken = length(&mut rng);
            let input = &source[..taken];

            for entry in &CODECS {
                let encoded = common::encode_to_vec(entry.codec, input);
                assert_eq!(
                    encoded,
                    entry.reference.encode(input).as_bytes(),
                    "{} length {taken}",
                    entry.name
                );

                let decoded = common::decode_to_vec(entry.codec, &encoded).expect("our own");
                assert_eq!(decoded, input, "{} length {taken}", entry.name);
                check_decode(entry, &encoded);
            }
        }
    }
}

/// An encoding with a few characters changed is read, or refused, alike
#[test]
fn random_corruptions_are_judged_like_the_reference() {
    for seed in [0xA409_3822u64, 0x299F_31D0, 0x082E_FA98, 0xEC4E_6C89] {
        let mut rng = Rng::new(seed);
        let mut source = vec![0u8; 600];

        for _ in 0..1000 {
            rng.fill(&mut source);
            let taken = rng.below(601);

            for entry in &CODECS {
                let encoded = common::encode_to_vec(entry.codec, &source[..taken]);
                if encoded.is_empty() {
                    continue;
                }

                let mut corrupted = encoded.clone();
                for _ in 0..=rng.below(4) {
                    let position = rng.below(corrupted.len());
                    corrupted[position] = if rng.below(4) == 0 {
                        rng.next_u64() as u8
                    } else {
                        rng.pick(&SOUP)
                    };
                }
                check_decode(entry, &corrupted);
            }
        }
    }
}

/// Characters drawn out of a pool that is mostly, but not only, an alphabet
#[test]
fn random_character_soup_is_judged_like_the_reference() {
    for seed in [0x4528_21E6u64, 0x38D0_1377, 0xBE54_66CF, 0x34E9_0C6C] {
        let mut rng = Rng::new(seed);
        let mut encoded = [0u8; 160];

        for _ in 0..4000 {
            let taken = rng.below(161);
            for slot in encoded[..taken].iter_mut() {
                *slot = rng.pick(&SOUP);
            }
            for entry in &CODECS {
                check_decode(entry, &encoded[..taken]);
            }
        }
    }
}

/// An encoding cut and joined at random offsets, which is where padding ends
/// up in the middle and lengths end up where no encoder leaves them
#[test]
fn random_splices_are_judged_like_the_reference() {
    for seed in [0xC0AC_29B7u64, 0xC97C_50DD, 0x3F84_D5B5, 0xB547_0917] {
        let mut rng = Rng::new(seed);
        let mut source = vec![0u8; 200];

        for _ in 0..1000 {
            rng.fill(&mut source);
            let entry = &CODECS[rng.below(CODECS.len())];

            let left = common::encode_to_vec(entry.codec, &source[..rng.below(101)]);
            let right = common::encode_to_vec(entry.codec, &source[100..100 + rng.below(101)]);

            let mut spliced = left[..rng.below(left.len() + 1)].to_vec();
            spliced.extend_from_slice(&right[..rng.below(right.len() + 1)]);
            check_decode(entry, &spliced);
        }
    }
}

/// Whatever the outcome, a conversion writes nothing past what it returns and
/// nothing at all into a buffer it refused
#[test]
fn random_conversions_write_only_what_they_return() {
    let mut rng = Rng::new(0x9216_D5D9);
    let mut source = vec![0u8; 512];

    for _ in 0..2000 {
        rng.fill(&mut source);
        let taken = rng.below(513);
        let input = &source[..taken];
        let entry = &CODECS[rng.below(CODECS.len())];
        let needed = entry.codec.encoded_len(taken).expect("a length this small");

        let room = rng.below(needed + 32);
        let mut encoded = vec![0xCCu8; room];
        match entry.codec.encode(input, &mut encoded) {
            Ok(written) => {
                assert_eq!(written, needed);
                assert!(encoded[written..].iter().all(|&byte| byte == 0xCC));
            }
            Err(_) => {
                assert!(room < needed);
                assert!(encoded.iter().all(|&byte| byte == 0xCC));
            }
        }
        if room < needed {
            continue;
        }

        let room = rng.below(taken + 32);
        let mut decoded = vec![0xCCu8; room];
        match entry.codec.decode(&encoded[..needed], &mut decoded) {
            Ok(written) => {
                assert_eq!(written, taken);
                assert_eq!(&decoded[..written], input);
                assert!(decoded[written..].iter().all(|&byte| byte == 0xCC));
            }
            Err(_) => {
                assert!(room < taken);
                assert!(decoded.iter().all(|&byte| byte == 0xCC));
            }
        }
    }
}
