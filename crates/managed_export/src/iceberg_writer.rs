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

use super::{
    append,
    change_log_schema,
    Change,
};

const ENCODING_PROPERTY: &str = "convex.porter.encoding";
const ENCODING: &str = "convex-change-log-json-v1";
const RETIRED_PROPERTY: &str = "convex.porter.retired";
// Glue lowercases names, so these record the exact source table.
const COMPONENT_PROPERTY: &str = "convex.porter.component";
const TABLE_PROPERTY: &str = "convex.porter.table";
const ROOT_COMPONENT: &str = "app";
// Glue and Athena limit database and table names to 255 bytes.
const MAX_NAME_BYTES: usize = 255;
// S3 keys are at most 1024 bytes; leave room for Iceberg's file names.
const MAX_LOCATION_BYTES: usize = 896;

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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceTable {
    pub component: String,
    pub table: String,
}

pub struct IcebergChangeWriter {
    catalog: Arc<dyn Catalog>,
    namespace: NamespaceIdent,
    warehouse_uri: String,
    selected: Vec<SourceTable>,
    tables: BTreeMap<SourceTable, Table>,
    /// Metadata location of every table in the namespace, by Glue name.
    locations: BTreeMap<String, String>,
    /// Tables in `locations` that the catalog does not hold yet. Registering
    /// reads the table's metadata from S3, so it waits until a page uses it.
    unregistered: BTreeMap<String, String>,
    /// Glue names of the tables marked retired, when known. `None` makes
    /// retirement load every table to find out.
    retired: Option<BTreeSet<String>>,
    snapshot_properties: HashMap<String, String>,
    /// Parquet bytes this writer has committed to the bucket. Billed to the
    /// customer as data egress.
    bytes_written: u64,
}

impl IcebergChangeWriter {
    /// Writes Iceberg data and metadata to S3 without touching Glue. Table
    /// pointers start at `locations` and live in memory; the caller commits
    /// [`Self::locations`] and [`Self::retired`] once the page succeeds. Each
    /// append records `snapshot_properties` in its snapshot summary.
    pub async fn staged(
        namespace: &str,
        warehouse_uri: &str,
        destination: S3Destination,
        selected: Vec<SourceTable>,
        locations: &BTreeMap<String, String>,
        retired: &BTreeSet<String>,
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
        let mut writer = Self::open(Arc::new(catalog), namespace, warehouse_uri, selected).await?;
        writer.locations = locations.clone();
        writer.unregistered = locations.clone();
        writer.retired = Some(retired.clone());
        writer.snapshot_properties = snapshot_properties;
        Ok(writer)
    }

    /// Metadata location of every table after this page, by Glue name.
    pub fn locations(&self) -> &BTreeMap<String, String> {
        &self.locations
    }

