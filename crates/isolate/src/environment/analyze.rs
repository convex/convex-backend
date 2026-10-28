use std::{
    collections::{
        btree_map::Entry,
        BTreeMap,
        VecDeque,
    },
    path::Path,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::{
    anyhow,
    Context,
};
use common::{
    errors::JsError,
    json::JsonForm as _,
    knobs::{
        DATABASE_UDF_SYSTEM_TIMEOUT,
        ISOLATE_ANALYZE_USER_TIMEOUT,
    },
    log_lines::LogLevel,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    types::{
        HttpActionRoute,
        RoutableMethod,
        UdfType,
    },
};
use deno_core::{
    v8::{
        self,
        scope,
        GetPropertyNamesArgs,
    },
    ModuleResolutionError,
};
use errors::ErrorMetadata;
use model::{
    cron_jobs::types::{
        CronIdentifier,
        CronSpec,
    },
    environment_variables::types::{
        EnvVarName,
        EnvVarValue,
    },
    modules::{
        function_validators::{
            ArgsValidator,
            ReturnsValidator,
        },
        module_versions::{
            invalid_function_name_error,
            AnalyzedFunction,
            AnalyzedHttpRoute,
            AnalyzedHttpRoutes,
            AnalyzedModule,
            AnalyzedSourcePosition,
            Visibility,
        },
        user_error::{
            ModuleNotFoundError,
            SystemModuleNotFoundError,
        },
    },
    udf_config::types::UdfConfig,
};
use rand::SeedableRng;
use rand_chacha::ChaCha12Rng;
use serde_json::Value as JsonValue;
use sync_types::{
    module_path::InvalidModulePathError,
    CanonicalizedModulePath,
    FunctionName,
    ModulePath,
};
use value::{
    heap_size::WithHeapSize,
    NamespacedTableMapping,
};

use super::ModuleCodeCacheResult;
use crate::{
    context_cache::ContextCache,
    environment::{
        helpers::{
            module_loader::{
                module_specifier_from_path,
                module_specifier_from_str,
                path_from_module_specifier,
            },
            syscall_error::{
                syscall_description_for_error,
                syscall_name_for_error,
            },
        },
        AsyncOpRequest,
        JsEnvironment,
        OpProvider,
        SyscallProvider,
    },
    execution_scope::ExecutionScope,
    helpers::{
        self,
        source_map_from_slice,
    },
    isolate::Isolate,
    metrics::{
        log_source_map_missing,
        log_source_map_origin_in_separate_module,
        log_source_map_token_lookup_failed,
    },
    module_cache::V8ModuleSource,
    request_scope::RequestScope,
    strings::{
        self,
    },
    timeout::Timeout,
    ConcurrencyPermit,
};

pub struct AnalyzeEnvironment {
    modules: Arc<BTreeMap<CanonicalizedModulePath, Arc<V8ModuleSource>>>,
    // This is used to lazily cache the result of sourcemap::SourceMap::from_slice across
    // modules and functions. There are certain source maps whose source origin we don't
    // need to construct during analysis (i.e. if all of the UDFs it defines have function
    // bodies outside the current module), so keeping this mapping lazy allows for avoiding
    // unnecessary source map parsing.
    source_maps_cache: BTreeMap<CanonicalizedModulePath, Option<sourcemap::SourceMap>>,
    rng: ChaCha12Rng,
    unix_timestamp: UnixTimestamp,
    environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
    // Collect logs during analysis for push failure reporting (max 100 entries)
    collected_logs: VecDeque<String>,
}

impl OpProvider for AnalyzeEnvironment {
    fn trace(&mut self, _level: LogLevel, messages: Vec<String>) -> anyhow::Result<()> {
        // These logs are only shown to the pusher on error.
        let log_message = messages.join(" ");

        // Keep only the last 100 log entries
        if self.collected_logs.len() >= 100 {
            self.collected_logs.pop_front();
        }
        self.collected_logs.push_back(log_message.clone());

        tracing::warn!("Console access at import time: {}", log_message);
        Ok(())
    }

    fn rng(&mut self) -> anyhow::Result<&mut ChaCha12Rng> {
        Ok(&mut self.rng)
    }

    fn crypto_rng(&mut self) -> anyhow::Result<super::crypto_rng::CryptoRng> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoCryptoRngDuringImport",
            "Cannot use cryptographic randomness at import time"
        ))
    }

    fn unix_timestamp(&mut self) -> anyhow::Result<UnixTimestamp> {
        Ok(self.unix_timestamp)
    }

    fn performance_now(&mut self) -> anyhow::Result<Duration> {
        Ok(Duration::ZERO)
    }

    fn performance_time_origin(&mut self) -> anyhow::Result<UnixTimestamp> {
        Ok(self.unix_timestamp)
    }

    fn get_environment_variable(
        &mut self,
        name: EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        let value = self.environment_variables.get(&name).cloned();
        Ok(value)
    }

    fn get_all_table_mappings(&mut self) -> anyhow::Result<NamespacedTableMapping> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoTableMappingFetchDuringImport",
            "Getting the table mapping unsupported at import time"
        ))
    }
}

