use std::{
    collections::BTreeMap,
    sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use common::{
    auth::{
        AuthConfig,
        SerializedAuthConfig,
    },
    knobs::{
        DATABASE_UDF_SYSTEM_TIMEOUT,
        DATABASE_UDF_USER_TIMEOUT,
    },
    log_lines::LogLevel,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
};
use deno_core::{
    v8::{
        self,
        scope,
    },
    ModuleSpecifier,
};
use errors::ErrorMetadata;
use model::{
    config::types::AUTH_CONFIG_FILE_NAME,
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
use rand_chacha::ChaCha12Rng;
use regex::Regex;
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

pub struct AuthConfigEnvironment {
    auth_config_bundle: ModuleSource,
    source_map: Option<SourceMap>,
    environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
}

impl OpProvider for AuthConfigEnvironment {
    fn trace(&mut self, _level: LogLevel, messages: Vec<String>) -> anyhow::Result<()> {
        tracing::warn!(
            "Unexpected Console access when evaluating auth config file: {}",
            messages.join(" ")
        );
        Ok(())
    }

    fn rng(&mut self) -> anyhow::Result<&mut ChaCha12Rng> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoRandomDuringAuthConfig",
            "Math.random unsupported when evaluating auth config file"
        ))
    }

    fn crypto_rng(&mut self) -> anyhow::Result<super::crypto_rng::CryptoRng> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoCryptoRngDuringAuthConfig",
            "Cannot use cryptographic randomness when evaluating auth config file"
        ))
    }

    fn unix_timestamp(&mut self) -> anyhow::Result<UnixTimestamp> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoDateDuringAuthConfig",
            "Date unsupported when evaluating auth config file"
        ))
    }

    fn performance_now(&mut self) -> anyhow::Result<Duration> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoPerformanceDuringAuthConfig",
            "The Performance API is not supported when evaluating auth config file"
        ))
    }

    fn performance_time_origin(&mut self) -> anyhow::Result<UnixTimestamp> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoPerformanceDuringAuthConfig",
            "The Performance API is not supported when evaluating auth config file"
        ))
    }

    fn get_environment_variable(
        &mut self,
        name: EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        self.environment_variables
            .get(&name)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(ErrorMetadata::bad_request(
                    // Special cased in Convex CLI!!!
                    "AuthConfigMissingEnvironmentVariable",
                    format!(
                        "Environment variable {name} is used in auth config file but its value \
                         was not set"
                    ),
                ))
            })
            .map(Some)
    }

    fn get_all_table_mappings(&mut self) -> anyhow::Result<NamespacedTableMapping> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoTableMappingFetchDuringAuthConfig",
            "Getting the table mapping unsupported when evaluating auth config file"
        ))
    }
}

impl<RT: Runtime> SyscallProvider<RT> for AuthConfigEnvironment {
    async fn lookup_source(
        &mut self,
        path: &str,
        _timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<Option<(Arc<V8ModuleSource>, ModuleCodeCacheResult)>> {
        if path != AUTH_CONFIG_FILE_NAME {
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoImportModuleDuringAuthConfig",
                format!("Can't import {path} while evaluating auth config file")
            ))
        }
        Ok(Some((
            Arc::new(V8ModuleSource::new(FullModuleSource {
                source: self.auth_config_bundle.clone(),
                source_map: self.source_map.clone(),
            })),
            ModuleCodeCacheResult::noop(),
        )))
    }

    fn syscall(&mut self, name: &str, _args: JsonValue) -> anyhow::Result<JsonValue> {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoSyscallDuringAuthConfig",
            format!("Syscall {name} unsupported when evaluating auth config file")
        ))
    }
}

