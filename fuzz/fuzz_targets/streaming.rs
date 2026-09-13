//! Conversions taken a piece at a time, cut where the fuzzer says, held against the whole call
//!
//! The first bytes are the cut plan and the rest is the payload, so the fuzzer steers both.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tape_base64::{Codec, DecodeError};
use tape_base64_fuzz::{decode_to_vec, encode_to_vec, pin, CODECS};

/// Cut `input` into pieces whose lengths cycle through `plan`, one character each when the plan is empty
fn pieces<'a>(input: &'a [u8], plan: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut rest = input;
    let mut at = 0;
    while !rest.is_empty() {
        let take = plan
            .get(at)
            .map_or(1, |&byte| usize::from(byte % 9) + 1)
            .min(rest.len());
        out.push(&rest[..take]);
        rest = &rest[take..];
        at = (at + 1) % plan.len().max(1);
    }
    out
}

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
    assert!(!refused, "a refusal was not reported again at the end");
    out.extend_from_slice(&spot[..written]);
    Ok(out)
}

fuzz_target!(|data: &[u8]| {
    pin();
    let (plan, payload) = data.split_at(data.len().min(8));
    for (codec, _) in &CODECS {
        let whole = encode_to_vec(*codec, payload);
        assert_eq!(
            streamed_encode(*codec, &pieces(payload, plan)),
            whole,
            "encoded differently in pieces"
        );

        assert_eq!(
            streamed_decode(*codec, &pieces(&whole, plan)),
            Ok(payload.to_vec()),
            "read its own encoding differently in pieces"
        );

        // The payload itself as an encoding, valid or not, has to refuse the same way in pieces.
        assert_eq!(
            streamed_decode(*codec, &pieces(payload, plan)),
            decode_to_vec(*codec, payload),
            "disagreed with the whole call about arbitrary input"
        );
    }
});
