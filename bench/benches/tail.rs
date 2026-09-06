//! The fixed cost of a conversion, at the sizes where it is most of the cost

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

const SIZES: [usize; 8] = [1, 3, 6, 12, 24, 32, 48, 64];

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index as u8).wrapping_mul(37)).collect()
}

fn tail(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("tail");
    for size in SIZES {
        let input = data(size);
        let mut encoded = vec![0u8; size / 3 * 4 + 8];
        let chars = tape_base64::STANDARD.encode(&input, &mut encoded).unwrap();
        let mut out = vec![0u8; size + 8];

        group.bench_with_input(BenchmarkId::new("encode", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.encode(black_box(&input), black_box(&mut encoded)).unwrap())
        });
        let encoded = encoded[..chars].to_vec();
        group.bench_with_input(BenchmarkId::new("decode", size), &size, |bencher, _| {
            bencher.iter(|| tape_base64::STANDARD.decode(black_box(&encoded), black_box(&mut out)).unwrap())
        });
    }
    group.finish();
}

criterion_group!(benches, tail);
criterion_main!(benches);
