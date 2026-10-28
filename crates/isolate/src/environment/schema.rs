use std::{
    sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use common::{
    json::JsonForm as _,
    knobs::{
        DATABASE_UDF_SYSTEM_TIMEOUT,
        DATABASE_UDF_USER_TIMEOUT,
    },
    log_lines::LogLevel,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    schemas::{
        invalid_schema_export_error,
        missing_schema_export_error,
        DatabaseSchema,
    },
    try_anyhow,
};
use deno_core::{
    v8::{
        self,
        scope,
    },
    ModuleSpecifier,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use model::{
    environment_variables::types::{
        EnvVarName,
        EnvVarValue,
    },
    modules::module_versions::{
        FullModuleSource,
        ModuleSource,
        SourceMap,
    },
};
use rand::SeedableRng;
use rand_chacha::ChaCha12Rng;
use serde_json::Value as JsonValue;
use value::NamespacedTableMapping;

use super::ModuleCodeCacheResult;
use crate::{
    context_cache::ContextCache,
    environment::{
        helpers::syscall_error::{
            syscall_description_for_error,
            syscall_name_for_error,
        },
        AsyncOpRequest,
        JsEnvironment,
        OpProvider,
        SyscallProvider,
    },
    helpers,
    isolate::{
        Isolate,
        CONVEX_SCHEME,
    },
    module_cache::V8ModuleSource,
    request_scope::RequestScope,
    strings,
    timeout::Timeout,
    ConcurrencyPermit,
};

pub struct SchemaEnvironment {
    schema_bundle: ModuleSource,
    source_map: Option<SourceMap>,
    rng: ChaCha12Rng,
    unix_timestamp: UnixTimestamp,
}

impl OpProvider for SchemaEnvironment {
    fn trace(&mut self, _level: LogLevel, messages: Vec<String>) -> anyhow::Result<()> {
        tracing::warn!(
            "Unexpected Console access at schema evaluation time: {}",
            messages.join(" ")
        );
        Ok(())
    }

    fn rng(&mut self) -> anyhow::Result<&mut ChaCha12Rng> {
        Ok(&mut self.rng)
    }

    fn crypto_rng(&mut self) -> anyhow::Result<super::crypto_rng::CryptoRng> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoCryptoRngInSchema",
            "Cannot use cryptographic randomness when evaluating schema"
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
        _name: EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoEnvironmentVariablesInSchema",
            "Environment variables unsupported when evaluating schema"
        ))
    }

    fn get_all_table_mappings(&mut self) -> anyhow::Result<NamespacedTableMapping> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoTableMappingFetchInSchema",
            "Getting the table mapping unsupported when evaluating schema"
        ))
    }
}

impl<RT: Runtime> SyscallProvider<RT> for SchemaEnvironment {
    async fn lookup_source(
        &mut self,
        path: &str,
        _timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<Option<(Arc<V8ModuleSource>, ModuleCodeCacheResult)>> {
        if path != "schema.js" {
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoImportModuleInSchema",
                format!("Can't import {path} while evaluating schema")
            ))
        }
        Ok(Some((
            Arc::new(V8ModuleSource::new(FullModuleSource {
                source: self.schema_bundle.clone(),
                source_map: self.source_map.clone(),
            })),
            ModuleCodeCacheResult::noop(),
        )))
    }

    fn syscall(&mut self, name: &str, _args: JsonValue) -> anyhow::Result<JsonValue> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoSyscallInSchema",
            format!("Syscall {name} unsupported when evaluating schema")
        ));
    }
}

impl<RT: Runtime> JsEnvironment<RT> for SchemaEnvironment {
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
            format!("No{}InSchema", syscall_name_for_error(&name)),
            format!(
                "{} unsupported while evaluating schema",
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
            format!("No{}InSchema", request.name_for_error()),
            format!(
                "{} unsupported while evaluating schema",
                request.description_for_error()
            ),
        ))
    }

    fn user_timeout(&self) -> std::time::Duration {
        *DATABASE_UDF_USER_TIMEOUT
    }

    fn system_timeout(&self) -> std::time::Duration {
        *DATABASE_UDF_SYSTEM_TIMEOUT
    }
}

