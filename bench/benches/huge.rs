//! Sizes that cross out of L2, out of L3, and into memory
//!
//! `base64-turbo` switches to non-temporal stores above 4 MiB, a threshold it
//! measured on a 9 MiB last level cache. A part with a much larger one reaches
//! that threshold while the working set still fits, so this walks the sizes on
//! either side of it.
//!
//! Criterion reuses one output buffer across iterations, so the destination is
//! warm in a way a one-shot caller's would not be. That is the same harness the
//! other benches use, and it is the condition under which bypassing the cache
//! is worst, so read these as the warm-destination case rather than the only
//! one.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

/// 1 MiB through 128 MiB, spanning L2, L3 and memory on any current part
const SIZES: [usize; 8] = [
    1 << 20,
    2 << 20,
    4 << 20,
    8 << 20,
    16 << 20,
    32 << 20,
    64 << 20,
    128 << 20,
];

fn data(len: usize) -> Vec<u8> {
    let mut state = 0x243F_6A88_85A3_08D3u64;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        out.push(state.wrapping_mul(0x2545_F491_4F6C_DD1D) as u8);
    }
    out
}

fn encode(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("encode-huge");
    group.sample_size(10);
    for size in SIZES {
        let input = data(size);
        let mut out = vec![0u8; size / 3 * 4 + 8];
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("tape-base64", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.encode(black_box(&input), black_box(&mut out)).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("base64-turbo", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_turbo::STANDARD
                    .encode_into(black_box(&input[..]), black_box(&mut out))
                    .unwrap()
            })
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
    }
    group.finish();
}

fn decode(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("decode-huge");
    group.sample_size(10);
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
        group.bench_with_input(BenchmarkId::new("base64-turbo", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_turbo::STANDARD
                    .decode_into(black_box(encoded), black_box(&mut out))
                    .unwrap()
            })
        });
        group.bench_with_input(BenchmarkId::new("base64-simd", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_simd::STANDARD
                    .decode(black_box(encoded), base64_simd::Out::from_slice(black_box(&mut out)))
                    .unwrap()
                    .len()
            })
        });
    }
    group.finish();
}

criterion_group!(benches, encode, decode);
criterion_main!(benches);