impl<RT: Runtime> SyscallProvider<RT> for AnalyzeEnvironment {
    async fn lookup_source(
        &mut self,
        path: &str,
        _timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<Option<(Arc<V8ModuleSource>, ModuleCodeCacheResult)>> {
        let p = ModulePath::from_str(path)?.canonicalize();
        let result = self.modules.get(&p).cloned();
        Ok(result.map(|m| (m, ModuleCodeCacheResult::noop())))
    }

    fn syscall(&mut self, name: &str, _args: JsonValue) -> anyhow::Result<JsonValue> {
        match name {
            "count" | "get" | "insert" | "update" | "replace" | "queryStreamNext" | "queryPage"
            | "remove" => anyhow::bail!(ErrorMetadata::bad_request(
                "NoDbDuringImport",
                "Can't use database at import time"
            )),
            _ => anyhow::bail!(ErrorMetadata::bad_request(
                "NoSyscallDuringImport",
                format!("Syscall {name} unsupported at import time")
            )),
        }
    }
}

impl<RT: Runtime> JsEnvironment<RT> for AnalyzeEnvironment {
    type AsyncResolver = v8::Global<v8::PromiseResolver>;
    type SyscallProvider = Self;

    fn syscall_provider(&mut self) -> &mut Self::SyscallProvider {
        self
    }

    fn start_async_syscall(
        &mut self,
        name: String,
        _args: JsonValue,
        _resolver: Self::AsyncResolver,
    ) -> anyhow::Result<()> {
        anyhow::bail!(ErrorMetadata::bad_request(
            format!("No{}DuringImport", syscall_name_for_error(&name)),
            format!(
                "{} unsupported at import time",
                syscall_description_for_error(&name),
            ),
        ))
    }

    fn start_async_op(
        &mut self,
        request: AsyncOpRequest,
        _resolver: Self::AsyncResolver,
    ) -> anyhow::Result<()> {
        anyhow::bail!(ErrorMetadata::bad_request(
            format!("No{}DuringImport", request.name_for_error()),
            format!(
                "{} unsupported at import time",
                request.description_for_error()
            ),
        ))
    }

    fn user_timeout(&self) -> std::time::Duration {
        *ISOLATE_ANALYZE_USER_TIMEOUT
    }

    fn system_timeout(&self) -> std::time::Duration {
        // NB: System timeout isn't relevant for analyze, since we don't support
        // any async syscalls and don't pause the isolate's timeout.
        *DATABASE_UDF_SYSTEM_TIMEOUT
    }
}

impl AnalyzeEnvironment {
    #[fastrace::trace]
    pub async fn analyze<RT: Runtime>(
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        permit: ConcurrencyPermit,
        isolate_clean: &mut bool,
        udf_config: UdfConfig,
        modules: Arc<BTreeMap<CanonicalizedModulePath, Arc<V8ModuleSource>>>,
        to_analyze: CanonicalizedModulePath,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<Result<AnalyzedModule, JsError>> {
        anyhow::ensure!(!to_analyze.is_deps());
        let rng = ChaCha12Rng::from_seed(udf_config.import_phase_rng_seed);
        let unix_timestamp = udf_config.import_phase_unix_timestamp;
        let environment = AnalyzeEnvironment {
            modules,
            source_maps_cache: BTreeMap::new(),
            rng,
            unix_timestamp,
            environment_variables,
            collected_logs: VecDeque::new(),
        };
        let (handle, state, mut timeout) = isolate
            .start_request(context_cache, permit, environment)
            .await?;
        scope!(let handle_scope, isolate.isolate());
        let v8_context = context_cache.get_or_create_fresh_context(handle_scope);
        let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);
        let mut isolate_context = RequestScope::new(context_scope, handle.clone(), state, false)?;
        let handle = isolate_context.handle();
        let result = Self::run_analyze(&mut isolate_context, &mut timeout, to_analyze).await;

        // Perform a microtask checkpoint one last time before taking the environment
        // to ensure the microtask queue is empty. Otherwise, JS from this request may
        // leak to a subsequent one on isolate reuse.
        isolate_context.checkpoint();
        *isolate_clean = true;

        let error_logs = if let Ok(Err(_)) = result {
            let state = isolate_context.take_state().expect("Lost RequestState?");
            state.environment.collected_logs.clone()
        } else {
            VecDeque::new()
        };

        // Unlink the request from the isolate.
        // After this point, it's unsafe to run js code in the isolate that
        // expects the current request's environment.
        // If the microtask queue is somehow nonempty after this point but before
        // the next request starts, the isolate may panic.
        drop(isolate_context);
        drop(timeout);

        // Suppress the original error if the isolate was forcibly terminated.
        if let Err(e) = handle.take_termination_error("analyze")? {
            return Ok(Err(e));
        }

        if let Ok(Err(mut js_error)) = result {
            if !error_logs.is_empty() {
                let logs_text = error_logs.iter().cloned().collect::<Vec<_>>().join("\n");
                js_error.message = format!("{}\n\n{}", js_error.message, logs_text);
            }
            return Ok(Err(js_error));
        }

        result
    }

