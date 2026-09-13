//! What the three fuzz targets share: the codecs beside their reference engines, and a pin

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::engine::GeneralPurpose;
use tape_base64::{Codec, DecodeError};

/// The four codecs, each beside the reference engine it has to agree with
pub const CODECS: [(Codec, GeneralPurpose); 4] = [
    (tape_base64::STANDARD, STANDARD),
    (tape_base64::STANDARD_NO_PAD, STANDARD_NO_PAD),
    (tape_base64::URL_SAFE, URL_SAFE),
    (tape_base64::URL_SAFE_NO_PAD, URL_SAFE_NO_PAD),
];

/// Pin one x86 path for the whole run when `TAPE_PATH` asks for it, since dispatch only ever picks the widest
pub fn pin() {
    #[cfg(target_arch = "x86_64")]
    {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            use tape_base64::testing::{available, force, AVX2, PORTABLE, WIDE};
            let widest = available();
            let asked = std::env::var("TAPE_PATH").unwrap_or_default();
            let marker = match asked.as_str() {
                "portable" => PORTABLE,
                "avx2" if widest >= AVX2 => AVX2,
                "avx512" if widest == WIDE => WIDE,
                "" => return,
                other => panic!("this machine cannot run {other}"),
            };
            // SAFETY: the marker was checked against what the processor reports.
            unsafe { force(marker) };
        });
    }
}

/// Encode into a buffer of exactly the length the codec asks for
pub fn encode_to_vec(codec: Codec, input: &[u8]) -> Vec<u8> {
    let needed = codec
        .encoded_len(input.len())
        .expect("a length with an encoding");
    let mut encoded = vec![0u8; needed];
    let written = codec
        .encode(input, &mut encoded)
        .expect("an exactly sized buffer is enough");
    assert_eq!(written, needed, "encode wrote past what it asked for");
    encoded
}

/// Decode into a buffer sized from the character count alone
pub fn decode_to_vec(codec: Codec, encoded: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut decoded = vec![0u8; codec.decoded_len_max(encoded.len())];
    let written = codec.decode(encoded, &mut decoded)?;
    decoded.truncate(written);
    Ok(decoded)
}
