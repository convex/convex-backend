use std::{
    collections::BTreeMap,
    str::FromStr,
};

use common::{
    bootstrap_model::index::IndexMetadata,
    components::ComponentPath,
    document::ResolvedDocument,
    http::RequestDestination,
    log_streaming::{
        LogEvent,
        StructuredLogEvent,
    },
    pii::PII,
    runtime::UnixTimestamp,
    types::{
        AccessTokenId,
        GenericIndexName,
        IndexDiff,
        IndexName,
        MemberId,
        SystemStopState,
        UsageLimitStopState,
    },
};
use errors::ErrorMetadata;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
use value::{
    codegen_convex_serialization,
    obj,
    remove_int64,
    remove_nullable_string,
    remove_object,
    remove_string,
    remove_vec,
    remove_vec_of_strings,
    val,
    ConvexObject,
    ConvexValue,
    FieldName,
    TableName,
};

use super::integration_config::{
    IntegrationAuditConfig,
    IntegrationAuditCredentialChanges,
};
use crate::{
    auth::types::AuthDiff,
    backend_state::types::OldBackendState,
    components::config::{
        ComponentDiff,
        SerializedComponentDiff,
    },
    config::types::ConfigDiff,
    deployment_audit_log::developer_index_config::{
        DeveloperIndexConfig,
        SerializedDeveloperIndexConfig,
        SerializedNamedDeveloperIndexConfig,
    },
    environment_variables::types::EnvVarName,
    snapshot_imports::types::{
        ImportFormat,
        ImportMode,
        ImportRequestor,
    },
    source_packages::types::{
        NodeVersionDiff,
        SerializedNodeVersionDiff,
    },
    usage_limits::types::UsageLimitConfig,
};

pub const DEPLOYMENT_AUDIT_LOG_TABLE: TableName = TableName::const_new("_deployment_audit_log");

#[derive(Default, Serialize, Deserialize)]
struct IntegrationAuditMetadata {
    id: String,
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<IntegrationAuditConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<IntegrationAuditConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current: Option<IntegrationAuditConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential_changes: Option<IntegrationAuditCredentialChanges>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AuditLogIndexDiff {
    pub added_indexes: Vec<(IndexName, DeveloperIndexConfig)>,
    pub removed_indexes: Vec<(IndexName, DeveloperIndexConfig)>,
    pub enabled_indexes: Vec<(IndexName, DeveloperIndexConfig)>,
    pub disabled_indexes: Vec<(IndexName, DeveloperIndexConfig)>,
}

impl From<IndexDiff> for AuditLogIndexDiff {
    fn from(diff: IndexDiff) -> Self {
        let added_indexes = diff
            .added
            .into_iter()
            .map(|IndexMetadata::<TableName> { name, config }| (name, config.into()))
            .collect();
        let removed_indexes = diff
            .dropped
            .into_iter()
            .map(|index| (index.name.clone(), index.into_value().config.into()))
            .collect();
        let enabled_indexes = diff
            .enabled
            .into_iter()
            .map(|index| (index.name.clone(), index.into_value().config.into()))
            .collect();
        let disabled_indexes = diff
            .disabled
            .into_iter()
            .map(|index| (index.name.clone(), index.into_value().config.into()))
            .collect();
        Self {
            added_indexes,
            removed_indexes,
            enabled_indexes,
            disabled_indexes,
        }
    }
}

#[derive(Debug, Clone, strum::EnumDiscriminants)]
#[strum_discriminants(
    name(DeploymentAuditLogEventKind),
    derive(strum::EnumIter, strum::IntoStaticStr),
    strum(serialize_all = "snake_case")
)]
pub enum DeploymentAuditLogEvent {
    CreateEnvironmentVariable {
        name: EnvVarName,
    },
    UpdateEnvironmentVariable {
        name: EnvVarName,
    },
    DeleteEnvironmentVariable {
        name: EnvVarName,
    },
    // The config fields are pinned to fixed values in proptests: generating
    // nested arbitrary `UsageLimitConfig`s inside this large derived enum
    // strategy overflows the test thread's stack.
    CreateUsageLimit {
        id: String,
        config: UsageLimitConfig,
    },
    UpdateUsageLimit {
        id: String,
        previous: UsageLimitConfig,
        current: UsageLimitConfig,
    },
    DeleteUsageLimit {
        id: String,
        config: UsageLimitConfig,
    },
    UsageLimitExceeded {
        id: String,
        config: UsageLimitConfig,
    },
    /// The deployment's usage-limit stop state changed: it was disabled by
    /// crossing a `Disable` limit, or re-enabled after coming back under
    /// every enabled `Disable` limit. Deployment-level, unlike the per-limit
    /// `UsageLimitExceeded`.
    ChangeUsageLimitStopState {
        old_state: UsageLimitStopState,
        new_state: UsageLimitStopState,
    },
    ReplaceEnvironmentVariable {
        previous_name: EnvVarName,
        name: EnvVarName,
    },
    UpdateCanonicalUrl {
        request_destination: RequestDestination,
        url: String,
    },
    DeleteCanonicalUrl {
        request_destination: RequestDestination,
    },
    PushConfig {
        config_diff: ConfigDiff,
    },
    PushConfigWithComponents {
        diffs: PushComponentDiffs,
    },
    BuildIndexes {
        added_indexes: Vec<(IndexName, DeveloperIndexConfig)>,
        removed_indexes: Vec<(IndexName, DeveloperIndexConfig)>,
    },
    // Deprecated: replaced by ChangeUserStopState / ChangeSystemStopState
    ChangeDeploymentState {
        old_state: OldBackendState,
        new_state: OldBackendState,
    },
    PauseDeployment,
    UnpauseDeployment,
    ChangeSystemStopState {
        old_state: SystemStopState,
        new_state: SystemStopState,
    },
    // TODO: consider adding table names once this is logged for more places
    // and we have a story about limiting size.
    ClearTables,
    SnapshotImport {
        table_names: BTreeMap<ComponentPath, Vec<TableName>>,
        table_count: u64,
        import_mode: ImportMode,
        import_format: ImportFormat,
        requestor: ImportRequestor,
        table_names_deleted: BTreeMap<ComponentPath, Vec<TableName>>,
        table_count_deleted: u64,
    },
    DeleteScheduledJobsTable {
        component_id: Option<String>,
        component: ComponentPath,
    },
    DeleteTables {
        component_id: Option<String>,
        component: ComponentPath,
        table_names: Vec<TableName>,
    },
    DeleteComponent {
        component_id: Option<String>,
        component: ComponentPath,
    },
    CancelAllScheduledFunctions {
        component_id: Option<String>,
        component: ComponentPath,
    },
    CancelScheduledFunction {
        component_id: Option<String>,
        component: ComponentPath,
        scheduled_function_id: String,
        function_path: Option<String>,
    },
    RequestExport {
        id: String,
        component_id: Option<String>,
        component: ComponentPath,
        format: String,
        requestor: String,
    },
    DownloadExport {
        id: String,
        component_id: Option<String>,
        component: ComponentPath,
        format: String,
        requestor: String,
    },
    CancelExport {
        id: String,
    },
    SetExportExpiration {
        id: String,
        expiration_ts_ms: i64,
    },
    CreateIntegration {
        id: String,
        r#type: String,
        config: Option<IntegrationAuditConfig>,
    },
    UpdateIntegration {
        id: String,
        r#type: String,
        previous: Option<IntegrationAuditConfig>,
        current: Option<IntegrationAuditConfig>,
        credential_changes: Option<IntegrationAuditCredentialChanges>,
    },
    DeleteIntegration {
        id: String,
        r#type: String,
        config: Option<IntegrationAuditConfig>,
    },
    // System UDF audit events (from dashboard mutations)
    AddDocuments {
        component_id: Option<String>,
        component: ComponentPath,
        table: String,
        document_ids: Vec<String>,
    },
    DeleteDocuments {
        component_id: Option<String>,
        component: ComponentPath,
        table: String,
        document_ids: Vec<String>,
    },
    UpdateDocuments {
        component_id: Option<String>,
        component: ComponentPath,
        table: String,
        document_ids: Vec<String>,
    },
    CreateTable {
        component_id: Option<String>,
        component: ComponentPath,
        table: String,
    },
    DeleteFiles {
        component_id: Option<String>,
        component: ComponentPath,
        storage_ids: Vec<String>,
    },
    GenerateUploadUrl {
        component_id: Option<String>,
        component: ComponentPath,
    },
    CreateDataSync {
        sync_id: String,
    },
}

