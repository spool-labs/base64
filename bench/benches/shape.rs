//! Where the time goes at sizes too small for the loop to matter
//!
//! Four rows at each length. `nothing` is an empty timed body, which every
//! other row includes. `copy` moves the same bytes for scale. `kernel` is the
//! vector stage alone, and `codec` is the whole public call.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

/// Every block boundary, and the awkward lengths just past each
const SIZES: [usize; 16] = [1, 3, 12, 21, 24, 30, 32, 33, 45, 48, 51, 64, 72, 96, 120, 144];

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

fn encode(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("shape-encode");
    for size in SIZES {
        let input = data(size);
        let mut out = vec![0u8; size / 3 * 4 + 8];
        let mut copy = vec![0u8; size];

        group.bench_with_input(BenchmarkId::new("nothing", size), &size, |bencher, _| {
            bencher.iter(|| black_box(black_box(&input).len() + black_box(&mut out).len()))
        });
        group.bench_with_input(BenchmarkId::new("copy", size), &size, |bencher, _| {
            bencher.iter(|| black_box(&mut copy).copy_from_slice(black_box(&input)))
        });
        group.bench_with_input(BenchmarkId::new("kernel", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::testing::vector_encode_bulk(black_box(&input), black_box(&mut out), false))
        });
        group.bench_with_input(BenchmarkId::new("codec", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.encode(black_box(&input), black_box(&mut out)).unwrap())
        });
    }
    group.finish();
}

fn decode(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("shape-decode");
    for size in SIZES {
        let input = data(size);
        let mut encoded = vec![0u8; size / 3 * 4 + 8];
        let chars = tape_base64::STANDARD.encode(&input, &mut encoded).unwrap();
        let encoded = &encoded[..chars];
        let mut out = vec![0u8; size + 8];

        group.bench_with_input(BenchmarkId::new("nothing", size), &size, |bencher, _| {
            bencher.iter(|| black_box(black_box(encoded).len() + black_box(&mut out).len()))
        });
        group.bench_with_input(BenchmarkId::new("kernel", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::testing::vector_decode_bulk(black_box(encoded), black_box(&mut out), false))
        });
        group.bench_with_input(BenchmarkId::new("codec", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.decode(black_box(encoded), black_box(&mut out)).unwrap())
        });
    }
    group.finish();
}

criterion_group!(benches, encode, decode);
criterion_main!(benches);
