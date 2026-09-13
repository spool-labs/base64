//! Base64 encoding and decoding
//!
//! One kernel per architecture. Every conversion writes into a caller's
//! buffer, nothing allocates, and the crate is `no_std`.
//!
//! ```
//! let mut encoded = [0u8; 8];
//! let written = tape_base64::encode(b"hello", &mut encoded).unwrap();
//! assert_eq!(&encoded[..written], b"aGVsbG8=");
//!
//! let mut decoded = [0u8; 5];
//! let written = tape_base64::decode(b"aGVsbG8=", &mut decoded).unwrap();
//! assert_eq!(&decoded[..written], b"hello");
//! ```

#![no_std]
#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]

#[cfg(feature = "std")]
extern crate std;

mod alphabet;
mod backend;
mod codec;
mod error;
mod scalar;
mod stream;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
mod neon;

#[cfg(target_arch = "x86_64")]
mod x86;

#[cfg(any(target_arch = "x86_64", test))]
mod wide;

pub use error::{DecodeError, EncodeError};
pub use stream::{Decoder, Encoder};

use alphabet::{Standard, UrlSafe};

/// Which 64 characters a codec spells values with
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Standard,
    UrlSafe,
}

/// An alphabet and a padding rule
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Codec {
    kind: Kind,
    is_padded: bool,
}

/// RFC 4648 section 4: the alphabet ending `+/`, padded with `=`
pub const STANDARD: Codec = Codec {
    kind: Kind::Standard,
    is_padded: true,
};

/// RFC 4648 section 4 without padding, where `=` is refused like any byte
pub const STANDARD_NO_PAD: Codec = Codec {
    kind: Kind::Standard,
    is_padded: false,
};

/// RFC 4648 section 5: the alphabet ending `-_`, with padding
pub const URL_SAFE: Codec = Codec {
    kind: Kind::UrlSafe,
    is_padded: true,
};

/// RFC 4648 section 5 without padding
pub const URL_SAFE_NO_PAD: Codec = Codec {
    kind: Kind::UrlSafe,
    is_padded: false,
};

impl Codec {
    /// Characters the encoding of `len` bytes takes, `None` if it overflows
    #[inline]
    pub const fn encoded_len(&self, len: usize) -> Option<usize> {
        let groups = len / 3;
        self.length(groups, len % 3)
    }

    /// Characters this many whole groups and this many bytes over take
    #[inline]
    const fn length(&self, groups: usize, rest: usize) -> Option<usize> {
        if groups > MAX_GROUPS {
            return None;
        }
        let tail = if rest == 0 {
            0
        } else if self.is_padded {
            4
        } else {
            rest + 1
        };
        Some(groups * 4 + tail)
    }

    /// The most bytes the decoding of `len` characters can take
    #[inline]
    pub const fn decoded_len_max(&self, len: usize) -> usize {
        decoded_len(len)
    }

    /// Bytes `input` decodes to, from its length and padding without reading it
    #[inline]
    pub fn decoded_len(&self, input: &[u8]) -> Result<usize, DecodeError> {
        Ok(decoded_len(self.payload(input)?.len()))
    }

    /// Encode `input` into `out`, returning how many characters were written
    #[inline]
    pub fn encode(&self, input: &[u8], out: &mut [u8]) -> Result<usize, EncodeError> {
        let groups = input.len() / 3;
        let needed = match self.length(groups, input.len() - groups * 3) {
            Some(needed) => needed,
            None => return Err(EncodeError::TooLong),
        };
        if out.len() < needed {
            return Err(EncodeError::BufferTooSmall { needed });
        }
        let out = &mut out[..needed];
        match self.kind {
            Kind::Standard => codec::encode::<Standard>(input, out, groups, self.is_padded),
            Kind::UrlSafe => codec::encode::<UrlSafe>(input, out, groups, self.is_padded),
        }
        Ok(needed)
    }

    /// Decode `input` into `out`, returning how many bytes were written
    #[inline]
    pub fn decode(&self, input: &[u8], out: &mut [u8]) -> Result<usize, DecodeError> {
        let payload = self.payload(input)?;
        let quads = payload.len() / 4;
        let needed = decoded_len(payload.len());
        if out.len() < needed {
            return Err(DecodeError::BufferTooSmall { needed });
        }
        let out = &mut out[..needed];
        match self.kind {
            Kind::Standard => codec::decode::<Standard>(payload, out, quads),
            Kind::UrlSafe => codec::decode::<UrlSafe>(payload, out, quads),
        }
    }