impl From<IndexDiff> for DeploymentAuditLogEvent {
    fn from(value: IndexDiff) -> Self {
        let added_indexes = value
            .added
            .into_iter()
            .map(|m| (m.name, DeveloperIndexConfig::from(m.config)))
            .collect();

        let removed_indexes = value
            .dropped
            .into_iter()
            .map(|d| {
                (
                    d.name.clone(),
                    DeveloperIndexConfig::from(d.into_value().config),
                )
            })
            .collect();

        DeploymentAuditLogEvent::BuildIndexes {
            added_indexes,
            removed_indexes,
        }
    }
}

impl DeploymentAuditLogEventKind {
    pub fn action(self) -> &'static str {
        self.into()
    }

    pub fn actions() -> impl Iterator<Item = &'static str> {
        use strum::IntoEnumIterator;
        Self::iter().map(Self::action)
    }
}

impl DeploymentAuditLogEvent {
    pub fn action(&self) -> &'static str {
        DeploymentAuditLogEventKind::from(self).action()
    }

    pub fn metadata(self) -> anyhow::Result<ConvexObject> {
        match self {
            DeploymentAuditLogEvent::CreateEnvironmentVariable { name }
            | DeploymentAuditLogEvent::UpdateEnvironmentVariable { name }
            | DeploymentAuditLogEvent::DeleteEnvironmentVariable { name } => {
                obj!("variable_name" => name.to_string())
            },
            DeploymentAuditLogEvent::CreateUsageLimit { id, config }
            | DeploymentAuditLogEvent::DeleteUsageLimit { id, config }
            | DeploymentAuditLogEvent::UsageLimitExceeded { id, config } => {
                obj!(
                    "id" => id,
                    "config" => ConvexValue::try_from(config)?,
                )
            },
            DeploymentAuditLogEvent::UpdateUsageLimit {
                id,
                previous,
                current,
            } => {
                obj!(
                    "id" => id,
                    "previous" => ConvexValue::try_from(previous)?,
                    "current" => ConvexValue::try_from(current)?,
                )
            },
            DeploymentAuditLogEvent::ReplaceEnvironmentVariable {
                previous_name,
                name,
            } => {
                obj!("variable_name" => name.to_string(), "previous_variable_name" => previous_name.to_string())
            },
            DeploymentAuditLogEvent::UpdateCanonicalUrl {
                request_destination,
                url,
            } => {
                obj!("request_destination" => request_destination.to_string(), "url" => url)
            },
            DeploymentAuditLogEvent::DeleteCanonicalUrl {
                request_destination,
            } => {
                obj!("request_destination" => request_destination.to_string())
            },
            DeploymentAuditLogEvent::PushConfig { config_diff } => {
                ConvexObject::try_from(config_diff)
            },
            DeploymentAuditLogEvent::PushConfigWithComponents { diffs } => diffs.try_into(),
            DeploymentAuditLogEvent::BuildIndexes {
                added_indexes,
                removed_indexes,
            } => {
                let added_indexes_metadata: Vec<ConvexValue> = added_indexes
                    .into_iter()
                    .map(|(name, config)| {
                        let config_value = ConvexValue::try_from(config)?;
                        let name_value = ConvexValue::try_from(name.to_string())?;
                        let metadata_value = match config_value {
                            ConvexValue::Object(o) => ConvexValue::Object(
                                o.insert(const { FieldName::const_new("name") }, name_value)?,
                            ),
                            _ => anyhow::bail!("Expected config value to be an object"),
                        };
                        Ok(metadata_value)
                    })
                    .collect::<anyhow::Result<Vec<ConvexValue>>>()?;

                let removed_indexes_metadata: Vec<ConvexValue> = removed_indexes
                    .into_iter()
                    .map(|(name, config)| {
                        let config_value = ConvexValue::try_from(config)?;
                        let name_value = ConvexValue::try_from(name.to_string())?;
                        let metadata_value = match config_value {
                            ConvexValue::Object(o) => ConvexValue::Object(
                                o.insert(const { FieldName::const_new("name") }, name_value)?,
                            ),
                            _ => anyhow::bail!("Expected config value to be an object"),
                        };
                        Ok(metadata_value)
                    })
                    .collect::<anyhow::Result<Vec<ConvexValue>>>()?;

                obj!(
                    "added_indexes" => added_indexes_metadata,
                    "removed_indexes" => removed_indexes_metadata
                )
            },
            DeploymentAuditLogEvent::ChangeDeploymentState {
                old_state,
                new_state,
            } => {
                obj!("old_state" => old_state.to_string(), "new_state" => new_state.to_string())
            },
            DeploymentAuditLogEvent::PauseDeployment => obj!(),
            DeploymentAuditLogEvent::UnpauseDeployment => obj!(),
            DeploymentAuditLogEvent::ChangeUsageLimitStopState {
                old_state,
                new_state,
            } => {
                obj!("old_state" => old_state.to_string(), "new_state" => new_state.to_string())
            },
            DeploymentAuditLogEvent::ChangeSystemStopState {
                old_state,
                new_state,
            } => {
                obj!("old_state" => old_state.to_string(), "new_state" => new_state.to_string())
            },
            DeploymentAuditLogEvent::SnapshotImport {
                table_names,
                table_count,
                import_mode,
                import_format,
                requestor,
                table_names_deleted,
                table_count_deleted,
            } => {
                let table_names: Vec<_> = table_names
                    .into_iter()
                    .map(|(component_path, table_names)| {
                        let component_path: ConvexValue = component_path.serialize().try_into()?;
                        let table_names: Vec<_> = table_names
                            .into_iter()
                            .map(|table_name| {
                                anyhow::Ok(ConvexValue::String(table_name.to_string().try_into()?))
                            })
                            .try_collect()?;
                        anyhow::Ok(val!({
                            "component" => component_path,
                            "table_names" => table_names,
                        }))
                    })
                    .try_collect()?;
                let table_names_deleted: Vec<_> = table_names_deleted
                    .into_iter()
                    .map(|(component_path, table_names)| {
                        let component_path: ConvexValue = component_path.serialize().try_into()?;
                        let table_names: Vec<_> = table_names
                            .into_iter()
                            .map(|table_name| {
                                anyhow::Ok(ConvexValue::String(table_name.to_string().try_into()?))
                            })
                            .try_collect()?;
                        anyhow::Ok(val!({
                            "component" => component_path,
                            "table_names" => table_names,
                        }))
                    })
                    .try_collect()?;
                obj!(
                    "table_names" => table_names,
                    "table_count" => table_count as i64,
                    "import_mode" => import_mode.to_string(),
                    "import_format" => ConvexObject::try_from(import_format)?,
                    "requestor" => ConvexObject::try_from(requestor)?,
                    "table_names_deleted" => table_names_deleted,
                    "table_count_deleted" => table_count_deleted as i64,
                )
            },
            DeploymentAuditLogEvent::ClearTables => obj!(),
            DeploymentAuditLogEvent::DeleteScheduledJobsTable {
                component_id,
                component,
            } => {
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize()
                )
            },
            DeploymentAuditLogEvent::DeleteTables {
                component_id,
                component,
                table_names,
            } => {
                let table_names: Vec<ConvexValue> = table_names
                    .into_iter()
                    .map(|name| anyhow::Ok(ConvexValue::String(name.to_string().try_into()?)))
                    .try_collect()?;
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize(),
                    "table_names" => table_names
                )
            },
            DeploymentAuditLogEvent::DeleteComponent {
                component_id,
                component,
            } => {
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize()
                )
            },
            DeploymentAuditLogEvent::CancelAllScheduledFunctions {
                component_id,
                component,
            } => {
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize()
                )
            },
            DeploymentAuditLogEvent::CancelScheduledFunction {
                component_id,
                component,
                scheduled_function_id,
                function_path,
            } => {
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize(),
                    "scheduled_function_id" => scheduled_function_id,
                    "function_path" => function_path
                )
            },
            DeploymentAuditLogEvent::RequestExport {
                id,
                component_id,
                component,
                format,
                requestor,
            }
            | DeploymentAuditLogEvent::DownloadExport {
                id,
                component_id,
                component,
                format,
                requestor,
            } => {
                obj!(
                    "id" => id,
                    "component_id" => component_id,
                    "component" => component.serialize(),
                    "format" => format,
                    "requestor" => requestor
                )
            },
            DeploymentAuditLogEvent::CancelExport { id } => {
                obj!("id" => id)
            },
            DeploymentAuditLogEvent::SetExportExpiration {
                id,
                expiration_ts_ms,
            } => {
                obj!("id" => id, "expiration_ts_ms" => expiration_ts_ms)
            },
            DeploymentAuditLogEvent::CreateIntegration { id, r#type, config }
            | DeploymentAuditLogEvent::DeleteIntegration { id, r#type, config } => {
                value::serde::to_object(IntegrationAuditMetadata {
                    id: id.clone(),
                    r#type: r#type.clone(),
                    config: config.clone(),
                    ..Default::default()
                })
            },
            DeploymentAuditLogEvent::UpdateIntegration {
                id,
                r#type,
                previous,
                current,
                credential_changes,
            } => value::serde::to_object(IntegrationAuditMetadata {
                id: id.clone(),
                r#type: r#type.clone(),
                previous: previous.clone(),
                current: current.clone(),
                credential_changes: credential_changes.clone(),
                ..Default::default()
            }),
            DeploymentAuditLogEvent::AddDocuments {
                component_id,
                component,
                table,
                document_ids,
            } => {
                let ids: Vec<ConvexValue> = document_ids
                    .into_iter()
                    .map(|id| anyhow::Ok(ConvexValue::String(id.try_into()?)))
                    .try_collect()?;
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize(),
                    "table" => table,
                    "document_ids" => ids
                )
            },
            DeploymentAuditLogEvent::DeleteDocuments {
                component_id,
                component,
                table,
                document_ids,
            } => {
                let ids: Vec<ConvexValue> = document_ids
                    .into_iter()
                    .map(|id| anyhow::Ok(ConvexValue::String(id.try_into()?)))
                    .try_collect()?;
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize(),
                    "table" => table,
                    "document_ids" => ids
                )
            },
            DeploymentAuditLogEvent::UpdateDocuments {
                component_id,
                component,
                table,
                document_ids,
            } => {
                let ids: Vec<ConvexValue> = document_ids
                    .into_iter()
                    .map(|id| anyhow::Ok(ConvexValue::String(id.try_into()?)))
                    .try_collect()?;
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize(),
                    "table" => table,
                    "document_ids" => ids,
                )
            },
            DeploymentAuditLogEvent::CreateTable {
                component_id,
                component,
                table,
            } => {
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize(),
                    "table" => table
                )
            },
            DeploymentAuditLogEvent::DeleteFiles {
                component_id,
                component,
                storage_ids,
            } => {
                let ids: Vec<ConvexValue> = storage_ids
                    .into_iter()
                    .map(|id| anyhow::Ok(ConvexValue::String(id.try_into()?)))
                    .try_collect()?;
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize(),
                    "storage_ids" => ids
                )
            },
            DeploymentAuditLogEvent::GenerateUploadUrl {
                component_id,
                component,
            } => {
                obj!(
                    "component_id" => component_id,
                    "component" => component.serialize()
                )
            },
            DeploymentAuditLogEvent::CreateDataSync { sync_id } => {
                obj!("sync_id" => sync_id)
            },
        }
    }

    pub fn into_audit_log_object(
        self,
        member_id: Option<i64>,
        token_id: Option<i64>,
        app_client_id: Option<&str>,
        client_ip: Option<&str>,
        client_user_agent: Option<&str>,
    ) -> anyhow::Result<ConvexObject> {
        let action = self.action().to_string();
        let metadata = self.metadata()?;
        value::serde::to_object(SerializedDeploymentAuditLogEntry {
            action,
            metadata,
            member_id,
            token_id,
            app_client_id: app_client_id.map(str::to_string),
            client_ip: client_ip.map(str::to_string),
            client_user_agent: client_user_agent.map(str::to_string),
        })
    }

    pub fn to_log_event(
        event: DeploymentAuditLogEvent,
        timestamp: UnixTimestamp,
    ) -> anyhow::Result<LogEvent> {
        let action = event.action().to_string();
        let JsonValue::Object(metadata_fields) = event.metadata()?.into() else {
            anyhow::bail!("DeploymentAuditLogEvent metdata was not a JSON object")
        };
        Ok(LogEvent {
            timestamp,
            event: StructuredLogEvent::DeploymentAuditLog {
                action,
                metadata: metadata_fields,
            },
        })
    }
}

