use std::hint::black_box;

use criterion::{
    criterion_group,
    criterion_main,
    Criterion,
};
use value::base32;

pub fn benchmark_base32(c: &mut Criterion) {
    let value = (0..23u8).chain([0; 5]).collect::<Vec<_>>();
    let len = 23;
    c.bench_function("base32::encode", |b| {
        b.iter(|| base32::encode(black_box(&value[..len])))
    });
    c.bench_function("base32::encode_into", |b| {
        b.iter(|| {
            let mut buf = [0; 40];
            base32::encode_into::<true>(&mut buf, black_box(&value[..]), len);
            buf
        })
    });
    let encoded = base32::encode(&value[..len]);
    c.bench_function("base32::decode", |b| {
        b.iter(|| base32::decode(black_box(&encoded)))
    });
}

criterion_group!(benches, benchmark_base32);
criterion_main!(benches);
