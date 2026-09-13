//! Driving a conversion: the vector stage, the portable stage, then the tail

use crate::alphabet::Alphabet;
use crate::backend;
use crate::error::DecodeError;
use crate::scalar;

/// Write the encoding of `input` into `out`, which is exactly long enough
pub(crate) fn encode<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize, is_padded: bool) {
    let written = encode_groups::<A>(input, out, groups);
    debug_assert_eq!(written, groups * 4);
    scalar::encode_rest::<A>(&input[groups * 3..], &mut out[groups * 4..], is_padded);
}

/// Convert `groups` whole groups, returning how many characters were written
pub(crate) fn encode_groups<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    let room = out.len();
    // The clamps never fire; they keep the compiler from checking these bounds again.
    let done = backend::encode_bulk::<A>(input, out, groups);
    let (read, written) = ((done * 3).min(input.len()), (done * 4).min(room));
    let (input, out) = (&input[read..], &mut out[written..]);

    let done = scalar::encode_bulk::<A>(input, out, groups - done);
    (written + done * 4).min(room)
}

/// Write the decoding of `input`, which has no padding, into `out`
pub(crate) fn decode<A: Alphabet>(
    input: &[u8],
    out: &mut [u8],
    quads: usize,
) -> Result<usize, DecodeError> {
    let written = decode_quads::<A>(input, out, quads)?;
    debug_assert_eq!(written, quads * 3);
    let read = quads * 4;
    Ok(written + scalar::decode_rest::<A>(&input[read..], &mut out[quads * 3..], read)?)
}

/// Convert `quads` whole groups, refusing at a foreign character and keeping what came before
pub(crate) fn decode_quads<A: Alphabet>(
    input: &[u8],
    out: &mut [u8],
    quads: usize,
) -> Result<usize, DecodeError> {
    let (written, failure) = scan_quads::<A>(input, out, quads);
    match failure {
        Some(error) => Err(error),
        None => Ok(written),
    }
}

/// Convert `quads` whole groups, returning bytes written and the character that stopped it
pub(crate) fn scan_quads<A: Alphabet>(
    input: &[u8],
    out: &mut [u8],
    quads: usize,
) -> (usize, Option<DecodeError>) {
    let done = backend::decode_bulk::<A>(input, out, quads);
    let (taken, put) = ((done * 4).min(input.len()), (done * 3).min(out.len()));
    let (rest, tail) = (&input[taken..], &mut out[put..]);

    let more = scalar::decode_bulk::<A>(rest, tail, quads - done);
    let (read, wrote) = ((more * 4).min(rest.len()), (more * 3).min(tail.len()));

    // Four or more characters left means a stage stopped on a foreign character.
    if rest.len() - read >= 4 {
        let error = scalar::find_invalid::<A>(&rest[read..], taken + read);
        return (put + wrote, Some(error));
    }
    (put + wrote, None)
}
