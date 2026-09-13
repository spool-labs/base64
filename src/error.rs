//! What a conversion refuses, and where

use core::fmt;

/// Why an encode could not be written
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodeError {
    /// The output buffer is shorter than the encoding needs
    BufferTooSmall {
        /// Bytes the encoding needs
        needed: usize,
    },
    /// The encoding of an input this long does not fit in a `usize`
    TooLong,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::BufferTooSmall { needed } => {
                write!(f, "output buffer too small, needs {needed} bytes")
            }
            EncodeError::TooLong => f.write_str("input too long to encode"),
        }
    }
}

/// Why a decode could not be completed
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    /// A byte that is not in the alphabet, at this offset in the input
    InvalidByte {
        /// Offset of the byte in the input
        position: usize,
        /// The byte itself
        byte: u8,
    },
    /// A character count that no encoding can produce
    InvalidLength {
        /// The length that was refused
        length: usize,
    },
    /// The final character has bits that the decoded output cannot hold
    InvalidLastSymbol {
        /// Offset of the character in the input
        position: usize,
        /// The character itself
        byte: u8,
    },
    /// Padding that is absent, too long, or not at the end
    InvalidPadding {
        /// Offset at which the padding went wrong
        position: usize,
    },
    /// The output buffer is shorter than the decoding needs
    BufferTooSmall {
        /// Bytes the decoding needs
        needed: usize,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::InvalidByte { position, byte } => {
                write!(f, "invalid byte {byte:#04x} at offset {position}")
            }
            DecodeError::InvalidLength { length } => {
                write!(f, "invalid encoded length {length}")
            }
            DecodeError::InvalidLastSymbol { position, byte } => {
                write!(
                    f,
                    "symbol {byte:#04x} at offset {position} has bits to spare"
                )
            }
            DecodeError::InvalidPadding { position } => {
                write!(f, "invalid padding at offset {position}")
            }
            DecodeError::BufferTooSmall { needed } => {
                write!(f, "output buffer too small, needs {needed} bytes")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for EncodeError {}

#[cfg(feature = "std")]
impl std::error::Error for DecodeError {}