    fn get_source_map(
        &mut self,
        path: &CanonicalizedModulePath,
    ) -> anyhow::Result<&Option<sourcemap::SourceMap>> {
        match self.source_maps_cache.entry(path.clone()) {
            Entry::Occupied(e) => Ok(e.into_mut()),
            Entry::Vacant(e) => {
                let module_config = self
                    .modules
                    .get(path)
                    .context("could not find module config in environment")?;
                let source_map = module_config
                    .source_map()
                    .and_then(|m| source_map_from_slice(m.as_bytes()));

                // cache it
                Ok(e.insert(source_map))
            },
        }
    }

    async fn run_analyze<RT: Runtime>(
        isolate: &mut RequestScope<'_, '_, '_, RT, Self>,
        timeout: &mut Timeout<RT>,
        path: CanonicalizedModulePath,
    ) -> anyhow::Result<Result<AnalyzedModule, JsError>> {
        scope!(let v8_scope, isolate.scope());
        let mut scope = RequestScope::<RT, Self>::enter(v8_scope);

        // module_specifier is the key in the ModuleMap which we use to address the
        // ModuleId for this module. We then use this ModuleId to fetch the
        // v8::Module for evaluation.
        let module_specifier = module_specifier_from_path(&path)?;
        // Register the module and its dependencies with V8, instantiate the module, and
        // evaluate the module. After this, we can inspect the module's
        // in-memory objects to find functions which we can analyze as UDFs.
        // For more info on registration/instantiation see here: https://choubey.gitbook.io/internals-of-deno/import-and-ops/registration-and-instantiation
        let module: v8::Local<v8::Module> = match scope
            .eval_module(&module_specifier, timeout)
            .await
        {
            Ok(m) => m,
            Err(e) => {
                if e.is::<ModuleNotFoundError>()
                    || e.is::<ModuleResolutionError>()
                    || e.is::<SystemModuleNotFoundError>()
                    || e.is::<InvalidModulePathError>()
                {
                    return Ok(Err(JsError::from_message(format!("{e}"))));
                }
                match e.downcast::<JsError>() {
                    Ok(e) => {
                        return Ok(Err(JsError {
                            message: format!("Failed to analyze {}: {}", path.as_str(), e.message),
                            custom_data: None,
                            frames: e.frames,
                        }))
                    },
                    Err(e) => return Err(e),
                }
            },
        };

        // Gather UDFs, HTTP action routes, and crons
        let functions = match udf_analyze(&mut scope, &module, &path)? {
            Err(e) => return Ok(Err(e)),
            Ok(funcs) => WithHeapSize::from(funcs),
        };

        let mut http_routes = None;
        if path.is_http() {
            let routes = match http_analyze(&mut scope, &module, &path)? {
                Err(err) => {
                    return Ok(Err(err));
                },
                Ok(value) => value,
            };
            http_routes = Some(routes);
        }

        let mut cron_specs = None;
        if path.is_cron() {
            let crons = match cron_analyze(&mut scope, &module, &path)? {
                Err(err) => {
                    return Ok(Err(err));
                },
                Ok(value) => value,
            };
            cron_specs = Some(WithHeapSize::from(crons));
        }

        // Get source_index of current module
        let source_index = scope
            .state_mut()?
            .environment
            .get_source_map(&path)?
            .as_ref()
            .and_then(|source_map| {
                for (i, filename) in source_map.sources().enumerate() {
                    if Path::new(filename).file_stem()
                        != Path::new(module_specifier.path()).file_stem()
                    {
                        continue;
                    }

                    return source_map.get_source_contents(i as u32).map(|_| i as u32);
                }
                None
            });

        let reuse_context = module
            .get_module_namespace()
            .to_object(&scope)
            .context("Module namespace wasn't an object?")?
            .get(
                &scope,
                strings::experimental_reuseContext.create(&scope)?.into(),
            )
            .is_some_and(|value| value.is_true());

        Ok(Ok(AnalyzedModule {
            functions,
            http_routes,
            cron_specs,
            source_index,
            reuse_context,
        }))
    }
}

fn make_str_val<'s>(
    scope: &mut v8::PinScope<'s, '_, ()>,
    value: &str,
) -> anyhow::Result<v8::Local<'s, v8::Value>> {
    let v8_str_val: v8::Local<v8::Value> = v8::String::new(scope, value)
        .ok_or_else(|| anyhow!("Failed to create v8 string for {}", value))?
        .into();
    Ok(v8_str_val)
}

