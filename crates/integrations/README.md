## Integrations

`LogManager` routes and batches log events for configured integrations. Each
integration starts an `IntegrationClient` that receives events from the manager.
Integration configuration and lifecycle state live in the `_log_sinks` system
table, through `model::integrations::IntegrationsModel`.

### Adding an integration

1. Add its configuration and provider variant to `IntegrationConfig` and
   `IntegrationType` in `crates/model/src/integrations/types/`.
2. Implement its `start` method in `integrations/`, returning an
   `IntegrationClient`. See `integrations/datadog.rs` for an example.
3. Extend the creation, update, and response types in
   `crates/local_backend/src/integrations.rs`. The `/api/v1/integrations`
   endpoints share their implementation with the legacy log stream endpoints.
4. Add the startup branch to `LogManager::config_to_integration_client`.

The S3 export integration uses its own worker in `application::s3_export_worker`
to export documents; log streaming integrations receive events from
`LogManager`.
