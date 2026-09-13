//! What a decode refuses, and where it says the trouble is

use tape_base64::{DecodeError, EncodeError, STANDARD, STANDARD_NO_PAD, URL_SAFE};

/// The offset reported is the offset of the byte, including inside a block the vector stage took
#[test]
fn invalid_byte_is_found_at_its_own_offset() {
    let mut encoded = [b'A'; 256];
    for position in [0usize, 1, 3, 4, 17, 63, 64, 65, 127, 191, 251] {
        encoded.iter_mut().for_each(|slot| *slot = b'A');
        encoded[position] = b'*';
        let mut out = [0u8; 192];
        let error = STANDARD.decode(&encoded, &mut out).unwrap_err();
        assert_eq!(
            error,
            DecodeError::InvalidByte {
                position,
                byte: b'*'
            }
        );
    }
}

/// Every byte outside the alphabet is refused, and every byte inside it is not
#[test]
fn exactly_the_alphabet_is_accepted() {
    for byte in 0..=u8::MAX {
        let encoded = [b'A', b'A', b'A', byte];
        let mut out = [0u8; 3];
        let accepted = STANDARD_NO_PAD.decode(&encoded, &mut out).is_ok();
        let in_alphabet = byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/';
        assert_eq!(accepted, in_alphabet, "byte {byte:#04x}");
    }
}

/// A character count of one past a group holds six bits, which is not a byte
#[test]
fn lengths_no_encoder_produces() {
    let mut out = [0u8; 256];
    let all_a = [b'A'; 256];
    for length in [1usize, 5, 65, 129] {
        assert_eq!(
            STANDARD_NO_PAD
                .decode(&all_a[..length], &mut out)
                .unwrap_err(),
            DecodeError::InvalidLength { length }
        );
    }
    for length in [1usize, 2, 3, 5, 6, 7, 65, 67] {
        assert_eq!(
            STANDARD.decode(&all_a[..length], &mut out).unwrap_err(),
            DecodeError::InvalidLength { length }
        );
    }
}

/// A padded codec wants the padding, and an unpadded one refuses it
#[test]
fn padding_is_required_or_refused() {
    let mut out = [0u8; 8];
    assert_eq!(STANDARD.decode(b"Zg==", &mut out).unwrap(), 1);
    assert!(STANDARD.decode(b"Zg", &mut out).is_err());
    assert_eq!(STANDARD_NO_PAD.decode(b"Zg", &mut out).unwrap(), 1);
    assert_eq!(
        STANDARD_NO_PAD.decode(b"Zg==", &mut out).unwrap_err(),
        DecodeError::InvalidByte {
            position: 2,
            byte: b'='
        }
    );
}

/// Three characters of padding leave one character, which holds no byte
#[test]
fn too_much_padding() {
    let mut out = [0u8; 8];
    assert_eq!(
        STANDARD.decode(b"Z===", &mut out).unwrap_err(),
        DecodeError::InvalidPadding { position: 1 }
    );
    assert_eq!(
        STANDARD.decode(b"====", &mut out).unwrap_err(),
        DecodeError::InvalidPadding { position: 1 }
    );
}

/// Padding anywhere but the end is a byte outside the alphabet
#[test]
fn padding_in_the_middle() {
    let mut out = [0u8; 16];
    assert_eq!(
        STANDARD.decode(b"Zg==Zg==", &mut out).unwrap_err(),
        DecodeError::InvalidByte {
            position: 2,
            byte: b'='
        }
    );
}

/// A short group's last character holds spare bits, which have to be zero
#[test]
fn trailing_bits_have_to_be_clear() {
    let mut out = [0u8; 8];
    // `Zg==` is one byte: the second character's low four bits are spare.
    assert_eq!(STANDARD.decode(b"Zg==", &mut out).unwrap(), 1);
    assert_eq!(
        STANDARD.decode(b"Zh==", &mut out).unwrap_err(),
        DecodeError::InvalidLastSymbol {
            position: 1,
            byte: b'h'
        }
    );
    // `Zm8=` is two bytes: the third character's low two bits are spare.
    assert_eq!(STANDARD.decode(b"Zm8=", &mut out).unwrap(), 2);
    assert_eq!(
        STANDARD.decode(b"Zm9=", &mut out).unwrap_err(),
        DecodeError::InvalidLastSymbol {
            position: 2,
            byte: b'9'
        }
    );
}

/// A buffer too short is said so before anything is written
#[test]
fn buffers_too_small() {
    let mut out = [0u8; 3];
    assert_eq!(
        STANDARD.encode(b"foo", &mut out).unwrap_err(),
        EncodeError::BufferTooSmall { needed: 4 }
    );
    let mut out = [0u8; 2];
    assert_eq!(
        STANDARD.decode(b"Zm9v", &mut out).unwrap_err(),
        DecodeError::BufferTooSmall { needed: 3 }
    );
}

/// The url-safe alphabet refuses the standard one's last two characters
#[test]
fn url_safe_refuses_the_other_alphabet() {
    let mut out = [0u8; 8];
    assert_eq!(
        URL_SAFE.decode(b"++++", &mut out).unwrap_err(),
        DecodeError::InvalidByte {
            position: 0,
            byte: b'+'
        }
    );
    assert_eq!(URL_SAFE.decode(b"----", &mut out).unwrap(), 3);
}

/// An input whose encoding would not fit in a `usize` is refused rather than wrapping
#[test]
fn lengths_whose_encoding_has_no_length() {
    // No allocation: `encoded_len` is asked about a length, not given a buffer.
    assert_eq!(STANDARD.encoded_len(usize::MAX), None);
    assert_eq!(STANDARD_NO_PAD.encoded_len(usize::MAX), None);
    // Two thirds of the range still has an encoding, and a length is all this asks for
    assert!(STANDARD.encoded_len(usize::MAX / 2).is_some());

    // The largest input that does fit, and the first that does not
    let biggest = (usize::MAX - 4) / 4 * 3 + 2;
    assert!(STANDARD.encoded_len(biggest).is_some());
    assert_eq!(STANDARD.encoded_len(biggest + 1), None);
}