    /// Glue names of the tables marked retired after this page, when known.
    pub fn retired(&self) -> Option<&BTreeSet<String>> {
        self.retired.as_ref()
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
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

    fn record(&mut self, table: &Table) -> Result<()> {
        self.locations.insert(
            table.identifier().name().to_owned(),
            table.metadata_location_result()?.to_owned(),
        );
        Ok(())
    }

    #[fastrace::trace]
    async fn open(
        catalog: Arc<dyn Catalog>,
        namespace: &str,
        warehouse_uri: &str,
        selected: Vec<SourceTable>,
    ) -> Result<Self> {
        ensure!(
            is_glue_name(namespace),
            "Invalid Glue database name {namespace:?}"
        );
        Self::check_names(warehouse_uri, &selected)?;
        let namespace = NamespaceIdent::new(namespace.to_owned());
        if !catalog.namespace_exists(&namespace).await? {
            catalog.create_namespace(&namespace, HashMap::new()).await?;
        }
        let writer = Self {
            catalog,
            namespace,
            warehouse_uri: warehouse_uri.trim_end_matches('/').to_owned(),
            selected,
            tables: BTreeMap::new(),
            locations: BTreeMap::new(),
            unregistered: BTreeMap::new(),
            retired: None,
            snapshot_properties: HashMap::new(),
            bytes_written: 0,
        };
        Ok(writer)
    }

    fn component(source: &SourceTable) -> String {
        if source.component.is_empty() {
            ROOT_COMPONENT.to_owned()
        } else {
            source.component.replace('/', "__")
        }
    }

    fn table_name(source: &SourceTable) -> Result<String> {
        let name = format!("{}__{}", Self::component(source), source.table).to_lowercase();
        ensure!(
            is_glue_name(&name),
            "Cannot name a Glue table for {source:?}: names must be at most {MAX_NAME_BYTES} bytes"
        );
        Ok(name)
    }

    /// Directories keep the exact Convex names, since S3 keys are
    /// case-sensitive.
    fn directory(source: &SourceTable) -> String {
        format!("{}/{}", Self::component(source), source.table)
    }

    fn location(warehouse_uri: &str, source: &SourceTable) -> String {
        format!(
            "{}/{}",
            warehouse_uri.trim_end_matches('/'),
            Self::directory(source)
        )
    }

    fn validate(&self, source: &SourceTable, table: &Table) -> Result<()> {
        let metadata = table.metadata();
        ensure!(
            table.identifier().name() == Self::table_name(source)?
                && metadata.location().trim_end_matches('/')
                    == Self::location(&self.warehouse_uri, source),
            "Iceberg table name or location does not match its source"
        );
        ensure!(
            metadata.current_schema().as_struct() == change_log_schema()?.as_struct()
                && metadata.default_partition_spec().is_unpartitioned(),
            "Iceberg table schema or partitioning mismatch"
        );
        Ok(())
    }

    fn check_names<'a>(
        warehouse_uri: &str,
        sources: impl IntoIterator<Item = &'a SourceTable>,
    ) -> Result<()> {
        let mut by_name = BTreeMap::new();
        let mut by_directory = BTreeMap::new();
        for source in sources {
            ensure!(
                Self::location(warehouse_uri, source).len() <= MAX_LOCATION_BYTES,
                "Cannot store {source:?}: its S3 location is longer than {MAX_LOCATION_BYTES} \
                 bytes"
            );
            for (key, claimed) in [
                (Self::table_name(source)?, &mut by_name),
                (Self::directory(source), &mut by_directory),
            ] {
                if let Some(other) = claimed.insert(key.clone(), source)
                    && other != source
                {
                    anyhow::bail!(
                        "Convex tables {other:?} and {source:?} both map to Iceberg table {key}"
                    );
                }
            }
        }
        Ok(())
    }

    #[fastrace::trace]
    async fn load_existing(&mut self, ident: &TableIdent) -> Result<Option<Table>> {
        self.register(ident.name()).await?;
        if !self.catalog.table_exists(ident).await? {
            return Ok(None);
        }
        let table = self.catalog.load_table(ident).await?;
        self.validate(&source_of(&table)?, &table)?;
        Ok(Some(table))
    }

    #[fastrace::trace]
    async fn create(&mut self, source: &SourceTable) -> Result<()> {
        let name = Self::table_name(source)?;
        let creation = TableCreation::builder()
            .name(name.clone())
            .location(Self::location(&self.warehouse_uri, source))
            .schema(change_log_schema()?)
            .properties(HashMap::from([
                (ENCODING_PROPERTY.to_owned(), ENCODING.to_owned()),
                (COMPONENT_PROPERTY.to_owned(), source.component.clone()),
                (TABLE_PROPERTY.to_owned(), source.table.clone()),
            ]))
            .build();
        let table = self.catalog.create_table(&self.namespace, creation).await?;
        self.record(&table)?;
        self.tables.insert(source.clone(), table);
        Ok(())
    }

