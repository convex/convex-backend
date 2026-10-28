use std::sync::Arc;

use anyhow::Context;
use common::{
    bootstrap_model::index::{
        text_index::{
            FragmentedTextSegment,
            TextIndexSnapshot,
            TextIndexSnapshotData,
            TextIndexState,
        },
        IndexConfig,
        IndexMetadata,
        TabletIndexMetadata,
    },
    document::{
        ParsedDocument,
        ResolvedDocument,
    },
    pause::PauseController,
    persistence::PersistenceReader,
    query::{
        Query,
        QueryOperator,
        QuerySource,
        Search,
        SearchFilterExpression,
    },
    runtime::testing::TestRuntime,
    types::{
        GenericIndexName,
        IndexDescriptor,
        IndexName,
        TabletIndexName,
    },
    version::MIN_NPM_VERSION_FOR_PREFIX_SEARCH,
};
use database::{
    test_helpers::{
        DbFixtures,
        DbFixturesArgs,
    },
    Database,
    IndexModel,
    ResolvedQuery,
    TestFacingModel,
    Transaction,
};
use events::testing::TestUsageEventLogger;
use futures::try_join;
use maplit::btreeset;
use must_let::must_let;
use search::{
    searcher::InProcessSearcher,
    Searcher,
    SegmentTermMetadataFetcher,
    TantivySearchIndexSchema,
    MAX_CANDIDATE_REVISIONS,
};
use storage::Storage;
use sync_types::Timestamp;
use usage_tracking::UsageCounter;
use value::{
    assert_obj,
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    search_compactor::CompactionConfig,
    text_index_worker::{
        compactor::{
            new_text_compactor_for_tests,
            TextIndexCompactor,
        },
        flusher::{
            backfill_text_indexes,
            FlusherBuilder,
            TextIndexFlusher,
        },
        BuildTextIndexArgs,
        TextIndexMetadataWriter,
    },
    FlusherType,
};

#[derive(Clone)]
pub struct TextFixtures {
    pub rt: TestRuntime,
    pub storage: Arc<dyn Storage>,
    pub db: Database<TestRuntime>,
    pub reader: Arc<dyn PersistenceReader>,
    pub test_usage_logger: TestUsageEventLogger,
    pub usage_counter: UsageCounter,
    searcher: Arc<dyn Searcher>,
    segment_term_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
    writer: TextIndexMetadataWriter<TestRuntime>,
    namespace: TableNamespace,
    config: CompactionConfig,
}

impl TextFixtures {
    pub async fn new(rt: TestRuntime) -> anyhow::Result<Self> {
        Self::new_with_config(rt, CompactionConfig::default()).await
    }

    pub async fn new_with_config(
        rt: TestRuntime,
        config: CompactionConfig,
    ) -> anyhow::Result<Self> {
        let in_process_searcher = InProcessSearcher::new(rt.clone())?;
        let DbFixtures {
            tp,
            db,
            search_storage,
            searcher,
            test_usage_logger,
            usage_counter,
            ..
        } = DbFixtures::new_with_args(
            &rt,
            DbFixturesArgs {
                searcher: Some(Arc::new(in_process_searcher.clone())),
                ..Default::default()
            },
        )
        .await?;
        let segment_term_metadata_fetcher = Arc::new(in_process_searcher);
        let writer = TextIndexMetadataWriter::new(
            rt.clone(),
            db.clone(),
            search_storage.clone(),
            BuildTextIndexArgs {
                search_storage: search_storage.clone(),
                segment_term_metadata_fetcher: segment_term_metadata_fetcher.clone(),
            },
        );

        Ok(Self {
            rt,
            db,
            reader: tp.reader(),
            storage: search_storage,
            segment_term_metadata_fetcher,
            writer,
            namespace: TableNamespace::test_user(),
            searcher,
            config,
            test_usage_logger,
            usage_counter,
        })
    }

    pub(crate) fn new_search_flusher_builder(&self) -> FlusherBuilder<TestRuntime> {
        FlusherBuilder::new(
            self.rt.clone(),
            self.db.clone(),
            self.reader.clone(),
            self.storage.clone(),
            self.segment_term_metadata_fetcher.clone(),
            self.writer.clone(),
            FlusherType::Backfill,
        )
    }

    pub fn new_backfill_text_flusher(&self) -> TextIndexFlusher<TestRuntime> {
        self.new_search_flusher_builder().set_soft_limit(0).build()
    }

    pub fn new_live_text_flusher(&self) -> TextIndexFlusher<TestRuntime> {
        self.new_search_flusher_builder()
            .set_soft_limit(0)
            .set_live_flush()
            .build()
    }

    pub fn new_backfill_flusher_with_soft_limit(&self) -> TextIndexFlusher<TestRuntime> {
        self.new_search_flusher_builder()
            .set_soft_limit(2048)
            .build()
    }

    pub fn new_live_flusher_with_soft_limit(&self) -> TextIndexFlusher<TestRuntime> {
        self.new_search_flusher_builder()
            .set_soft_limit(2048)
            .set_live_flush()
            .build()
    }

