use std::{
    collections::BTreeMap,
    sync::Arc,
};

use anyhow::Context;
use common::{
    document::ParsedDocument,
    query::Order,
    runtime::Runtime,
    types::NodeDependency,
};
use database::{
    SystemMetadataModel,
    Transaction,
};
use value::{
    id_v6::DeveloperDocumentId,
    TableName,
    TableNamespace,
};

use self::types::{
    ExternalDepsPackage,
    ExternalDepsPackageId,
};
use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;

const NUM_EXTERNAL_DEPS_CACHE_ENTRIES: usize = 10;

pub const EXTERNAL_PACKAGES_TABLE: TableName = TableName::const_new("_external_deps_packages");

pub struct ExternalPackagesTable;
impl SystemTable for ExternalPackagesTable {
    type Metadata = ExternalDepsPackage;

    const TABLE_NAME: TableName = EXTERNAL_PACKAGES_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct ExternalPackagesModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> ExternalPackagesModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    #[fastrace::trace]
    pub async fn get(
        &mut self,
        external_deps_package_id: ExternalDepsPackageId,
    ) -> anyhow::Result<ParsedDocument<ExternalDepsPackage>> {
        let id: DeveloperDocumentId = external_deps_package_id.into();
        Ok(Arc::unwrap_or_clone(
            self.tx
                .get_system::<ExternalPackagesTable>(TableNamespace::Global, id)
                .await?
                .context("Couldn't find external package")?,
        ))
    }

    pub async fn put(
        &mut self,
        external_deps_package: ExternalDepsPackage,
    ) -> anyhow::Result<ExternalDepsPackageId> {
        let id = SystemMetadataModel::new_global(self.tx)
            .insert(&EXTERNAL_PACKAGES_TABLE, external_deps_package.try_into()?)
            .await?;
        let doc_id: DeveloperDocumentId = id.into();
        Ok(doc_id.into())
    }

    #[fastrace::trace]
    pub async fn get_cached_package_match(
        &mut self,
        deps: Vec<NodeDependency>,
    ) -> anyhow::Result<Option<(ExternalDepsPackageId, ExternalDepsPackage)>> {
        let deps_map: BTreeMap<String, String> = deps
            .into_iter()
            .map(|dep| (dep.package, dep.version))
            .collect();

        // Check at most NUM_EXTERNAL_DEPS_CACHE_ENTRIES entries for a match
        let mut cache_entries_checked = 0;
        let index = SystemIndex::<ExternalPackagesTable>::by_creation_time();
        let mut query = self
            .tx
            .query_system(TableNamespace::Global, &index)?
            .order(Order::Desc)
            .build();
        while let Some(pkg) = query.next().await?
            && cache_entries_checked < NUM_EXTERNAL_DEPS_CACHE_ENTRIES
        {
            let id = pkg.id();

            let pkg_deps_map: BTreeMap<String, String> = pkg
                .deps
                .clone()
                .into_iter()
                .map(|dep| (dep.package, dep.version))
                .collect();
            if pkg_deps_map.eq(&deps_map) {
                return Ok(Some((
                    DeveloperDocumentId::from(id).into(),
                    (**pkg).clone(),
                )));
            }

            cache_entries_checked += 1;
        }
        Ok(None)
    }
}
