use std::sync::Arc;

use common::{
    document::ParsedDocument,
    runtime::Runtime,
};
use database::{
    SystemMetadataModel,
    Transaction,
};
use value::{
    TableName,
    TableNamespace,
};

use self::types::AwsLambdaVersion;
use crate::{
    source_packages::types::SourcePackageId,
    SystemIndex,
    SystemTable,
};

pub mod types;

pub const AWS_LAMBDA_VERSIONS_TABLE: TableName = TableName::const_new("_aws_lambda_versions");

pub struct AwsLambdaVersionsTable;
impl SystemTable for AwsLambdaVersionsTable {
    type Metadata = AwsLambdaVersion;

    const TABLE_NAME: TableName = AWS_LAMBDA_VERSIONS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct AwsLambdaVersionsModel<'a, RT: Runtime> {
    lambda_name: String,
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> AwsLambdaVersionsModel<'a, RT> {
    pub fn new(lambda_name: String, tx: &'a mut Transaction<RT>) -> Self {
        Self { lambda_name, tx }
    }

    pub async fn mark_deployed(&mut self, new_version: AwsLambdaVersion) -> anyhow::Result<()> {
        // AWS' sentinel is "$LATEST". Make sure we never write this by accident.
        anyhow::ensure!(new_version.lambda_version != "$LATEST");
        anyhow::ensure!(new_version.lambda_version != "");
        if let Some(latest) = self.latest_version_document().await? {
            SystemMetadataModel::new_global(self.tx)
                .replace(latest.id(), new_version.try_into()?)
                .await?;
        } else {
            SystemMetadataModel::new_global(self.tx)
                .insert(&AWS_LAMBDA_VERSIONS_TABLE, new_version.try_into()?)
                .await?;
        }
        Ok(())
    }

    pub async fn latest_version(&mut self) -> anyhow::Result<Option<AwsLambdaVersion>> {
        Ok(self.latest_version_document().await?.map(|v| (**v).clone()))
    }

    pub async fn latest_version_document(
        &mut self,
    ) -> anyhow::Result<Option<Arc<ParsedDocument<AwsLambdaVersion>>>> {
        // Do a full table scan on lambda versions table. Good enough
        // because table is small. If it were bigger, we'd want index on lambda_name
        let mut query = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<AwsLambdaVersionsTable>::by_creation_time(),
            )?
            .all()
            .await?
            .into_iter()
            .filter(|doc| doc.lambda_name == self.lambda_name);
        let doc = query.next();
        anyhow::ensure!(
            query.next().is_none(),
            "found more than one lambda version for {}",
            self.lambda_name
        );
        Ok(doc)
    }

    /// Determines if we should route execute requests to static lambda or not.
    /// Decides by fetching the latest source package and see if it matches
    /// the current latest deployed static Lambda. If not, should route to
    /// dynamic Lambda.
    ///
    /// Requires
    /// - Lambda name this model is constructed with is that of the static
    ///   Lambda.
    pub async fn static_lambda_ready(
        &mut self,
        source_pkg_id: &SourcePackageId,
    ) -> anyhow::Result<Option<Arc<ParsedDocument<AwsLambdaVersion>>>> {
        // Fetch static lambda version
        let static_version = self.latest_version_document().await?;

        // Check if static version source package ID matches latest source package
        if let Some(static_version) = static_version {
            let source_package_id = static_version.package_desc.as_static()?;
            let should_use_static = source_package_id.as_ref() == Some(source_pkg_id);

            Ok(should_use_static.then_some(static_version))
        } else {
            Ok(None)
        }
    }
}