    /// Requires one writer per namespace at a time. An `iceberg` commit rebases
    /// onto the table it reloads by name, so an append that overlaps another
    /// writer's drop and recreate would land in the new table.
    #[fastrace::trace]
    pub async fn apply(
        &mut self,
        truncated: &[SourceTable],
        changes: Vec<(SourceTable, Change)>,
    ) -> Result<()> {
        Self::check_names(
            &self.warehouse_uri,
            self.selected
                .iter()
                .chain(truncated)
                .chain(changes.iter().map(|(source, _)| source)),
        )?;
        for source in truncated {
            let name = Self::table_name(source)?;
            // No two selected tables share a name, so any other table holding
            // this name is no longer selected, such as `Users` after the
            // deployment replaced it with `users`.
            let ident = TableIdent::new(self.namespace.clone(), name);
            if let Some(table) = self.load_existing(&ident).await? {
                self.catalog.drop_table(&ident).await?;
                self.locations.remove(ident.name());
                // The recreated table starts unmarked.
                if let Some(retired) = &mut self.retired {
                    retired.remove(ident.name());
                }
                self.tables.remove(&source_of(&table)?);
            }
            self.create(source).await?;
        }
        let mut by_table: BTreeMap<SourceTable, Vec<Change>> = BTreeMap::new();
        for (source, change) in changes {
            by_table.entry(source).or_default().push(change);
        }
        for (source, changes) in by_table {
            if !self.tables.contains_key(&source) {
                let ident = TableIdent::new(self.namespace.clone(), Self::table_name(&source)?);
                if let Some(table) = self.load_existing(&ident).await? {
                    ensure!(
                        source_of(&table)? == source,
                        "Glue table {} belongs to another source table",
                        ident.name()
                    );
                    self.tables.insert(source.clone(), table);
                } else {
                    self.create(&source).await?;
                }
            }
            let (table, bytes_written) = append(
                &*self.catalog,
                &self.tables[&source],
                &changes,
                &self.snapshot_properties,
            )
            .await?;
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
        let selected_names = selected
            .iter()
            .map(Self::table_name)
            .collect::<Result<BTreeSet<_>>>()?;
        let unselected = |locations: &BTreeMap<String, String>| -> BTreeSet<String> {
            locations
                .keys()
                .filter(|name| !selected_names.contains(*name))
                .cloned()
                .collect()
        };
        // `locations` is complete whenever `retired` is known.
        if self.retired.as_ref() == Some(&unselected(&self.locations)) {
            return Ok(());
        }
        // Retirement needs the full catalog, including tables untouched by this page.
        for name in self.unregistered.keys().cloned().collect::<Vec<_>>() {
            self.register(&name).await?;
        }
        for ident in self.catalog.list_tables(&self.namespace).await? {
            if let Some(table) = self.load_existing(&ident).await? {
                self.record(&table)?;
                self.tables.insert(source_of(&table)?, table);
            }
        }
        let retired = unselected(&self.locations);
        for (source, table) in &mut self.tables {
            let retired = !selected.contains(source);
            let marked = table
                .metadata()
                .properties()
                .get(RETIRED_PROPERTY)
                .is_some_and(|value| value == "true");
            if marked != retired {
                let tx = Transaction::new(table);
                let tx = tx
                    .update_table_properties()
                    .set(RETIRED_PROPERTY.to_owned(), retired.to_string())
                    .apply(tx)?;
                *table = tx.commit(&*self.catalog).await?;
                self.locations.insert(
                    table.identifier().name().to_owned(),
                    table.metadata_location_result()?.to_owned(),
                );
            }
        }
        self.retired = Some(retired);
        Ok(())
    }
}

fn source_of(table: &Table) -> Result<SourceTable> {
    let property = |key| table.metadata().properties().get(key).cloned();
    ensure!(
        property(ENCODING_PROPERTY).as_deref() == Some(ENCODING),
        "Glue table {} was not written by a Convex export",
        table.identifier()
    );
    Ok(SourceTable {
        component: property(COMPONENT_PROPERTY).context("Missing component property")?,
        table: property(TABLE_PROPERTY).context("Missing table property")?,
    })
}

fn is_glue_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
