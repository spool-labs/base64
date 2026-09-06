//! Every path this machine can run, pinned in turn
//!
//! Dispatch picks the widest path the processor reports, so on any one
//! machine the rest of the suite only ever exercises that one. Here each
//! path the machine can actually execute is pinned and put through the same
//! conversions, and every path has to produce the same characters and the
//! same bytes as the portable walk.
//!
//! Pinning is a property of the whole process and the harness runs tests in
//! parallel, so every test here holds `PIN` for as long as it has a path
//! pinned.

#![cfg(all(target_arch = "x86_64", feature = "internals"))]

mod common;

use std::sync::{Mutex, MutexGuard};

use base64::Engine as _;

use common::{Rng, CODECS};
use tape_base64::testing;
use tape_base64::{Codec, DecodeError, STANDARD_NO_PAD, URL_SAFE_NO_PAD};

/// Held for as long as a path is pinned, so two tests never pin at once
static PIN: Mutex<()> = Mutex::new(());

/// Lengths that put every block size against every remainder, and a few long
/// enough that the loop runs many times
///
/// Under Miri every block is interpreted rather than executed, so the sweep is
/// cut to the block boundaries and their neighbours.
fn lengths() -> Vec<usize> {
    if !cfg!(miri) {
        let mut lengths: Vec<usize> = (0..=512).collect();
        lengths.extend([1232, 4096, 4097]);
        return lengths;
    }
    let mut lengths = vec![0, 1, 2, 3, 4, 5, 512];
    for edge in [
        12, 16, 24, 32, 48, 64, 72, 96, 128, 192, 210, 216, 256, 288, 384,
    ] {
        lengths.extend([edge - 1, edge, edge + 1]);
    }
    lengths
}

/// One in `step` of a sweep under Miri, all of it anywhere else
fn thinned(count: usize, step: usize) -> impl Iterator<Item = usize> {
    (0..count).step_by(if cfg!(miri) { step } else { 1 })
}

/// Take `PIN`, ignoring a poisoning left by an earlier failure
fn pinned() -> MutexGuard<'static, ()> {
    PIN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Pin a path, refusing one this processor has not got
fn pin(chosen: u8) {
    assert!(
        chosen <= testing::available(),
        "pinned a path this processor cannot execute"
    );
    // SAFETY: the path is no wider than what cpuid reported.
    unsafe { testing::force(chosen) };
}

/// The wide path is chosen exactly where the processor reports what it uses
///
/// `available` reads cpuid itself, so its answer is held against the standard
/// library's detection of the same three features.
#[test]
fn the_wide_path_is_taken_only_where_vbmi_is() {
    let reported = is_x86_feature_detected!("avx512f")
        && is_x86_feature_detected!("avx512bw")
        && is_x86_feature_detected!("avx512vbmi");
    assert_eq!(
        testing::available() == testing::WIDE,
        reported && cfg!(feature = "avx512"),
        "vbmi reported {reported}, path {}",
        testing::available()
    );
}

/// Convert on whatever path is pinned, and say what came out
fn convert(codec: Codec, input: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut encoded = vec![0u8; codec.encoded_len(input.len()).unwrap()];
    let written = codec.encode(input, &mut encoded).unwrap();
    assert_eq!(written, encoded.len());

    let mut decoded = vec![0u8; input.len()];
    let back = codec.decode(&encoded, &mut decoded).unwrap();
    assert_eq!(back, input.len());
    (encoded, decoded)
}

