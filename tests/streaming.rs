//! Streamed conversions held against whole-input conversions
//!
//! Every input goes through in pieces of several shapes, and what comes out
//! has to be what one call would have produced, refusals included.

mod common;

use base64::Engine as _;

use common::{Rng, CODECS};
use tape_base64::{Codec, DecodeError};

/// Characters drawn on when an encoding needs corrupting
const SOUP: [u8; 32] = [
    b'A', b'B', b'M', b'Z', b'a', b'b', b'm', b'z', b'0', b'5', b'9', b'+', b'/', b'-', b'_', b'=',
    b'@', b'[', b'`', b'{', b':', b';', b'<', b'>', b'^', b'.', b',', 0x00, 0x0A, 0x7F, 0x80, 0xFF,
];

/// Encode by pushing the pieces, then finishing
fn streamed_encode(codec: Codec, pieces: &[&[u8]]) -> Vec<u8> {
    let mut encoder = codec.encoder();
    let mut out = Vec::new();

    for piece in pieces {
        let mut spot = vec![0u8; piece.len() * 2 + 8];
        let written = encoder.push(piece, &mut spot).expect("a roomy buffer");
        out.extend_from_slice(&spot[..written]);
    }

    let mut spot = [0u8; 4];
    let written = encoder.finish(&mut spot).expect("a roomy buffer");
    out.extend_from_slice(&spot[..written]);
    out
}

/// Decode by pushing the pieces, ignoring what pushes say, and finishing
///
/// A refusal speaks twice, once where it happened and once at the end, and
/// the end is the one this holds inputs to. Once one has spoken, what the
/// pushes wrote is no longer compared, as with a whole-input refusal.
fn streamed_decode(codec: Codec, pieces: &[&[u8]]) -> Result<Vec<u8>, DecodeError> {
    let mut decoder = codec.decoder();
    let mut out = Vec::new();
    let mut refused = false;

    for piece in pieces {
        let mut spot = vec![0u8; piece.len() * 2 + 8];
        match decoder.push(piece, &mut spot) {
            Ok(written) if !refused => out.extend_from_slice(&spot[..written]),
            Ok(_) => {}
            Err(_) => refused = true,
        }
    }

    let mut spot = [0u8; 4];
    let written = decoder.finish(&mut spot)?;
    assert!(!refused, "a refusal is reported again at the end");
    out.extend_from_slice(&spot[..written]);
    Ok(out)
}

/// Split `input` into single characters
fn singles(input: &[u8]) -> Vec<&[u8]> {
    input.iter().map(core::slice::from_ref).collect()
}

/// Split `input` into pieces of up to `most` characters each
fn chunked<'a>(input: &'a [u8], rng: &mut Rng, most: usize) -> Vec<&'a [u8]> {
    let mut pieces = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        let take = (rng.below(most) + 1).min(rest.len());
        pieces.push(&rest[..take]);
        rest = &rest[take..];
    }
    pieces
}

/// Split `input` at every cut in turn, one cut per shape
fn cuts<'a>(input: &'a [u8], rng: &mut Rng, shapes: &mut Vec<Vec<&'a [u8]>>) {
    for at in 0..=input.len() {
        shapes.push(vec![&input[..at], &input[at..]]);
    }
    if input.len() > 6 {
        shapes.push(chunked(input, rng, 7));
    }
}

// a streamed encoding is the whole encoding, at every cut
#[test]
fn encode_pieces_match() {
    for entry in &CODECS {
        for len in 0..70usize {
            let input: Vec<u8> = (0..len).map(|at| (at * 37 + 11) as u8).collect();
            let whole = common::encode_to_vec(entry.codec, &input);

            let mut shapes = Vec::new();
            cuts(&input, &mut Rng::new(len as u64 + 1), &mut shapes);
            shapes.push(singles(&input));
            for pieces in &shapes {
                assert_eq!(
                    streamed_encode(entry.codec, pieces),
                    whole,
                    "{} at {} pieces",
                    entry.name,
                    pieces.len()
                );
            }
        }
    }
}

