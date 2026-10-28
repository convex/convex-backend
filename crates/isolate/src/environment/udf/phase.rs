use std::{
    collections::BTreeMap,
    ops::{
        Deref,
        DerefMut,
    },
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use common::{
    components::{
        CanonicalizedComponentModulePath,
        ComponentId,
    },
    document::ParsedDocument,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    types::ModuleEnvironment,
};
use database::{
    BiggestDocumentWrites,
    BootstrapComponentsModel,
    FunctionExecutionSize,
    SnoopedTransaction,
    Transaction,
    TransactionReadSet,
};
use errors::ErrorMetadata;
use model::{
    environment_variables::types::{
        EnvVarName,
        EnvVarValue,
    },
    modules::ModuleModel,
    source_packages::{
        types::SourcePackage,
        SourcePackageModel,
    },
    udf_config::UdfConfigModel,
};
use rand::{
    Rng as _,
    SeedableRng,
};
use rand_chacha::ChaCha12Rng;
use sync_types::ModulePath;
use udf::environment::PreloadedEnvVars;
use value::{
    identifier::Identifier,
    ConvexValue,
};

use crate::{
    environment::{
        helpers::{
            performance_unsupported,
            PerformanceApi,
            Phase,
        },
        ModuleCodeCacheResult,
    },
    module_cache::{
        ModuleCache,
        V8ModuleSource,
    },
    timeout::{
        PauseReason,
        Timeout,
    },
};

/// UDF execution has two phases:
///
/// 1. We start by loading all imported modules, evaluating them, and inserting
///    them into the module map.
/// 2. We find the query or mutation function in the specified module and run
///    it.
///
/// We shouldn't be looking at the database in the first step (other than to
/// load code), and we shouldn't be performing dynamic imports in the second
/// step. This structure is responsible for enforcing these invariants.
pub struct UdfPhase<RT: Runtime> {
    phase: Phase,

    // We "check out" the transaction when executing a cross-component
    // call. Until we implement subtransactions, we cannot run any
    // user code concurrently with a component call.
    tx: Option<MaybeSnooped<RT>>,

    pub rt: RT,
    module_loader: Arc<dyn ModuleCache<RT>>,
    preloaded: UdfPreloaded,
    component: ComponentId,
}

enum UdfPreloaded {
    Created {
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
    },
    Ready {
        rng: Option<ChaCha12Rng>,
        observed_rng_during_execution: bool,
        unix_timestamp: Option<UnixTimestamp>,
        observed_time_during_execution: bool,
        performance_api: Option<PerformanceApi>,
        observed_identity_during_execution: bool,
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        env_vars: PreloadedEnvVars,
        component: ComponentId,
        component_arguments: Option<BTreeMap<Identifier, ConvexValue>>,
        source_package: Option<Arc<ParsedDocument<SourcePackage>>>,
    },
}

enum MaybeSnooped<RT: Runtime> {
    Tx(Transaction<RT>),
    SnoopedTx(SnoopedTransaction<RT>),
}

impl<RT: Runtime> Deref for MaybeSnooped<RT> {
    type Target = Transaction<RT>;

    fn deref(&self) -> &Self::Target {
        match self {
            MaybeSnooped::Tx(tx) => tx,
            MaybeSnooped::SnoopedTx(tx) => tx,
        }
    }
}
impl<RT: Runtime> DerefMut for MaybeSnooped<RT> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            MaybeSnooped::Tx(tx) => tx,
            MaybeSnooped::SnoopedTx(tx) => tx,
        }
    }
}

impl<RT: Runtime> UdfPhase<RT> {
    pub fn new(
        tx: Transaction<RT>,
        rt: RT,
        module_loader: Arc<dyn ModuleCache<RT>>,
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        component: ComponentId,
    ) -> Self {
        Self {
            phase: Phase::Importing,
            tx: Some(MaybeSnooped::Tx(tx)),
            rt,
            module_loader,
            preloaded: UdfPreloaded::Created {
                default_system_env_vars,
            },
            component,
        }
    }

