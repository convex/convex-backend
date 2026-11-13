use std::{
    collections::{
        BTreeMap,
        BTreeSet,
        HashMap,
    },
    sync::Arc,
};

use anyhow::{
    ensure,
    Context,
    Result,
};
use common::knobs::S3_EXPORT_TABLE_CONCURRENCY;
use errors::ErrorMetadata;
use futures::{
    stream,
    StreamExt,
    TryStreamExt,
};
use iceberg::{
    io::{
        S3_ACCESS_KEY_ID,
        S3_ENDPOINT,
        S3_PATH_STYLE_ACCESS,
        S3_REGION,
        S3_SECRET_ACCESS_KEY,
        S3_SESSION_TOKEN,
    },
    memory::{
        MemoryCatalogBuilder,
        MEMORY_CATALOG_WAREHOUSE,
    },
    spec::SchemaRef,
    table::Table,
    transaction::{
        ApplyTransactionAction,
        Transaction,
    },
    Catalog,
    CatalogBuilder,
    NamespaceIdent,
    TableCreation,
    TableIdent,
};
use iceberg_storage_opendal::OpenDalStorageFactory;
use value::TabletId;

use super::{
    append,
    columns::{
        self,
        metadata_schema,
        AddColumn,
    },
    names::{
        self,
        is_glue_name,
    },
    Change,
};

const ENCODING_PROPERTY: &str = "convex.porter.encoding";
pub(crate) const ENCODING: &str = "convex-change-log-typed-v1";
const RETIRED_PROPERTY: &str = "convex.porter.retired";
// Glue lowercases names, so these record the exact source table.
const COMPONENT_PROPERTY: &str = "convex.porter.component";
const TABLE_PROPERTY: &str = "convex.porter.table";
/// Base32, as in the Glue name.
const TABLET_PROPERTY: &str = "convex.porter.tablet";
// S3 keys are at most 1024 bytes; leave room for Iceberg's file names.
const MAX_LOCATION_BYTES: usize = 896;
const REDIRECT_PROPERTIES: [&str; 3] = [
    "write.data.path",
    "write.metadata.path",
    "write.object-storage.path",
];
/// Live plus retired. The state is rewritten every page, so this bounds it.
/// Matches MAX_USER_TABLES.
const MAX_TABLES: usize = 10_000;

#[derive(Clone)]
pub struct S3Destination {
    pub bucket: String,
    pub region: String,
    pub endpoint_url: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Set when the keys are a role session rather than an IAM user's keys.
    pub session_token: Option<String>,
}

/// One Convex tablet. `import --replace` and delete-and-recreate make a new
/// tablet under the same name, which exports as a new Iceberg table.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceTable {
    pub component: String,
    pub table: String,
    pub tablet: TabletId,
}

/// One Iceberg table, as the state records it under its Glue name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableState {
    /// As of first export. The Glue name is built from these and does not
    /// follow a later rename.
    pub component: String,
    pub table: String,
    /// Base32, as in the Glue name.
    pub tablet: String,
    /// Current `metadata.json`; moves every commit.
    pub metadata: String,
    /// Deselected or replaced.
    pub retired: bool,
}

impl TableState {
    pub fn source(&self) -> Result<SourceTable> {
        Ok(SourceTable {
            component: self.component.clone(),
            table: self.table.clone(),
            tablet: names::parse_tablet(&self.tablet)?,
        })
    }
}

pub struct IcebergChangeWriter {
    catalog: Arc<dyn Catalog>,
    namespace: NamespaceIdent,
    warehouse_uri: String,
    tables: BTreeMap<SourceTable, Table>,
    /// Every table ever created, by Glue name. The caller commits it after
    /// the page.
    state: BTreeMap<String, TableState>,
    /// State entries not yet registered in the catalog. Registering reads
    /// metadata from S3, so it waits until a page needs the table.
    unregistered: BTreeMap<String, String>,
    /// Whether `state` lists every table in the namespace. False for a writer
    /// opened on a catalog alone, until retirement reconciles.
    complete: bool,
    snapshot_properties: HashMap<String, String>,
    /// Parquet bytes this writer has committed to storage. S3 exports bill
    /// these bytes to the customer as data egress.
    bytes_written: u64,
}

