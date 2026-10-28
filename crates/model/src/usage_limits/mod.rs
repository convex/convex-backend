use std::sync::LazyLock;

use common::{
    document::{
        ParseDocument,
        ParsedDocument,
        CREATION_TIME_FIELD_PATH,
    },
    runtime::Runtime,
};
use database::{
    SystemMetadataModel,
    Transaction,
};
use errors::ErrorMetadata;
use value::{
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;
use common::types::DeploymentType;
use types::{
    UsageLimitConfig,
    UsageLimitKey,
    UsageLimitType,
};

use crate::backend_info::BackendInfoModel;

pub const USAGE_LIMITS_TABLE: TableName = TableName::const_new("_usage_limits");

pub static USAGE_LIMITS_INDEX_BY_SELECTOR: LazyLock<SystemIndex<UsageLimitsTable>> =
    LazyLock::new(|| {
        SystemIndex::new(
            "by_selector",
            [
                &USAGE_LIMIT_METRIC_FIELD,
                &USAGE_LIMIT_WINDOW_FIELD,
                &USAGE_LIMIT_TYPE_FIELD,
                &CREATION_TIME_FIELD_PATH,
            ],
        )
        .unwrap()
    });
static USAGE_LIMIT_METRIC_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "metric".parse().expect("invalid metric field"));
static USAGE_LIMIT_WINDOW_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "window".parse().expect("invalid window field"));
static USAGE_LIMIT_TYPE_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "limitType".parse().expect("invalid limitType field"));

pub struct UsageLimitsTable;
impl SystemTable for UsageLimitsTable {
    type Metadata = UsageLimitConfig;

    const TABLE_NAME: TableName = USAGE_LIMITS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![USAGE_LIMITS_INDEX_BY_SELECTOR.clone()]
    }
}

pub struct UsageLimitsModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> UsageLimitsModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn list(&mut self) -> anyhow::Result<Vec<ParsedDocument<UsageLimitConfig>>> {
        let configs = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<UsageLimitsTable>::by_id(),
            )?
            .all()
            .await?
            .into_iter()
            .map(|config| (*config).clone())
            .collect();
        Ok(configs)
    }

    pub async fn get(
        &mut self,
        id: ResolvedDocumentId,
    ) -> anyhow::Result<Option<ParsedDocument<UsageLimitConfig>>> {
        let Some(document) = self.tx.get(id).await? else {
            return Ok(None);
        };
        Ok(Some(document.parse()?))
    }

    pub async fn create(&mut self, config: UsageLimitConfig) -> anyhow::Result<ResolvedDocumentId> {
        self.validate_write(None, &config).await?;
        SystemMetadataModel::new_global(self.tx)
            .insert(&USAGE_LIMITS_TABLE, config.try_into()?)
            .await
    }

    pub async fn replace(
        &mut self,
        id: ResolvedDocumentId,
        config: UsageLimitConfig,
    ) -> anyhow::Result<()> {
        if self.get(id).await?.is_none() {
            return Err(ErrorMetadata::not_found(
                "UsageLimitNotFound",
                "The usage limit couldn't be found.",
            )
            .into());
        }
        self.validate_write(Some(id), &config).await?;
        SystemMetadataModel::new_global(self.tx)
            .replace(id, config.try_into()?)
            .await?;
        Ok(())
    }

    pub async fn delete(
        &mut self,
        id: ResolvedDocumentId,
    ) -> anyhow::Result<Option<UsageLimitConfig>> {
        let Some(config) = self.get(id).await? else {
            return Ok(None);
        };
        SystemMetadataModel::new_global(self.tx)
            .delete(config.id())
            .await?;
        Ok(Some(config.into_value()))
    }

    async fn validate_write(
        &mut self,
        replacing: Option<ResolvedDocumentId>,
        config: &UsageLimitConfig,
    ) -> anyhow::Result<()> {
        config.validate()?;
        let key = config.key();
        let existing = self.get_by_key(key).await?;
        if let Some(existing) = existing
            && Some(existing.id()) != replacing
        {
            return Err(ErrorMetadata::bad_request(
                "DuplicateUsageLimit",
                "A usage limit already exists for this metric, window, and limit type.",
            )
            .into());
        }

        // A "warning" limit's only effect is emailing the team. Dev deployments
        // don't send usage limit emails, so a warning limit there would silently
        // never fire; reject it rather than let one be configured. Deployments
        // with no backend info (e.g. self-hosted) have no dev/prod distinction,
        // so they're allowed.
        if config.limit_type == UsageLimitType::Warning {
            let deployment_type = BackendInfoModel::new(self.tx)
                .get()
                .await?
                .map(|bi| bi.deployment_type);
            if deployment_type == Some(DeploymentType::Dev) {
                return Err(ErrorMetadata::bad_request(
                    "UsageLimitWarningNotSupported",
                    "Warning usage limits aren't supported on development deployments, which \
                     don't receive usage limit email notifications.",
                )
                .into());
            }
        }
        Ok(())
    }

    async fn get_by_key(
        &mut self,
        key: UsageLimitKey,
    ) -> anyhow::Result<Option<ParsedDocument<UsageLimitConfig>>> {
        let metric = key.metric.to_string();
        let window = key.window.to_string();
        let limit_type = key.limit_type.to_string();
        Ok(self
            .tx
            .query_system(TableNamespace::Global, &USAGE_LIMITS_INDEX_BY_SELECTOR)?
            .eq(&[metric.as_str()])?
            .eq(&[window.as_str()])?
            .eq(&[limit_type.as_str()])?
            .unique()
            .await?
            .map(|config| (*config).clone()))
    }
}