    #[fastrace::trace]
    pub async fn initialize(&mut self, timeout: &mut Timeout<RT>) -> anyhow::Result<()> {
        anyhow::ensure!(self.phase == Phase::Importing);
        let UdfPreloaded::Created {
            default_system_env_vars,
        } = &self.preloaded
        else {
            anyhow::bail!("UdfPhase initialized twice");
        };
        let default_system_env_vars = default_system_env_vars.clone();
        let component = self.component;
        self.preloaded = timeout
            .with_release_permit(PauseReason::UdfInitialize, async {
                let component_args = if !component.is_root() {
                    Some(
                        BootstrapComponentsModel::new(self.tx_mut()?)
                            .load_component_args(component)
                            .await?,
                    )
                } else {
                    None
                };

                // UdfConfig might not be defined for super old modules or system modules.
                let udf_config = UdfConfigModel::new(self.tx_mut()?, component.into())
                    .get()
                    .await?;
                let rng = udf_config
                    .as_ref()
                    .map(|c| ChaCha12Rng::from_seed(c.import_phase_rng_seed));
                let unix_timestamp = udf_config.as_ref().map(|c| c.import_phase_unix_timestamp);

                let env_vars = PreloadedEnvVars::load(
                    self.tx_mut()?,
                    component,
                    default_system_env_vars.clone(),
                )
                .await?;

                let source_package =
                    if ModuleModel::new(self.tx_mut()?).has_pending_module(component) {
                        // If there is a pending write in ModulesTable, this may be an
                        // ad-hoc test query (`execute_standalone_module`).
                        // Reading the source package from a snapshot query doesn't
                        // work in that case since it doesn't see pending writes.
                        // TODO: even if it did, the test query codepath breaks the
                        // get_latest() invariant since it leaves behind modules
                        // with mixed source packages.
                        None
                    } else {
                        let mut snapshot_tx = self.tx_mut()?.clone_for_snapshot_query();
                        // Load all modules from the latest source package, but don't
                        // record a read dependency on all the modules and their source
                        // packages.
                        SourcePackageModel::new(&mut snapshot_tx, component.into())
                            .get_latest()
                            .await?
                    };

                Ok(UdfPreloaded::Ready {
                    rng,
                    observed_rng_during_execution: false,
                    unix_timestamp,
                    observed_time_during_execution: false,
                    performance_api: unix_timestamp.map(PerformanceApi::new),
                    observed_identity_during_execution: false,
                    default_system_env_vars,
                    env_vars,
                    component,
                    component_arguments: component_args,
                    source_package,
                })
            })
            .await?;

        Ok(())
    }

    pub fn default_system_env_vars(&self) -> &BTreeMap<EnvVarName, EnvVarValue> {
        match &self.preloaded {
            UdfPreloaded::Created {
                default_system_env_vars,
            } => default_system_env_vars,
            UdfPreloaded::Ready {
                default_system_env_vars,
                ..
            } => default_system_env_vars,
        }
    }

    pub fn component(&self) -> anyhow::Result<ComponentId> {
        let UdfPreloaded::Ready { component, .. } = &self.preloaded else {
            anyhow::bail!("Phase not initialized");
        };
        Ok(*component)
    }

