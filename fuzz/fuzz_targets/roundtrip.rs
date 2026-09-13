//! Arbitrary bytes encoded, matched against the reference, and read back into exact buffers

#![no_main]

use base64::Engine as _;
use libfuzzer_sys::fuzz_target;
use tape_base64::{DecodeError, EncodeError};
use tape_base64_fuzz::{encode_to_vec, pin, CODECS};

fuzz_target!(|data: &[u8]| {
    pin();
    for (codec, reference) in &CODECS {
        let encoded = encode_to_vec(*codec, data);
        assert_eq!(
            encoded,
            reference.encode(data).as_bytes(),
            "encoded differently"
        );

        let needed = encoded.len();
        if needed > 0 {
            let mut short = vec![0xCCu8; needed - 1];
            assert_eq!(
                codec.encode(data, &mut short),
                Err(EncodeError::BufferTooSmall { needed }),
                "a short buffer was not refused"
            );
            assert!(
                short.iter().all(|&byte| byte == 0xCC),
                "wrote into a refused buffer"
            );
        }

        assert_eq!(
            codec.decoded_len(&encoded),
            Ok(data.len()),
            "the length said otherwise"
        );
        let mut room = vec![0u8; data.len()];
        assert_eq!(
            codec.decode(&encoded, &mut room),
            Ok(data.len()),
            "an exact buffer refused"
        );
        assert_eq!(room, data, "read back differently");

        if !data.is_empty() {
            let mut short = vec![0xCCu8; data.len() - 1];
            assert_eq!(
                codec.decode(&encoded, &mut short),
                Err(DecodeError::BufferTooSmall { needed: data.len() }),
                "a short buffer was not refused"
            );
            assert!(
                short.iter().all(|&byte| byte == 0xCC),
                "wrote into a refused buffer"
            );
        }
    }
});
