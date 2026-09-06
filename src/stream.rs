//! Conversions that take their input a piece at a time
//!
//! Each holds the few characters a piece boundary can leave behind, so the
//! pieces of an input convert to exactly what the pieces joined convert to.

use crate::alphabet::{Alphabet, Standard, UrlSafe};
use crate::codec;
use crate::error::{DecodeError, EncodeError};
use crate::scalar;
use crate::{Codec, Kind};

/// An encode taken a piece at a time
///
/// Made by the encoder method on `Codec`. Pieces arrive in any sizes, and
/// the encoding `finish` completes is the encoding of the pieces joined.
pub struct Encoder {
    kind: Kind,
    is_padded: bool,
    /// Bytes carried until the group they started is whole
    held: [u8; 2],
    held_len: u8,
}

impl Codec {
    /// An encoder that takes its input a piece at a time
    pub fn encoder(&self) -> Encoder {
        Encoder {
            kind: self.kind,
            is_padded: self.is_padded,
            held: [0; 2],
            held_len: 0,
        }
    }

    /// A decoder that takes its input a piece at a time
    pub fn decoder(&self) -> Decoder {
        Decoder {
            kind: self.kind,
            is_padded: self.is_padded,
            quad: [0; 3],
            quad_len: 0,
            total: 0,
            run: 0,
            pending: None,
            state: State::Reading,
        }
    }
}

impl Encoder {
    /// Encode all of `input`, writing into `out`
    ///
    /// Returns how many characters were written. The call refuses a buffer
    /// too short for the whole piece rather than convert part of it, so a
    /// refusal consumes nothing.
    pub fn push(&mut self, input: &[u8], out: &mut [u8]) -> Result<usize, EncodeError> {
        let groups = (usize::from(self.held_len) + input.len()) / 3;
        let needed = groups * 4;
        if out.len() < needed {
            return Err(EncodeError::BufferTooSmall { needed });
        }
        match self.kind {
            Kind::Standard => self.pushed::<Standard>(input, out, groups),
            Kind::UrlSafe => self.pushed::<UrlSafe>(input, out, groups),
        }
    }

    /// Write the group any held bytes belong to, and with it the encoding
    ///
    /// An unpadded codec that ends between groups simply ends there.
    pub fn finish(self, out: &mut [u8]) -> Result<usize, EncodeError> {
        let held = usize::from(self.held_len);
        let count = match (held, self.is_padded) {
            (0, _) => 0,
            (_, true) => 4,
            (_, false) => held + 1,
        };
        if out.len() < count {
            return Err(EncodeError::BufferTooSmall { needed: count });
        }
        match self.kind {
            Kind::Standard => {
                scalar::encode_rest::<Standard>(&self.held[..held], out, self.is_padded)
            }
            Kind::UrlSafe => {
                scalar::encode_rest::<UrlSafe>(&self.held[..held], out, self.is_padded)
            }
        }
        Ok(count)
    }

    /// Convert `groups` whole groups out of the held bytes and `input`
    fn pushed<A: Alphabet>(
        &mut self,
        input: &[u8],
        out: &mut [u8],
        groups: usize,
    ) -> Result<usize, EncodeError> {
        let held = usize::from(self.held_len);
        let mut consumed = 0;
        let mut written = 0;

        // A group the last piece left short of three finishes here, out of
        // what this piece starts with.
        if groups > 0 && held > 0 {
            let mut group = [0u8; 3];
            group[..held].copy_from_slice(&self.held[..held]);
            group[held..].copy_from_slice(&input[..3 - held]);
            scalar::encode_group::<A>(group, &mut out[..4]);
            self.held_len = 0;
            consumed = 3 - held;
            written = 4;
        }

        written += codec::encode_groups::<A>(
            &input[consumed..],
            &mut out[written..],
            groups - written / 4,
        );

        // What is left waits with nothing behind it. Past a whole group that
        // is up to two bytes of this piece, before one the piece joins what
        // was already held.
        let spare = (held + input.len()) % 3;
        if groups > 0 {
            self.held[..spare].copy_from_slice(&input[input.len() - spare..]);
            self.held_len = spare as u8;
        } else {
            self.held[held..held + input.len()].copy_from_slice(input);
            self.held_len = (held + input.len()) as u8;
        }
        Ok(written)
    }
}

