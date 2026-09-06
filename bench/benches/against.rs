//! This crate against the ones on crates.io, and against Firedancer

use std::hint::black_box;

use base64::Engine as _;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

/// A pubkey, a signature, a Solana packet, and sizes where only the loop is left
const SIZES: [usize; 8] = [32, 64, 128, 1232, 4096, 16_384, 65_536, 1_048_576];

fn data(len: usize) -> Vec<u8> {
    let mut state = 0x243F_6A88_85A3_08D3u64;
    (0..len)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_F491_4F6C_DD1D) as u8
        })
        .collect()
}

unsafe extern "C" {
    /// Firedancer's `fd_base64_encode`, which always pads
    fn fd_base64_encode(out: *mut u8, input: *const u8, len: usize) -> usize;
    /// Firedancer's `fd_base64_decode`, which returns -1 on a refusal
    fn fd_base64_decode(out: *mut u8, input: *const u8, len: usize) -> isize;
}

/// Every contender has to agree with this crate before it is worth timing
fn check_agreement(input: &[u8]) {
    let mut mine = vec![0u8; input.len() / 3 * 4 + 8];
    let chars = tape_base64::STANDARD.encode(input, &mut mine).unwrap();

    let mut theirs = vec![0u8; mine.len()];
    // SAFETY: the buffer is longer than the padded encoding of `input`, which
    // is all `fd_base64_encode` writes.
    let count = unsafe { fd_base64_encode(theirs.as_mut_ptr(), input.as_ptr(), input.len()) };
    assert_eq!(&theirs[..count], &mine[..chars], "firedancer encode differs");

    let mut back = vec![0u8; input.len() + 8];
    // SAFETY: the buffer is longer than the decoding of `chars` characters.
    let count = unsafe { fd_base64_decode(back.as_mut_ptr(), mine.as_ptr().cast(), chars) };
    assert_eq!(count, input.len() as isize, "firedancer decode differs");
    assert_eq!(&back[..input.len()], input, "firedancer decode differs");

    assert_eq!(
        base64::engine::general_purpose::STANDARD.encode(input).as_bytes(),
        &mine[..chars],
        "the base64 crate encode differs"
    );
}

fn simd_engine() -> base64::engine::simd::Simd {
    base64::engine::simd::Simd::standard(base64::engine::general_purpose::GeneralPurposeConfig::new())
}

fn encode(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("encode");
    let simd = simd_engine();
    for size in SIZES {
        let input = data(size);
        check_agreement(&input);
        let mut out = vec![0u8; size / 3 * 4 + 8];
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("tape-base64", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.encode(black_box(&input), black_box(&mut out)).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("base64-simd", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_simd::STANDARD
                    .encode(
                        black_box(&input),
                        base64_simd::Out::from_slice(black_box(&mut out)),
                    )
                    .len()
            })
        });
        group.bench_with_input(BenchmarkId::new("base64-turbo", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_turbo::STANDARD
                    .encode_into(black_box(&input[..]), black_box(&mut out))
                    .unwrap()
            })
        });
        group.bench_with_input(BenchmarkId::new("base64-simd-engine", size), &size, |bencher, _| {
            bencher.iter(|| simd.encode_slice(black_box(&input), black_box(&mut out)).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("base64", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64::engine::general_purpose::STANDARD
                    .encode_slice(black_box(&input), black_box(&mut out))
                    .unwrap()
            })
        });
        group.bench_with_input(BenchmarkId::new("firedancer", size), &size, |bencher, _| {
            // SAFETY: `out` is longer than the padded encoding of `input`.
            bencher.iter(|| unsafe {
                fd_base64_encode(
                    black_box(out.as_mut_ptr()),
                    black_box(input.as_ptr()),
                    black_box(input.len()),
                )
            })
        });
    }
    group.finish();
}

fn decode(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("decode");
    let simd = simd_engine();
    for size in SIZES {
        let input = data(size);
        let mut encoded = vec![0u8; size / 3 * 4 + 8];
        let chars = tape_base64::STANDARD.encode(&input, &mut encoded).unwrap();
        let encoded = &encoded[..chars];
        let mut out = vec![0u8; size + 8];
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("tape-base64", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.decode(black_box(encoded), black_box(&mut out)).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("base64-simd", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_simd::STANDARD
                    .decode(black_box(encoded), base64_simd::Out::from_slice(black_box(&mut out)))
                    .unwrap()
                    .len()
            })
        });
        group.bench_with_input(BenchmarkId::new("base64-turbo", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_turbo::STANDARD
                    .decode_into(black_box(encoded), black_box(&mut out))
                    .unwrap()
            })
        });
        group.bench_with_input(BenchmarkId::new("base64-simd-engine", size), &size, |bencher, _| {
            bencher.iter(|| simd.decode_slice(black_box(encoded), black_box(&mut out)).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("base64", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64::engine::general_purpose::STANDARD
                    .decode_slice(black_box(encoded), black_box(&mut out))
                    .unwrap()
            })
        });
        group.bench_with_input(BenchmarkId::new("firedancer", size), &size, |bencher, _| {
            // SAFETY: `out` is longer than the decoding of `encoded`.
            bencher.iter(|| unsafe {
                fd_base64_decode(
                    black_box(out.as_mut_ptr()),
                    black_box(encoded.as_ptr().cast()),
                    black_box(encoded.len()),
                )
            })
        });
    }
    group.finish();
}

criterion_group!(benches, encode, decode);
criterion_main!(benches);