    pub fn new_compactor(&self) -> TextIndexCompactor<TestRuntime> {
        new_text_compactor_for_tests(
            self.rt.clone(),
            self.db.clone(),
            self.storage.clone(),
            self.searcher.clone(),
            self.segment_term_metadata_fetcher.clone(),
            self.config.clone(),
        )
    }

    pub async fn enabled_text_index(&self) -> anyhow::Result<TextIndexData> {
        let index_data = self.backfilled_text_index().await?;
        let mut tx = self.db.begin_system().await?;
        IndexModel::new(&mut tx)
            .enable_index_for_testing(index_data.namespace, &index_data.index_name)
            .await?;
        self.db.commit(tx).await?;
        Ok(index_data)
    }

    pub async fn staged_backfilled_text_index(&self) -> anyhow::Result<TextIndexData> {
        // Start as enabled -> disable it.
        let index_data = self.enabled_text_index().await?;
        let mut tx = self.db.begin_system().await?;
        IndexModel::new(&mut tx)
            .disable_index_for_test(index_data.index_id)
            .await?;
        self.db.commit(tx).await?;
        Ok(index_data)
    }

    pub async fn backfilled_text_index(&self) -> anyhow::Result<TextIndexData> {
        let index_data = self.insert_backfilling_text_index().await?;
        self.backfill().await?;

        Ok(index_data)
    }

    pub async fn backfill(&self) -> anyhow::Result<()> {
        backfill_text_indexes(
            self.rt.clone(),
            self.db.clone(),
            self.reader.clone(),
            self.storage.clone(),
            self.segment_term_metadata_fetcher.clone(),
        )
        .await
    }

    pub async fn assert_backfilled(&self, index_name: &IndexName) -> anyhow::Result<Timestamp> {
        let mut tx = self.db.begin_system().await?;
        let new_metadata = IndexModel::new(&mut tx)
            .pending_index_metadata(self.namespace, index_name)?
            .context("Index missing or in an unexpected state")?
            .into_value();
        must_let!(let IndexMetadata {
            config: IndexConfig::Text {
                on_disk_state: TextIndexState::Backfilled {
                    snapshot: TextIndexSnapshot { ts, .. },
                    staged: _,
                },
                ..
            },
            ..
        } = new_metadata);
        Ok(ts)
    }

    pub async fn insert_backfilling_text_index(&self) -> anyhow::Result<TextIndexData> {
        let mut tx = self.db.begin_system().await?;
        let index_metadata = backfilling_text_index()?;
        let index_name = &index_metadata.name;
        let index_id = IndexModel::new(&mut tx)
            .add_application_index(self.namespace, index_metadata.clone())
            .await?;
        let table_id = tx
            .table_mapping()
            .namespace(self.namespace)
            .id(index_name.table())?
            .tablet_id;
        self.db.commit(tx).await?;

        let resolved_index_name = TabletIndexName::new(table_id, index_name.descriptor().clone())?;
        let IndexConfig::Text { ref spec, .. } = index_metadata.config else {
            anyhow::bail!("Not a text index?")
        };
        let tantivy_schema = TantivySearchIndexSchema::new(spec);
        Ok(TextIndexData {
            index_id,
            resolved_index_name,
            index_name: index_name.clone(),
            namespace: self.namespace,
            tantivy_schema,
        })
    }

    pub async fn insert_backfilling_text_index_with_document(
        &self,
    ) -> anyhow::Result<TextIndexData> {
        let index_data = self.insert_backfilling_text_index().await?;
        let mut tx = self.db.begin_system().await?;
        add_document(&mut tx, index_data.index_name.table(), "A long text field").await?;
        self.db.commit(tx).await?;
        Ok(index_data)
    }

    pub async fn get_index_metadata(
        &self,
        index_name: GenericIndexName<TableName>,
    ) -> anyhow::Result<ParsedDocument<TabletIndexMetadata>> {
        let mut tx = self.db.begin_system().await?;
        let mut index_model = IndexModel::new(&mut tx);
        let mut metadata = index_model.enabled_index_metadata(self.namespace, &index_name)?;
        if metadata.is_none() {
            metadata = index_model.pending_index_metadata(self.namespace, &index_name)?;
        }
        let metadata = metadata.context("Index is neither pending nor enabled!")?;
        Ok(metadata)
    }

    pub async fn get_segments_metadata(
        &self,
        index_name: GenericIndexName<TableName>,
    ) -> anyhow::Result<Vec<FragmentedTextSegment>> {
        let metadata = self.get_index_metadata(index_name).await?;
        must_let!(let IndexConfig::Text { on_disk_state, .. } = &metadata.config);
        match on_disk_state {
            TextIndexState::Backfilling(state) => Ok(state.segments.clone()),
            TextIndexState::Backfilled { snapshot, .. }
            | TextIndexState::SnapshottedAt(snapshot) => {
                must_let!(let TextIndexSnapshotData::MultiSegment(segments) = &snapshot.data);
                Ok(segments.clone())
            },
        }
    }