/// A decode taken a piece at a time
///
/// Made by the decoder method on `Codec`. It reads where `Codec::decode`
/// reads and refuses what it refuses, offsets included. What it cannot name
/// until the input ends, it holds until `finish`.
pub struct Decoder {
    kind: Kind,
    is_padded: bool,
    /// Characters of a group the pieces have filled so far
    quad: [u8; 3],
    quad_len: u8,
    /// Characters accounted for so far, padding included
    total: usize,
    /// Padding characters at the end of what has been read, three deciding a case
    run: u8,
    /// A group whose last character is padding. It is an ending exactly when
    /// nothing follows, so both its bytes and its judgment wait.
    pending: Option<Pending>,
    state: State,
}

/// A group that may be an ending
struct Pending {
    /// The characters before the padding
    group: [u8; 3],
    /// How many of them carry bits, two or three
    group_len: u8,
    /// Where the padding starts, counted over the whole input
    pad: usize,
}

/// Where a reading stands
enum State {
    /// Whole groups convert as they complete
    Reading,
    /// A padded ending converted, and only a quiet end can follow it
    Ended {
        /// Character count when the ending completed
        at: usize,
        /// Where the ending's first padding character sits
        pad: usize,
    },
    /// A refusal became certain, and only its naming waits for the end
    Refused(Option<DecodeError>),
}

impl Decoder {
    /// Decode all of `input`, writing into `out`
    ///
    /// Returns how many bytes were written. The call asks for an upper bound
    /// of what the piece can write, which a refusal found partway may leave
    /// partly unspent. What comes before a refusal stays written.
    pub fn push(&mut self, input: &[u8], out: &mut [u8]) -> Result<usize, DecodeError> {
        match self.state {
            State::Reading => {}
            State::Ended { .. } | State::Refused(_) => {
                self.tally(input);
                return Ok(0);
            }
        }
        if input.is_empty() {
            return Ok(0);
        }
        let needed = (usize::from(self.quad_len) + input.len()) / 4 * 3 + 2;
        if out.len() < needed {
            return Err(DecodeError::BufferTooSmall { needed });
        }
        match self.kind {
            Kind::Standard => self.read::<Standard>(input, out),
            Kind::UrlSafe => self.read::<UrlSafe>(input, out),
        }
    }

    /// Complete the decode, writing what the ending held back
    ///
    /// The result is what `Codec::decode` returns for the pieces joined,
    /// errors included.
    pub fn finish(mut self, out: &mut [u8]) -> Result<usize, DecodeError> {
        let held = usize::from(self.quad_len);
        match core::mem::replace(&mut self.state, State::Reading) {
            State::Reading => {
                self.check_length()?;
                // A group held over with padding aboard is the ending the
                // input stopped on, which nothing has judged until now.
                if let Some(pending) = self.pending.take() {
                    return self.settle(pending, out);
                }
                if held > 0 {
                    let offset = self.total - held;
                    return match self.kind {
                        Kind::Standard => {
                            scalar::decode_rest::<Standard>(&self.quad[..held], out, offset)
                        }
                        Kind::UrlSafe => {
                            scalar::decode_rest::<UrlSafe>(&self.quad[..held], out, offset)
                        }
                    };
                }
                Ok(0)
            }
            State::Ended { at, pad } if self.total == at => {
                // An ending is still the wrong length when its groups were short.
                self.check_length()?;
                match self.pending.take() {
                    // The ending stood, and its characters become bytes here.
                    Some(pending) => self.settle(pending, out),
                    None => Ok(0),
                }
            }
            State::Ended { pad, .. } => self.name(Some(DecodeError::InvalidByte {
                position: pad,
                byte: b'=',
            })),
            State::Refused(cause) => self.name(cause),
        }
    }