#[fastrace::trace]
fn parse_args_validator<'s, RT: Runtime>(
    scope: &mut ExecutionScope<RT, AnalyzeEnvironment>,
    function: v8::Local<v8::Object>,
    function_identifier_for_error: String,
) -> anyhow::Result<Result<ArgsValidator, JsError>> {
    // Call `exportArgs` to get the args validator.
    let export_args = strings::exportArgs.create(scope)?;

    let args = match function.get(scope, export_args.into()) {
        Some(export_args_value) if export_args_value.is_function() => {
            let export_args_function: v8::Local<v8::Function> = export_args_value.try_into()?;
            let result_v8 = scope
                .with_try_catch(|s| export_args_function.call(s, function.into(), &[]))??
                .context("Missing return value from successful function call")?;
            let result_v8_str = match v8::Local::<v8::String>::try_from(result_v8) {
                Ok(s) => s,
                Err(_) => {
                    let message = format!(
                        "Invalid exportArgs return value: \
                         {function_identifier_for_error}.exportArgs() didn't return a string."
                    );
                    return Ok(Err(JsError::from_message(message)));
                },
            };
            let result_str = helpers::to_rust_string(scope, &result_v8_str)?;
            match ArgsValidator::json_deserialize(&result_str) {
                Ok(validator) => validator,
                Err(parse_error) => {
                    let message = format!(
                        "Invalid JSON returned from {function_identifier_for_error}.exportArgs(): \
                         {parse_error}"
                    );
                    return Ok(Err(JsError::from_message(message)));
                },
            }
        },
        // `exportArgs` will be undefined if this is before npm
        // package v0.13.0. Default to `Unvalidated`.
        Some(export_args_value) if export_args_value.is_undefined() => ArgsValidator::Unvalidated,
        Some(_) => {
            let message = format!(
                "{function_identifier_for_error}.exportArgs is not a function or `undefined`."
            );
            return Ok(Err(JsError::from_message(message)));
        },
        None => ArgsValidator::Unvalidated,
    };
    Ok(Ok(args))
}