impl<RT: Runtime> JsEnvironment<RT> for AuthConfigEnvironment {
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
            format!("No{}DuringAuthConfig", syscall_name_for_error(&name)),
            format!(
                "{} unsupported while evaluating auth config file",
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
            format!("No{}DuringAuthConfig", request.name_for_error()),
            format!(
                "{} unsupported while evaluating auth config file",
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

impl AuthConfigEnvironment {
    pub async fn evaluate_auth_config<RT: Runtime>(
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        permit: ConcurrencyPermit,
        auth_config_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<AuthConfig> {
        let environment = Self {
            auth_config_bundle,
            source_map,
            environment_variables,
        };
        let (handle, state, mut timeout) = isolate
            .start_request(context_cache, permit, environment)
            .await?;
        scope!(let handle_scope, isolate.isolate());
        let v8_context = context_cache.get_or_create_fresh_context(handle_scope);
        let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);
        let mut isolate_context = RequestScope::new(context_scope, handle.clone(), state, false)?;
        let handle = isolate_context.handle();
        let result = Self::run_evaluate_auth_config(&mut isolate_context, &mut timeout).await;

        // Drain the microtask queue, to clean up the isolate.
        isolate_context.checkpoint();

        // Unlink the request from the isolate.
        // After this point, it's unsafe to run js code in the isolate that
        // expects the current request's environment.
        // If the microtask queue is somehow nonempty after this point but before
        // the next request starts, the isolate may panic.
        drop(isolate_context);
        drop(timeout);

        handle.take_termination_error("auth")??;
        result
    }

    async fn run_evaluate_auth_config<RT: Runtime>(
        isolate: &mut RequestScope<'_, '_, '_, RT, Self>,
        timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<AuthConfig> {
        scope!(let v8_scope, isolate.scope());
        let mut scope = RequestScope::<RT, Self>::enter(v8_scope);

        let auth_config_url = ModuleSpecifier::parse(&format!("{CONVEX_SCHEME}:/auth.config.js"))?;
        let module = scope.eval_module(&auth_config_url, timeout).await?;
        let namespace = module
            .get_module_namespace()
            .to_object(&scope)
            .ok_or_else(|| anyhow!("Module namespace wasn't an object?"))?;
        let default_str = strings::default.create(&scope)?;
        let config_val: v8::Local<v8::Value> = namespace
            .get(&scope, default_str.into())
            .ok_or_else(missing_export_error)?;
        if config_val.is_null_or_undefined() {
            anyhow::bail!(missing_export_error());
        }

        let config_v8_str = v8::json::stringify(&scope, config_val).ok_or_else(|| {
            ErrorMetadata::bad_request(
                "AuthConfigUnserializableError",
                format!("auth config file can only contain strings {SEE_AUTH_DOCS}"),
            )
        })?;
        let config_str = helpers::to_rust_string(&scope, &config_v8_str)?;

        // Custom errors for misconfigured `convex/auth.config.ts` files that
        // are helpful because we allow extra properties in the
        // authoritative deserialization.
        check_for_common_confusions(&config_str)?;

        let config: AuthConfig = serde_json::from_str::<SerializedAuthConfig>(&config_str)
            .map_err(|error| config_not_matching_schema_error(strip_position(&error.to_string())))?
            .try_into()?;
        Ok(config)
    }
}

fn check_for_common_confusions(config_str: &str) -> anyhow::Result<()> {
    let raw_config: JsonValue = serde_json::from_str(config_str)
        .map_err(|error| config_not_matching_schema_error(strip_position(&error.to_string())))?;

    if let JsonValue::Object(ref config_obj) = raw_config
        && let Some(JsonValue::Array(providers)) = config_obj.get("providers")
    {
        for (index, config_obj) in providers.iter().enumerate() {
            if let JsonValue::Object(obj) = config_obj {
                let has_domain = obj.contains_key("domain");
                let has_issuer = obj.contains_key("issuer");
                let issuer = obj.get("issuer").and_then(|v| v.as_str());
                let has_bad_application_id =
                    obj.contains_key("applicationId") || obj.contains_key("applicationid");
                let has_application_id = obj.contains_key("applicationID");
                let type_value = obj
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if has_bad_application_id {
                    anyhow::bail!(config_not_matching_schema_error(format!(
                        "Provider at index {index} must have applicationID property spelled \
                         lowercase 'application', capital I, capital D."
                    )));
                }
                if type_value != "customJwt" && type_value != "oidc" && type_value != "unknown" {
                    anyhow::bail!(config_not_matching_schema_error(format!(
                        "Provider at index {index} has unexpected 'type' value '{type_value}'"
                    )));
                }

                if type_value == "customJwt" && has_domain {
                    anyhow::bail!(config_not_matching_schema_error(format!(
                        "Provider at index {index} is a customJwt so cannot have a 'domain' \
                         specified",
                    )));
                }

                let is_oidc = type_value == "oidc" || type_value == "unknown";
                if is_oidc && has_issuer {
                    anyhow::bail!(config_not_matching_schema_error(format!(
                        "Provider at index {index} is oidc so cannot have an 'issuer' specified.",
                    )));
                }

                if !has_application_id
                    && (issuer == Some("https://api.workos.com/")
                        || issuer == Some("https://api.workos.com"))
                {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InsecureConfiguration",
                        format!(
                            "This auth configuration appears potentially insecure: Provider at \
                             index {index} has an issuer that is shared among many applications, \
                             so must specify an ApplicationID to check against an `aud` field of \
                             a JWT.",
                        ),
                    ));
                }
            }
        }
    }

    Ok(())
}

// It's not meaningful for the user to see the serialized
// position in the result of the auth config execution
fn strip_position(error_message: &str) -> String {
    let re = Regex::new(r"at line \d+ column \d+$").unwrap();
    re.replace(error_message, "").to_string()
}

const SEE_AUTH_DOCS: &str =
    "To learn more, see the auth documentation at https://docs.convex.dev/auth.";

pub fn missing_export_error() -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "AuthConfigMissingExportError",
        format!("auth config file is missing default export. {SEE_AUTH_DOCS}"),
    )
}

pub fn config_not_matching_schema_error(error: String) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "AuthConfigNotMatchingSchemaError",
        format!("auth config file must include a list of provider credentials: {error}"),
    )
}