impl IcebergChangeWriter {
    /// Writes data and metadata to S3 without touching Glue. Tables start
    /// from `state`; the caller commits [`Self::state`] after the page.
    /// Appends record `snapshot_properties` in their snapshot summary.
    pub async fn staged(
        namespace: &str,
        warehouse_uri: &str,
        destination: S3Destination,
        selected: Vec<SourceTable>,
        state: &BTreeMap<String, TableState>,
        snapshot_properties: HashMap<String, String>,
    ) -> Result<Self> {
        ensure!(
            warehouse_uri.starts_with(&format!("s3://{}/", destination.bucket)),
            "Warehouse does not belong to destination bucket"
        );
        let mut props = HashMap::from([
            (
                MEMORY_CATALOG_WAREHOUSE.to_owned(),
                warehouse_uri.to_owned(),
            ),
            (S3_REGION.to_owned(), destination.region),
            (S3_ACCESS_KEY_ID.to_owned(), destination.access_key_id),
            (
                S3_SECRET_ACCESS_KEY.to_owned(),
                destination.secret_access_key,
            ),
        ]);
        if let Some(token) = destination.session_token {
            props.insert(S3_SESSION_TOKEN.to_owned(), token);
        }
        if let Some(endpoint) = destination.endpoint_url {
            props.insert(S3_ENDPOINT.to_owned(), endpoint);
            props.insert(S3_PATH_STYLE_ACCESS.to_owned(), "true".to_owned());
        }
        let catalog = MemoryCatalogBuilder::default()
            .with_storage_factory(Arc::new(OpenDalStorageFactory::S3 {
                customized_credential_load: None,
            }))
            .load("porter", props)
            .await?;
        Self::staged_with_catalog(
            Arc::new(catalog),
            namespace,
            warehouse_uri,
            selected,
            state,
            snapshot_properties,
        )
        .await
    }

    /// Stages an export using an injected catalog and its storage. The
    /// catalog's namespace must start with no table registrations; `state`
    /// is the complete committed state and its metadata pointers register
    /// lazily as pages use them. Each append
    /// records `snapshot_properties` in its snapshot summary, and
    /// [`Self::bytes_written`] counts Parquet bytes committed by this writer.
    /// The caller commits [`Self::state`] once the page succeeds.
    ///
    /// Use one writer per namespace at a time. To restart with OpenDAL memory
    /// storage, retain the catalog, drop the writer, and clear its
    /// registrations with [`Catalog::drop_table`] before opening the next
    /// writer with saved committed state. `drop_table` preserves files;
    /// `purge_table` deletes them. Building another memory catalog creates
    /// a separate backing store even with the same warehouse URI, which
    /// provides isolation between test cases.
    pub async fn staged_with_catalog(
        catalog: Arc<dyn Catalog>,
        namespace: &str,
        warehouse_uri: &str,
        selected: Vec<SourceTable>,
        state: &BTreeMap<String, TableState>,
        snapshot_properties: HashMap<String, String>,
    ) -> Result<Self> {
        let mut writer = Self::open_empty(catalog, namespace, warehouse_uri).await?;
        ensure!(
            writer
                .catalog
                .list_tables(&writer.namespace)
                .await?
                .is_empty(),
            "Staged catalog namespace must have no table registrations"
        );
        for (name, table) in state {
            ensure!(
                *name == names::glue_name(&table.source()?),
                "State entry {name:?} does not name its source"
            );
            writer
                .unregistered
                .insert(name.clone(), table.metadata.clone());
        }
        writer.state = state.clone();
        writer.complete = true;
        writer.snapshot_properties = snapshot_properties;
        drop(selected);
        Ok(writer)
    }

    /// Every table after this page, by Glue name.
    pub fn state(&self) -> &BTreeMap<String, TableState> {
        &self.state
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// The schema of each table this writer holds, by its current metadata
    /// file. The Glue mirror builds a table's columns from it.
    pub fn schemas(&self) -> HashMap<String, SchemaRef> {
        self.tables
            .values()
            .filter_map(|table| {
                let location = table.metadata_location()?.to_owned();
                Some((location, table.metadata().current_schema().clone()))
            })
            .collect()
    }

    /// A tablet keeps its first name for the life of the export, even if the
    /// component path changes. Only an unseen tablet gets a fresh name.
    fn name(&self, source: &SourceTable) -> String {
        let tablet = names::tablet_str(source.tablet);
        self.state
            .iter()
            .find(|(_, entry)| entry.tablet == tablet)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| names::glue_name(source))
    }