#[fastrace::trace]
fn parse_returns_validator<'s, RT: Runtime>(
    scope: &mut ExecutionScope<RT, AnalyzeEnvironment>,
    function: v8::Local<v8::Object>,
    function_identifier_for_error: String,
) -> anyhow::Result<Result<ReturnsValidator, JsError>> {
    // TODO(CX-6287) unify argument and returns validators
    // Call `exportReturns` to get the returns validator.
    let export_returns = strings::exportReturns.create(scope)?;
    let returns = match function.get(scope, export_returns.into()) {
        Some(export_returns_value) if export_returns_value.is_function() => {
            let export_returns_function: v8::Local<v8::Function> =
                export_returns_value.try_into()?;
            let result_v8 = scope
                .with_try_catch(|s| export_returns_function.call(s, function.into(), &[]))??
                .context("Missing return value from successful function call")?;

            let result_v8_str = match v8::Local::<v8::String>::try_from(result_v8) {
                Ok(s) => s,
                Err(_) => {
                    let message = format!(
                        "Invalid exportReturns return value: \
                         {function_identifier_for_error}.exportReturns() didn't return a string."
                    );
                    return Ok(Err(JsError::from_message(message)));
                },
            };
            let result_str = helpers::to_rust_string(scope, &result_v8_str)?;
            match ReturnsValidator::json_deserialize(&result_str) {
                Ok(validator) => validator,
                Err(parse_error) => {
                    let message = format!(
                        "Invalid JSON returned from \
                         {function_identifier_for_error}.exportReturns(): {parse_error}"
                    );
                    return Ok(Err(JsError::from_message(message)));
                },
            }
        },
        Some(export_output_value) if export_output_value.is_undefined() => {
            ReturnsValidator::Unvalidated
        },
        Some(_) => {
            let message = format!(
                "{function_identifier_for_error}.exportReturns is not a function or `undefined`."
            );
            return Ok(Err(JsError::from_message(message)));
        },
        None => ReturnsValidator::Unvalidated,
    };
    Ok(Ok(returns))
}
#[fastrace::trace]
fn udf_analyze<RT: Runtime>(
    scope: &mut ExecutionScope<RT, AnalyzeEnvironment>,
    module: &v8::Local<v8::Module>,
    module_path: &CanonicalizedModulePath,
) -> anyhow::Result<Result<Vec<AnalyzedFunction>, JsError>> {
    let namespace = module
        .get_module_namespace()
        .to_object(scope)
        .ok_or_else(|| anyhow!("Module namespace wasn't an object?"))?;
    let property_names = namespace
        .get_property_names(scope, GetPropertyNamesArgs::default())
        .ok_or_else(|| anyhow!("Failed to get module namespace property names"))?;

    // Iterate the properties and get the ones that are valid UDFs
    let mut functions = vec![];
    for i in 0..property_names.length() {
        let property_name = property_names
            .get_index(scope, i)
            .ok_or_else(|| anyhow!("Failed to get index {} on property names", i))?;
        let property_value = namespace
            .get(scope, property_name)
            .ok_or_else(|| anyhow!("Failed to get property name on module namespace"))?;
        let function: v8::Local<v8::Object> = match property_value.try_into() {
            Ok(f) => f,
            Err(_) => continue,
        };

        let property_name: v8::Local<v8::String> = property_name.try_into()?;
        let property_name = helpers::to_rust_string(scope, &property_name)?;

        let is_query_property = strings::isQuery.create(scope)?.into();
        let is_query: bool = function.has(scope, is_query_property).unwrap_or(false);

        let is_mutation_property = strings::isMutation.create(scope)?.into();
        let is_mutation: bool = function.has(scope, is_mutation_property).unwrap_or(false);

        let is_action_property = strings::isAction.create(scope)?.into();
        let is_action: bool = function.has(scope, is_action_property).unwrap_or(false);

        let udf_type = match (is_query, is_mutation, is_action) {
            (true, false, false) => UdfType::Query,
            (false, true, false) => UdfType::Mutation,
            (false, false, true) => UdfType::Action,
            _ => {
                tracing::debug!(
                    "Skipping function export that is not a mutation, query, or action: {} => \
                     ({is_query}, {is_mutation}, {is_action})",
                    property_name
                );
                continue;
            },
        };

        let is_public_property = strings::isPublic.create(scope)?.into();
        let is_public = function.has(scope, is_public_property).unwrap_or(false);

        let is_internal_property = strings::isInternal.create(scope)?.into();
        let is_internal = function.has(scope, is_internal_property).unwrap_or(false);

        let args =
            parse_args_validator(scope, function, format!("{module_path:?}:{property_name}"))??;

        let returns =
            parse_returns_validator(scope, function, format!("{module_path:?}:{property_name}"))??;

        let visibility = match (is_public, is_internal) {
            (true, false) => Some(Visibility::Public),
            (false, true) => Some(Visibility::Internal),
            (false, false) => None,
            (true, true) => {
                tracing::warn!(
                    "Skipping function export that is marked both public and internal: {}",
                    property_name
                );
                continue;
            },
        };

        let handler_str = strings::_handler.create(scope)?;
        let handler = match function.get(scope, handler_str.into()) {
            Some(handler_value) if handler_value.is_function() => {
                let handler: v8::Local<v8::Function> = handler_value.try_into()?;
                handler
            },
            Some(handler_value) if !handler_value.is_undefined() => {
                let message = format!("{module_path:?}:{property_name}.handler is not a function.");
                return Ok(Err(JsError::from_message(message)));
            },
            _ => match function.try_into() {
                Ok(f) => f,
                Err(_) => {
                    let message = format!("{module_path:?}:{property_name} is not a function.");
                    return Ok(Err(JsError::from_message(message)));
                },
            },
        };

        // These are originally zero-indexed, so we just add 1
        let lineno = handler
            .get_script_line_number()
            .ok_or_else(|| anyhow!("Failed to get function line number"))?
            + 1;
        let linecol = handler
            .get_script_column_number()
            .ok_or_else(|| anyhow!("Failed to get function column number"))?
            + 1;

        // Get the appropriate source map to look in
        let (fn_source_map, fn_canon_path) = {
            let resource_name_val = handler
                .get_script_origin(scope)
                .resource_name()
                .context("resource_name was None")?;
            let resource_name = resource_name_val.to_rust_string_lossy(scope);
            let resource_url = module_specifier_from_str(&resource_name)?;
            let canon_path = path_from_module_specifier(&resource_url)?;
            (
                scope.state_mut()?.environment.get_source_map(&canon_path)?,
                canon_path,
            )
        };

        let canonicalized_name: FunctionName = property_name
            .parse()
            .map_err(|e| invalid_function_name_error(module_path, &e))?;
        if let Some(Some(token)) = fn_source_map.as_ref().map(|sm| sm.lookup_token(lineno, linecol))
            // This condition is in place so that we don't have to jump to source in source mappings
            // to get back to the original source. This logic gets complicated and is not strictly necessary now
            && fn_canon_path.as_str() == module_path.as_str()
        {
            // Source map is valid; proceed with mapping in original source map
            functions.push(AnalyzedFunction::new(
                canonicalized_name.clone(),
                Some(AnalyzedSourcePosition {
                    path: fn_canon_path,
                    start_lineno: token.get_src_line(),
                    start_col: token.get_src_col(),
                }),
                udf_type,
                visibility.clone(),
                args.clone(),
                returns.clone(),
            )?);
        } else {
            // If there is no valid source map, push a function without a position
            functions.push(AnalyzedFunction::new(
                canonicalized_name.clone(),
                None,
                udf_type,
                visibility.clone(),
                args.clone(),
                returns.clone(),
            )?);

            // Log reason for fallback
            if fn_canon_path.as_str() != module_path.as_str() {
                log_source_map_origin_in_separate_module();
            } else if fn_source_map.is_none() {
                log_source_map_missing();
            } else {
                log_source_map_token_lookup_failed();
            }

            tracing::debug!(
                "Failed to resolve source position of {module_path:?}:{canonicalized_name}"
            );
        }
    }

    // Sort by line number where source position of None compares least
    functions.sort_by(|a, b| a.pos.cmp(&b.pos));

    Ok(Ok(functions))
}

