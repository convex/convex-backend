use std::{
    collections::{
        btree_map::Entry,
        BTreeMap,
        BTreeSet,
    },
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    auth::AuthInfo,
    bootstrap_model::{
        components::{
            definition::ComponentDefinitionMetadata,
            ComponentState,
        },
        schema::{
            SchemaMetadata,
            SchemaState,
        },
    },
    components::{
        ComponentDefinitionPath,
        ComponentId,
        ComponentName,
        ComponentPath,
        Resource,
    },
    errors::JsError,
    execution_context::RequestMetadata,
    knobs::FINISH_PUSH_MAX_OCC_FAILURES,
    runtime::{
        try_join,
        Runtime,
    },
    schemas::{
        validator::Validator,
        DatabaseSchema,
        DocumentSchema,
        TableValidationOutcome,
    },
    types::{
        EnvVarName,
        EnvVarValue,
        IndexName,
        ModuleEnvironment,
        NodeDependency,
        RepeatableTimestamp,
        Timestamp,
    },
    version::Version,
};
use database::{
    table_summary::table_summary_bootstrapping_error,
    BootstrapComponentsModel,
    IndexModel,
    OccRetryStats,
    SchemaModel,
    SchemaValidationModel,
    SchemaValidationWithProgress,
    Snapshot,
    StagedValidationWithProgress,
    TableShapes,
    Token,
    Transaction,
    ValidationState,
    WriteSource,
    MAX_OCC_FAILURES,
    SCHEMAS_TABLE,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use fastrace::{
    future::FutureExt as _,
    Span,
};
use futures::FutureExt;
use keybroker::Identity;
use maplit::btreeset;
use model::{
    auth::{
        types::AuthDiff,
        AuthInfoModel,
    },
    components::{
        config::{
            ComponentConfigModel,
            ComponentDefinitionConfigModel,
            ComponentDefinitionDiff,
            ComponentDiff,
            SchemaChange,
        },
        file_based_routing::file_based_exports,
        type_checking::{
            CheckedComponent,
            InitializerEvaluator,
            TypecheckContext,
        },
        types::{
            AppDefinitionConfig,
            ComponentDefinitionConfig,
            EvaluatedComponentDefinition,
            ProjectConfig,
        },
    },
    config::types::{
        deprecated_extract_environment_from_path,
        ConfigFile,
        ConfigMetadata,
        ModuleConfig,
        ModuleHashConfig,
    },
    deployment_audit_log::{
        developer_index_config::DeveloperIndexConfig,
        types::{
            DeploymentAuditLogEvent,
            PushComponentDiffs,
            PushMessage,
        },
    },
    environment_variables::EnvironmentVariablesModel,
    external_packages::types::ExternalDepsPackageId,
    modules::module_versions::{
        AnalyzedModule,
        ModuleSource,
        SourceMap,
    },
    source_packages::{
        types::{
            NodeVersion,
            NodeVersionDiff,
            SourcePackage,
        },
        upload_download::download_package,
        SourcePackageModel,
    },
    udf_config::types::UdfConfig,
};
use serde::{
    Deserialize,
    Serialize,
};
use sync_types::{
    module_path::InvalidModulePathError,
    CanonicalizedModulePath,
    ModulePath,
};
use udf::{
    environment::system_env_var_overrides,
    EvaluateAppDefinitionsResult,
};
use usage_tracking::FunctionUsageTracker;
use value::{
    identifier::Identifier,
    sha256::Sha256Digest,
    DeveloperDocumentId,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    schema_worker::table_shape_provider,
    validate_env_var_values,
    Application,
    ApplyConfigArgs,
    ConfigMetadataAndSchema,
};

pub struct PushAnalytics {
    pub config: ConfigMetadata,
    pub modules: Vec<ModuleConfig>,
    pub udf_server_version: Version,
    pub analyze_results: BTreeMap<CanonicalizedModulePath, AnalyzedModule>,
    pub schema: Option<DatabaseSchema>,
}

pub struct PushMetrics {
    pub build_external_deps_time: Duration,
    pub upload_source_package_time: Duration,
    pub analyze_time: Duration,
    pub occ_stats: OccRetryStats,
}

struct EvaluatedPushContents {
    app: CheckedComponent,
    auth_info: Vec<AuthInfo>,
    component_definition_packages: BTreeMap<ComponentDefinitionPath, SourcePackage>,
    evaluated_components: BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
    external_deps_id: Option<ExternalDepsPackageId>,
    user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
    app_functions: Vec<ModuleConfig>,
}

impl<RT: Runtime> Application<RT> {
    #[fastrace::trace]
    pub async fn start_push(&self, config: &ProjectConfig) -> anyhow::Result<StartPushResult> {
        let EvaluatedPushContents {
            app,
            auth_info,
            component_definition_packages,
            mut evaluated_components,
            external_deps_id,
            user_environment_variables,
            app_functions,
        } = self.evaluate_push_contents(config).await?;

        let skip_index_diff = config.dry_run || config.for_codegen;
        let mut schema_change = self
            .handle_schema_change_in_start_push(&app, &evaluated_components, skip_index_diff)
            .await?;
        if skip_index_diff {
            // Compute index diffs in a throwaway transaction so they're returned in
            // the response but not committed as pending indexes.
            let dry_run_schema_change = self
                .handle_schema_change_read_only(&app, &evaluated_components)
                .await?;
            schema_change.index_diffs = dry_run_schema_change.index_diffs;
        }
        self.database
            .load_indexes_into_memory(btreeset! { SCHEMAS_TABLE.clone() })
            .await?;

        // TODO(ENG-7533): Clean up exports from the start push response when we've
        // updated clients to use `functions` directly.
        for (path, definition) in evaluated_components.iter_mut() {
            // We don't need to include exports for the root since we don't use codegen
            // for the app's `api` object.
            if path.is_root() {
                continue;
            }
            anyhow::ensure!(definition.definition.exports.is_empty());
            definition.definition.exports = file_based_exports(&definition.functions)?;
        }

        let resp = StartPushResponse {
            environment_variables: user_environment_variables,
            external_deps_id,
            component_definition_packages,
            app_auth: auth_info,
            analysis: evaluated_components,
            app,
            schema_change,
        };
        Ok(StartPushResult {
            response: resp,
            app_functions,
        })
    }

    #[fastrace::trace]
    async fn evaluate_push_contents(
        &self,
        config: &ProjectConfig,
    ) -> anyhow::Result<EvaluatedPushContents> {
        let (external_deps_id, component_definition_packages, app_functions) =
            self.upload_packages(config).await?;

        let app_udf_config = self
            .generate_udf_config(
                config.app_definition.udf_server_version.clone(),
                TableNamespace::root_component(),
                &Identity::system(),
            )
            .await?;
        let app_pkg = component_definition_packages
            .get(&ComponentDefinitionPath::root())
            .context("No package for app?")?;

        let (user_environment_variables, system_env_var_overrides) = {
            let mut tx = self.begin(Identity::system()).await?;
            let vars = EnvironmentVariablesModel::new(&mut tx).get_all().await?;
            let system_env_var_overrides = system_env_var_overrides(&mut tx).await?;
            tx.into_token()?;
            (vars, system_env_var_overrides)
        };
        let (auth_module, app_analysis) = self
            .analyze_modules_with_auth_config(
                app_udf_config.clone(),
                app_functions.clone(),
                app_pkg.clone(),
                user_environment_variables.clone(),
                system_env_var_overrides.clone(),
            )
            .await?;

        let auth_info = Application::get_evaluated_auth_config(
            self.runner(),
            user_environment_variables.clone(),
            system_env_var_overrides.clone(),
            auth_module,
            &ConfigFile {
                functions: config.config.functions.clone(),
                auth_info: if config.config.auth_info.is_empty() {
                    None
                } else {
                    let auth_info = config
                        .config
                        .auth_info
                        .clone()
                        .into_iter()
                        .map(|v| v.try_into())
                        .collect::<Result<Vec<_>, _>>()?;
                    Some(auth_info)
                },
            },
        )
        .await?;

        let evaluated_components = self
            .evaluate_components(
                config,
                &component_definition_packages,
                app_analysis,
                app_udf_config,
                user_environment_variables.clone(),
                system_env_var_overrides,
            )
            .await?;
        validate_env_var_declarations(&evaluated_components)?;
        // Build and typecheck the component tree. We don't strictly need to do this
        // before `/finish_push`, but it's better to fail fast here on errors before
        // waiting for schema backfills to complete.
        let initializer_evaluator = ApplicationInitializerEvaluator::new(
            self,
            config,
            evaluated_components
                .iter()
                .map(|(k, v)| (k.clone(), v.definition.clone()))
                .collect(),
        )?;
        let ctx = if config.for_codegen {
            TypecheckContext::new_for_codegen(&evaluated_components, &initializer_evaluator)
        } else {
            TypecheckContext::new(&evaluated_components, &initializer_evaluator)
        };
        let app = ctx.instantiate_root().await?;

        Ok(EvaluatedPushContents {
            app,
            auth_info,
            component_definition_packages,
            evaluated_components,
            external_deps_id,
            user_environment_variables,
            app_functions,
        })
    }

    #[fastrace::trace]
    async fn handle_schema_change_in_start_push(
        &self,
        app: &CheckedComponent,
        evaluated_components: &BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
        skip_index_diff: bool,
    ) -> anyhow::Result<SchemaChange> {
        let (_ts, schema_change) = self
            .execute_with_occ_retries(
                Identity::system(),
                FunctionUsageTracker::new(),
                MAX_OCC_FAILURES,
                WriteSource::system("start_push"),
                |tx| {
                    async move {
                        let schema_change = ComponentConfigModel::new(tx)
                            .start_component_schema_changes(
                                app,
                                evaluated_components,
                                skip_index_diff,
                            )
                            .await?;
                        Ok(schema_change)
                    }
                    .into()
                },
            )
            .await?;
        Ok(schema_change)
    }

    #[fastrace::trace]
    async fn handle_schema_change_read_only(
        &self,
        app: &CheckedComponent,
        evaluated_components: &BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
    ) -> anyhow::Result<SchemaChange> {
        let mut tx = self.begin(Identity::system()).await?;
        let schema_change = ComponentConfigModel::new(&mut tx)
            .start_component_schema_changes(app, evaluated_components, false)
            .await?;
        drop(tx);
        Ok(schema_change)
    }

    #[fastrace::trace]
    async fn evaluate_components(
        &self,
        config: &ProjectConfig,
        component_definition_packages: &BTreeMap<ComponentDefinitionPath, SourcePackage>,
        app_analysis: BTreeMap<CanonicalizedModulePath, AnalyzedModule>,
        app_udf_config: UdfConfig,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_var_overrides: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>> {
        let mut app_schema = None;
        if let Some(schema_module) = &config.app_definition.schema {
            app_schema = Some(self.evaluate_schema(schema_module.clone()).await?);
        }

        let mut component_analysis_by_def_path = BTreeMap::new();
        let mut component_schema_by_def_path = BTreeMap::new();
        let mut component_udf_config_by_def_path = BTreeMap::new();

        for component_def in &config.component_definitions {
            // The rng seed and unix timestamp are tied to the root because all component
            // definitions may not correspond to an existing `UdfConfig` yet. Instead, we
            // use the root's values because always know it will have a defined config.
            let udf_config = UdfConfig {
                server_version: component_def.udf_server_version.clone(),
                import_phase_rng_seed: app_udf_config.import_phase_rng_seed,
                import_phase_unix_timestamp: app_udf_config.import_phase_unix_timestamp,
            };
            component_udf_config_by_def_path
                .insert(component_def.definition_path.clone(), udf_config.clone());

            let component_pkg = component_definition_packages
                .get(&component_def.definition_path)
                .context("No package for component?")?;
            let component_analysis = self
                .analyze_modules(
                    udf_config,
                    component_def.functions.clone(),
                    component_pkg.clone(),
                    // User env vars are root-only; analyze() itself supplies
                    // the default system env vars.
                    BTreeMap::new(),
                    BTreeMap::new(),
                )
                .await?;
            anyhow::ensure!(component_analysis_by_def_path
                .insert(component_def.definition_path.clone(), component_analysis)
                .is_none());

            if let Some(schema_module) = &component_def.schema {
                let schema = match self.evaluate_schema(schema_module.clone()).await {
                    Ok(schema) => schema,
                    Err(e) => {
                        // Try to downcast to a JsError and turn that into a user-visible error if
                        // so.
                        let e = e.downcast::<JsError>()?;
                        anyhow::bail!(ErrorMetadata::bad_request("InvalidSchema", e.to_string()));
                    },
                };
                anyhow::ensure!(component_schema_by_def_path
                    .insert(component_def.definition_path.clone(), schema)
                    .is_none());
            }
        }

        let mut evaluated_definitions = BTreeMap::new();

        if let Some(ref app_definition) = config.app_definition.definition {
            let mut dependency_graph = BTreeSet::new();
            let mut component_definitions = BTreeMap::new();

            for dep in &config.app_definition.dependencies {
                dependency_graph.insert((ComponentDefinitionPath::root(), dep.clone()));
            }

            for component_def in &config.component_definitions {
                anyhow::ensure!(!component_def.definition_path.is_root());
                component_definitions.insert(
                    component_def.definition_path.clone(),
                    component_def.definition.clone(),
                );
                for dep in &component_def.dependencies {
                    dependency_graph.insert((component_def.definition_path.clone(), dep.clone()));
                }
            }

            let definition_result = self
                .evaluate_app_definitions(
                    app_definition.clone(),
                    component_definitions,
                    dependency_graph,
                    user_environment_variables,
                    system_env_var_overrides,
                )
                .await;
            evaluated_definitions = match definition_result {
                Ok(r) => r,
                Err(e) => {
                    let e = e.downcast::<JsError>()?;
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidConvexConfig",
                        e.to_string()
                    ));
                },
            };
        } else {
            evaluated_definitions.insert(
                ComponentDefinitionPath::root(),
                ComponentDefinitionMetadata::default_root(),
            );
        }

        let mut evaluated_components = BTreeMap::new();
        evaluated_components.insert(
            ComponentDefinitionPath::root(),
            EvaluatedComponentDefinition {
                definition: evaluated_definitions[&ComponentDefinitionPath::root()].clone(),
                schema: app_schema.clone(),
                functions: app_analysis.clone(),
                udf_config: app_udf_config.clone(),
            },
        );
        for (path, definition) in &evaluated_definitions {
            if path.is_root() {
                continue;
            }
            evaluated_components.insert(
                path.clone(),
                EvaluatedComponentDefinition {
                    definition: definition.clone(),
                    schema: component_schema_by_def_path.get(path).cloned(),
                    functions: component_analysis_by_def_path
                        .get(path)
                        .context("Missing analysis for component?")?
                        .clone(),
                    udf_config: component_udf_config_by_def_path
                        .get(path)
                        .context("Missing UDF config for component?")?
                        .clone(),
                },
            );
        }
        Ok(evaluated_components)
    }

    #[fastrace::trace]
    async fn evaluate_app_definitions(
        &self,
        app_definition: ModuleConfig,
        component_definitions: BTreeMap<ComponentDefinitionPath, ModuleConfig>,
        dependency_graph: BTreeSet<(ComponentDefinitionPath, ComponentDefinitionPath)>,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_var_overrides: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<EvaluateAppDefinitionsResult> {
        self.runner
            .evaluate_app_definitions(
                app_definition,
                component_definitions,
                dependency_graph,
                user_environment_variables,
                system_env_var_overrides,
            )
            .await
    }

    #[fastrace::trace]
    pub async fn evaluate_push(
        &self,
        config: &ProjectConfig,
    ) -> anyhow::Result<EvaluatePushResponse> {
        let EvaluatedPushContents {
            app,
            evaluated_components,
            ..
        } = self.evaluate_push_contents(config).await?;

        let schema_change = self
            .handle_schema_change_read_only(&app, &evaluated_components)
            .await?;

        Ok(EvaluatePushResponse { schema_change })
    }

    /// Predict, without side effects, the schema validation and index
    /// backfill work a push of `config` would trigger. Only the schema
    /// bundles are evaluated — modules are neither analyzed nor uploaded —
    /// and the transaction is dropped uncommitted.
    #[fastrace::trace]
    pub async fn evaluate_schema_prediction(
        &self,
        config: &ProjectConfig,
    ) -> anyhow::Result<EvaluateSchemaPredictionResponse> {
        // `None` = the definition is in the push but has no schema.ts.
        let mut pushed_schemas: BTreeMap<ComponentDefinitionPath, Option<DatabaseSchema>> =
            BTreeMap::new();
        let app_schema = match &config.app_definition.schema {
            Some(module) => Some(self.evaluate_schema_or_user_error(module.clone()).await?),
            None => None,
        };
        pushed_schemas.insert(ComponentDefinitionPath::root(), app_schema);
        for component_def in &config.component_definitions {
            let schema = match &component_def.schema {
                Some(module) => Some(self.evaluate_schema_or_user_error(module.clone()).await?),
                None => None,
            };
            pushed_schemas.insert(component_def.definition_path.clone(), schema);
        }

        let mut tx = self.begin(Identity::system()).await?;
        let ts = tx.begin_timestamp();
        let snapshot = self.snapshot(ts)?;
        if snapshot.table_counts.is_none() {
            // Document counts and sizes aren't available until table
            // summaries finish bootstrapping. Rather than predicting with
            // partial data, fail with a retriable error, matching how other
            // callers of table counts (e.g. `Transaction::must_table_counts`)
            // treat this as a transient condition.
            return Err(table_summary_bootstrapping_error(None));
        }
        let table_shapes = self.table_shapes_at(ts).await?;

        // Shape and validator subset checks can pin the CPU on large shapes,
        // so run the prediction on its own task.
        try_join("evaluate_schema_prediction", async move {
            let definitions = BootstrapComponentsModel::new(&mut tx)
                .load_all_definitions()
                .await?;
            let mut definition_paths_by_id = BTreeMap::new();
            for (path, definition) in &definitions {
                definition_paths_by_id.insert(definition.developer_id(), path.clone());
            }

            // The root component's namespace exists even when the
            // `_components` table has no root document.
            let mut instances = vec![(
                ComponentId::Root,
                ComponentPath::root(),
                ComponentDefinitionPath::root(),
            )];
            for component in BootstrapComponentsModel::new(&mut tx)
                .load_all_components()
                .await?
            {
                if component.component_type.is_root() {
                    continue;
                }
                let component_id = ComponentId::Child(component.developer_id());
                let Some(path) =
                    BootstrapComponentsModel::new(&mut tx).get_component_path(component_id)
                else {
                    continue;
                };
                let unmounted = matches!(component.state, ComponentState::Unmounted);
                let Some(definition_path) = definition_paths_by_id.get(&component.definition_id)
                else {
                    // An unmounted component's definition can be gone from
                    // `_component_definitions` while its instance and tables
                    // are still left in place (see
                    // `model::components::config`'s "leaving existing schema
                    // and tables in place for deleted component"); there's
                    // nothing to predict for it.
                    if unmounted {
                        continue;
                    }
                    anyhow::bail!("component {path:?} references an unknown definition");
                };
                instances.push((component_id, path, definition_path.clone()));
            }

            let mut component_schema_evaluations = BTreeMap::new();
            let mut instantiated_definitions = BTreeSet::new();
            for (component_id, component_path, definition_path) in instances {
                instantiated_definitions.insert(definition_path.clone());
                let prediction = predict_component_schema(
                    &mut tx,
                    &snapshot,
                    &table_shapes,
                    ts,
                    component_id,
                    definition_path.clone(),
                    pushed_schemas.get(&definition_path),
                )
                .await?;
                component_schema_evaluations.insert(component_path, prediction);
            }
            let new_component_definitions = pushed_schemas
                .keys()
                .filter(|path| !instantiated_definitions.contains(*path))
                .cloned()
                .collect();
            drop(tx);
            Ok(EvaluateSchemaPredictionResponse {
                component_schema_evaluations,
                new_component_definitions,
            })
        })
        .await
    }

    async fn evaluate_schema_or_user_error(
        &self,
        module: ModuleConfig,
    ) -> anyhow::Result<DatabaseSchema> {
        match self.evaluate_schema(module).await {
            Ok(schema) => Ok(schema),
            Err(e) => {
                let e = e.downcast::<JsError>()?;
                anyhow::bail!(ErrorMetadata::bad_request("InvalidSchema", e.to_string()))
            },
        }
    }

    #[fastrace::trace]
    pub async fn wait_for_schema(
        &self,
        identity: Identity,
        schema_change: SchemaChange,
        timeout: Duration,
    ) -> anyhow::Result<SchemaStatus> {
        let deadline = self.runtime().monotonic_now() + timeout;
        loop {
            let (status, token) = self
                .load_component_schema_status(&identity, &schema_change)
                .await?;
            let now = self.runtime().monotonic_now();
            let in_progress = matches!(status, SchemaStatus::InProgress { .. });
            if !in_progress || now > deadline {
                return Ok(status);
            }
            let subscription_fut = self.subscribe_and_wait_for_invalidation(token);
            tokio::select! {
                _ = subscription_fut.fuse() => {},
                _ = self.runtime.wait(deadline - now)
                    .in_span(fastrace::Span::enter_with_local_parent("wait_for_deadline"))
                 => {},
            }
        }
    }

    #[fastrace::trace]
    pub(crate) async fn load_component_schema_status(
        &self,
        identity: &Identity,
        schema_change: &SchemaChange,
    ) -> anyhow::Result<(SchemaStatus, Token)> {
        let mut tx = self.begin(identity.clone()).await?;
        let mut components_status = BTreeMap::new();
        for (component_path, schema_id) in &schema_change.schema_ids {
            let Some(schema_id) = schema_id else {
                continue;
            };
            let schema_table_number = tx.table_mapping().tablet_number(schema_id.table())?;
            let schema_id = ResolvedDocumentId::new(
                schema_id.table(),
                DeveloperDocumentId::new(schema_table_number, schema_id.internal_id()),
            );
            let document = tx
                .get(schema_id)
                .await?
                .context("Missing schema document")?;
            let SchemaMetadata { state, .. } = document.into_value().0.try_into()?;
            let schema_validation_complete = match state {
                SchemaState::Pending => false,
                SchemaState::Active | SchemaState::Validated => true,
                SchemaState::Failed { error, table_name } => {
                    let status = SchemaStatus::Failed {
                        error,
                        component_path: component_path.clone(),
                        table_name,
                    };
                    return Ok((status, tx.into_token()?));
                },
                SchemaState::Overwritten => {
                    return Ok((SchemaStatus::RaceDetected, tx.into_token()?))
                },
            };

            let component_id = if component_path.is_root() {
                ComponentId::Root
            } else {
                let existing =
                    BootstrapComponentsModel::new(&mut tx).resolve_path(component_path)?;
                let allocated = schema_change.allocated_component_ids.get(component_path);
                let internal_id = match (existing, allocated) {
                    (None, Some(id)) => *id,
                    (Some(doc), None) => doc.id().into(),
                    (Some(doc), Some(id)) if doc.developer_id() == *id => *id,
                    // A concurrent push created this component with a different namespace.
                    // The CLI retries the push on a 409.
                    (Some(_), Some(_)) => anyhow::bail!(ErrorMetadata::conflict(
                        "RaceDetected",
                        format!("Component {component_path} was created by a concurrent push"),
                    )),
                    r => anyhow::bail!("Invalid existing component state: {r:?}"),
                };
                ComponentId::Child(internal_id)
            };
            let namespace = TableNamespace::from(component_id);
            let mut indexes_complete = 0;
            let mut indexes_total = 0;
            for index in IndexModel::new(&mut tx)
                .get_application_indexes(namespace)
                .await?
            {
                // Skip counting indexes that are staged
                if index.config.is_staged() {
                    continue;
                }
                if !index.config.is_backfilling() {
                    indexes_complete += 1;
                }
                indexes_total += 1;
            }
            components_status.insert(
                component_path.clone(),
                ComponentSchemaStatus {
                    schema_validation_complete,
                    indexes_complete,
                    indexes_total,
                },
            );
        }
        let status = if components_status.values().all(|c| c.is_complete()) {
            SchemaStatus::Complete
        } else {
            SchemaStatus::InProgress {
                components: components_status,
            }
        };
        let token = tx.into_token()?;
        Ok((status, token))
    }

    #[fastrace::trace]
    pub async fn finish_push(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        mut start_push: StartPushResponse,
        message: Option<PushMessage>,
    ) -> anyhow::Result<(FinishPushDiff, Timestamp)> {
        // Download all source packages. We can remove this once we don't store source
        // in the database.
        let mut downloaded_source_packages = BTreeMap::new();
        for (definition_path, source_package) in &start_push.component_definition_packages {
            let package = download_package(self.modules_storage().clone(), source_package).await?;
            downloaded_source_packages.insert(definition_path.clone(), package);
        }

        // TODO(ENG-7533): Strip out exports from the `StartPushResponse` since we don't
        // want to actually store it in the database. Remove this path once
        // we've stopped sending exports down to the client.
        for definition in start_push.analysis.values_mut() {
            definition.definition.exports = BTreeMap::new();
        }

        let finish_push_write_source = "finish_push";

        let (diff, ts) = self
            .execute_with_audit_log_events_and_occ_retries_with_timestamp(
                identity.clone(),
                request_metadata,
                finish_push_write_source,
                *FINISH_PUSH_MAX_OCC_FAILURES,
                |tx| {
                    let start_push = &start_push;
                    let downloaded_source_packages = &downloaded_source_packages;
                    let message = &message;
                    async move {
                        // Validate that environment variables haven't changed since `start_push`.
                        let environment_variables =
                            EnvironmentVariablesModel::new(tx).get_all().await?;
                        if environment_variables != start_push.environment_variables {
                            anyhow::bail!(ErrorMetadata::bad_request(
                                "RaceDetected",
                                "Environment variables have changed during push"
                            ));
                        }

                        // Validate that all required env vars declared in the
                        // app definition are present.
                        if let Some(app_def) =
                            start_push.analysis.get(&ComponentDefinitionPath::root())
                        {
                            let missing: Vec<_> = app_def
                                .definition
                                .required_env_var_names()
                                .into_iter()
                                .filter(|name| {
                                    !environment_variables
                                        .iter()
                                        .any(|(k, _)| k.to_string() == *name)
                                })
                                .collect();
                            if !missing.is_empty() {
                                anyhow::bail!(ErrorMetadata::bad_request(
                                    "MissingEnvironmentVariables",
                                    format!(
                                        "Required environment variables are not set: {}. Set them \
                                         in the Convex dashboard or CLI before pushing.",
                                        missing.join(", ")
                                    )
                                ));
                            }

                            // Validate existing values match the new validators.
                            validate_env_var_values(
                                &environment_variables,
                                &app_def.definition.env_vars,
                            )?;
                        }

                        // Update app state: auth info and UDF server version.
                        let auth_diff = AuthInfoModel::new(tx)
                            .put(start_push.app_auth.clone())
                            .await?;

                        let prev_node_version = SourcePackageModel::new(tx, TableNamespace::Global)
                            .get_latest()
                            .await?
                            .and_then(|p| p.node_version);

                        // Diff the component definitions.
                        let (definition_diffs, modules_by_definition, udf_config_by_definition) =
                            ComponentDefinitionConfigModel::new(tx)
                                .apply_component_definitions_diff(
                                    &start_push.analysis,
                                    &start_push.component_definition_packages,
                                    downloaded_source_packages,
                                )
                                .await?;

                        // Diff component tree.
                        let component_diffs = ComponentConfigModel::new(tx)
                            .apply_component_tree_diff(
                                &start_push.app,
                                udf_config_by_definition,
                                &start_push.schema_change,
                                modules_by_definition,
                            )
                            .await?;

                        let next_node_version = SourcePackageModel::new(tx, TableNamespace::Global)
                            .get_latest()
                            .await?
                            .and_then(|p| p.node_version);

                        let node_version_diff =
                            (prev_node_version != next_node_version).then_some(NodeVersionDiff {
                                previous_version: prev_node_version,
                                next_version: next_node_version,
                            });

                        let diffs = PushComponentDiffs {
                            auth_diff: auth_diff.clone(),
                            component_diffs: component_diffs.clone(),
                            message: message.clone(),
                            node_version_diff,
                        };
                        let audit_log_events =
                            vec![DeploymentAuditLogEvent::PushConfigWithComponents { diffs }];
                        let diff = FinishPushDiff {
                            auth_diff,
                            definition_diffs,
                            component_diffs,
                        };
                        Ok((diff, audit_log_events))
                    }
                    .in_span(Span::enter_with_local_parent("finish_push_tx"))
                    .into()
                },
            )
            .await
            .map_err(|e| {
                if let Some(occ_error_info) = e.occ_info()
                    && let Some(write_source) = occ_error_info.write_source
                    && write_source == finish_push_write_source
                {
                    e.context(ErrorMetadata::bad_request(
                        "ConcurrentPush",
                        "Are you running multiple `npx convex dev` processes in the same \
                         directory?"
                            .to_string(),
                    ))
                } else {
                    e
                }
            })?;

        Ok((diff, ts))
    }

    /// N.B.: does not check auth
    pub async fn push_config_no_components(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        config_file: ConfigFile,
        modules: Vec<ModuleConfig>,
        udf_server_version: Version,
        schema_id: Option<String>,
        node_dependencies: Option<Vec<NodeDependencyJson>>,
        node_version: Option<NodeVersion>,
    ) -> anyhow::Result<(PushAnalytics, PushMetrics)> {
        let begin_build_external_deps = Instant::now();
        // Upload external node dependencies separately
        let external_deps_id_and_pkg = if let Some(deps) = node_dependencies
            && !deps.is_empty()
        {
            let deps: Vec<_> = deps.into_iter().map(NodeDependency::from).collect();
            Some(self.build_external_node_deps(deps).await?)
        } else {
            None
        };
        let end_build_external_deps = Instant::now();
        let external_deps_pkg_size = external_deps_id_and_pkg
            .as_ref()
            .map(|(_, pkg)| pkg.package_size)
            .unwrap_or_default();

        let source_package = self
            .upload_package(&modules, external_deps_id_and_pkg, node_version)
            .await?;
        let end_upload_source_package = Instant::now();
        // Verify that we have not exceeded the max unzipped file size
        let combined_pkg_size = source_package.package_size + external_deps_pkg_size;
        combined_pkg_size.verify_size()?;

        let udf_config = self
            .generate_udf_config(
                udf_server_version,
                TableNamespace::root_component(),
                &Identity::system(),
            )
            .await?;
        let begin_analyze = Instant::now();
        // Note: This is not transactional with the rest of the deploy to avoid keeping
        // a transaction open for a long time.
        let mut tx = self.begin(Identity::system()).await?;
        let user_environment_variables = EnvironmentVariablesModel::new(&mut tx).get_all().await?;
        let system_env_var_overrides = system_env_var_overrides(&mut tx).await?;
        drop(tx);
        // Run analyze to make sure the new modules are valid.
        let (auth_module, analyze_results) = self
            .analyze_modules_with_auth_config(
                udf_config.clone(),
                modules.clone(),
                source_package.clone(),
                user_environment_variables,
                system_env_var_overrides,
            )
            .await?;
        let end_analyze = Instant::now();
        let (
            ConfigMetadataAndSchema {
                config_metadata,
                schema,
            },
            occ_stats,
        ) = self
            .apply_config_with_retries(
                identity.clone(),
                request_metadata,
                ApplyConfigArgs {
                    auth_module,
                    config_file,
                    schema_id,
                    modules: modules.clone(),
                    udf_config: udf_config.clone(),
                    source_package,
                    analyze_results: analyze_results.clone(),
                },
            )
            .await?;

        Ok((
            PushAnalytics {
                config: config_metadata,
                modules,
                udf_server_version: udf_config.server_version,
                analyze_results,
                schema,
            },
            PushMetrics {
                build_external_deps_time: end_build_external_deps - begin_build_external_deps,
                upload_source_package_time: end_upload_source_package - end_build_external_deps,
                analyze_time: end_analyze - begin_analyze,
                occ_stats,
            },
        ))
    }
}

struct ApplicationInitializerEvaluator<'a, RT: Runtime> {
    application: &'a Application<RT>,
    component_definitions: BTreeMap<ComponentDefinitionPath, ModuleConfig>,
    evaluated_definitions: BTreeMap<ComponentDefinitionPath, ComponentDefinitionMetadata>,
}