    pub fn component_arguments(&self) -> anyhow::Result<&BTreeMap<Identifier, ConvexValue>> {
        let UdfPreloaded::Ready {
            component_arguments: component_args,
            ..
        } = &self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        let Some(component_args) = component_args else {
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoComponentArgs",
                "Component arguments are not available within the app",
            ));
        };
        if self.phase != Phase::Executing {
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoComponentArgsDuringImport",
                "Can't use `componentArg` at import time",
            ));
        }
        Ok(component_args)
    }

    #[fastrace::trace]
    pub async fn get_module(
        &mut self,
        module_path: &ModulePath,
        timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<Option<(Arc<V8ModuleSource>, ModuleCodeCacheResult)>> {
        if self.phase != Phase::Importing {
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoDynamicImport",
                format!("Can't dynamically import {module_path:?} in a query or mutation")
            ));
        }
        let UdfPreloaded::Ready {
            component,
            source_package,
            ..
        } = &self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        let component = *component;
        let path = CanonicalizedComponentModulePath {
            component,
            module_path: module_path.clone().canonicalize(),
        };
        let source_package = source_package.clone();
        let Some((module_metadata, module_source)) = timeout
            .with_release_permit(PauseReason::LoadModule, async {
                let Some(module_metadata) = ModuleModel::new(self.tx_mut()?)
                    .get_metadata(path.clone())
                    .await?
                else {
                    return Ok(None);
                };
                anyhow::ensure!(
                    module_metadata.environment == ModuleEnvironment::Isolate,
                    "Trying to execute {:?} in isolate, but it is bundled for {:?}.",
                    path.module_path,
                    module_metadata.environment
                );
                let source_package = if let Some(pkg) = source_package {
                    pkg
                } else {
                    SourcePackageModel::new(self.tx_mut()?, component.into())
                        .get(module_metadata.source_package_id)
                        .await?
                };
                let module_source = self
                    .module_loader
                    .get_module_with_metadata(&module_metadata, &source_package)
                    .await?;
                Ok(Some((module_metadata, module_source)))
            })
            .await?
        else {
            return Ok(None);
        };
        let code_cache_result = self
            .module_loader
            .clone()
            .code_cache_result(&module_metadata);
        Ok(Some((module_source, code_cache_result)))
    }

    pub fn tx(&mut self) -> anyhow::Result<&mut Transaction<RT>> {
        if self.phase != Phase::Executing {
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoDbDuringImport",
                "Can't use database at import time",
            ));
        }
        self.tx_mut()
    }

    pub fn start_nested_udf(
        &mut self,
    ) -> anyhow::Result<(Transaction<RT>, [u8; 32], UnixTimestamp)> {
        let tx = self
            .tx
            .take()
            .context("Transaction missing due to concurrent component call")?;
        let MaybeSnooped::Tx(tx) = tx else {
            // snooping is only used during initialization
            anyhow::bail!("Transaction is still snooped during start_nested_udf?");
        };
        let UdfPreloaded::Ready {
            ref mut rng,
            unix_timestamp,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        let (rng, unix_timestamp) =
            (rng.as_mut().zip(unix_timestamp)).context(ErrorMetadata::bad_request(
                "NoCallsDuringImport",
                "Cannot call another function at import time",
            ))?;
        // Note: it's up to the caller to update observed_rng / observed_time after the
        // nested UDF returns
        let rng_seed = rng.random();
        Ok((tx, rng_seed, unix_timestamp))
    }

    pub fn put_tx(&mut self, tx: Transaction<RT>) -> anyhow::Result<()> {
        anyhow::ensure!(self.tx.is_none());
        self.tx = Some(MaybeSnooped::Tx(tx));
        Ok(())
    }

    pub fn tx_mut(&mut self) -> anyhow::Result<&mut Transaction<RT>> {
        self.tx
            .as_deref_mut()
            .context("Transaction missing due to concurrent component call")
    }

    pub fn snoop_reads(&mut self) -> anyhow::Result<()> {
        match self.tx.take() {
            Some(MaybeSnooped::Tx(tx)) => {
                self.tx = Some(MaybeSnooped::SnoopedTx(tx.snoop_reads()));
                Ok(())
            },
            Some(MaybeSnooped::SnoopedTx(_)) => {
                anyhow::bail!("Called snoop_reads while already snooping")
            },
            None => anyhow::bail!("Transaction missing due to concurrent component call"),
        }
    }

    pub fn finish_snoop(&mut self) -> anyhow::Result<TransactionReadSet> {
        match self.tx.take() {
            Some(MaybeSnooped::SnoopedTx(tx)) => {
                let (tx, reads) = tx.finish_snoop();
                self.tx = Some(MaybeSnooped::Tx(tx));
                Ok(reads)
            },
            Some(MaybeSnooped::Tx(_)) => {
                anyhow::bail!("Called finish_snoop while not snooping")
            },
            None => anyhow::bail!("Transaction missing due to concurrent component call"),
        }
    }

    fn tx_ref(&self) -> anyhow::Result<&Transaction<RT>> {
        self.tx
            .as_deref()
            .context("Transaction missing due to concurrent component call")
    }

    pub fn into_transaction(self) -> anyhow::Result<Transaction<RT>> {
        self.tx
            .context("Transaction missing due to concurrent component call")
            .map(|tx| match tx {
                MaybeSnooped::Tx(tx) => tx,
                MaybeSnooped::SnoopedTx(tx) => tx.finish_snoop().0,
            })
    }

    pub fn biggest_document_writes(&self) -> anyhow::Result<Option<BiggestDocumentWrites>> {
        Ok(self.tx_ref()?.biggest_document_writes())
    }

    pub fn execution_size(&self) -> anyhow::Result<FunctionExecutionSize> {
        Ok(self.tx_ref()?.execution_size())
    }

    pub fn begin_execution(
        &mut self,
        rng_seed: [u8; 32],
        execution_unix_timestamp: UnixTimestamp,
    ) -> anyhow::Result<()> {
        if self.phase != Phase::Importing {
            anyhow::bail!("Phase was already {:?}", self.phase)
        }
        let UdfPreloaded::Ready {
            ref mut rng,
            ref mut unix_timestamp,
            ref mut performance_api,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        self.phase = Phase::Executing;
        *rng = Some(ChaCha12Rng::from_seed(rng_seed));
        *unix_timestamp = Some(execution_unix_timestamp);
        if let Some(performance_api) = performance_api {
            performance_api.begin_execution(&self.rt, execution_unix_timestamp)?;
        }
        Ok(())
    }

    pub fn get_environment_variable(
        &mut self,
        name: EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        let UdfPreloaded::Ready { ref env_vars, .. } = self.preloaded else {
            anyhow::bail!("Phase not initialized");
        };
        let tx = self
            .tx
            .as_mut()
            .context("Transaction missing due to concurrent component call")?;
        env_vars.get(tx, &name)
    }

    pub fn rng(&mut self) -> anyhow::Result<&mut ChaCha12Rng> {
        let UdfPreloaded::Ready {
            ref mut rng,
            ref mut observed_rng_during_execution,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        if self.phase == Phase::Executing {
            *observed_rng_during_execution = true;
        }
        let Some(rng) = rng else {
            // Fail for old module without import time rng populated.
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoRandomDuringImport",
                "Math.random unsupported at import time",
            ));
        };
        Ok(rng)
    }

    pub fn unix_timestamp(&mut self) -> anyhow::Result<UnixTimestamp> {
        let UdfPreloaded::Ready {
            unix_timestamp,
            ref mut observed_time_during_execution,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        if self.phase == Phase::Executing {
            *observed_time_during_execution = true;
        }
        let Some(unix_timestamp) = unix_timestamp else {
            // Fail for old modules without import time timestamp populated.
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoDateDuringImport",
                "Date unsupported at import time"
            ));
        };
        Ok(unix_timestamp)
    }

    pub fn performance_now_incrementing(&mut self) -> anyhow::Result<Duration> {
        let UdfPreloaded::Ready {
            performance_api,
            observed_time_during_execution,
            ..
        } = &mut self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        match (&self.phase, &*performance_api) {
            (Phase::Importing, Some(PerformanceApi::Importing(api))) => Ok(api.now()),
            (Phase::Executing, Some(PerformanceApi::Executing(api))) => {
                *observed_time_during_execution = true;
                Ok(api.now_incrementing(&self.rt))
            },
            (_, None) => anyhow::bail!(performance_unsupported()),
            _ => anyhow::bail!("performance API state does not match phase"),
        }
    }

    pub fn performance_now_fixed(&mut self) -> anyhow::Result<Duration> {
        let UdfPreloaded::Ready {
            performance_api,
            observed_time_during_execution,
            ..
        } = &mut self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        match (&self.phase, &*performance_api) {
            (Phase::Importing, Some(PerformanceApi::Importing(api))) => Ok(api.now()),
            (Phase::Executing, Some(PerformanceApi::Executing(api))) => {
                *observed_time_during_execution = true;
                Ok(api.now_fixed())
            },
            (_, None) => anyhow::bail!(performance_unsupported()),
            _ => anyhow::bail!("performance API state does not match phase"),
        }
    }

    pub fn performance_time_origin(&self) -> anyhow::Result<UnixTimestamp> {
        let UdfPreloaded::Ready {
            performance_api, ..
        } = &self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        let Some(performance_api) = performance_api else {
            anyhow::bail!(performance_unsupported());
        };
        Ok(performance_api.time_origin())
    }

    pub fn observe_time(&mut self) {
        if let UdfPreloaded::Ready {
            observed_time_during_execution,
            ..
        } = &mut self.preloaded
        {
            *observed_time_during_execution = true;
        }
    }

    pub fn observe_rng(&mut self) {
        if let UdfPreloaded::Ready {
            observed_rng_during_execution,
            ..
        } = &mut self.preloaded
        {
            *observed_rng_during_execution = true;
        }
    }

    pub fn observe_identity(&mut self) -> anyhow::Result<()> {
        let UdfPreloaded::Ready {
            ref mut observed_identity_during_execution,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        *observed_identity_during_execution = true;
        Ok(())
    }

    pub fn observed_rng(&self) -> bool {
        match self.preloaded {
            UdfPreloaded::Ready {
                observed_rng_during_execution,
                ..
            } => observed_rng_during_execution,
            UdfPreloaded::Created { .. } => false,
        }
    }

    pub fn observed_time(&self) -> bool {
        match self.preloaded {
            UdfPreloaded::Ready {
                observed_time_during_execution,
                ..
            } => observed_time_during_execution,
            UdfPreloaded::Created { .. } => false,
        }
    }

    pub fn observed_identity(&self) -> bool {
        match self.preloaded {
            UdfPreloaded::Ready {
                observed_identity_during_execution,
                ..
            } => observed_identity_during_execution,
            UdfPreloaded::Created { .. } => false,
        }
    }

    pub fn module_loader(&self) -> &Arc<dyn ModuleCache<RT>> {
        &self.module_loader
    }
}