    async fn register(&mut self, name: &str) -> Result<()> {
        if let Some(location) = self.unregistered.remove(name) {
            self.catalog
                .register_table(
                    &TableIdent::new(self.namespace.clone(), name.to_owned()),
                    location,
                )
                .await?;
        }
        Ok(())
    }

    /// Records `table`'s current metadata pointer.
    fn record(&mut self, table: &Table) -> Result<()> {
        let name = table.identifier().name();
        let metadata = table.metadata_location_result()?.to_owned();
        match self.state.get_mut(name) {
            Some(entry) => entry.metadata = metadata,
            None => {
                let source = source_of(table)?;
                let retired = table
                    .metadata()
                    .properties()
                    .get(RETIRED_PROPERTY)
                    .is_some_and(|value| value == "true");
                self.state.insert(
                    name.to_owned(),
                    TableState {
                        component: source.component,
                        table: source.table,
                        tablet: names::tablet_str(source.tablet),
                        metadata,
                        retired,
                    },
                );
            },
        }
        Ok(())
    }

    async fn open_empty(
        catalog: Arc<dyn Catalog>,
        namespace: &str,
        warehouse_uri: &str,
    ) -> Result<Self> {
        ensure!(
            is_glue_name(namespace),
            "Invalid Glue database name {namespace:?}"
        );
        let namespace = NamespaceIdent::new(namespace.to_owned());
        if !catalog.namespace_exists(&namespace).await? {
            catalog.create_namespace(&namespace, HashMap::new()).await?;
        }
        Ok(Self {
            catalog,
            namespace,
            warehouse_uri: warehouse_uri.trim_end_matches('/').to_owned(),
            tables: BTreeMap::new(),
            state: BTreeMap::new(),
            unregistered: BTreeMap::new(),
            complete: false,
            snapshot_properties: HashMap::new(),
            bytes_written: 0,
        })
    }

    /// Each table gets its own directory, so distinct names never share files.
    fn location(&self, name: &str) -> String {
        format!("{}/{name}", self.warehouse_uri)
    }

    fn validate(&self, source: &SourceTable, table: &Table) -> Result<()> {
        let metadata = table.metadata();
        let name = table.identifier().name();
        ensure!(
            name == self.name(source)
                && metadata.location().trim_end_matches('/') == self.location(name),
            modified(
                table,
                "its name or location no longer matches its Convex table"
            )
        );
        // Field columns follow the metadata columns and differ per table.
        ensure!(
            metadata
                .current_schema()
                .as_struct()
                .fields()
                .starts_with(metadata_schema()?.as_struct().fields())
                && metadata.default_partition_spec().is_unpartitioned(),
            modified(table, "its metadata columns or partitioning were changed")
        );
        // These would send writes outside the table's own directory.
        for key in REDIRECT_PROPERTIES {
            ensure!(
                !metadata.properties().contains_key(key),
                modified(table, &format!("it sets {key}"))
            );
        }
        Ok(())
    }

    /// Loads `name` if the catalog has it. A table under this name written by
    /// another tablet means a stale state, so that is an error.
    async fn load_existing(&mut self, source: &SourceTable, name: &str) -> Result<Option<Table>> {
        let unregistered = self.unregistered.remove(name);
        self.load(source, name, unregistered).await
    }

    /// [`Self::load_existing`] for a table already taken out of
    /// `unregistered`, whose metadata file is `unregistered` if it had one.
    #[fastrace::trace]
    async fn load(
        &self,
        source: &SourceTable,
        name: &str,
        unregistered: Option<String>,
    ) -> Result<Option<Table>> {
        let ident = TableIdent::new(self.namespace.clone(), name.to_owned());
        if let Some(location) = unregistered {
            self.catalog.register_table(&ident, location).await?;
        }
        if !self.catalog.table_exists(&ident).await? {
            return Ok(None);
        }
        let table = self.catalog.load_table(&ident).await?;
        let owner = source_of(&table)?;
        ensure!(
            owner.tablet == source.tablet,
            modified(&table, "its tablet property names another Convex table")
        );
        if let Some(entry) = self.state.get(name) {
            ensure!(
                entry.component == owner.component && entry.table == owner.table,
                modified(&table, "its component or table property was changed")
            );
        }
        self.validate(source, &table)?;
        Ok(Some(table))
    }