impl SchemaEnvironment {
    pub async fn evaluate_schema<RT: Runtime>(
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        permit: ConcurrencyPermit,
        schema_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        rng_seed: [u8; 32],
        unix_timestamp: UnixTimestamp,
    ) -> anyhow::Result<DatabaseSchema> {
        let rng = ChaCha12Rng::from_seed(rng_seed);
        let environment = Self {
            schema_bundle,
            source_map,
            rng,
            unix_timestamp,
        };
        let (handle, state, mut timeout) = isolate
            .start_request(context_cache, permit, environment)
            .await?;
        scope!(let handle_scope, isolate.isolate());
        let v8_context = context_cache.get_or_create_fresh_context(handle_scope);
        let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);
        let mut isolate_context = RequestScope::new(context_scope, handle.clone(), state, false)?;
        let handle = isolate_context.handle();
        let result = Self::run_evaluate_schema(&mut isolate_context, &mut timeout).await;

        // Drain the microtask queue, to clean up the isolate.
        isolate_context.checkpoint();

        // Unlink the request from the isolate.
        // After this point, it's unsafe to run js code in the isolate that
        // expects the current request's environment.
        // If the microtask queue is somehow nonempty after this point but before
        // the next request starts, the isolate may panic.
        drop(isolate_context);
        drop(timeout);

        handle.take_termination_error("schema")??;
        result
    }

    async fn run_evaluate_schema<RT: Runtime>(
        isolate: &mut RequestScope<'_, '_, '_, RT, Self>,
        timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<DatabaseSchema> {
        scope!(let v8_scope, isolate.scope());
        let mut scope = RequestScope::<RT, Self>::enter(v8_scope);

        let schema_url = ModuleSpecifier::parse(&format!("{CONVEX_SCHEME}:/schema.js"))?;
        let module = scope.eval_module(&schema_url, timeout).await?;
        let namespace = module
            .get_module_namespace()
            .to_object(&scope)
            .ok_or_else(|| anyhow!("Module namespace wasn't an object?"))?;
        let default_str = strings::default.create(&scope)?;
        let schema_val: v8::Local<v8::Value> = namespace
            .get(&scope, default_str.into())
            .ok_or_else(missing_schema_export_error)?;
        if schema_val.is_null_or_undefined() {
            anyhow::bail!(missing_schema_export_error());
        }
        let export_str = strings::export.create(&scope)?;
        let v8_schema_result: anyhow::Result<v8::Local<v8::String>> = try_anyhow!({
            let schema_obj: v8::Local<v8::Object> = schema_val.try_into()?;
            let export_function: v8::Local<v8::Function> = schema_obj
                .get(&scope, export_str.into())
                .ok_or_else(|| anyhow!("Couldn't find 'export' method on schema object"))?
                .try_into()?;

            match scope.with_try_catch(|s| export_function.call(s, schema_obj.into(), &[]))?? {
                Some(r) => Ok(v8::Local::<v8::String>::try_from(r)?),
                None => Err(anyhow!(
                    "Missing return value from successful function call"
                )),
            }?
        });
        // If we can't export the schema into a string, probably there is
        // something funky in their `schema.ts` file, so throw
        // `invalid_schema_export_error`
        let v8_schema_str: v8::Local<v8::String> =
            v8_schema_result.map_err(|_| invalid_schema_export_error())?;

        let result_str = helpers::to_rust_string(&scope, &v8_schema_str)?;
        DatabaseSchema::json_deserialize(&result_str).map_err(|e| {
            if e.is_bad_request() {
                e
            } else {
                let msg = e.to_string();
                e.context(ErrorMetadata::bad_request("InvalidSchema", msg))
            }
        })
    }
}