/// The `convex/http.js` default export, must be an HTTP router. In addition to
/// normal module analysis, this module may contain a Vec of
/// `AnalyzedHttpRoute`s returned by `Router.getRoutes()` which is currently
/// used only by the dashboard for dispaying HTTP routes. These routes are
/// publicly accessible at domains like `https://happy-otter-123.convex.site`.
fn http_analyze<RT: Runtime>(
    scope: &mut ExecutionScope<RT, AnalyzeEnvironment>,
    module: &v8::Local<v8::Module>,
    module_path: &CanonicalizedModulePath,
) -> anyhow::Result<Result<AnalyzedHttpRoutes, JsError>> {
    let mut http_routes: Vec<AnalyzedHttpRoute> = vec![];

    let namespace = module
        .get_module_namespace()
        .to_object(scope)
        .ok_or_else(|| anyhow!("Module namespace wasn't an object?"))?;
    let property_names = namespace
        .get_property_names(scope, GetPropertyNamesArgs::default())
        .ok_or_else(|| anyhow!("Failed to get module namespace property names"))?;

    let mut default_property_name: Option<v8::Local<v8::Value>> = None;
    for i in 0..property_names.length() {
        let property_name_v8 = property_names
            .get_index(scope, i)
            .ok_or_else(|| anyhow!("Failed to get index {} on property names", i))?;
        let property_name: v8::Local<v8::String> = property_name_v8.try_into()?;
        let property_name = helpers::to_rust_string(scope, &property_name)?;
        if property_name == "default" {
            default_property_name = Some(property_name_v8);
        }
    }
    if default_property_name.is_none() {
        let message = "`convex/http.js` must have a default export of a Router.".to_string();
        return Ok(Err(JsError::from_message(message)));
    }
    let default_property_name = default_property_name.expect("no default property name");
    let router_value: v8::Local<v8::Value> = namespace
        .get(scope, default_property_name)
        .ok_or_else(|| anyhow!("Failed to get property name on module namespace"))?;

    let Some(router) = router_value.to_object(scope) else {
        let message = "The default export of `convex/http.js` is not a Router.".to_string();
        return Ok(Err(JsError::from_message(message)));
    };

    let is_router_str = make_str_val(scope, "isRouter")?;
    let get_routes_str = make_str_val(scope, "getRoutes")?;
    let length_str = make_str_val(scope, "length")?;

    let mut is_router = false;
    if let Some(true) = router.has(scope, is_router_str) {
        is_router = router
            .get(scope, is_router_str)
            .ok_or_else(|| anyhow!("Missing `isRouter`"))?
            .is_true();
    }

    if !is_router {
        let message = "The default export of `convex/http.js` is not a Router.".to_string();
        return Ok(Err(JsError::from_message(message)));
    }

    let get_routes = match router.get(scope, get_routes_str) {
        Some(get_routes) => {
            let get_routes: Result<v8::Local<v8::Function>, _> = get_routes.try_into();
            match get_routes {
                Ok(get_routes) => get_routes,
                Err(_) => {
                    let message = ".getRoutes property on Router not found".to_string();
                    return Ok(Err(JsError::from_message(message)));
                },
            }
        },
        None => {
            let message = ".get_routes of Router is not a function".to_string();
            return Ok(Err(JsError::from_message(message)));
        },
    };

    let global = scope.get_current_context().global(scope);

    // function get_routes(): [path: string, method: string, handler:
    // HttpAction][]
    let routes_arr = match get_routes.call(scope, global.into(), &[]) {
        Some(routes_arr) => {
            let routes_arr: Result<v8::Local<v8::Object>, _> = routes_arr.try_into();
            match routes_arr {
                Ok(routes_arr) => routes_arr,
                Err(_) => {
                    return routes_error("return value is not an array");
                },
            }
        },
        None => {
            return routes_error("no value returned");
        },
    };

    let Some(len): Option<v8::Local<v8::Value>> = routes_arr.get(scope, length_str) else {
        return routes_error("return value is not an array");
    };
    let len = len
        .int32_value(scope)
        .expect("length could not be converted to i32")
        .try_into()
        .expect("length could not be converted to u32");

    for i in 0..len {
        let Some(entry) = routes_arr.get_index(scope, i) else {
            return routes_error(format!("problem with arr[{i}]").as_str());
        };
        let Some(entry) = entry.to_object(scope) else {
            return routes_error(format!("arr[{i}] is not an object").as_str());
        };

        let Some(path) = entry.get_index(scope, 0) else {
            return routes_error(format!("problem with arr[{i}][0]").as_str());
        };
        let path: Result<v8::Local<v8::String>, _> = path.try_into();
        let Ok(path) = path else {
            return routes_error(format!("arr[{i}][0] is not a string").as_str());
        };
        let path: String = path.to_rust_string_lossy(scope);

        let Some(method) = entry.get_index(scope, 1) else {
            return routes_error(format!("problem with arr[{i}][1]").as_str());
        };
        let method: Result<v8::Local<v8::String>, _> = method.try_into();
        let Ok(method) = method else {
            return routes_error(format!("arr[{i}][1] is not a string").as_str());
        };
        let method: String = method.to_rust_string_lossy(scope);

        let Ok(method): Result<RoutableMethod, _> = method.parse() else {
            return routes_error(
                format!(
                    "'{method}' is not not a routable method (one of GET, POST, PUT, DELETE, \
                     PATCH, OPTIONS)"
                )
                .as_str(),
            );
        };

        let Some(function) = entry.get_index(scope, 2) else {
            return routes_error(format!("problem with third element of {i} of array").as_str());
        };
        let function: Result<v8::Local<v8::Object>, _> = function.try_into();
        let Ok(function) = function else {
            return routes_error(format!("arr[{i}][2] not an HttpAction").as_str());
        };

        let handler_str = strings::_handler.create(scope)?;
        let handler = match function.get(scope, handler_str.into()) {
            Some(handler_value) if handler_value.is_function() => {
                let handler: v8::Local<v8::Function> = handler_value.try_into()?;
                handler
            },
            Some(handler_value) if !handler_value.is_undefined() => {
                let message = format!("arr[{i}][2].handler is not a function");
                return Ok(Err(JsError::from_message(message)));
            },
            _ => match function.try_into() {
                Ok(f) => f,
                Err(_) => {
                    let message = format!("arr[{i}][2] is not an HttpAction");
                    return Ok(Err(JsError::from_message(message)));
                },
            },
        };

        // These are originally zero-indexed, so we just add 1
        let lineno = handler
            .get_script_line_number()
            .ok_or_else(|| anyhow!("Failed to get function line number"))?
            + 1;
        let linecol = handler
            .get_script_column_number()
            .ok_or_else(|| anyhow!("Failed to get function column number"))?
            + 1;

        // Get the appropriate source map to look in
        let (fn_source_map, fn_canon_path) = {
            let resource_name_val = handler
                .get_script_origin(scope)
                .resource_name()
                .context("resource_name was None")?;
            let resource_name = resource_name_val.to_rust_string_lossy(scope);
            let resource_url = module_specifier_from_str(&resource_name)?;
            let canon_path = path_from_module_specifier(&resource_url)?;
            let source_map = scope.state_mut()?.environment.get_source_map(&canon_path)?;
            (source_map, canon_path)
        };

        let source_pos = fn_source_map
            .as_ref()
            .and_then(|sm| sm.lookup_token(lineno, linecol))
            .and_then(|token| {
                if fn_canon_path.as_str() == module_path.as_str() {
                    Some(AnalyzedSourcePosition {
                        path: fn_canon_path,
                        start_lineno: token.get_src_line(),
                        start_col: token.get_src_col(),
                    })
                } else {
                    None
                }
            });
        if source_pos.is_none() {
            tracing::warn!("Failed to resolve {module_path:?}:{path}");
        }
        http_routes.push(AnalyzedHttpRoute {
            route: HttpActionRoute {
                path: path.clone(),
                method,
                matched: true,
            },
            pos: source_pos,
        });
    }

    // Sort by line number where source position of None compares least
    http_routes.sort_by(|a, b| a.pos.cmp(&b.pos));
    let http_routes = AnalyzedHttpRoutes::new(http_routes);
    Ok(Ok(http_routes))
}