    /// Tables this page creates: new ones plus the truncated ones
    /// [`Self::apply`] recreates. The caller supplies their seed columns.
    pub fn new_tables(
        &self,
        truncated: &[SourceTable],
        changes: &[(SourceTable, Change)],
    ) -> Result<BTreeSet<SourceTable>> {
        let mut new: BTreeSet<SourceTable> = truncated.iter().cloned().collect();
        for (source, _) in changes {
            if !self.state.contains_key(&self.name(source)) {
                new.insert(source.clone());
            }
        }
        Ok(new)
    }

    /// Creates `source`'s table with the metadata columns and `seeds`.
    #[fastrace::trace]
    async fn create(&mut self, source: &SourceTable, seeds: Vec<AddColumn>) -> Result<()> {
        ensure!(
            self.state.len() < MAX_TABLES,
            "An export may create at most {MAX_TABLES} tables"
        );
        let name = self.name(source);
        let location = self.location(&name);
        ensure!(
            location.len() <= MAX_LOCATION_BYTES,
            "Cannot store {source:?}: its S3 location is longer than {MAX_LOCATION_BYTES} bytes"
        );
        let creation = TableCreation::builder()
            .name(name)
            .location(location)
            .schema(metadata_schema()?)
            .properties(HashMap::from([
                (ENCODING_PROPERTY.to_owned(), ENCODING.to_owned()),
                (COMPONENT_PROPERTY.to_owned(), source.component.clone()),
                (TABLE_PROPERTY.to_owned(), source.table.clone()),
                (TABLET_PROPERTY.to_owned(), names::tablet_str(source.tablet)),
            ]))
            .build();
        let table = self.catalog.create_table(&self.namespace, creation).await?;
        let table = columns::commit_columns(&*self.catalog, &table, seeds).await?;
        self.record(&table)?;
        self.tables.insert(source.clone(), table);
        Ok(())
    }

    /// Requires one writer per namespace at a time. An `iceberg` commit rebases
    /// onto the table it reloads by name, so an append that overlaps another
    /// writer's drop and recreate would land in the new table.
    ///
    /// `seeds` holds the columns each table in [`Self::new_tables`] starts
    /// with; a table without an entry starts with the metadata columns only.
    #[fastrace::trace]
    pub async fn apply(
        &mut self,
        truncated: &[SourceTable],
        changes: Vec<(SourceTable, Change)>,
        mut seeds: BTreeMap<SourceTable, Vec<AddColumn>>,
    ) -> Result<()> {
        for source in truncated {
            let name = self.name(source);
            if self.load_existing(source, &name).await?.is_some() {
                self.catalog
                    .drop_table(&TableIdent::new(self.namespace.clone(), name.clone()))
                    .await?;
                self.state.remove(&name);
                self.tables.remove(source);
            }
            self.create(source, seeds.remove(source).unwrap_or_default())
                .await?;
        }
        let mut by_table: BTreeMap<SourceTable, Vec<Change>> = BTreeMap::new();
        for (source, change) in changes {
            by_table.entry(source).or_default().push(change);
        }
        let mut missing = Vec::new();
        for source in by_table.keys() {
            if !self.tables.contains_key(source) {
                let name = self.name(source);
                let unregistered = self.unregistered.remove(&name);
                missing.push((source.clone(), name, unregistered));
            }
        }
        let this = &*self;
        let loaded: Vec<(SourceTable, Option<Table>)> = stream::iter(missing)
            .map(|(source, name, unregistered)| async move {
                let table = this.load(&source, &name, unregistered).await?;
                anyhow::Ok((source, table))
            })
            .buffer_unordered(*S3_EXPORT_TABLE_CONCURRENCY)
            .try_collect()
            .await?;
        for (source, table) in loaded {
            match table {
                Some(table) => {
                    self.tables.insert(source, table);
                },
                None => {
                    self.create(&source, seeds.remove(&source).unwrap_or_default())
                        .await?;
                },
            }
        }
        // Each table commits on its own, so appends to different tables are
        // independent. A page that fails partway is retried whole, and readers
        // dedupe the rows a retry repeats.
        let (catalog, tables, snapshot_properties) =
            (&*self.catalog, &self.tables, &self.snapshot_properties);
        let appended: Vec<(SourceTable, Table, u64)> = stream::iter(by_table)
            .map(|(source, changes)| async move {
                let (table, bytes_written) =
                    append(catalog, &tables[&source], &changes, snapshot_properties).await?;
                anyhow::Ok((source, table, bytes_written))
            })
            .buffer_unordered(*S3_EXPORT_TABLE_CONCURRENCY)
            .try_collect()
            .await?;
        for (source, table, bytes_written) in appended {
            self.bytes_written += bytes_written;
            self.record(&table)?;
            self.tables.insert(source, table);
        }
        Ok(())
    }

