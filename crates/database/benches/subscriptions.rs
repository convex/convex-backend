#![feature(try_blocks)]

/// To run this test, download the datasets from https://www.dropbox.com/sh/f0q1o7tbfuissm8/AAAkB-JggUKL7KFCtl1nsRf1a?dl=0
/// and run `DATASET=/location/to/datasets/ cargo bench -p database --bench
/// subscriptions --features testing`
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
    str::FromStr,
    time::Instant,
};

use common::{
    document_index_keys::{
        DatabaseIndexWrite,
        DocumentIndexKeys,
        IndexKeyUpdate,
        TextIndexWrite,
    },
    testing::TestIdGenerator,
    types::{
        GenericIndexName,
        IndexDescriptor,
        TabletIndexName,
    },
};
use criterion::{
    criterion_group,
    criterion_main,
    BenchmarkId,
    Criterion,
};
use database::{
    subscription::SubscriptionManager,
    write_log::WriteInIndex,
    Token,
    WriteSource,
};
use humansize::{
    FormatSize,
    BINARY,
};
use itertools::Itertools;
use search::{
    convex_en,
    query::{
        tokenize,
        TextQueryTerm,
    },
};
use serde::Deserialize;
use sync_types::Timestamp;
use tokio::runtime::Runtime;
use value::{
    ConvexString,
    DeveloperDocumentId,
    FieldPath,
    InternalId,
    ResolvedDocumentId,
    TabletId,
    TabletIdAndTableNumber,
};

const MAX_LOAD_SIZE: usize = 4 << 20;
const TOTAL_SUBSCRIPTIONS: usize = 500;

#[derive(Deserialize)]
struct SearchDocument {
    text: String,
}

fn path() -> String {
    env::var("DATASET").expect(
        "Set the `DATASET` variable to point to the test dataset (https://www.dropbox.com/sh/f0q1o7tbfuissm8/AAAkB-JggUKL7KFCtl1nsRf1a?dl=0)",
    )
}

fn prefixes() -> Vec<bool> {
    vec![true, false]
}

fn load_datasets(
    table_id: TabletIdAndTableNumber,
    max_size: usize,
) -> anyhow::Result<BTreeMap<String, (Vec<(ResolvedDocumentId, DocumentIndexKeys)>, Vec<String>)>> {
    let mut next_id = 0u64;
    let mut alloc_id = || {
        let mut result = [0; 16];
        result[0..8].copy_from_slice(&next_id.to_le_bytes()[..]);
        next_id += 1;
        InternalId(result)
    };

    let path = path();
    let start = Instant::now();
    println!("Loading from {path}...");
    let datasets = ["tweets", "wikipedia", "gutenberg"];

    let mut loaded = BTreeMap::new();
    let mut n = 0;
    let mut bytes = 0;
    let mut terms = 0;
    for dataset in datasets {
        let mut frequency_map: BTreeMap<String, u32> = BTreeMap::new();
        let f = File::open(format!("{path}/{dataset}.jsonl"))?;
        let f = BufReader::new(f);
        let mut documents = vec![];
        let mut m = 0;
        for line in f.lines() {
            if m > max_size {
                break;
            }
            let d: SearchDocument = serde_json::from_str(&line?)?;
            bytes += d.text.len();
            m += d.text.len();
            n += 1;
            let internal_id = alloc_id();
            let id = ResolvedDocumentId::new(
                table_id.tablet_id,
                DeveloperDocumentId::new(table_id.table_number, internal_id),
            );
            let tokenizer = convex_en();
            {
                let mut stream = tokenizer.token_stream(&d.text);
                while let Some(token) = stream.next() {
                    terms += 1;
                    *frequency_map.entry(token.text.clone()).or_default() += 1;
                }
            }

            let field_path = FieldPath::from_str("body")?;
            documents.push((
                id,
                DocumentIndexKeys::with_search_index_for_test_with_filters(
                    id,
                    index_name(table_id.tablet_id),
                    field_path,
                    tokenize(ConvexString::try_from(d.text).unwrap()),
                    BTreeMap::new(),
                ),
            ));
        }

        let terms_by_frequency: Vec<String> = frequency_map
            .into_iter()
            .sorted_by_key(|value| value.1)
            .map(|(key, _)| key)
            .collect();
        println!(
            "{dataset}: {}, {} docs, {terms} terms, {} unique terms",
            m.format_size(BINARY),
            documents.len(),
            terms_by_frequency.len()
        );
        loaded.insert(dataset.to_string(), (documents, terms_by_frequency));
    }

    println!(
        "Loaded {n} rows ({} bytes of text) in {:?}",
        bytes.format_size(BINARY),
        start.elapsed()
    );
    Ok(loaded)
}