impl TryFrom<DeploymentAuditLogEvent> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(value: DeploymentAuditLogEvent) -> anyhow::Result<Self> {
        obj!("action" => value.action(), "metadata" => value.metadata()?)
    }
}

#[derive(Debug, Clone)]
pub enum DeploymentAuditLogActor {
    Token {
        member_id: Option<MemberId>,
        token_id: AccessTokenId,
        client_id: Option<String>,
    },
    Member {
        member_id: MemberId,
    },
    System,
}

#[derive(Serialize, Deserialize)]
struct SerializedDeploymentAuditLogEntry {
    action: String,
    metadata: ConvexObject,
    member_id: Option<i64>,
    token_id: Option<i64>,
    app_client_id: Option<String>,
    client_ip: Option<String>,
    client_user_agent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeploymentAuditLogEntry {
    pub client_ip: Option<String>,
    pub client_user_agent: Option<String>,
    pub create_time: i64,
    pub actor: DeploymentAuditLogActor,
    pub action: DeploymentAuditLogEvent,
}

impl DeploymentAuditLogEntry {
    pub fn from_audit_log_object(object: ConvexObject, create_time: i64) -> anyhow::Result<Self> {
        let SerializedDeploymentAuditLogEntry {
            action,
            metadata,
            member_id,
            token_id,
            app_client_id,
            client_ip,
            client_user_agent,
        } = value::serde::from_object(object)?;

        let member_id = member_id
            .map(|id| anyhow::Ok(MemberId(u64::try_from(id)?)))
            .transpose()?;
        let actor = match token_id {
            Some(token_id) => DeploymentAuditLogActor::Token {
                member_id,
                token_id: u64::try_from(token_id)
                    .map(AccessTokenId)
                    .unwrap_or_else(|_| AccessTokenId::unknown()),
                client_id: app_client_id,
            },
            None => match member_id {
                Some(member_id) => DeploymentAuditLogActor::Member { member_id },
                None => DeploymentAuditLogActor::System,
            },
        };

        let action = DeploymentAuditLogEvent::try_from(obj!(
            "action" => action,
            "metadata" => metadata,
        )?)?;
        Ok(Self {
            client_ip,
            client_user_agent,
            create_time,
            actor,
            action,
        })
    }
}

impl TryFrom<ResolvedDocument> for DeploymentAuditLogEntry {
    type Error = anyhow::Error;