// a streamed decode of a valid encoding is the whole decode, at every cut
#[test]
fn decode_pieces_match() {
    for entry in &CODECS {
        for len in 0..70usize {
            let input: Vec<u8> = (0..len).map(|at| (at * 53 + 7) as u8).collect();
            let encoded = common::encode_to_vec(entry.codec, &input);
            let joined = encoded.as_slice();

            let mut shapes = Vec::new();
            cuts(joined, &mut Rng::new(len as u64 + 2), &mut shapes);
            shapes.push(singles(joined));
            for pieces in &shapes {
                let mine = streamed_decode(entry.codec, pieces).expect("a valid encoding");
                assert_eq!(mine, input, "{} in {} pieces", entry.name, pieces.len());
                assert!(entry.reference.decode(joined).is_ok());
            }
        }
    }
}

// corrupted and spliced encodings refuse the same way streamed as whole
#[test]
fn refusals_match_the_batch() {
    let mut rng = Rng::new(0x243F_6A88);

    for _ in 0..4000 {
        let entry = &CODECS[rng.below(CODECS.len())];
        let mut source = [0u8; 60];
        rng.fill(&mut source);
        let taken = rng.below(61);

        let mut encoded = common::encode_to_vec(entry.codec, &source[..taken]);
        for _ in 0..=rng.below(3) {
            let at = rng.below(encoded.len().max(1));
            if encoded.is_empty() {
                break;
            }
            encoded[at] = if rng.below(3) == 0 {
                rng.next_u64() as u8
            } else {
                rng.pick(&SOUP)
            };
        }

        let left = common::encode_to_vec(entry.codec, &source[..rng.below(taken / 2 + 1)]);
        let right = common::encode_to_vec(
            entry.codec,
            &source[taken / 2..taken.max(taken / 2 + rng.below(taken - taken / 2 + 1))],
        );
        let candidates: [&[u8]; 3] = [&encoded, &left, &right];

        for joined in candidates {
            let batch = common::decode_to_vec(entry.codec, joined);
            let mut shapes = Vec::new();
            cuts(joined, &mut rng, &mut shapes);
            shapes.push(singles(joined));

            for pieces in shapes.iter().take(5) {
                let streamed = streamed_decode(entry.codec, pieces);
                match (&batch, &streamed) {
                    (Ok(theirs), Ok(mine)) => {
                        assert_eq!(mine, theirs, "{} reading {}", entry.name, label(joined))
                    }
                    (Err(theirs), Err(mine)) => {
                        assert_eq!(mine, theirs, "{} refusing {}", entry.name, label(joined))
                    }
                    _ => panic!(
                        "{} disagreed with itself about {}: {batch:?} against {streamed:?}",
                        entry.name,
                        label(joined)
                    ),
                }
                assert_eq!(
                    streamed.is_ok(),
                    entry.reference.decode(joined).is_ok(),
                    "{} disagrees with the reference about {}",
                    entry.name,
                    label(joined)
                );
            }
        }
    }
}

// a long input corrupted early refuses the same way in any chunking
//
// What follows a refusal is counted rather than read, and the count is what
// the length and padding rules are applied to at the end.
#[test]
fn a_refusal_partway_matches_the_batch() {
    let mut rng = Rng::new(0x0BAD_5EED);
    let mut source = vec![0u8; 512];

    for _ in 0..200 {
        let entry = &CODECS[rng.below(CODECS.len())];
        rng.fill(&mut source);
        let taken = 64 + rng.below(449);
        let mut encoded = common::encode_to_vec(entry.codec, &source[..taken]);

        let at = rng.below(encoded.len() / 4);
        encoded[at] = rng.pick(&SOUP);
        // An ending of padding, and a length an encoder may never produce
        encoded.resize(encoded.len() + rng.below(4), b'=');
        encoded.truncate(encoded.len() - rng.below(3));

        let batch = common::decode_to_vec(entry.codec, &encoded);
        let mut shapes = vec![vec![encoded.as_slice()]];
        for most in [1, 5, 64, 512] {
            shapes.push(chunked(&encoded, &mut rng, most));
        }
        for pieces in &shapes {
            assert_eq!(
                streamed_decode(entry.codec, pieces),
                batch,
                "{} in {} pieces reading {}",
                entry.name,
                pieces.len(),
                label(&encoded)
            );
        }
    }
}

// a push into a buffer shorter than the piece costs refuses and consumes nothing
#[test]
fn short_encode_buffer() {
    let mut encoder = tape_base64::STANDARD.encoder();
    let mut spot = [0u8; 3];

    assert!(encoder.push(b"abcd", &mut spot).is_err());
    assert!(encoder.push(b"abc", &mut spot).is_err());

    let mut spot = [0u8; 4];
    assert_eq!(encoder.push(b"abc", &mut spot).unwrap(), 4);
    assert_eq!(&spot, b"YWJj");
}