    /// Sets `convex.porter.retired` on owned tables absent from `selected` and
    /// clears it on the rest. Call this only at a consistent snapshot, when
    /// `selected` lists every source table.
    #[fastrace::trace]
    pub async fn retire_unselected(&mut self, selected: &[SourceTable]) -> Result<()> {
        if !self.complete {
            // Retirement needs every table, not just this page's.
            for name in self.unregistered.keys().cloned().collect::<Vec<_>>() {
                self.register(&name).await?;
            }
            for ident in self.catalog.list_tables(&self.namespace).await? {
                let table = self.catalog.load_table(&ident).await?;
                let source = source_of(&table)?;
                self.validate(&source, &table)?;
                self.record(&table)?;
                self.tables.insert(source, table);
            }
            self.complete = true;
        }
        let mut flips: Vec<(String, SourceTable, bool)> = Vec::new();
        for (name, entry) in &self.state {
            let source = entry.source()?;
            let retired = !selected.iter().any(|s| s.tablet == source.tablet);
            if entry.retired != retired {
                flips.push((name.clone(), source, retired));
            }
        }
        for (name, source, retired) in flips {
            if !self.tables.contains_key(&source) {
                let table = self.load_existing(&source, &name).await?.with_context(|| {
                    format!("The state lists {name}, but the catalog has no such table")
                })?;
                self.tables.insert(source.clone(), table);
            }
            let table = self
                .tables
                .remove(&source)
                .context("table was just loaded")?;
            let tx = Transaction::new(&table);
            let tx = tx
                .update_table_properties()
                .set(RETIRED_PROPERTY.to_owned(), retired.to_string())
                .apply(tx)?;
            let table = tx.commit(&*self.catalog).await?;
            self.record(&table)?;
            self.state
                .get_mut(&name)
                .context("table was just recorded")?
                .retired = retired;
            self.tables.insert(source, table);
        }
        Ok(())
    }
}

/// The Convex tablet behind `table`. Another encoding (the retired JSON
/// `payload` format) means an older Porter mid-deploy; its rows may be the
/// only copy, so stop rather than drop it.
fn source_of(table: &Table) -> Result<SourceTable> {
    let property = |key| table.metadata().properties().get(key).cloned();
    let encoding = property(ENCODING_PROPERTY).ok_or_else(|| {
        ErrorMetadata::bad_request(
            "S3ExportForeignTable",
            format!(
                "Glue table {} was not written by a Convex export. Remove it from the export's \
                 Glue database or recreate the export.",
                table.identifier().name()
            ),
        )
    })?;
    ensure!(
        encoding == ENCODING,
        ErrorMetadata::bad_request(
            "S3ExportTableModified",
            format!(
                "Glue table {} is in the {encoding} format, which this export no longer writes. \
                 Recreate the export to start over in {ENCODING}.",
                table.identifier().name()
            ),
        )
    );
    let required = |key| {
        property(key).ok_or_else(|| modified(table, &format!("its {key} property is missing")))
    };
    Ok(SourceTable {
        component: required(COMPONENT_PROPERTY)?,
        table: required(TABLE_PROPERTY)?,
        tablet: names::parse_tablet(&required(TABLET_PROPERTY)?)
            .map_err(|_| modified(table, "its tablet property is not a tablet ID"))?,
    })
}

/// A user error, so the dashboard shows it: the customer owns the table.
fn modified(table: &Table, what: &str) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "S3ExportTableModified",
        format!(
            "Glue table {} was modified outside Convex: {what}. Recreate the export to start over.",
            table.identifier().name()
        ),
    )
}
