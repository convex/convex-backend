#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]

use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    env,
    fs::File,
    io::{
        BufRead,
        BufReader,
    },
    sync::LazyLock,
    time::Instant,
};

use common::{
    bootstrap_model::index::text_index::TextIndexSpec,
    document::{
        CreationTime,
        ResolvedDocument,
    },
    types::{
        Timestamp,
        WriteTimestamp,
    },
};
use divan::counter::BytesCount;
use search::{
    MemoryTextIndex,
    TantivySearchIndexSchema,
};
use serde::Deserialize;
use value::{
    assert_obj,
    DeveloperDocumentId,
    InternalId,
    ResolvedDocumentId,
    TableNumber,
    TabletId,
    TabletIdAndTableNumber,
};

// Comment this out if you don't need memory profiling.
#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const MAX_LOAD_SIZE: usize = 4 << 20;

#[derive(Deserialize)]
struct SearchDocument {
    text: String,
}

struct Dataset {
    schema: TantivySearchIndexSchema,
    loaded: BTreeMap<String, Vec<(InternalId, CreationTime, ResolvedDocument, usize)>>,
}

impl Dataset {
    fn load(path: &str) -> anyhow::Result<Self> {
        let mut next_id = 0u64;
        let mut alloc_id = || {
            let mut result = [0; 16];
            result[0..8].copy_from_slice(&next_id.to_le_bytes()[..]);
            next_id += 1;
            InternalId(result)
        };

        let table_id = TabletIdAndTableNumber {
            tablet_id: TabletId(alloc_id()),
            table_number: TableNumber::try_from(123).expect("Could not create table number"),
        };
        let config = TextIndexSpec {
            search_field: "body".parse()?,
            filter_fields: BTreeSet::new(),
        };

        let schema = TantivySearchIndexSchema::new(&config);
        let datasets = ["tweets", "wikipedia", "gutenberg"];

        let mut loaded = BTreeMap::new();
        for dataset in datasets {
            let f = File::open(format!("{path}/{dataset}.jsonl"))?;
            let f = BufReader::new(f);
            let mut documents = vec![];
            for line in f.lines() {
                let d: SearchDocument = serde_json::from_str(&line?)?;
                let size = d.text.len();
                let internal_id = alloc_id();
                let id = ResolvedDocumentId::new(
                    table_id.tablet_id,
                    DeveloperDocumentId::new(table_id.table_number, internal_id),
                );
                let value = assert_obj!("body" => d.text);
                let creation_time = CreationTime::try_from(1.)?;
                let document = ResolvedDocument::new(id, creation_time, value)?;
                documents.push((internal_id, creation_time, document, size));
            }
            loaded.insert(dataset.to_string(), documents);
        }

        Ok(Dataset { schema, loaded })
    }
}

static DATASET: LazyLock<Dataset> = LazyLock::new(|| {
    let path = env::var("DATASET")
        .expect("Set the `DATASET` variable to point to the test dataset (https://www.dropbox.com/sh/f0q1o7tbfuissm8/AAAkB-JggUKL7KFCtl1nsRf1a?dl=0)");
    Dataset::load(&path).unwrap()
});

fn dataset_args() -> impl Iterator<Item = &'static str> {
    ["tweets", "wikipedia", "gutenberg"].into_iter()
}

#[divan::bench(
    args = dataset_args(),
    max_time = 10,
)]
fn load(bencher: divan::Bencher, dataset_name: &str) {
    let dataset = &*DATASET;
    let documents = &dataset.loaded[dataset_name];

    let mut to_load = Vec::new();
    let mut total_size = 0;
    for (internal_id, creation_time, document, size) in documents {
        total_size += size;
        if total_size > MAX_LOAD_SIZE {
            break;
        }
        let terms = dataset.schema.index_into_terms(document).unwrap();
        to_load.push((*internal_id, *creation_time, terms));
    }
    bencher.counter(BytesCount::new(total_size)).bench(|| {
        let mut index = MemoryTextIndex::new(WriteTimestamp::Committed(Timestamp::MIN));
        for (internal_id, creation_time, terms) in &to_load {
            index
                .update(
                    *internal_id,
                    WriteTimestamp::Pending,
                    None,
                    Some((terms.clone(), *creation_time)),
                )
                .unwrap();
        }
        index
    });
}

fn main() {
    let start = Instant::now();
    LazyLock::force(&DATASET);
    println!("Loaded dataset in {:?}", start.elapsed());
    divan::main();
}