#[test]
fn every_path_this_machine_has_agrees_with_the_portable_one() {
    let _pinned = pinned();
    let widest = testing::available();
    let mut paths = vec![("portable", testing::PORTABLE)];
    if widest >= testing::AVX2 {
        paths.push(("avx2", testing::AVX2));
    }
    if widest >= testing::WIDE {
        paths.push(("avx512vbmi", testing::WIDE));
    }
    eprintln!(
        "paths on this machine: {}",
        paths
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let lengths = lengths();

    let mut rng = Rng::new(0x5A5A_1234);
    let mut input = vec![0u8; 4200];
    rng.fill(&mut input);

    for entry in &CODECS {
        let (name, codec) = (entry.name, entry.codec);
        for &len in &lengths {
            let slice = &input[..len];

            pin(testing::PORTABLE);
            let expected = convert(codec, slice);
            assert_eq!(
                expected.0,
                entry.reference.encode(slice).as_bytes(),
                "the portable walk spelled {name} length {len} differently"
            );

            for &(path_name, path) in &paths[1..] {
                pin(path);
                let found = convert(codec, slice);
                assert_eq!(
                    found.0, expected.0,
                    "{path_name} encoded {name} length {len} differently"
                );
                assert_eq!(
                    found.1, expected.1,
                    "{path_name} decoded {name} length {len} differently"
                );
            }
        }
    }

    // Every byte outside the alphabet has to be refused on every path, at
    // the offset it sits at, wherever in a block that falls
    for &(path_name, path) in &paths {
        pin(path);
        for position in thinned(256, 11) {
            let mut encoded = vec![b'A'; 256];
            encoded[position] = b'\n';
            let mut out = vec![0u8; 192];
            assert_eq!(
                STANDARD_NO_PAD.decode(&encoded, &mut out).unwrap_err(),
                DecodeError::InvalidByte {
                    position,
                    byte: b'\n'
                },
                "{path_name} missed the byte at {position}"
            );
        }
    }

    // A refusal has to be the same refusal on every path: the same reason, at
    // the same offset, and the same bytes written before it
    let mut rng = Rng::new(0x4A3B_2C1D);
    let mut source = vec![0u8; if cfg!(miri) { 96 } else { 300 }];
    rng.fill(&mut source);

    for entry in &CODECS {
        let (name, codec) = (entry.name, entry.codec);
        pin(testing::PORTABLE);
        let needed = codec
            .encoded_len(source.len())
            .expect("a length this small");
        let mut clean = vec![0u8; needed];
        codec.encode(&source, &mut clean).expect("exactly enough");

        for position in thinned(clean.len(), 13) {
            for byte in [b'*', b'=', b'\n', 0xFF] {
                let mut corrupted = clean.clone();
                corrupted[position] = byte;

                pin(testing::PORTABLE);
                let mut walked = vec![0u8; source.len()];
                let expected = codec.decode(&corrupted, &mut walked);

                for &(path_name, path) in &paths[1..] {
                    pin(path);
                    let mut bytes = vec![0u8; source.len()];
                    let found = codec.decode(&corrupted, &mut bytes);
                    assert_eq!(
                        found, expected,
                        "{path_name} judged {name} at offset {position} differently"
                    );
                    if let Ok(written) = found {
                        assert_eq!(bytes[..written], walked[..written], "{path_name} {name}");
                    }
                }
            }
        }
    }

    pin(widest);
}

/// The whole alphabet, and nothing but it, on every path
#[test]
fn every_path_accepts_exactly_the_alphabet() {
    let _pinned = pinned();
    let widest = testing::available();
    let mut paths = vec![testing::PORTABLE];
    if widest >= testing::AVX2 {
        paths.push(testing::AVX2);
    }
    if widest >= testing::WIDE {
        paths.push(testing::WIDE);
    }

    for path in paths {
        pin(path);
        for byte in 0..=u8::MAX {
            // Deep inside a block, so the vector stage is the one judging it
            let mut encoded = [b'A'; 64];
            encoded[33] = byte;
            let mut out = [0u8; 48];
            let accepted = STANDARD_NO_PAD.decode(&encoded, &mut out).is_ok();
            let in_alphabet = byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/';
            assert_eq!(accepted, in_alphabet, "path {path}, byte {byte:#04x}");

            let accepted = URL_SAFE_NO_PAD.decode(&encoded, &mut out).is_ok();
            let in_alphabet = byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_';
            assert_eq!(
                accepted, in_alphabet,
                "path {path} url-safe, byte {byte:#04x}"
            );
        }
    }
    pin(widest);
}

/// Convert out of a buffer that ends exactly where the input does
///
/// The x86 encoder has one block that reads four bytes before itself and four
/// past its end, because one load is cheaper than two. It is only supposed to
/// be reached where those bytes are inside the buffer. If the bound is
/// counted over the first block of an unrolled run rather than the whole run,
/// the last block of the run reads past the input, and only for an input
/// whose length is a multiple of three, because that is the case where the
/// groups end exactly at the end.
///
/// Nothing here can observe a read past the end on its own. Run under a tool
/// that can:
///
/// ```sh
/// valgrind --error-exitcode=9 target/release/deps/forced-*
/// ```
#[test]
fn lengths_whose_groups_end_at_the_buffer() {
    let _pinned = pinned();
    let widest = testing::available();
    let mut rng = Rng::new(0x9E37_79B9);

    for path in [testing::PORTABLE, testing::AVX2] {
        if path > widest {
            continue;
        }
        pin(path);
        // Every length that is a whole number of groups, across the sizes
        // where the unrolled run starts and stops being used
        for groups in (1..600).step_by(if cfg!(miri) { 11 } else { 1 }) {
            let len = groups * 3;
            let mut input = vec![0u8; len];
            rng.fill(&mut input);
            // Exactly sized, so the allocation ends where the input does
            let mut encoded = vec![0u8; len / 3 * 4];
            assert_eq!(
                STANDARD_NO_PAD.encode(&input, &mut encoded).unwrap(),
                encoded.len()
            );

            let mut back = vec![0u8; len];
            assert_eq!(STANDARD_NO_PAD.decode(&encoded, &mut back).unwrap(), len);
            assert_eq!(back, input, "path {path}, {groups} groups");
        }
    }
    pin(widest);
}

/// A buffer whose start is 16 byte aligned, which is what streaming wants
fn aligned(len: usize) -> (Vec<u8>, usize) {
    let room = vec![0u8; len + 16];
    let offset = room.as_ptr().align_offset(16);
    (room, offset)
}

#[test]
fn the_streaming_store_writes_what_the_walk_would() {
    let _pinned = pinned();
    if testing::available() < testing::AVX2 {
        return;
    }
    pin(testing::AVX2);
    // Far below any real cache, so these lengths reach the streaming store
    // rather than needing a buffer the size of a last level cache to get there.
    testing::force_stream_floor(64);

    let mut rng = Rng::new(0x5EED_1234_ABCD_0001);
    // Past the size below which the dispatcher will not even ask, which the
    // kernel lowers under Miri so that an interpreted run can reach it.
    let candidate = if cfg!(miri) { 1 << 10 } else { 1 << 16 };
    // Enough lengths to cover every block boundary the run can end on.
    let sweep = (candidate..candidate + 48).step_by(if cfg!(miri) { 7 } else { 1 });
    for len in sweep.chain([candidate * 2]) {
        let mut input = vec![0u8; len];
        rng.fill(&mut input);
        for codec in CODECS {
            let wanted = codec.codec.encoded_len(len).unwrap();
            let (mut room, at) = aligned(wanted);
            let written = codec
                .codec
                .encode(&input, &mut room[at..at + wanted])
                .unwrap();
            assert_eq!(written, wanted, "len {len}");

            let mut expected = vec![0u8; wanted];
            pin(testing::PORTABLE);
            let walked = codec.codec.encode(&input, &mut expected).unwrap();
            pin(testing::AVX2);
            assert_eq!(walked, wanted);
            assert_eq!(
                &room[at..at + wanted],
                &expected[..],
                "streaming store differs at len {len}"
            );
        }
    }

    testing::force_stream_floor(0);
    pin(testing::available());
}
