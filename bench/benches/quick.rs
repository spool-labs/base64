//! A short run for trying one kernel variant against another

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

const SIZES: [usize; 6] = [12, 21, 32, 64, 1232, 16_384];

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

fn quick(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("quick");
    for size in SIZES {
        let input = data(size);
        let mut encoded = vec![0u8; size / 3 * 4 + 8];
        let chars = tape_base64::STANDARD.encode(&input, &mut encoded).unwrap();
        let mut out = vec![0u8; size + 8];
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("encode", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.encode(black_box(&input), black_box(&mut encoded)).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("encode-simd", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_simd::STANDARD
                    .encode(black_box(&input), base64_simd::Out::from_slice(black_box(&mut encoded)))
                    .len()
            })
        });
        group.bench_with_input(BenchmarkId::new("encode-turbo", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_turbo::STANDARD
                    .encode_into(black_box(&input[..]), black_box(&mut encoded))
                    .unwrap()
            })
        });

        let encoded = encoded[..chars].to_vec();
        group.bench_with_input(BenchmarkId::new("decode", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.decode(black_box(&encoded), black_box(&mut out)).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("decode-simd", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_simd::STANDARD
                    .decode(black_box(&encoded[..]), base64_simd::Out::from_slice(black_box(&mut out)))
                    .unwrap()
                    .len()
            })
        });
        group.bench_with_input(BenchmarkId::new("decode-turbo", size), &size, |bencher, _| {
            bencher.iter(|| {
                base64_turbo::STANDARD
                    .decode_into(black_box(&encoded[..]), black_box(&mut out))
                    .unwrap()
            })
        });
    }
    group.finish();
}

criterion_group!(benches, quick);
criterion_main!(benches);