    fn try_from(document: ResolvedDocument) -> anyhow::Result<Self> {
        let create_time = f64::from(document.creation_time()) as i64;
        let object: ConvexObject = document.into_value().into_value();
        Self::from_audit_log_object(object, create_time)
    }
}

fn value_to_index_metadata(
    value: ConvexValue,
) -> anyhow::Result<(IndexName, DeveloperIndexConfig)> {
    let obj = ConvexObject::try_from(value)?;
    let mut fields = BTreeMap::from(obj);
    let name = remove_string(&mut fields, "name")?;
    let obj = ConvexObject::try_from(fields)?;
    let developer_index_config = obj.try_into()?;
    Ok((IndexName::from_str(&name)?, developer_index_config))
}

impl TryFrom<ConvexObject> for DeploymentAuditLogEvent {
    type Error = anyhow::Error;

    fn try_from(obj: ConvexObject) -> anyhow::Result<Self> {
        let mut fields = BTreeMap::from(obj);
        let action: String = remove_string(&mut fields, "action")?;
        let metadata: ConvexObject = remove_object(&mut fields, "metadata")?;
        let mut fields = BTreeMap::from(metadata);
        let event = match &*action {
            "create_environment_variable" => DeploymentAuditLogEvent::CreateEnvironmentVariable {
                name: remove_string(&mut fields, "variable_name")?.parse()?,
            },
            "delete_environment_variable" => DeploymentAuditLogEvent::DeleteEnvironmentVariable {
                name: remove_string(&mut fields, "variable_name")?.parse()?,
            },
            "update_environment_variable" => DeploymentAuditLogEvent::UpdateEnvironmentVariable {
                name: remove_string(&mut fields, "variable_name")?.parse()?,
            },
            "create_usage_limit" => DeploymentAuditLogEvent::CreateUsageLimit {
                id: remove_string(&mut fields, "id")?,
                config: remove_object(&mut fields, "config")?,
            },
            "update_usage_limit" => DeploymentAuditLogEvent::UpdateUsageLimit {
                id: remove_string(&mut fields, "id")?,
                previous: remove_object(&mut fields, "previous")?,
                current: remove_object(&mut fields, "current")?,
            },
            "delete_usage_limit" => DeploymentAuditLogEvent::DeleteUsageLimit {
                id: remove_string(&mut fields, "id")?,
                config: remove_object(&mut fields, "config")?,
            },
            "usage_limit_exceeded" => DeploymentAuditLogEvent::UsageLimitExceeded {
                id: remove_string(&mut fields, "id")?,
                config: remove_object(&mut fields, "config")?,
            },
            "change_usage_limit_stop_state" => DeploymentAuditLogEvent::ChangeUsageLimitStopState {
                old_state: remove_string(&mut fields, "old_state")?.parse()?,
                new_state: remove_string(&mut fields, "new_state")?.parse()?,
            },
            "replace_environment_variable" => DeploymentAuditLogEvent::ReplaceEnvironmentVariable {
                previous_name: remove_string(&mut fields, "previous_variable_name")?.parse()?,
                name: remove_string(&mut fields, "variable_name")?.parse()?,
            },
            "update_canonical_url" => DeploymentAuditLogEvent::UpdateCanonicalUrl {
                request_destination: remove_string(&mut fields, "request_destination")?.parse()?,
                url: remove_string(&mut fields, "url")?,
            },
            "delete_canonical_url" => DeploymentAuditLogEvent::DeleteCanonicalUrl {
                request_destination: remove_string(&mut fields, "request_destination")?.parse()?,
            },
            "push_config" => DeploymentAuditLogEvent::PushConfig {
                config_diff: ConvexObject::try_from(fields)?.try_into()?,
            },
            "push_config_with_components" => DeploymentAuditLogEvent::PushConfigWithComponents {
                diffs: ConvexObject::try_from(fields)?.try_into()?,
            },
            "build_indexes" => {
                let added_indexes = remove_vec(&mut fields, "added_indexes")?
                    .into_iter()
                    .map(value_to_index_metadata)
                    .try_collect()?;
                let removed_indexes = remove_vec(&mut fields, "removed_indexes")?
                    .into_iter()
                    .map(value_to_index_metadata)
                    .try_collect()?;
                DeploymentAuditLogEvent::BuildIndexes {
                    added_indexes,
                    removed_indexes,
                }
            },
            "change_deployment_state" => DeploymentAuditLogEvent::ChangeDeploymentState {
                old_state: remove_string(&mut fields, "old_state")?.parse()?,
                new_state: remove_string(&mut fields, "new_state")?.parse()?,
            },
            "pause_deployment" => DeploymentAuditLogEvent::PauseDeployment,
            "unpause_deployment" => DeploymentAuditLogEvent::UnpauseDeployment,
            "change_system_stop_state" => DeploymentAuditLogEvent::ChangeSystemStopState {
                old_state: remove_string(&mut fields, "old_state")?.parse()?,
                new_state: remove_string(&mut fields, "new_state")?.parse()?,
            },
            "clear_tables" => DeploymentAuditLogEvent::ClearTables,
            "snapshot_import" => {
                let table_names: BTreeMap<_, _> = remove_vec(&mut fields, "table_names")?
                    .into_iter()
                    .map(|v| {
                        let o: ConvexObject = v.try_into()?;
                        let mut fields = BTreeMap::from(o);
                        let component = ComponentPath::deserialize(
                            remove_nullable_string(&mut fields, "component")?.as_deref(),
                        )?;
                        let table_names: Vec<_> =
                            remove_vec_of_strings(&mut fields, "table_names")?
                                .iter()
                                .map(|s| TableName::from_str(s))
                                .try_collect()?;
                        anyhow::Ok((component, table_names))
                    })
                    .try_collect()?;
                let table_names_deleted: BTreeMap<_, _> =
                    remove_vec(&mut fields, "table_names_deleted")?
                        .into_iter()
                        .map(|v| {
                            let o: ConvexObject = v.try_into()?;
                            let mut fields = BTreeMap::from(o);
                            let component = ComponentPath::deserialize(
                                remove_nullable_string(&mut fields, "component")?.as_deref(),
                            )?;
                            let table_names: Vec<_> =
                                remove_vec_of_strings(&mut fields, "table_names")?
                                    .iter()
                                    .map(|s| TableName::from_str(s))
                                    .try_collect()?;
                            anyhow::Ok((component, table_names))
                        })
                        .try_collect()?;
                DeploymentAuditLogEvent::SnapshotImport {
                    table_names,
                    table_count: remove_int64(&mut fields, "table_count")? as u64,
                    import_mode: remove_string(&mut fields, "import_mode")?.parse()?,
                    import_format: remove_object(&mut fields, "import_format")?,
                    requestor: remove_object(&mut fields, "requestor")?,
                    table_names_deleted,
                    table_count_deleted: remove_int64(&mut fields, "table_count_deleted")? as u64,
                }
            },
            "delete_scheduled_jobs_table" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                DeploymentAuditLogEvent::DeleteScheduledJobsTable {
                    component_id,
                    component,
                }
            },
            "delete_tables" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let table_names: Vec<TableName> =
                    remove_vec_of_strings(&mut fields, "table_names")?
                        .iter()
                        .map(|s| TableName::from_str(s))
                        .try_collect()?;
                DeploymentAuditLogEvent::DeleteTables {
                    component_id,
                    component,
                    table_names,
                }
            },
            "delete_component" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                DeploymentAuditLogEvent::DeleteComponent {
                    component_id,
                    component,
                }
            },
            "cancel_all_scheduled_functions" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                DeploymentAuditLogEvent::CancelAllScheduledFunctions {
                    component_id,
                    component,
                }
            },
            "cancel_scheduled_function" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let scheduled_function_id = remove_string(&mut fields, "scheduled_function_id")?;
                let function_path = remove_nullable_string(&mut fields, "function_path")?;
                DeploymentAuditLogEvent::CancelScheduledFunction {
                    component_id,
                    component,
                    scheduled_function_id,
                    function_path,
                }
            },
            "request_export" => {
                let id = remove_string(&mut fields, "id")?;
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let format = remove_string(&mut fields, "format")?;
                let requestor = remove_string(&mut fields, "requestor")?;
                DeploymentAuditLogEvent::RequestExport {
                    id,
                    component_id,
                    component,
                    format,
                    requestor,
                }
            },
            "download_export" => {
                let id = remove_string(&mut fields, "id")?;
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let format = remove_string(&mut fields, "format")?;
                let requestor = remove_string(&mut fields, "requestor")?;
                DeploymentAuditLogEvent::DownloadExport {
                    id,
                    component_id,
                    component,
                    format,
                    requestor,
                }
            },
            "cancel_export" => {
                let id = remove_string(&mut fields, "id")?;
                DeploymentAuditLogEvent::CancelExport { id }
            },
            "set_export_expiration" => {
                let id = remove_string(&mut fields, "id")?;
                let expiration_ts_ms = remove_int64(&mut fields, "expiration_ts_ms")?;
                DeploymentAuditLogEvent::SetExportExpiration {
                    id,
                    expiration_ts_ms,
                }
            },
            "create_integration" => {
                let metadata: IntegrationAuditMetadata =
                    value::serde::from_object(fields.try_into()?)?;
                DeploymentAuditLogEvent::CreateIntegration {
                    id: metadata.id,
                    r#type: metadata.r#type,
                    config: metadata.config,
                }
            },
            "update_integration" => {
                let metadata: IntegrationAuditMetadata =
                    value::serde::from_object(fields.try_into()?)?;
                DeploymentAuditLogEvent::UpdateIntegration {
                    id: metadata.id,
                    r#type: metadata.r#type,
                    previous: metadata.previous,
                    current: metadata.current,
                    credential_changes: metadata.credential_changes,
                }
            },
            "delete_integration" => {
                let metadata: IntegrationAuditMetadata =
                    value::serde::from_object(fields.try_into()?)?;
                DeploymentAuditLogEvent::DeleteIntegration {
                    id: metadata.id,
                    r#type: metadata.r#type,
                    config: metadata.config,
                }
            },
            "add_documents" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let table = remove_string(&mut fields, "table")?;
                let document_ids = remove_vec_of_strings(&mut fields, "document_ids")?;
                DeploymentAuditLogEvent::AddDocuments {
                    component_id,
                    component,
                    table,
                    document_ids,
                }
            },
            "delete_documents" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let table = remove_string(&mut fields, "table")?;
                let document_ids = remove_vec_of_strings(&mut fields, "document_ids")?;
                DeploymentAuditLogEvent::DeleteDocuments {
                    component_id,
                    component,
                    table,
                    document_ids,
                }
            },
            "update_documents" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let table = remove_string(&mut fields, "table")?;
                let document_ids = remove_vec_of_strings(&mut fields, "document_ids")?;
                DeploymentAuditLogEvent::UpdateDocuments {
                    component_id,
                    component,
                    table,
                    document_ids,
                }
            },
            "create_table" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let table = remove_string(&mut fields, "table")?;
                DeploymentAuditLogEvent::CreateTable {
                    component_id,
                    component,
                    table,
                }
            },
            "delete_files" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                let storage_ids = remove_vec_of_strings(&mut fields, "storage_ids")?;
                DeploymentAuditLogEvent::DeleteFiles {
                    component_id,
                    component,
                    storage_ids,
                }
            },
            "generate_upload_url" => {
                let component_id = remove_nullable_string(&mut fields, "component_id")?;
                let component = ComponentPath::deserialize(
                    remove_nullable_string(&mut fields, "component")?.as_deref(),
                )?;
                DeploymentAuditLogEvent::GenerateUploadUrl {
                    component_id,
                    component,
                }
            },
            "create_data_sync" => DeploymentAuditLogEvent::CreateDataSync {
                sync_id: remove_string(&mut fields, "sync_id")?,
            },
            _ => anyhow::bail!("action {action} unrecognized"),
        };
        Ok(event)
    }
}