fn index_name(tablet_id: TabletId) -> TabletIndexName {
    GenericIndexName::new(tablet_id, IndexDescriptor::new("index").unwrap()).unwrap()
}

fn create_subscription_token(tablet_id: TabletId, prefix: bool, token: String) -> Token {
    let term = if prefix {
        TextQueryTerm::Prefix(token)
    } else {
        TextQueryTerm::Exact(token)
    };
    Token::text_search_token(
        index_name(tablet_id),
        FieldPath::from_str("body").unwrap(),
        vec![term],
    )
}

fn create_tokens(
    tablet_id: TabletId,
    terms_by_frequency: &Vec<String>,
    prefix: bool,
    count: usize,
) -> Vec<Token> {
    let total_unique_terms = terms_by_frequency.len();
    assert!(count <= total_unique_terms);
    terms_by_frequency
        .iter()
        .chunks(total_unique_terms / count)
        .into_iter()
        // Due to rounding errors we may end up with more chunks than required
        .take(count)
        .map(|chunk| {
            let token = chunk.into_iter().next().unwrap();
            create_subscription_token(tablet_id, prefix, token.clone())
        })
        .collect::<Vec<_>>()
}

fn create_subscriptions(
    tablet_id: TabletId,
    terms_by_frequency: &Vec<String>,
    prefix: bool,
    count: usize,
) -> SubscriptionManager {
    let mut subscription_manager = SubscriptionManager::new_for_testing();
    let tokens = create_tokens(tablet_id, terms_by_frequency, prefix, count);
    for token in tokens {
        // this drops the Subscription but in these tests we don't run the
        // worker that removes dropped subscriptions
        _ = subscription_manager.subscribe_for_testing(token).unwrap();
    }
    subscription_manager
}

fn bench_query(c: &mut Criterion) {
    let rt = Runtime::new().expect("Failed to create Tokio runtime");

    let mut id_generator = TestIdGenerator::new();
    let table_name = id_generator.generate_table_name();
    let table_id = id_generator.user_table_id(&table_name);

    let datasets = load_datasets(table_id, MAX_LOAD_SIZE).unwrap();

    for prefix in prefixes() {
        for (dataset, (data, terms_by_frequency)) in &datasets {
            let subscription_manager = rt.block_on(async {
                create_subscriptions(
                    table_id.tablet_id,
                    terms_by_frequency,
                    prefix,
                    TOTAL_SUBSCRIPTIONS,
                )
            });

            let mut group = c.benchmark_group("subscriptions");

            group.throughput(criterion::Throughput::Elements(data.len() as u64));
            group.sample_size(if prefix { 10 } else { 100 });
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{TOTAL_SUBSCRIPTIONS}/{dataset}/{prefix}")),
                data,
                |b, documents| {
                    b.to_async(&rt).iter(|| async {
                        let dummy_ts = Timestamp::MIN;
                        for (_doc_id, doc_index_keys) in documents {
                            let mut to_notify = BTreeSet::new();
                            for (idx_name, idx_key_update) in &doc_index_keys.0 {
                                match &idx_key_update.update {
                                    IndexKeyUpdate::Database(u) => {
                                        let update = DatabaseIndexWrite {
                                            document_id: idx_key_update.document_id,
                                            update: u.clone(),
                                            new_document: idx_key_update.new_document.clone(),
                                        };
                                        let write = WriteInIndex {
                                            ts: dummy_ts,
                                            index_updates: vec![update],
                                            write_source: WriteSource::system("bench"),
                                        };
                                        let interval_map =
                                            subscription_manager.interval_map(idx_name).unwrap();
                                        SubscriptionManager::overlapping_database(
                                            interval_map,
                                            std::iter::once(&write),
                                            &mut |id, _ts, _ws| {
                                                to_notify.insert(id);
                                            },
                                            &mut 0,
                                        );
                                    },
                                    IndexKeyUpdate::Text(u) => {
                                        let update = TextIndexWrite {
                                            document_id: idx_key_update.document_id,
                                            update: u.clone(),
                                        };
                                        let write = WriteInIndex {
                                            ts: dummy_ts,
                                            index_updates: vec![update],
                                            write_source: WriteSource::system("bench"),
                                        };
                                        let text_subscription = subscription_manager
                                            .text_subscription_for_index(idx_name)
                                            .unwrap();
                                        subscription_manager.overlapping_text(
                                            text_subscription,
                                            std::iter::once(&write),
                                            &mut |id, _ts, _ws| {
                                                to_notify.insert(id);
                                            },
                                            &mut 0,
                                        );
                                    },
                                }
                            }
                        }
                    })
                },
            );
            group.finish();
        }
    }
}

criterion_group!(benches, bench_query);
criterion_main!(benches);
