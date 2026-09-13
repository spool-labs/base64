//! Exports timed from JavaScript: each call converts the same buffer `iters` times.

use core::hint::black_box;

use base64::Engine as _;

/// Leak a buffer for the harness to fill and read.
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    Box::leak(vec![0u8; len].into_boxed_slice()).as_mut_ptr()
}

/// The longest encoding `len` bytes can have, for sizing the output.
#[no_mangle]
pub extern "C" fn encoded_len(len: usize) -> usize {
    tape_base64::encoded_len(len).unwrap()
}

/// Encode `iters` times with crate `which`, returning the characters written.
///
/// # Safety
/// `input` and `out` are buffers the harness allocated with `alloc`, of the lengths given.
#[no_mangle]
pub unsafe extern "C" fn encode(which: u32, input: *const u8, len: usize, out: *mut u8, out_len: usize, iters: u32) -> usize {
    let input = core::slice::from_raw_parts(input, len);
    let out = core::slice::from_raw_parts_mut(out, out_len);
    let mut written = 0;
    for _ in 0..iters {
        written = match which {
            0 => tape_base64::STANDARD.encode(black_box(input), black_box(&mut *out)).unwrap(),
            1 => base64_simd::STANDARD.encode(black_box(input), base64_simd::Out::from_slice(black_box(&mut *out))).len(),
            _ => base64::engine::general_purpose::STANDARD.encode_slice(black_box(input), black_box(&mut *out)).unwrap(),
        };
        black_box(&*out);
    }
    written
}

/// Decode `iters` times with crate `which`, returning the bytes written, or usize::MAX on a refusal.
///
/// # Safety
/// `input` and `out` are buffers the harness allocated with `alloc`, of the lengths given.
#[no_mangle]
pub unsafe extern "C" fn decode(which: u32, input: *const u8, len: usize, out: *mut u8, out_len: usize, iters: u32) -> usize {
    let input = core::slice::from_raw_parts(input, len);
    let out = core::slice::from_raw_parts_mut(out, out_len);
    let mut written = 0;
    for _ in 0..iters {
        written = match which {
            0 => match tape_base64::STANDARD.decode(black_box(input), black_box(&mut *out)) {
                Ok(n) => n,
                Err(_) => usize::MAX,
            },
            1 => match base64_simd::STANDARD.decode(black_box(input), base64_simd::Out::from_slice(black_box(&mut *out))) {
                Ok(bytes) => bytes.len(),
                Err(_) => usize::MAX,
            },
            _ => match base64::engine::general_purpose::STANDARD.decode_slice(black_box(input), black_box(&mut *out)) {
                Ok(n) => n,
                Err(_) => usize::MAX,
            },
        };
        black_box(&*out);
    }
    written
}