    pub async fn replace_document(
        &self,
        doc_id: ResolvedDocumentId,
        text: &str,
    ) -> anyhow::Result<()> {
        let mut tx = self.db.begin_system().await?;
        let document = assert_obj!(
            "text" => text,
            "channel" => "#general",
        );
        TestFacingModel::new(&mut tx)
            .replace_inner(doc_id, document)
            .await?;
        self.db.commit(tx).await?;
        Ok(())
    }

    pub async fn add_document(&self, text: &str) -> anyhow::Result<ResolvedDocumentId> {
        let table_name = TABLE_NAME.parse::<TableName>()?;
        let mut tx = self.db.begin_system().await?;
        let doc_id = add_document(&mut tx, &table_name, text).await?;
        self.db.commit(tx).await?;
        Ok(doc_id)
    }

    pub async fn enable_index(
        &self,
        index_name: &GenericIndexName<TableName>,
    ) -> anyhow::Result<()> {
        let mut tx = self.db.begin_system().await?;
        IndexModel::new(&mut tx)
            .enable_index_for_testing(self.namespace, index_name)
            .await?;
        self.db.commit(tx).await?;
        Ok(())
    }

    pub async fn search(
        &self,
        index_name: GenericIndexName<TableName>,
        query_string: &str,
    ) -> anyhow::Result<Vec<ResolvedDocument>> {
        let mut tx = self.db.begin_system().await?;
        self.search_with_tx(&mut tx, index_name, query_string).await
    }

    pub async fn search_with_tx(
        &self,
        tx: &mut Transaction<TestRuntime>,
        index_name: GenericIndexName<TableName>,
        query_string: &str,
    ) -> anyhow::Result<Vec<ResolvedDocument>> {
        self.search_with_filters_tx(tx, index_name, query_string, vec![])
            .await
    }

    /// Searches `query_string` on the search field, additionally constrained
    /// by `filters`.
    pub async fn search_with_filters_tx(
        &self,
        tx: &mut Transaction<TestRuntime>,
        index_name: GenericIndexName<TableName>,
        query_string: &str,
        mut filters: Vec<SearchFilterExpression>,
    ) -> anyhow::Result<Vec<ResolvedDocument>> {
        filters.insert(
            0,
            SearchFilterExpression::Search(SEARCH_FIELD.parse()?, query_string.into()),
        );
        let search = Search {
            table: index_name.table().clone(),
            index_name,
            filters,
        };

        let query = Query {
            source: QuerySource::Search(search),
            operators: vec![QueryOperator::Limit(MAX_CANDIDATE_REVISIONS)],
        };
        let mut query_stream = ResolvedQuery::new_with_version(
            tx,
            TableNamespace::test_user(),
            query,
            Some(MIN_NPM_VERSION_FOR_PREFIX_SEARCH.clone()),
        )?;
        let mut values = vec![];
        while let Some(value) = query_stream.next(tx, None).await? {
            values.push(value);
        }
        Ok(values)
    }

    pub async fn run_compaction_during_flush(
        &self,
        pause: PauseController,
        label: &'static str,
    ) -> anyhow::Result<()> {
        let flusher = self.new_live_text_flusher();
        let hold_guard = pause.hold(label);
        let flush = flusher.step();
        let compactor = self.new_compactor();
        let compact_during_flush = async move {
            if let Some(pause_guard) = hold_guard.wait_for_blocked().await {
                let (metrics, _) = compactor.step().await?;
                for (_index_name, num_segments_compacted) in metrics {
                    assert!(num_segments_compacted > 0);
                }
                pause_guard.unpause();
            };
            Ok::<(), anyhow::Error>(())
        };
        try_join!(flush, compact_during_flush)?;
        Ok(())
    }
}

const TABLE_NAME: &str = "table";
const SEARCH_FIELD: &str = "text";

pub struct TextIndexData {
    pub index_id: ResolvedDocumentId,
    pub index_name: IndexName,
    pub resolved_index_name: TabletIndexName,
    pub namespace: TableNamespace,
    pub tantivy_schema: TantivySearchIndexSchema,
}

pub fn backfilling_text_index() -> anyhow::Result<IndexMetadata<TableName>> {
    let table_name: TableName = TABLE_NAME.parse()?;
    let index_name = IndexName::new(table_name, IndexDescriptor::new("search_index")?)?;
    let search_field: FieldPath = SEARCH_FIELD.parse()?;
    let filter_field: FieldPath = "channel".parse()?;
    let metadata = IndexMetadata::new_backfilling_text_index(
        index_name,
        search_field,
        btreeset![filter_field],
    );
    Ok(metadata)
}

pub async fn add_document(
    tx: &mut Transaction<TestRuntime>,
    table_name: &TableName,
    text: &str,
) -> anyhow::Result<ResolvedDocumentId> {
    add_document_in_channel(tx, table_name, text, "#general").await
}

pub async fn add_document_in_channel(
    tx: &mut Transaction<TestRuntime>,
    table_name: &TableName,
    text: &str,
    channel: &str,
) -> anyhow::Result<ResolvedDocumentId> {
    let document = assert_obj!(
        "text" => text,
        "channel" => channel,
    );
    TestFacingModel::new(tx).insert(table_name, document).await
}
