use std::{
    collections::BTreeMap,
    hint::black_box,
};

use common::types::{
    Timestamp,
    WriteTimestamp,
};
use criterion::{
    criterion_group,
    criterion_main,
    Criterion,
};
use rand::Rng;
use value::InternalId;
use vector::{
    CompiledVectorSearch,
    MemoryVectorIndex,
    QdrantDocument,
};

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut rng = rand::rng();

    let n = 20000;
    let d = 1536;
    let k = 1024;

    let ts = Timestamp::must(1);

    let mut index = MemoryVectorIndex::new(WriteTimestamp::Committed(ts));

    for id in 1u128..=n {
        let id = InternalId(id.to_le_bytes());
        let document = QdrantDocument {
            internal_id: id,
            vector: (0..d)
                .map(|_| rng.random())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            filter_fields: BTreeMap::new(),
        };
        index
            .update(id, WriteTimestamp::Committed(ts), None, Some(document))
            .unwrap();
    }
    println!("size: {}", index.size());

    let search = CompiledVectorSearch {
        vector: (0..d)
            .map(|_| rng.random())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        limit: k,
        filter_conditions: BTreeMap::new(),
    };
    c.bench_function("query", |b| b.iter(|| index.query(ts, black_box(&search))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