    /// The characters of `input` that hold bits, padding checked and removed
    #[inline]
    fn payload<'input>(&self, input: &'input [u8]) -> Result<&'input [u8], DecodeError> {
        let len = input.len();
        if !length_ok(len, self.is_padded) {
            return Err(DecodeError::InvalidLength { length: len });
        }
        // Past this check the last group of a padded input is there to read.
        if !self.is_padded || len < 4 {
            return Ok(input);
        }

        // `&` not `&&`, so this is arithmetic rather than three dependent branches.
        let one = usize::from(input[len - 1] == b'=');
        let two = one & usize::from(input[len - 2] == b'=');
        let three = two & usize::from(input[len - 3] == b'=');
        // Two is the most padding an encoding has.
        if three != 0 {
            return Err(DecodeError::InvalidPadding { position: len - 3 });
        }
        Ok(&input[..len - one - two])
    }
}

/// Bytes that a payload of `len` characters, padding already off, decodes to
pub(crate) const fn decoded_len(len: usize) -> usize {
    len / 4 * 3 + (len % 4).saturating_sub(1)
}

/// Whether `len` characters are a count some encoding produces
pub(crate) const fn length_ok(len: usize, is_padded: bool) -> bool {
    if is_padded {
        len.is_multiple_of(4)
    } else {
        len % 4 != 1
    }
}

/// The most whole groups whose encoding still has a length
const MAX_GROUPS: usize = (usize::MAX - 4) / 4;

/// Encode with `STANDARD`, returning how many characters were written
#[inline]
pub fn encode(input: &[u8], out: &mut [u8]) -> Result<usize, EncodeError> {
    STANDARD.encode(input, out)
}

/// Decode with `STANDARD`, returning how many bytes were written
#[inline]
pub fn decode(input: &[u8], out: &mut [u8]) -> Result<usize, DecodeError> {
    STANDARD.decode(input, out)
}

/// Characters the `STANDARD` encoding of `len` bytes takes
#[inline]
pub const fn encoded_len(len: usize) -> Option<usize> {
    STANDARD.encoded_len(len)
}

/// Internals the test suite checks and the benchmarks time, not a public API
#[cfg(feature = "internals")]
#[doc(hidden)]
pub mod testing {
    use crate::alphabet::{Standard, UrlSafe};
    use crate::backend;
    use crate::error::DecodeError;
    use crate::scalar;

    #[cfg(target_arch = "x86_64")]
    use crate::x86;

    pub fn portable_encode(input: &[u8], out: &mut [u8], is_padded: bool) {
        let codec = if is_padded {
            crate::STANDARD
        } else {
            crate::STANDARD_NO_PAD
        };
        assert!(
            out.len() >= codec.encoded_len(input.len()).expect("a length this small"),
            "the walk wants room for the whole encoding"
        );
        let groups = scalar::encode_bulk::<Standard>(input, out, input.len() / 3);
        scalar::encode_rest::<Standard>(&input[groups * 3..], &mut out[groups * 4..], is_padded);
    }

    pub fn portable_decode(input: &[u8], out: &mut [u8]) -> Result<usize, DecodeError> {
        assert!(
            out.len() >= crate::decoded_len(input.len()),
            "the walk wants room for the whole decoding"
        );
        let quads = scalar::decode_bulk::<Standard>(input, out, input.len() / 4);
        let (read, written) = (quads * 4, quads * 3);
        if input.len() - read >= 4 {
            return Err(scalar::find_invalid::<Standard>(&input[read..], read));
        }
        Ok(written + scalar::decode_rest::<Standard>(&input[read..], &mut out[written..], read)?)
    }

    pub fn vector_encode_bulk(input: &[u8], out: &mut [u8], is_url_safe: bool) -> (usize, usize) {
        let groups = core::cmp::min(input.len() / 3, out.len() / 4);
        let done = if is_url_safe {
            backend::encode_bulk::<UrlSafe>(input, out, groups)
        } else {
            backend::encode_bulk::<Standard>(input, out, groups)
        };
        (done * 3, done * 4)
    }

    pub fn vector_decode_bulk(input: &[u8], out: &mut [u8], is_url_safe: bool) -> (usize, usize) {
        let quads = core::cmp::min(input.len() / 4, out.len() / 3);
        let done = if is_url_safe {
            backend::decode_bulk::<UrlSafe>(input, out, quads)
        } else {
            backend::decode_bulk::<Standard>(input, out, quads)
        };
        (done * 4, done * 3)
    }

    pub fn has_vector_path() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            x86::available() != x86::PORTABLE
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            cfg!(all(target_arch = "aarch64", target_feature = "neon"))
        }
    }

    /// Pin the codec to one path, for a test
    #[cfg(target_arch = "x86_64")]
    pub use crate::x86::{available, force, force_stream_floor, AVX2, PORTABLE, UNKNOWN, WIDE};
}
