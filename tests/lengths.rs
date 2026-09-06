//! Every length a block size could stumble on, and the buffers to match
//!
//! Kernels take blocks of twelve, sixteen, twenty-four, thirty-two, forty-eight
//! or sixty-four bytes and leave the rest to a portable tail, so the lengths
//! that break one are the multiples of a block size and their neighbours. Rather
//! than guess which, every length up to a kilobyte is converted and held against
//! the reference crate, and every one of them is also given an output buffer of
//! exactly the size asked for and one byte less than that.

mod common;

use base64::Engine as _;

use common::{Compared, Rng, CODECS};
use tape_base64::{DecodeError, EncodeError};

/// Convert one length four ways and hold all of it against the reference
fn compare_at_length(entry: &Compared, input: &[u8]) {
    let name = entry.name;
    let length = input.len();

    let encoded = common::encode_to_vec(entry.codec, input);
    let expected = entry.reference.encode(input);
    assert_eq!(
        encoded,
        expected.as_bytes(),
        "{name} encoded length {length} differently"
    );

    assert_eq!(
        entry.codec.decoded_len(&encoded).expect("our own encoding"),
        length,
        "{name} miscounted the decoding of length {length}"
    );
    assert!(
        entry.codec.decoded_len_max(encoded.len()) >= length,
        "{name} understated the room length {length} needs"
    );

    let decoded = common::decode_to_vec(entry.codec, &encoded).expect("our own encoding");
    assert_eq!(decoded, input, "{name} did not return length {length}");

    let theirs = entry
        .reference
        .decode(&encoded)
        .expect("the reference reads our characters");
    assert_eq!(theirs, input, "{name} disagreed at length {length}");
}

/// A different window of the source for each length, so that no encoding is a
/// prefix of the next
fn window(source: &[u8], length: usize) -> &[u8] {
    let start = (length * 7) % 64;
    &source[start..start + length]
}

/// Every length from nothing to a kilobyte, in all four codecs
#[test]
fn every_length_up_to_a_kilobyte_matches_the_reference() {
    let mut rng = Rng::new(0x1B4E_28BA);
    let mut source = vec![0u8; 1024 + 64];
    rng.fill(&mut source);

    for length in 0..=1024 {
        for entry in &CODECS {
            compare_at_length(entry, window(&source, length));
        }
    }
}

/// Past a kilobyte, every prime step and the multiples of every block size
#[test]
fn a_sparse_sweep_past_a_kilobyte_matches_the_reference() {
    let mut rng = Rng::new(0x2F3A_C10D);
    let mut source = vec![0u8; 8192 + 64];
    rng.fill(&mut source);

    let mut lengths: Vec<usize> = (1024..=8192).step_by(97).collect();
    // Every length under a kilobyte is covered above, so past it only the
    // round numbers a loop count could stumble on are worth the time
    for round in [1536usize, 2048, 3072, 4096, 6144, 8192] {
        lengths.extend([round - 1, round, round + 1]);
    }

    for length in lengths {
        for entry in &CODECS {
            compare_at_length(entry, window(&source, length));
        }
    }
}

/// A buffer of exactly the size asked for is enough, and one byte less is
/// refused without a byte of it being touched
#[test]
fn an_exact_buffer_is_enough_and_one_byte_short_is_not() {
    let mut rng = Rng::new(0x7C1B_9E42);
    let mut source = vec![0u8; 1024 + 64];
    rng.fill(&mut source);

    for length in 0..=1024 {
        let input = window(&source, length);
        for entry in &CODECS {
            let (name, codec) = (entry.name, entry.codec);
            let needed = codec.encoded_len(length).expect("a length this small");

            let mut exact = vec![0u8; needed];
            assert_eq!(
                codec.encode(input, &mut exact).expect("exactly enough"),
                needed,
                "{name} length {length}"
            );

            if needed > 0 {
                let mut short = vec![0xCCu8; needed - 1];
                assert_eq!(
                    codec.encode(input, &mut short).unwrap_err(),
                    EncodeError::BufferTooSmall { needed },
                    "{name} encoded length {length} into too little room"
                );
                assert!(
                    short.iter().all(|&byte| byte == 0xCC),
                    "{name} wrote into a buffer it refused, length {length}"
                );
            }

            let mut room = vec![0u8; length];
            assert_eq!(
                codec.decode(&exact, &mut room).expect("exactly enough"),
                length,
                "{name} length {length}"
            );
            assert_eq!(room, input, "{name} length {length}");

            if length > 0 {
                let mut short = vec![0xCCu8; length - 1];
                assert_eq!(
                    codec.decode(&exact, &mut short).unwrap_err(),
                    DecodeError::BufferTooSmall { needed: length },
                    "{name} decoded length {length} into too little room"
                );
                assert!(
                    short.iter().all(|&byte| byte == 0xCC),
                    "{name} wrote into a buffer it refused, length {length}"
                );
            }
        }
    }
}

/// Room past the end of a conversion is left as it was found
#[test]
fn a_buffer_with_room_to_spare_keeps_its_tail() {
    let mut rng = Rng::new(0x4D2A_11F0);
    let mut source = vec![0u8; 512 + 64];
    rng.fill(&mut source);

    for length in 0..=512 {
        let input = window(&source, length);
        for entry in &CODECS {
            let (name, codec) = (entry.name, entry.codec);
            let needed = codec.encoded_len(length).expect("a length this small");

            let mut encoded = vec![0xCCu8; needed + 17];
            let written = codec.encode(input, &mut encoded).expect("room to spare");
            assert_eq!(written, needed, "{name} length {length}");
            assert!(
                encoded[written..].iter().all(|&byte| byte == 0xCC),
                "{name} wrote past its encoding at length {length}"
            );

            let mut decoded = vec![0xCCu8; length + 17];
            let back = codec
                .decode(&encoded[..written], &mut decoded)
                .expect("room to spare");
            assert_eq!(back, length, "{name} length {length}");
            assert_eq!(&decoded[..back], input, "{name} length {length}");
            assert!(
                decoded[back..].iter().all(|&byte| byte == 0xCC),
                "{name} wrote past its decoding at length {length}"
            );
        }
    }
}
