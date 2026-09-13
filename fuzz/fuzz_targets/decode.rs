//! Arbitrary bytes read as an encoding, held against the reference on value and validity

#![no_main]

use base64::Engine as _;
use libfuzzer_sys::fuzz_target;
use tape_base64_fuzz::{decode_to_vec, pin, CODECS};

fuzz_target!(|data: &[u8]| {
    pin();
    for (codec, reference) in &CODECS {
        let mine = decode_to_vec(*codec, data);
        let theirs = reference.decode(data);
        match (&mine, &theirs) {
            (Ok(mine), Ok(theirs)) => assert_eq!(mine, theirs, "read as different bytes"),
            (Err(_), Err(_)) => {}
            _ => panic!("disagreed about validity: {mine:?} against {theirs:?}"),
        }
        if let Ok(mine) = &mine {
            assert_eq!(
                codec.decoded_len(data),
                Ok(mine.len()),
                "the length said otherwise"
            );
        }
    }
});
