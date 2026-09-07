//! Driving a conversion: the vector stage, the portable stage, then the tail

use crate::alphabet::Alphabet;
use crate::backend;
use crate::error::DecodeError;
use crate::scalar;

/// Write the encoding of `input` into `out`, which is exactly long enough
pub(crate) fn encode<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize, is_padded: bool) {
    let written = encode_groups::<A>(input, out, groups);
    // Where the tail goes was settled before the stages ran, and saying so
    // rather than dividing the count back down leaves the two independent.
    debug_assert_eq!(written, groups * 4);
    scalar::encode_rest::<A>(&input[groups * 3..], &mut out[groups * 4..], is_padded);
}

/// Convert `groups` whole groups, returning how many characters were written
///
/// The caller has checked there is room for all of them on both sides.
pub(crate) fn encode_groups<A: Alphabet>(input: &[u8], out: &mut [u8], groups: usize) -> usize {
    let room = out.len();
    // The clamps never fire. They tell the compiler each stage stayed inside
    // what it was given, which keeps a bounds check out of a call that cannot fail.
    let done = backend::encode_bulk::<A>(input, out, groups);
    let (read, written) = ((done * 3).min(input.len()), (done * 4).min(room));
    let (input, out) = (&input[read..], &mut out[written..]);

    let done = scalar::encode_bulk::<A>(input, out, groups - done);
    (written + done * 4).min(room)
}

/// Write the decoding of `input`, which carries no padding, into `out`
pub(crate) fn decode<A: Alphabet>(
    input: &[u8],
    out: &mut [u8],
    quads: usize,
) -> Result<usize, DecodeError> {
    let written = decode_quads::<A>(input, out, quads)?;
    // Every group converted, or the call above would have refused, so the tail
    // sits where the caller's own count put it.
    debug_assert_eq!(written, quads * 3);
    let read = quads * 4;
    Ok(written + scalar::decode_rest::<A>(&input[read..], &mut out[quads * 3..], read)?)
}

/// Convert `quads` whole groups, refusing any that holds a foreign character
///
/// The caller has checked there is room for all of them on both sides. What
/// was converted before the refusal stays written, as in `decode`.
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

/// Convert `quads` whole groups, stopping at the first foreign character
///
/// Returns how many bytes were written and, when a group was left
/// unconverted, the character that stopped it, counted in `input`.
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

    // Four or more characters left means a stage stopped on a character
    // outside the alphabet rather than on a partial group.
    if rest.len() - read >= 4 {
        let error = scalar::find_invalid::<A>(&rest[read..], taken + read);
        return (put + wrote, Some(error));
    }
    (put + wrote, None)
}