    /// Read a piece in the steady state, where whole groups convert as they land
    fn read<A: Alphabet>(&mut self, input: &[u8], out: &mut [u8]) -> Result<usize, DecodeError> {
        // A group held over with padding aboard is decided by this piece's
        // first character, and nothing else today matters.
        let mut consumed = 0;
        let mut written = 0;
        if let Some(pending) = self.pending.take() {
            let Some(&byte) = input.first() else {
                self.pending = Some(pending);
                return Ok(0);
            };
            consumed = 1;
            self.total += 1;
            if byte != b'=' {
                let cause = DecodeError::InvalidByte {
                    position: pending.pad,
                    byte: b'=',
                };
                return Err(self.refuse(Some(cause), 0, &input[consumed..]));
            }
            // The padding repeated, so the ending held. Its bytes wait for
            // the end, as does the judgment of whatever follows it.
            self.state = State::Ended {
                at: self.total,
                pad: pending.pad,
            };
            self.pending = Some(pending);
            self.tally(&input[consumed..]);
            return Ok(written);
        }

        let held = usize::from(self.quad_len);

        // A group the last piece left short of four completes here. It goes
        // alone, so the bulk stage below keeps meeting whole quads. Its
        // first characters were counted when earlier pieces read them.
        if held > 0 && input.len() >= 4 - held {
            let take = 4 - held;
            // A quad closed by padding waits on the character loop instead,
            // which weighs its padding one character at a time.
            let is_ending = self.is_padded && input[take - 1] == b'=';
            if !is_ending {
                let mut quad = [0u8; 4];
                quad[..held].copy_from_slice(&self.quad[..held]);
                quad[held..].copy_from_slice(&input[..take]);
                match self.take_quad::<A>(&quad, self.total - held, &mut out[written..]) {
                    Ok(count) => {
                        written += count;
                        self.total += take;
                    }
                    Err(cause) => {
                        self.total += take;
                        return Err(self.refuse(Some(cause), 0, &input[take..]));
                    }
                }
                self.quad_len = 0;
                consumed = take;
            }
        }

        // Whole quads from here convert in runs, at kernel speed. A run
        // stops at the first character no encoding carries, which for
        // padding is not yet a refusal: only a group's own conversion knows
        // whether an ending stood there.
        // A run holding padding is refused whole and handed back, so a piece
        // that ends in padding keeps its last group for the character loop,
        // which is what weighs padding anyway.
        let ending = if self.is_padded && input.last() == Some(&b'=') { 4 } else { 0 };
        let whole = (input.len() - consumed).saturating_sub(ending) / 4;
        if whole > 0 {
            let base = self.total;
            let began = consumed;
            let (bytes, failure) =
                codec::scan_quads::<A>(&input[consumed..consumed + whole * 4], &mut out[written..], whole);
            written += bytes;
            let taken = bytes / 3 * 4;
            consumed += taken;
            self.total += taken;

            if let Some(DecodeError::InvalidByte { position: local, byte }) = failure {
                if !self.is_padded || byte != b'=' {
                    self.total += local + 1 - taken;
                    let cause = DecodeError::InvalidByte {
                        position: base + local,
                        byte,
                    };
                    return Err(self.refuse(Some(cause), 0, &input[began + local + 1..]));
                }
            }
        }

        // What is left is short of a group, or carries the padding of an
        // ending. Neither commits until the input does.
        for index in consumed..input.len() {
            let byte = input[index];
            let rest = &input[index + 1..];
            self.total += 1;

            // A group held over with padding becomes an ending exactly when
            // this, its next character, is padding too.
            if let Some(pending) = self.pending.take() {
                if byte != b'=' {
                    let cause = DecodeError::InvalidByte {
                        position: pending.pad,
                        byte: b'=',
                    };
                    return Err(self.refuse(Some(cause), 0, rest));
                }
                // The padding repeated, so the ending held. Its bytes wait
                // for the end, as does the judgment of whatever follows it.
                self.state = State::Ended {
                    at: self.total,
                    pad: pending.pad,
                };
                self.pending = Some(pending);
                self.tally(rest);
                return Ok(written);
            }

            let held = usize::from(self.quad_len);

            // Padding ends a group, and whether it ends the input too is
            // the next character's news. Every character parked before it
            // has been found in the alphabet on arrival.
            if self.is_padded && byte == b'=' && held >= 2 {
                self.pending = Some(Pending {
                    group: [self.quad[0], self.quad[1], self.quad[2]],
                    group_len: held as u8,
                    pad: self.total - 1,
                });
                continue;
            }

            // Anything the alphabet has no use for, padding included, is
            // named where a whole-input decode would name it.
            if A::DECODE[byte as usize] == crate::alphabet::INVALID {
                let position = self.total - 1;
                return Err(self.refuse(
                    Some(DecodeError::InvalidByte { position, byte }),
                    u8::from(self.is_padded && byte == b'='),
                    rest,
                ));
            }

            if held == 3 {
                // The group completes here, padding or not.
                let quad = [self.quad[0], self.quad[1], self.quad[2], byte];
                match self.take_quad::<A>(&quad, self.total - 4, &mut out[written..]) {
                    Ok(count) => written += count,
                    Err(cause) => {
                        return Err(self.refuse(Some(cause), 0, rest));
                    }
                }
                self.quad_len = 0;
            } else {
                self.quad[held] = byte;
                self.quad_len += 1;
            }
        }
        Ok(written)
    }