// the decoder asks for an upper bound of room before it reads any of it
#[test]
fn short_decode_buffer() {
    let mut decoder = tape_base64::STANDARD.decoder();
    let mut spot = [0u8; 5];

    // Two whole groups owe six bytes, and the refusal reads none of them.
    assert_eq!(
        decoder.push(b"QUJDREVG", &mut spot),
        Err(DecodeError::BufferTooSmall { needed: 6 })
    );

    let mut room = [0u8; 6];
    assert_eq!(decoder.push(b"QUJDREVG", &mut room).unwrap(), 6);
    assert_eq!(&room, b"ABCDEF");
    assert_eq!(decoder.finish(&mut []).unwrap(), 0);
}

// what the sizing helpers hand out is room enough for the pieces too
#[test]
fn helper_sized_buffers_are_accepted() {
    const INPUTS: [&[u8]; 6] = [
        b"QQ==",
        b"aGVsbG8=",
        b"QQQQ",
        b"aGVsbG8h",
        b"Zm8=",
        b"Zm9vYmFy",
    ];
    for entry in &CODECS {
        for input in INPUTS {
            let Ok(exact) = entry.codec.decoded_len(input) else {
                continue;
            };
            let mut whole = vec![0u8; exact];
            let Ok(expected) = entry.codec.decode(input, &mut whole) else {
                continue;
            };

            for room in [exact, entry.codec.decoded_len_max(input.len())] {
                let mut out = vec![0u8; room];
                let mut decoder = entry.codec.decoder();
                let written = decoder
                    .push(input, &mut out)
                    .unwrap_or_else(|_| panic!("{} refused {room} bytes", entry.name));
                let rest = decoder.finish(&mut out[written..]).expect("the same room");
                assert_eq!(written + rest, expected, "{}", entry.name);
                assert_eq!(out[..expected], whole[..expected], "{}", entry.name);
            }
        }
    }
}

// a finish with no room for the ending refuses the way one whole call does
#[test]
fn short_finish_buffer() {
    let mut decoder = tape_base64::STANDARD.decoder();
    assert_eq!(decoder.push(b"QQ==", &mut [0u8; 8]).unwrap(), 0);
    assert_eq!(
        decoder.finish(&mut []),
        Err(DecodeError::BufferTooSmall { needed: 1 })
    );
    assert_eq!(
        tape_base64::STANDARD.decode(b"QQ==", &mut []),
        Err(DecodeError::BufferTooSmall { needed: 1 })
    );
}

// an ending carrying bits the output cannot hold is refused at `finish`, not
// dropped by a buffer too short to have written it
#[test]
fn finish_judges_the_last_symbol() {
    let codec = tape_base64::STANDARD_NO_PAD;
    let refusal = DecodeError::InvalidLastSymbol {
        position: 2,
        byte: b'B',
    };

    let mut decoder = codec.decoder();
    assert_eq!(decoder.push(b"AAB", &mut [0u8; 8]).unwrap(), 0);
    assert_eq!(
        decoder.finish(&mut [0u8; 1]),
        Err(DecodeError::BufferTooSmall { needed: 2 })
    );
    assert_eq!(
        codec.decode(b"AAB", &mut [0u8; 1]),
        Err(DecodeError::BufferTooSmall { needed: 2 })
    );

    let mut decoder = codec.decoder();
    assert_eq!(decoder.push(b"AAB", &mut [0u8; 8]).unwrap(), 0);
    assert_eq!(decoder.finish(&mut [0u8; 2]), Err(refusal));
    assert_eq!(codec.decode(b"AAB", &mut [0u8; 2]), Err(refusal));
}

// an ending followed by more input refuses at the end, wherever that is learned
#[test]
fn ending_then_more() {
    let pieces: [&[u8]; 3] = [b"AB", b"==", b"CD"];

    let mine = streamed_decode(tape_base64::STANDARD, &pieces);
    let batch = common::decode_to_vec(tape_base64::STANDARD, b"AB==CD");
    assert_eq!(mine, batch);
}

fn label(input: &[u8]) -> String {
    String::from_utf8_lossy(input).into_owned()
}