impl TryFrom<DeploymentAuditLogEvent> for serde_json::Map<String, JsonValue> {
    type Error = anyhow::Error;

    fn try_from(value: DeploymentAuditLogEvent) -> anyhow::Result<Self> {
        let mut map = serde_json::Map::new();
        let action = value.action();
        map.insert("action".to_string(), action.into());
        map.insert(
            "actionMetadata".to_string(),
            // Note that this serialization format might be ugly for certain types until the clean
            // export serialization project is complete.
            value.metadata()?.into(),
        );
        Ok(map)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedIndexDiff {
    pub added_indexes: Vec<SerializedNamedDeveloperIndexConfig>,
    pub removed_indexes: Vec<SerializedNamedDeveloperIndexConfig>,
    #[serde(default)]
    pub disabled_indexes: Vec<SerializedNamedDeveloperIndexConfig>,
    #[serde(default)]
    pub enabled_indexes: Vec<SerializedNamedDeveloperIndexConfig>,
}

impl From<AuditLogIndexDiff> for SerializedIndexDiff {
    fn from(diff: AuditLogIndexDiff) -> Self {
        let convert_to_serialized =
            |indexes: Vec<(GenericIndexName<TableName>, DeveloperIndexConfig)>| {
                indexes
                    .into_iter()
                    .map(|(name, config)| {
                        let name = name.to_string();
                        let index_config = SerializedDeveloperIndexConfig::from(config);
                        SerializedNamedDeveloperIndexConfig { name, index_config }
                    })
                    .collect()
            };
        let added_indexes = convert_to_serialized(diff.added_indexes);
        let removed_indexes = convert_to_serialized(diff.removed_indexes);
        let disabled_indexes = convert_to_serialized(diff.disabled_indexes);
        let enabled_indexes = convert_to_serialized(diff.enabled_indexes);
        Self {
            added_indexes,
            removed_indexes,
            disabled_indexes,
            enabled_indexes,
        }
    }
}

impl TryFrom<SerializedIndexDiff> for AuditLogIndexDiff {
    type Error = anyhow::Error;

    fn try_from(diff: SerializedIndexDiff) -> anyhow::Result<Self> {
        let convert_to_index_metadata = |indexes: Vec<SerializedNamedDeveloperIndexConfig>| {
            indexes
                .into_iter()
                .map(
                    |SerializedNamedDeveloperIndexConfig { name, index_config }| {
                        anyhow::Ok((name.parse()?, index_config.try_into()?))
                    },
                )
                .try_collect()
        };
        let added_indexes = convert_to_index_metadata(diff.added_indexes)?;
        let removed_indexes = convert_to_index_metadata(diff.removed_indexes)?;
        let disabled_indexes = convert_to_index_metadata(diff.disabled_indexes)?;
        let enabled_indexes = convert_to_index_metadata(diff.enabled_indexes)?;
        Ok(Self {
            added_indexes,
            removed_indexes,
            disabled_indexes,
            enabled_indexes,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PushComponentDiffs {
    pub auth_diff: AuthDiff,
    pub component_diffs: BTreeMap<ComponentPath, ComponentDiff>,
    pub message: Option<PushMessage>,
    pub node_version_diff: Option<NodeVersionDiff>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PushMessage(PII<String>);

impl PushMessage {
    const MAX_LENGTH: usize = 1024;
}

impl TryFrom<String> for PushMessage {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > PushMessage::MAX_LENGTH {
            anyhow::bail!(ErrorMetadata::bad_request(
                "PushMessageTooLong",
                format!(
                    "Push messages can be at most {} bytes long",
                    PushMessage::MAX_LENGTH
                ),
            ))
        }

        Ok(PushMessage(value.into()))
    }
}

impl TryFrom<SerializedPushComponentDiffs> for PushComponentDiffs {
    type Error = anyhow::Error;

    fn try_from(value: SerializedPushComponentDiffs) -> anyhow::Result<Self> {
        let component_diffs = value
            .component_diffs
            .into_iter()
            .map(
                |ComponentPathAndDiff {
                     component_path,
                     component_diff,
                 }| {
                    let path = ComponentPath::deserialize(component_path.as_deref())?;
                    let diff = ComponentDiff::try_from(component_diff)?;
                    Ok((path, diff))
                },
            )
            .collect::<anyhow::Result<BTreeMap<ComponentPath, ComponentDiff>>>()?;
        Ok(PushComponentDiffs {
            auth_diff: value.auth_diff.unwrap_or_default(),
            component_diffs,
            message: value.message.map(PushMessage::try_from).transpose()?,
            node_version_diff: value
                .node_version_diff
                .map(NodeVersionDiff::try_from)
                .transpose()?,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ComponentPathAndDiff {
    component_path: Option<String>,
    component_diff: SerializedComponentDiff,
}
#[derive(Serialize, Deserialize)]
pub struct SerializedPushComponentDiffs {
    auth_diff: Option<AuthDiff>,
    component_diffs: Vec<ComponentPathAndDiff>,
    message: Option<String>,
    #[serde(default)]
    node_version_diff: Option<SerializedNodeVersionDiff>,
}

impl TryFrom<PushComponentDiffs> for SerializedPushComponentDiffs {
    type Error = anyhow::Error;

    fn try_from(value: PushComponentDiffs) -> anyhow::Result<Self> {
        let auth_diff = value.auth_diff;
        let component_diffs = value
            .component_diffs
            .into_iter()
            .map(|(path, diff)| {
                let component_path = path.serialize();
                let component_diff = SerializedComponentDiff::try_from(diff)?;
                Ok(ComponentPathAndDiff {
                    component_path,
                    component_diff,
                })
            })
            .collect::<anyhow::Result<Vec<ComponentPathAndDiff>>>()?;
        let node_version_diff = value
            .node_version_diff
            .map(SerializedNodeVersionDiff::try_from)
            .transpose()?;
        Ok(SerializedPushComponentDiffs {
            auth_diff: Some(auth_diff),
            component_diffs,
            message: value.message.map(|m| (m.0).0),
            node_version_diff,
        })
    }
}

codegen_convex_serialization!(PushComponentDiffs, SerializedPushComponentDiffs);