    /// Convert one whole quad into the room at the start of `out`
    ///
    /// Refuses the first character of the quad outside the alphabet, named
    /// at `base` plus its place. A quad padded at its end never reaches
    /// here, since only what follows can say whether it was an ending.
    fn take_quad<A: Alphabet>(
        &mut self,
        quad: &[u8; 4],
        base: usize,
        out: &mut [u8],
    ) -> Result<usize, DecodeError> {
        let values = [
            A::DECODE[quad[0] as usize],
            A::DECODE[quad[1] as usize],
            A::DECODE[quad[2] as usize],
            A::DECODE[quad[3] as usize],
        ];
        if let Some(place) = values.iter().position(|&value| value == crate::alphabet::INVALID) {
            return Err(DecodeError::InvalidByte {
                position: base + place,
                byte: quad[place],
            });
        }
        let value = (u32::from(values[0]) << 18)
            | (u32::from(values[1]) << 12)
            | (u32::from(values[2]) << 6)
            | u32::from(values[3]);
        for (place, slot) in out[..3].iter_mut().enumerate() {
            *slot = (value >> (16 - place * 8)) as u8;
        }
        Ok(3)
    }

    /// Write a held group out, as the ending it has now been proven to be
    ///
    /// The characters sit one group back from the end of the input.
    fn settle(&mut self, pending: Pending, out: &mut [u8]) -> Result<usize, DecodeError> {
        let held = usize::from(pending.group_len);
        let offset = self.total - 4;
        let group = &pending.group[..held];
        let written = match self.kind {
            Kind::Standard => scalar::decode_rest::<Standard>(group, out, offset)?,
            Kind::UrlSafe => scalar::decode_rest::<UrlSafe>(group, out, offset)?,
        };
        self.state = State::Ended {
            at: self.total,
            pad: pending.pad,
        };
        Ok(written)
    }

    /// Move into the refused state, counting the characters behind the cause
    fn refuse(&mut self, cause: Option<DecodeError>, run: u8, rest: &[u8]) -> DecodeError {
        self.state = State::Refused(cause);
        self.quad_len = 0;
        self.pending = None;
        self.run = run;
        self.tally(rest);
        cause.unwrap_or(DecodeError::InvalidPadding {
            position: self.total.saturating_sub(3),
        })
    }

    /// Count characters that arrive too late to change anything
    ///
    /// A held group was waiting for exactly one character to say the input
    /// went on, and the first of these is it.
    fn tally(&mut self, input: &[u8]) {
        let Some((&first, rest)) = input.split_first() else {
            return;
        };
        self.total += 1;
        if let Some(pending) = self.pending.take() {
            let cause = DecodeError::InvalidByte {
                position: pending.pad,
                byte: b'=',
            };
            self.state = State::Refused(Some(cause));
        }
        self.run = if self.is_padded && first == b'=' {
            self.run.min(2) + 1
        } else {
            0
        };

        // Only the last three characters can leave a run of padding behind,
        // and three is where the count stops.
        self.total += rest.len();
        for &byte in &rest[rest.len().saturating_sub(3)..] {
            self.run = if self.is_padded && byte == b'=' {
                self.run.min(2) + 1
            } else {
                0
            };
        }
    }

    /// Name a refusal the way a whole-input decode would
    fn name(&self, cause: Option<DecodeError>) -> Result<usize, DecodeError> {
        self.check_length()?;
        if self.run >= 3 {
            return Err(DecodeError::InvalidPadding {
                position: self.total - 3,
            });
        }
        Err(cause.unwrap_or(DecodeError::InvalidPadding {
            position: self.total.saturating_sub(3),
        }))
    }

    /// Apply the length rule a whole-input decode applies before reading
    fn check_length(&self) -> Result<(), DecodeError> {
        let valid = if self.is_padded {
            self.total.is_multiple_of(4)
        } else {
            self.total % 4 != 1
        };
        if valid {
            Ok(())
        } else {
            Err(DecodeError::InvalidLength {
                length: self.total,
            })
        }
    }
}
