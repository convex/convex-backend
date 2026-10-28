use std::{
    hint::black_box,
    str::FromStr,
};

use criterion::{
    criterion_group,
    criterion_main,
    BenchmarkId,
    Criterion,
};
use value::{
    DeveloperDocumentId,
    InternalId,
    TableNumber,
};

// As of 11/16/2022 on an 14" MBP (M1 Max):
//
//   v4::encode              time:   [3.2255 µs 3.2293 µs 3.2331 µs]
//   v5::encode              time:   [44.737 ns 44.773 ns 44.817 ns]
//   v4::decode              time:   [2.4204 µs 2.4239 µs 2.4274 µs]
//   v5::decode              time:   [48.062 ns 48.145 ns 48.223 ns]
//
pub fn benchmark_encode(c: &mut Criterion) {
    let v5 = InternalId([0xba; 16]);
    c.bench_function("v5::encode", |b| b.iter(|| String::from(black_box(v5))));
}

pub fn benchmark_encode_v6(c: &mut Criterion) {
    for table_number in [1u32, 10000, 0xfffff] {
        let id = DeveloperDocumentId::new(
            TableNumber::try_from(table_number).unwrap(),
            InternalId([0xba; 16]),
        );
        c.bench_with_input(BenchmarkId::new("v6::encode", id), &id, |b, i| {
            b.iter(|| {
                black_box(i.encode_into(&mut Default::default()));
            })
        });
    }
}

pub fn benchmark_decode(c: &mut Criterion) {
    let v5 = String::from(InternalId([0xba; 16]));
    c.bench_function("v5::decode", |b| {
        b.iter(|| InternalId::from_str(black_box(&v5)))
    });
}

criterion_group!(
    benches,
    benchmark_encode,
    benchmark_encode_v6,
    benchmark_decode
);
criterion_main!(benches);
