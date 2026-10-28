use std::collections::BTreeMap;

use common::{
    bootstrap_model::index::{
        database_index::IndexedFields,
        IndexMetadata,
        TabletIndexMetadata,
        INDEX_TABLE,
    },
    document::{
        CreationTime,
        ResolvedDocument,
    },
    testing::TestIdGenerator,
    types::{
        GenericIndexName,
        Timestamp,
    },
};
use indexing::index_registry::IndexRegistry;
use value::ResolvedDocumentId;

fn gen_index_document(
    id_generator: &mut TestIdGenerator,
    metadata: TabletIndexMetadata,
) -> anyhow::Result<ResolvedDocument> {
    let index_id = id_generator.system_generate(&INDEX_TABLE);
    ResolvedDocument::new(index_id, CreationTime::ONE, metadata.try_into()?)
}

fn index_documents(
    id_generator: &mut TestIdGenerator,
    mut indexes: Vec<TabletIndexMetadata>,
) -> anyhow::Result<BTreeMap<ResolvedDocumentId, (Timestamp, ResolvedDocument)>> {
    let mut index_documents = BTreeMap::new();

    let index_table = id_generator.system_table_id(&INDEX_TABLE);
    // Add the _index.by_id index.
    indexes.push(IndexMetadata::new_enabled(
        GenericIndexName::by_id(index_table.tablet_id),
        IndexedFields::by_id(),
    ));
    let ts = Timestamp::must(0);
    for metadata in indexes {
        let doc = gen_index_document(id_generator, metadata.clone())?;
        index_documents.insert(doc.id(), (ts, doc));
    }
    Ok(index_documents)
}

#[divan::bench(args = [10, 100, 500, 1000])]
fn index_registry_bootstrap(bencher: divan::Bencher, num_indexes: usize) {
    let mut id_generator = TestIdGenerator::new();
    // Generate index documents
    let indexes = (0..num_indexes)
        .map(|i| {
            let table_id = id_generator.user_table_id(&format!("messages_{i}").parse().unwrap());
            IndexMetadata::new_enabled(
                GenericIndexName::by_id(table_id.tablet_id),
                IndexedFields::by_id(),
            )
        })
        .collect();
    let index_documents = index_documents(&mut id_generator, indexes).unwrap();

    // Benchmark
    bencher.bench(|| {
        IndexRegistry::bootstrap(
            &id_generator,
            index_documents.values().map(|(_, d)| d.clone()),
        )
        .unwrap();
    });
}

fn main() {
    divan::main();
}