impl<'a, RT: Runtime> ApplicationInitializerEvaluator<'a, RT> {
    fn new(
        application: &'a Application<RT>,
        config: &'a ProjectConfig,
        evaluated_definitions: BTreeMap<ComponentDefinitionPath, ComponentDefinitionMetadata>,
    ) -> anyhow::Result<Self> {
        let mut component_definitions = BTreeMap::new();
        for component_definition in &config.component_definitions {
            anyhow::ensure!(component_definitions
                .insert(
                    component_definition.definition_path.clone(),
                    component_definition.definition.clone(),
                )
                .is_none());
        }
        Ok(Self {
            application,
            component_definitions,
            evaluated_definitions,
        })
    }
}

#[async_trait]
impl<RT: Runtime> InitializerEvaluator for ApplicationInitializerEvaluator<'_, RT> {
    async fn evaluate(
        &self,
        path: ComponentDefinitionPath,
        args: BTreeMap<Identifier, Resource>,
        name: ComponentName,
    ) -> anyhow::Result<BTreeMap<Identifier, Resource>> {
        let component_definition = self
            .component_definitions
            .get(&path)
            .context(format!("Missing component definition for {path:?}"))?
            .clone();
        self.application
            .runner
            .evaluate_component_initializer(
                self.evaluated_definitions.clone(),
                path,
                component_definition,
                args,
                name,
            )
            .await
    }
}

