//! The portable codec, and the tails and errors every other path leaves behind

use crate::alphabet::{Alphabet, INVALID};
use crate::error::DecodeError;

/// Convert up to `groups` whole 3-byte groups, returning how many
pub(crate) fn encode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    // Usually nothing where a vector path ran, so the setup below stays off it.
    if groups == 0 {
        return 0;
    }
    // A vector stage leaves at most three groups, and the paired walk below
    // spends two divisions working out how far to go. Three groups is fewer
    // instructions done than counted.
    if groups < 4 {
        let mut at = 0;
        while at < groups && at * 3 + 3 <= input.len() && at * 4 + 4 <= out.len() {
            let group = [input[at * 3], input[at * 3 + 1], input[at * 3 + 2]];
            encode_group::<A>(group, &mut out[at * 4..]);
            at += 1;
        }
        return groups;
    }
    let chars = A::ENCODE;
    let (taken, room) = (input.len(), out.len());
    debug_assert!(groups * 3 <= taken && groups * 4 <= room);
    let mut read = 0;
    let mut written = 0;

    // Two groups at a time, because 6 bytes fit one word and the 8 characters
    // they spell fit another. The clamps never fire.
    let pairs = groups / 2;
    for (group, slot) in input[..(pairs * 6).min(taken)]
        .chunks_exact(6)
        .zip(out[..(pairs * 8).min(room)].chunks_exact_mut(8))
    {
        let mut word = [0u8; 8];
        word[..6].copy_from_slice(group);
        let word = u64::from_be_bytes(word) >> 16;
        for (place, spelled) in slot.iter_mut().enumerate() {
            *spelled = chars[((word >> (42 - place * 6)) & 0x3F) as usize];
        }
        read += 6;
        written += 8;
    }

    let rest = groups - pairs * 2;
    for (group, slot) in input[read.min(taken)..(read + rest * 3).min(taken)]
        .chunks_exact(3)
        .zip(out[written.min(room)..(written + rest * 4).min(room)].chunks_exact_mut(4))
    {
        let value = (u32::from(group[0]) << 16) | (u32::from(group[1]) << 8) | u32::from(group[2]);
        for (place, spelled) in slot.iter_mut().enumerate() {
            *spelled = chars[((value >> (18 - place * 6)) & 0x3F) as usize];
        }
    }

    groups
}

/// Spell one whole group into four characters
pub(crate) fn encode_group<A: Alphabet>(group: [u8; 3], out: &mut [u8]) {
    let value = (u32::from(group[0]) << 16) | (u32::from(group[1]) << 8) | u32::from(group[2]);
    for (place, spelled) in out[..4].iter_mut().enumerate() {
        *spelled = A::ENCODE[((value >> (18 - place * 6)) & 0x3F) as usize];
    }
}

/// Convert the one or two bytes a group short of three, with padding if asked
pub(crate) fn encode_rest<A: Alphabet>(input: &[u8], out: &mut [u8], is_padded: bool) {
    let chars = A::ENCODE;
    // Matched as a pattern, so nothing needs a length check.
    let (spelled, count) = match input {
        [byte] => {
            let value = *byte as usize;
            let spelled = [chars[value >> 2], chars[(value << 4) & 0x3F], b'=', b'='];
            (spelled, if is_padded { 4 } else { 2 })
        }
        [high, low] => {
            let value = ((*high as usize) << 8) | (*low as usize);
            let spelled = [
                chars[value >> 10],
                chars[(value >> 4) & 0x3F],
                chars[(value << 2) & 0x3F],
                b'=',
            ];
            (spelled, if is_padded { 4 } else { 3 })
        }
        _ => return,
    };
    if out.len() >= count {
        out[..count].copy_from_slice(&spelled[..count]);
    }
}

/// Convert up to `quads` whole 4-character groups, returning how many
///
/// Stops at the first group holding a character outside the alphabet rather
/// than reporting it, so a vector path can hand a suspect block over.
pub(crate) fn decode_bulk<A: Alphabet>(input: &[u8], out: &mut [u8], quads: usize) -> usize {
    if quads == 0 {
        return 0;
    }
    let values = A::DECODE;
    let (taken, room) = (input.len(), out.len());
    debug_assert!(quads * 4 <= taken && quads * 3 <= room);
    let mut done = 0;

    for (group, slot) in input[..(quads * 4).min(taken)]
        .chunks_exact(4)
        .zip(out[..(quads * 3).min(room)].chunks_exact_mut(3))
    {
        let first = values[group[0] as usize];
        let second = values[group[1] as usize];
        let third = values[group[2] as usize];
        let fourth = values[group[3] as usize];
        // `INVALID` is all ones, so one bad byte of the four shows in the or.
        if (first | second | third | fourth) == INVALID {
            break;
        }
        let value = (u32::from(first) << 18)
            | (u32::from(second) << 12)
            | (u32::from(third) << 6)
            | u32::from(fourth);
        for (place, byte) in slot.iter_mut().enumerate() {
            *byte = (value >> (16 - place * 8)) as u8;
        }
        done += 1;
    }

    done
}

/// Name the first character of `input` that is outside the alphabet
pub(crate) fn find_invalid<A: Alphabet>(input: &[u8], offset: usize) -> DecodeError {
    let values = A::DECODE;
    for (index, &byte) in input.iter().enumerate() {
        if values[byte as usize] == INVALID {
            return DecodeError::InvalidByte {
                position: offset + index,
                byte,
            };
        }
    }
    // Unreachable through the codec, which calls this only after a failure.
    DecodeError::InvalidLength {
        length: offset + input.len(),
    }
}

/// Convert the two or three characters a group short of four
pub(crate) fn decode_rest<A: Alphabet>(
    input: &[u8],
    out: &mut [u8],
    offset: usize,
) -> Result<usize, DecodeError> {
    let values = A::DECODE;
    // Every value outside the alphabet has its top bit set.
    match (input, out) {
        ([first, second], [byte, ..]) => {
            let (high, low) = (values[*first as usize], values[*second as usize]);
            if (high | low) & 0x80 != 0 {
                return Err(find_invalid::<A>(input, offset));
            }
            // These bits have no room in the output, so two spellings would
            // otherwise decode alike.
            if low & 0x0F != 0 {
                return Err(DecodeError::InvalidLastSymbol {
                    position: offset + 1,
                    byte: *second,
                });
            }
            *byte = (high << 2) | (low >> 4);
            Ok(1)
        }
        ([first, second, third], [one, two, ..]) => {
            let (high, middle, low) = (
                values[*first as usize],
                values[*second as usize],
                values[*third as usize],
            );
            if (high | middle | low) & 0x80 != 0 {
                return Err(find_invalid::<A>(input, offset));
            }
            if low & 0x03 != 0 {
                return Err(DecodeError::InvalidLastSymbol {
                    position: offset + 2,
                    byte: *third,
                });
            }
            *one = (high << 2) | (middle >> 4);
            *two = (middle << 4) | (low >> 2);
            Ok(2)
        }
        _ => Ok(0),
    }
}
