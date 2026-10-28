use std::{
    collections::HashSet,
    sync::Arc,
};

use anyhow::Context;
use common::{
    components::ComponentId,
    document::ParsedDocument,
    runtime::Runtime,
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

use crate::{
    modules::ModuleModel,
    source_packages::types::{
        SourcePackage,
        SourcePackageId,
    },
    SystemIndex,
    SystemTable,
};

pub mod types;
pub mod upload_download;

pub const SOURCE_PACKAGES_TABLE: TableName = TableName::const_new("_source_packages");

pub struct SourcePackagesTable;
impl SystemTable for SourcePackagesTable {
    type Metadata = SourcePackage;

    const TABLE_NAME: TableName = SOURCE_PACKAGES_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct SourcePackageModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
}

impl<'a, RT: Runtime> SourcePackageModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, namespace: TableNamespace) -> Self {
        Self { tx, namespace }
    }

    #[fastrace::trace]
    pub async fn put(&mut self, source_package: SourcePackage) -> anyhow::Result<SourcePackageId> {
        let document_id = SystemMetadataModel::new(self.tx, self.namespace)
            .insert(&SOURCE_PACKAGES_TABLE, source_package.try_into()?)
            .await?;
        let id: DeveloperDocumentId = document_id.into();
        Ok(id.into())
    }

    pub async fn get(
        &mut self,
        source_package_id: SourcePackageId,
    ) -> anyhow::Result<Arc<ParsedDocument<SourcePackage>>> {
        self.tx
            .get_system::<SourcePackagesTable>(
                self.namespace,
                DeveloperDocumentId::from(source_package_id),
            )
            .await?
            .context("Couldn't find source package")
    }

    pub async fn get_latest(
        &mut self,
    ) -> anyhow::Result<Option<Arc<ParsedDocument<SourcePackage>>>> {
        let mut latest_source_pkg: Option<Arc<ParsedDocument<SourcePackage>>> = None;

        // TODO(lee) pass component down, instead of deriving it from the tablet.
        let component = match self.namespace {
            TableNamespace::Global => ComponentId::Root,
            TableNamespace::ByComponent(id) => ComponentId::Child(id),
        };

        let mut seen = HashSet::new();
        for module in ModuleModel::new(self.tx)
            .get_all_metadata(component)
            .await?
        {
            if !seen.insert(module.source_package_id) {
                // Small CPU optimization to avoid going through the transaction fetch machinery
                continue;
            }
            let src_package = self.get(module.source_package_id).await?;
            if let Some(latest) = &latest_source_pkg {
                if src_package.creation_time() > latest.creation_time() {
                    latest_source_pkg = Some(src_package);
                }
            } else {
                latest_source_pkg = Some(src_package);
            }
        }

        Ok(latest_source_pkg)
    }
}