fn routes_error<OKType>(specific_error: &str) -> anyhow::Result<Result<OKType, JsError>> {
    let message = format!(
        "The `getRoutes()` method of Router did not return the expected type. `getRoutes()` \
         should be a function returning an array of entries of the form [path: string, method: \
         string, handler: HttpAction] ({specific_error})",
    );
    Ok(Err(JsError::from_message(message)))
}

/// The `convex/cron.js` default export must be a Crons object.
fn cron_analyze<RT: Runtime>(
    scope: &mut ExecutionScope<RT, AnalyzeEnvironment>,
    module: &v8::Local<v8::Module>,
    _module_path: &CanonicalizedModulePath,
) -> anyhow::Result<Result<BTreeMap<CronIdentifier, CronSpec>, JsError>> {
    let namespace = module
        .get_module_namespace()
        .to_object(scope)
        .ok_or_else(|| anyhow!("Module namespace wasn't an object?"))?;
    let property_names = namespace
        .get_property_names(scope, GetPropertyNamesArgs::default())
        .ok_or_else(|| anyhow!("Failed to get module namespace property names"))?;

    let mut default_property_name: Option<v8::Local<v8::Value>> = None;
    for i in 0..property_names.length() {
        let property_name_v8 = property_names
            .get_index(scope, i)
            .ok_or_else(|| anyhow!("Failed to get index {} on property names", i))?;
        let property_name: v8::Local<v8::String> = property_name_v8.try_into()?;
        let property_name = helpers::to_rust_string(scope, &property_name)?;
        if property_name == "default" {
            default_property_name = Some(property_name_v8);
        }
    }
    if default_property_name.is_none() {
        let message = "`convex/crons.js` must have a default export of a Crons object.".to_string();
        return Ok(Err(JsError::from_message(message)));
    }
    let default_property_name = default_property_name.expect("no default property name");
    let crons_value: v8::Local<v8::Value> = namespace
        .get(scope, default_property_name)
        .ok_or_else(|| anyhow!("Failed to get property name on module namespace"))?;

    let Some(crons) = crons_value.to_object(scope) else {
        let message = "The default export of `convex/cron.js` is not a Router.".to_string();
        return Ok(Err(JsError::from_message(message)));
    };

    let is_crons_str = make_str_val(scope, "isCrons")?;
    let export_str = make_str_val(scope, "export")?;

    let mut is_crons = false;
    if let Some(true) = crons.has(scope, is_crons_str) {
        is_crons = crons
            .get(scope, is_crons_str)
            .ok_or_else(|| anyhow!("Missing `isCrons`"))?
            .is_true();
    }

    if !is_crons {
        let message = "The default export of `convex/crons.js` is not a Crons object.".to_string();
        return Ok(Err(JsError::from_message(message)));
    }

    let export_function = match crons.get(scope, export_str) {
        Some(export) => {
            let export: Result<v8::Local<v8::Function>, _> = export.try_into();
            match export {
                Ok(export) => export,
                Err(_) => {
                    let message = ".export property on Crons object not found".to_string();
                    return Ok(Err(JsError::from_message(message)));
                },
            }
        },
        None => {
            let message = ".export of Crons object is not a function".to_string();
            return Ok(Err(JsError::from_message(message)));
        },
    };

    let result_v8 = match export_function.call(scope, crons.into(), &[]) {
        Some(r) => v8::Local::<v8::String>::try_from(r)?,
        None => anyhow::bail!("Missing return value from successful function call"),
    };
    let result_str = helpers::to_rust_string(scope, &result_v8)?;
    let export_json: BTreeMap<String, JsonValue> = serde_json::from_str(&result_str)?;

    let export_json = export_json;

    let mut cron_specs = BTreeMap::new();

    for (k, v) in export_json {
        let (identifier, cronspec) = match (k.parse(), CronSpec::from_exported_json(v)) {
            (Ok(k), Ok(v)) => (k, v),
            (Err(e), _) | (_, Err(e)) => {
                let msg = e.to_string();
                anyhow::bail!(e.context(ErrorMetadata::bad_request("InvalidCron", msg)))
            },
        };
        cron_specs.insert(identifier, cronspec);
    }

    Ok(Ok(cron_specs))
}