fn validate_env_var_declarations(
    evaluated_components: &BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
) -> anyhow::Result<()> {
    for (path, evaluated) in evaluated_components {
        for (name, env_var_validator) in &evaluated.definition.env_vars {
            if !env_var_validator.validator.is_string_like_validator() {
                let component_label = if path.is_root() {
                    "the app".to_string()
                } else {
                    format!("component {path}", path = String::from(path.clone()))
                };
                anyhow::bail!(ErrorMetadata::bad_request(
                    "InvalidEnvVarDeclaration",
                    format!(
                        "Env var `{name}` on {component_label} has a non-string validator. \
                         Component env vars must be declared with `v.string()`, \
                         `v.literal(\"...\")`, or a union of those."
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Convex code push is a multiphase process.
///
/// Clients that want to push code send this message to the backend. They use
/// a resulting [StartPushResponse] to complete codegen and optionally
/// complete the code push.
///
/// They also might decide to not do a complete push (e.g. just getting the
/// analyze results to use for codegen).
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StartPushRequest {
    pub admin_key: String,

    pub functions: String,

    pub app_definition: AppDefinitionConfigJson,
    pub component_definitions: Vec<ComponentDefinitionConfigJson>,

    pub node_dependencies: Vec<NodeDependencyJson>,

    pub node_version: Option<String>,

    #[serde(default)]
    pub dry_run: bool,

    /// Indicates that this request is only for codegen and isn't initiating a
    /// full mutiphase push.
    #[serde(default)]
    pub for_codegen: bool,
}

impl StartPushRequest {
    pub fn into_project_config(self) -> anyhow::Result<ProjectConfig> {
        let node_version: Option<NodeVersion> = self.node_version.map(|v| v.parse()).transpose()?;
        if let Some(node_version) = node_version {
            node_version.check_supported_for_push()?;
        }

        Ok(ProjectConfig {
            config: ConfigMetadata {
                functions: self.functions,
                auth_info: vec![],
            },
            app_definition: self.app_definition.try_into()?,
            component_definitions: self
                .component_definitions
                .into_iter()
                .map(TryInto::try_into)
                .collect::<anyhow::Result<_>>()?,
            node_dependencies: self
                .node_dependencies
                .into_iter()
                .map(NodeDependency::from)
                .collect(),
            node_version,
            dry_run: self.dry_run,
            for_codegen: self.for_codegen,
        })
    }
}

#[derive(Clone, Debug)]
pub struct StartPushResponse {
    // We read the current environment variables when evaluating the definitions, so we need to
    // cancel the push if they change before the commit point.
    pub environment_variables: BTreeMap<EnvVarName, EnvVarValue>,

    pub external_deps_id: Option<ExternalDepsPackageId>,
    pub component_definition_packages: BTreeMap<ComponentDefinitionPath, SourcePackage>,

    pub app_auth: Vec<AuthInfo>,
    pub analysis: BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,

    pub app: CheckedComponent,

    pub schema_change: SchemaChange,
}

#[derive(Debug)]
pub struct StartPushResult {
    pub response: StartPushResponse,
    /// All runtime function modules in the app component
    pub app_functions: Vec<ModuleConfig>,
}

#[derive(Debug)]
pub struct EvaluatePushResponse {
    pub schema_change: SchemaChange,
}

/// Side-effect-free prediction of the schema validation and index backfill
/// work a push would trigger.
#[derive(Debug)]
pub struct EvaluateSchemaPredictionResponse {
    /// One entry per existing component instance; multiple instances of one
    /// definition each get their own entry.
    pub component_schema_evaluations: BTreeMap<ComponentPath, ComponentSchemaPrediction>,
    /// Pushed definitions with no existing instance: they get a fresh
    /// namespace, so nothing is walked and their indexes backfill trivially.
    pub new_component_definitions: Vec<ComponentDefinitionPath>,
}

#[derive(Debug)]
pub struct ComponentSchemaPrediction {
    pub definition_path: ComponentDefinitionPath,
    pub schema_validation: bool,
    pub tables: Vec<TablePrediction>,
    pub indexes: Vec<IndexPrediction>,
    /// Staged validators with a pending or valid validation that this push
    /// throws away. Staging one again later restarts its validation from the
    /// beginning.
    pub discarded_staged_validators: Vec<DiscardedStagedValidator>,
}

#[derive(Debug)]
pub struct DiscardedStagedValidator {
    pub table_name: TableName,
    pub state: StagedValidatorState,
    /// True when the new schema stages a different validator for the table;
    /// false when it stages nothing for it at all.
    pub replaced: bool,
}

#[derive(Debug)]
pub struct TablePrediction {
    pub name: TableName,
    pub outcome: TableValidationOutcome,
    pub num_docs: u64,
    pub size_bytes: u64,
    /// The table's staged validator under the active schema or an in-progress
    /// one, whichever validation is further along.
    pub staged: Option<StagedValidatorState>,
    /// The active schema's staged validator is still validating, and once it
    /// finishes, this push's enforced validator could skip the table's walk.
    pub can_skip_after_staged_validation: bool,
}

/// How far validation of a staged validator has gotten.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum StagedValidatorState {
    #[serde(rename_all = "camelCase")]
    Pending {
        num_docs_validated: u64,
        total_docs: Option<u64>,
    },
    Valid,
    Failed {
        error: String,
    },
}

impl From<&SchemaValidationWithProgress> for StagedValidatorState {
    fn from(staged: &SchemaValidationWithProgress) -> Self {
        match &staged.validation.state {
            ValidationState::Pending => Self::Pending {
                num_docs_validated: staged.progress.num_docs_validated,
                total_docs: staged.progress.total_docs,
            },
            ValidationState::Valid => Self::Valid,
            ValidationState::Failed { error } => Self::Failed {
                error: error.clone(),
            },
        }
    }
}

#[derive(Debug)]
pub struct IndexPrediction {
    pub name: IndexName,
    pub config: DeveloperIndexConfig,
    pub change: IndexChangePrediction,
    /// A backfill walk will run (or is already running) for this index. It
    /// blocks the push only when the index is not staged.
    pub needs_backfill: bool,
    /// Document count of the indexed table.
    pub num_docs: u64,
}

/// How a pushed schema changes an index, mirroring `IndexDiff`'s categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IndexChangePrediction {
    Added,
    Identical,
    Enabled,
    Disabled,
    Dropped,
}

async fn predict_component_schema<RT: Runtime>(
    tx: &mut Transaction<RT>,
    snapshot: &Snapshot,
    table_shapes: &Option<Arc<TableShapes>>,
    ts: RepeatableTimestamp,
    component_id: ComponentId,
    definition_path: ComponentDefinitionPath,
    pushed_schema: Option<&Option<DatabaseSchema>>,
) -> anyhow::Result<ComponentSchemaPrediction> {
    let namespace = TableNamespace::from(component_id);
    // An existing component whose definition is absent from the push keeps
    // its schema, tables, and indexes untouched.
    let Some(new_schema) = pushed_schema else {
        return Ok(ComponentSchemaPrediction {
            definition_path,
            schema_validation: false,
            tables: Vec::new(),
            indexes: Vec::new(),
            discarded_staged_validators: Vec::new(),
        });
    };

    // A pushed definition without a schema drops every index, matching
    // `start_component_schema_changes`.
    let empty_tables = BTreeMap::new();
    let index_diff = IndexModel::new(tx)
        .get_index_diff(
            namespace,
            new_schema
                .as_ref()
                .map(|schema| &schema.tables)
                .unwrap_or(&empty_tables),
        )
        .await?;
    let table_num_docs = |name: &IndexName| -> anyhow::Result<u64> {
        Ok(snapshot
            .must_table_count(namespace, name.table())?
            .num_values())
    };
    let mut indexes = Vec::new();
    for metadata in &index_diff.added {
        indexes.push(IndexPrediction {
            name: metadata.name.clone(),
            config: metadata.config.clone().into(),
            change: IndexChangePrediction::Added,
            needs_backfill: true,
            num_docs: table_num_docs(&metadata.name)?,
        });
    }
    for document in &index_diff.identical {
        indexes.push(IndexPrediction {
            name: document.name.clone(),
            config: document.config.clone().into(),
            change: IndexChangePrediction::Identical,
            needs_backfill: document.config.is_backfilling(),
            num_docs: table_num_docs(&document.name)?,
        });
    }
    for document in &index_diff.enabled {
        indexes.push(IndexPrediction {
            name: document.name.clone(),
            config: document.config.clone().into(),
            change: IndexChangePrediction::Enabled,
            needs_backfill: document.config.is_backfilling(),
            num_docs: table_num_docs(&document.name)?,
        });
    }
    for document in &index_diff.disabled {
        indexes.push(IndexPrediction {
            name: document.name.clone(),
            config: document.config.clone().into(),
            change: IndexChangePrediction::Disabled,
            needs_backfill: false,
            num_docs: table_num_docs(&document.name)?,
        });
    }
    for document in &index_diff.dropped {
        indexes.push(IndexPrediction {
            name: document.name.clone(),
            config: document.config.clone().into(),
            change: IndexChangePrediction::Dropped,
            needs_backfill: false,
            num_docs: table_num_docs(&document.name)?,
        });
    }

    let active = SchemaModel::new(tx, namespace)
        .get_by_state(SchemaState::Active)
        .await?;
    let valid_staged_validators = match &active {
        Some((active_id, active_schema)) => {
            SchemaValidationModel::new(tx, namespace)
                .valid_staged_validators(*active_id, active_schema)
                .await?
        },
        None => BTreeMap::new(),
    };
    let staged_validations = staged_validations(tx, namespace, active.as_ref()).await?;
    let carried_over: BTreeSet<_> = staged_validations
        .iter()
        .filter_map(|staged| {
            let validation = &staged.attempt.validation;
            let next = new_schema
                .as_ref()?
                .staged_schema_for_table(&validation.table_name)?;
            staged.can_reuse_for(next).then_some((
                validation.table_name.clone(),
                validation.validator_hash.clone()?,
            ))
        })
        .collect();
    let staged_validations: Vec<_> = staged_validations
        .into_iter()
        .map(|staged| staged.attempt)
        .collect();
    let mut staged_states: BTreeMap<TableName, StagedValidatorState> =
        most_advanced(&staged_validations, |validation| {
            Some(validation.validation.table_name.clone())
        })
        .into_iter()
        .map(|(table, validation)| (table, validation.into()))
        .collect();
    let still_validating = active_staged_validators_still_validating(
        active.as_ref().map(|(_, schema)| &**schema),
        &staged_validations,
    )?;

    let active_schema = active.map(|(_id, schema)| schema);
    let (schema_validation, tables) = match new_schema {
        Some(schema) => {
            let table_mapping = tx.table_mapping().namespace(namespace);
            let virtual_system_mapping = tx.virtual_system_mapping().clone();
            let outcomes = DatabaseSchema::table_validation_outcomes(
                schema,
                active_schema.as_deref(),
                &table_mapping,
                &virtual_system_mapping,
                &table_shape_provider(table_shapes, &table_mapping, ts),
                &valid_staged_validators,
            )?;
            let tables = outcomes
                .into_iter()
                .map(|(name, outcome)| {
                    let count = snapshot.must_table_count(namespace, name)?;
                    Ok(TablePrediction {
                        name: name.clone(),
                        outcome,
                        num_docs: count.num_values(),
                        size_bytes: count.total_size(),
                        staged: staged_states.remove(name),
                        can_skip_after_staged_validation: schema.schema_validation
                            && still_validating.get(name).is_some_and(|staged| {
                                Validator::from(Some(staged.clone()))
                                    .is_subset(&schema.tables[name].document_type.clone().into())
                            }),
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            (schema.schema_validation, tables)
        },
        None => (false, Vec::new()),
    };
    let mut retained = promoted_staged_validators(&tables, active_schema.as_deref())?;
    retained.extend(carried_over);
    let discarded_staged_validators =
        discarded_staged_validators(&staged_validations, new_schema.as_ref(), &retained);
    Ok(ComponentSchemaPrediction {
        definition_path,
        schema_validation,
        tables,
        indexes,
        discarded_staged_validators,
    })
}

/// Validations of staged validators that this push can discard: those under
/// the active schema, and those under a pending or validated schema, which a
/// push of any different schema overwrites.
async fn staged_validations<RT: Runtime>(
    tx: &mut Transaction<RT>,
    namespace: TableNamespace,
    active: Option<&(ResolvedDocumentId, Arc<DatabaseSchema>)>,
) -> anyhow::Result<Vec<StagedValidationWithProgress>> {
    let mut schemas: Vec<_> = active.cloned().into_iter().collect();
    for state in [SchemaState::Pending, SchemaState::Validated] {
        if let Some(schema) = SchemaModel::new(tx, namespace).get_by_state(state).await? {
            schemas.push(schema);
        }
    }
    let mut staged = Vec::new();
    for (schema_id, schema) in schemas {
        staged.extend(
            SchemaValidationModel::new(tx, namespace)
                .staged_validations_with_progress(schema_id, &schema)
                .await?,
        );
    }
    Ok(staged)
}

/// Groups `validations` by `key`, keeping the most advanced validation per
/// key: a valid one, else the one that has checked the most documents. A
/// table can have a validation under both the active schema and an in-progress
/// one.
fn most_advanced<K: Ord>(
    validations: &[SchemaValidationWithProgress],
    key: impl Fn(&SchemaValidationWithProgress) -> Option<K>,
) -> BTreeMap<K, &SchemaValidationWithProgress> {
    let advancement = |validation: &SchemaValidationWithProgress| {
        (
            matches!(validation.validation.state, ValidationState::Valid),
            validation.progress.num_docs_validated,
        )
    };
    let mut result: BTreeMap<K, &SchemaValidationWithProgress> = BTreeMap::new();
    for validation in validations {
        let Some(key) = key(validation) else {
            continue;
        };
        match result.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(validation);
            },
            Entry::Occupied(mut entry) => {
                if advancement(validation) > advancement(entry.get()) {
                    entry.insert(validation);
                }
            },
        }
    }
    result
}

/// The active schema's staged validators that are still validating, by table.
/// Once one finishes, a push enforcing a superset of it can skip the table's
/// walk.
fn active_staged_validators_still_validating(
    active_schema: Option<&DatabaseSchema>,
    staged_validations: &[SchemaValidationWithProgress],
) -> anyhow::Result<BTreeMap<TableName, DocumentSchema>> {
    let mut result = BTreeMap::new();
    let Some(active_schema) = active_schema else {
        return Ok(result);
    };
    for staged in staged_validations {
        let validation = &staged.validation;
        if !matches!(validation.state, ValidationState::Pending) {
            continue;
        }
        if let Some(validator) = active_schema.staged_schema_for_table(&validation.table_name)
            && Some(validator.content_hash()?) == validation.validator_hash
        {
            result.insert(validation.table_name.clone(), validator.clone());
        }
    }
    Ok(result)
}

/// The active schema's validated staged validators that this push promotes,
/// keyed by table and validator hash. Only the active schema's validator is
/// promoted; a different one staged for the same table by an in-progress
/// schema is still discarded.
fn promoted_staged_validators(
    tables: &[TablePrediction],
    active_schema: Option<&DatabaseSchema>,
) -> anyhow::Result<BTreeSet<(TableName, String)>> {
    let mut promoted = BTreeSet::new();
    for table in tables {
        if matches!(
            table.outcome,
            TableValidationOutcome::SupersetOfStagedValidated
        ) && let Some(validator) =
            active_schema.and_then(|schema| schema.staged_schema_for_table(&table.name))
        {
            promoted.insert((table.name.clone(), validator.content_hash()?));
        }
    }
    Ok(promoted)
}

/// Staged validators whose pending or valid validation this push throws away,
/// because `new_schema` neither reuses their validation nor promotes
/// them. Failed validations are left out: a redeploy retries them from scratch
/// anyway, so discarding one loses nothing.
fn discarded_staged_validators(
    staged_validations: &[SchemaValidationWithProgress],
    new_schema: Option<&DatabaseSchema>,
    retained: &BTreeSet<(TableName, String)>,
) -> Vec<DiscardedStagedValidator> {
    let by_validator = most_advanced(staged_validations, |staged| {
        let validation = &staged.validation;
        if matches!(validation.state, ValidationState::Failed { .. }) {
            return None;
        }
        Some((
            validation.table_name.clone(),
            validation.validator_hash.clone()?,
        ))
    });
    let mut discarded = Vec::new();
    for (key, validation) in by_validator {
        if retained.contains(&key) {
            continue;
        }
        let (table_name, _) = key;
        let replaced = new_schema
            .and_then(|schema| schema.staged_schema_for_table(&table_name))
            .is_some();
        discarded.push(DiscardedStagedValidator {
            table_name,
            state: validation.into(),
            replaced,
        });
    }
    discarded
}

impl From<NodeDependencyJson> for NodeDependency {
    fn from(value: NodeDependencyJson) -> Self {
        Self {
            package: value.name,
            version: value.version,
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppDefinitionConfigJson {
    pub definition: Option<ModuleJson>,
    pub dependencies: Vec<String>,
    pub schema: Option<ModuleJson>,
    // CLI versions <= 1.31.5 used functions and did not upload unchanged_module_hashes
    #[serde(alias = "functions")]
    pub changed_modules: Vec<ModuleJson>,
    #[serde(default)]
    pub unchanged_module_hashes: Vec<ModuleHashJson>,
    pub udf_server_version: String,
}

impl TryFrom<AppDefinitionConfigJson> for AppDefinitionConfig {
    type Error = anyhow::Error;

    fn try_from(value: AppDefinitionConfigJson) -> Result<Self, Self::Error> {
        Ok(Self {
            definition: value.definition.map(TryInto::try_into).transpose()?,
            dependencies: value
                .dependencies
                .into_iter()
                .map(|s| s.parse())
                .collect::<anyhow::Result<_>>()?,
            schema: value.schema.map(TryInto::try_into).transpose()?,
            changed_runtime_modules: value
                .changed_modules
                .into_iter()
                .map(TryInto::try_into)
                .collect::<anyhow::Result<_>>()?,
            udf_server_version: value.udf_server_version.parse()?,
            unchanged_runtime_module_hashes: value
                .unchanged_module_hashes
                .into_iter()
                .map(TryInto::try_into)
                .collect::<anyhow::Result<_>>()?,
        })
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDefinitionConfigJson {
    pub definition_path: String,
    pub definition: ModuleJson,
    pub dependencies: Vec<String>,
    pub schema: Option<ModuleJson>,
    pub functions: Vec<ModuleJson>,
    pub udf_server_version: String,
}

impl TryFrom<ComponentDefinitionConfigJson> for ComponentDefinitionConfig {
    type Error = anyhow::Error;

    fn try_from(value: ComponentDefinitionConfigJson) -> Result<Self, Self::Error> {
        let functions: Vec<ModuleConfig> = value
            .functions
            .into_iter()
            .map(TryInto::try_into)
            .collect::<anyhow::Result<_>>()?;
        for module in &functions {
            match module.environment {
                ModuleEnvironment::Node => {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "NodeActionsNotSupported",
                        format!(
                            "Node actions are not supported in components. Remove `\"use node;\" \
                             from {}",
                            module.path.as_str()
                        )
                    ));
                },
                ModuleEnvironment::Invalid | ModuleEnvironment::Isolate => {},
            }
        }
        Ok(Self {
            definition_path: value.definition_path.parse()?,
            definition: value.definition.try_into()?,
            dependencies: value
                .dependencies
                .into_iter()
                .map(|s| s.parse())
                .collect::<anyhow::Result<_>>()?,
            schema: value.schema.map(TryInto::try_into).transpose()?,
            functions,
            udf_server_version: value.udf_server_version.parse()?,
        })
    }
}

/// API level structure for representing modules as Json
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModuleJson {
    pub path: String,
    pub source: String,
    pub source_map: Option<SourceMap>,
    pub environment: Option<String>,
}

/// API level structure for representing module hashes as Json (for unchanged
/// modules)
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModuleHashJson {
    pub path: String,
    pub environment: Option<String>,
    pub sha256: String,
}

impl From<ModuleConfig> for ModuleJson {
    fn from(
        ModuleConfig {
            path,
            source,
            source_map,
            environment,
        }: ModuleConfig,
    ) -> ModuleJson {
        ModuleJson {
            path: path.into(),
            source: source.to_string(),
            source_map,
            environment: Some(environment.to_string()),
        }
    }
}

impl TryFrom<ModuleJson> for ModuleConfig {
    type Error = anyhow::Error;

    fn try_from(
        ModuleJson {
            path,
            source,
            source_map,
            environment,
        }: ModuleJson,
    ) -> anyhow::Result<ModuleConfig> {
        Ok(ModuleConfig {
            path: parse_module_path(&path)?,
            source: ModuleSource::new(&source),
            source_map,
            environment: parse_module_environment(&environment, &path)?,
        })
    }
}

impl TryFrom<ModuleHashJson> for ModuleHashConfig {
    type Error = anyhow::Error;

    fn try_from(
        ModuleHashJson {
            path,
            environment,
            sha256,
        }: ModuleHashJson,
    ) -> anyhow::Result<ModuleHashConfig> {
        let sha256_bytes = const_hex::decode(&sha256).context("Invalid hex in sha256")?;
        let sha256_array: [u8; 32] = sha256_bytes
            .try_into()
            .ok()
            .context("sha256 not 32 bytes")?;
        Ok(ModuleHashConfig {
            path: parse_module_path(&path)?,
            environment: parse_module_environment(&environment, &path)?,
            sha256: Sha256Digest::from(sha256_array),
        })
    }
}

pub fn parse_module_environment(
    environment: &Option<String>,
    path: &String,
) -> anyhow::Result<ModuleEnvironment> {
    Ok(match environment {
        Some(s) => s.parse()?,
        // Default to using the path for backwards compatibility
        None => deprecated_extract_environment_from_path(path.clone())?,
    })
}

pub fn parse_module_path(path: &str) -> anyhow::Result<ModulePath> {
    path.parse().map_err(|e: InvalidModulePathError| {
        ErrorMetadata::bad_request("BadConvexModuleIdentifier", e.to_string()).into()
    })
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NodeDependencyJson {
    name: String,
    version: String,
}

#[derive(Debug, Default)]
pub struct FinishPushDiff {
    pub auth_diff: AuthDiff,
    pub definition_diffs: BTreeMap<ComponentDefinitionPath, ComponentDefinitionDiff>,
    pub component_diffs: BTreeMap<ComponentPath, ComponentDiff>,
}

#[derive(Debug)]
pub enum SchemaStatus {
    InProgress {
        components: BTreeMap<ComponentPath, ComponentSchemaStatus>,
    },
    Failed {
        error: String,
        component_path: ComponentPath,
        table_name: Option<String>,
    },
    RaceDetected,
    Complete,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum SchemaStatusJson {
    #[serde(rename_all = "camelCase")]
    InProgress {
        components: BTreeMap<String, ComponentSchemaStatusJson>,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        error: String,
        component_path: String,
        table_name: Option<String>,
    },
    RaceDetected,
    Complete,
}

impl From<SchemaStatus> for SchemaStatusJson {
    fn from(value: SchemaStatus) -> Self {
        match value {
            SchemaStatus::InProgress { components } => SchemaStatusJson::InProgress {
                components: components
                    .into_iter()
                    .map(|(k, v)| (String::from(k), v.into()))
                    .collect(),
            },
            SchemaStatus::Failed {
                error,
                component_path,
                table_name,
            } => SchemaStatusJson::Failed {
                error,
                component_path: String::from(component_path),
                table_name,
            },
            SchemaStatus::RaceDetected => SchemaStatusJson::RaceDetected,
            SchemaStatus::Complete => SchemaStatusJson::Complete,
        }
    }
}

#[derive(Debug)]
pub struct ComponentSchemaStatus {
    pub schema_validation_complete: bool,
    pub indexes_complete: usize,
    pub indexes_total: usize,
}

impl ComponentSchemaStatus {
    pub fn is_complete(&self) -> bool {
        self.schema_validation_complete && self.indexes_complete == self.indexes_total
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSchemaStatusJson {
    pub schema_validation_complete: bool,
    pub indexes_complete: usize,
    pub indexes_total: usize,
}

impl From<ComponentSchemaStatus> for ComponentSchemaStatusJson {
    fn from(value: ComponentSchemaStatus) -> Self {
        Self {
            schema_validation_complete: value.schema_validation_complete,
            indexes_complete: value.indexes_complete,
            indexes_total: value.indexes_total,
        }
    }
}
