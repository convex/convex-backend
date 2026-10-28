use fivetran_common::{
    config::Config,
    fivetran_sdk::{
        schema_response,
        source_connector_server::SourceConnector,
        test_response,
        update_response,
        ConfigurationFormRequest,
        ConfigurationFormResponse,
        ConfigurationTest,
        SchemaRequest,
        SchemaResponse,
        Task,
        TestRequest,
        TestResponse,
        UpdateRequest,
        UpdateResponse,
        UpdateResponse as FivetranUpdateResponse,
    },
};
use futures::{
    stream::BoxStream,
    StreamExt,
};
use tonic::{
    Request,
    Response,
    Status,
};

use crate::{
    conversions::selection::{
        selection_from_fivetran,
        DEFAULT_FIVETRAN_SCHEMA_NAME,
    },
    convex_api::{
        ComponentPath,
        ConvexApi,
        Source,
    },
    log::log,
    schema::generate_fivetran_schema,
    sync::{
        sync,
        State,
        UpdateMessage,
    },
};

/// Implements the gRPC server endpoints used by Fivetran.
#[derive(Debug)]
pub struct ConvexConnector;

type ConnectorResult<T> = Result<Response<T>, Status>;

impl ConvexConnector {
    async fn _schema(&self, request: Request<SchemaRequest>) -> anyhow::Result<SchemaResponse> {
        let config = Config::from_parameters(request.into_inner().configuration)?;
        log(&format!("schema request for {}", config.deploy_url));

        let source = ConvexApi { config };

        let tables_by_component = source.get_table_column_names().await?;

        if tables_by_component
            .contains_key(&ComponentPath(DEFAULT_FIVETRAN_SCHEMA_NAME.to_string()))
        {
            anyhow::bail!(
                "Your Convex deployment contains a component named \
                 `{DEFAULT_FIVETRAN_SCHEMA_NAME}`. This conflicts with the name of the default \
                 Fivetran schema. Please rename the component to avoid issues.",
            );
        }

        Ok(SchemaResponse {
            response: Some(schema_response::Response::WithSchema(
                generate_fivetran_schema(tables_by_component),
            )),
            selection_not_supported: Some(false),
        })
    }
}

#[tonic::async_trait]
impl SourceConnector for ConvexConnector {
    type UpdateStream = BoxStream<'static, Result<UpdateResponse, Status>>;

    async fn configuration_form(
        &self,
        _: Request<ConfigurationFormRequest>,
    ) -> ConnectorResult<ConfigurationFormResponse> {
        log("configuration form request");
        Ok(Response::new(ConfigurationFormResponse {
            schema_selection_supported: true,
            table_selection_supported: true,
            fields: Config::fivetran_fields(),
            tests: vec![ConfigurationTest {
                name: "connection".to_string(),
                label: "Test connection".to_string(),
            }],
        }))
    }

    async fn test(&self, request: Request<TestRequest>) -> ConnectorResult<TestResponse> {
        log("test request");
        let config = match Config::from_parameters(request.into_inner().configuration) {
            Ok(config) => config,
            Err(error) => {
                return Ok(Response::new(TestResponse {
                    response: Some(test_response::Response::Failure(error.to_string())),
                }));
            },
        };
        log(&format!("test request for {}", config.deploy_url));
        let source = ConvexApi { config };

        // Perform an API request to verify if the credentials work
        match source.test_streaming_export_connection().await {
            Ok(_) => Ok(Response::new(TestResponse {
                response: Some(test_response::Response::Success(true)),
            })),
            Err(e) => Ok(Response::new(TestResponse {
                response: Some(test_response::Response::Failure(e.to_string())),
            })),
        }
    }

    async fn schema(&self, request: Request<SchemaRequest>) -> ConnectorResult<SchemaResponse> {
        log("schema request");
        self._schema(request)
            .await
            .map(Response::new)
            .map_err(|error| Status::internal(error.to_string()))
    }

    async fn update(&self, request: Request<UpdateRequest>) -> ConnectorResult<Self::UpdateStream> {
        log("update request");
        let inner = request.into_inner();
        let config = match Config::from_parameters(inner.configuration) {
            Ok(config) => config,
            Err(error) => {
                return error_to_task(error);
            },
        };
        log(&format!("update request for {}", config.deploy_url));

        let state = match deserialize_state_json(inner.state_json.as_deref().unwrap_or("{}")) {
            Ok(state) => state,
            Err(error) => {
                return error_to_task(error);
            },
        };

        let source = ConvexApi { config };

        let selection = match selection_from_fivetran(inner.selection) {
            Ok(selection) => selection,
            Err(error) => {
                return error_to_task(error);
            },
        };

        let sync = sync(source, state, selection);
        Ok(Response::new(sync_stream_to_update_stream(sync)))
    }
}

/// Converts the sync stream into the gRPC response stream, reporting errors as
/// `Task` operations instead of gRPC errors so that Fivetran surfaces their
/// message to the customer on the dashboard instead of a generic sync failure.
fn sync_stream_to_update_stream(
    sync: BoxStream<'static, anyhow::Result<UpdateMessage>>,
) -> <ConvexConnector as SourceConnector>::UpdateStream {
    sync.map(|result| Ok(result.map_or_else(task_response, FivetranUpdateResponse::from)))
        .boxed()
}

fn deserialize_state_json(state_json: &str) -> anyhow::Result<Option<State>> {
    // Deserialize to a serde_json::Value first
    let state: serde_json::Value = serde_json::from_str(state_json)?;

    // Special case {} - which means we're initializing from fresh state
    let state = if state == serde_json::json!({}) {
        None
    } else {
        let deserialized: State = serde_json::from_value(state)?;

        // Version 2 of the Fivetran Convex connector started supporting components.
        // This is a breaking change, since tables would previously be
        // imported to Fivetran without a “schema” (i.e. all components in the
        // same database). For this reason, we require customers that have
        // already set up a Fivetran connection to perform a full historical
        // resync.
        anyhow::ensure!(
            deserialized.version >= 2,
            "This Fivetran connection was created with an old version of the Convex connector. To \
             continue syncing, a full historical resync is required.

To perform a historical re-sync:
- In Fivetran, go to your connection page.
- Select the Setup tab.
- Click 'Resync all historical data'.
- In the confirmation pop-up window, click 'Re-sync Connection'.

Please note that this update to the Convex connector changes the way Convex components are \
             handled. Previously, tables from all components would be synced to the same \
             namespace. Starting from this update:
- Tables from your app will be synced as `{{destination prefix}}_app_{{table name}}` (instead of \
             `{{destination prefix}}_{{table name}}`)
- Tables from components installed in your app will be synced as `{{destination \
             prefix}}_{{component path}}_{{table name}}`
"
        );

        Some(deserialized)
    };

    Ok(state)
}

fn task_response(error: anyhow::Error) -> FivetranUpdateResponse {
    log(&format!("returning error to Fivetran as a task: {error:#}"));
    FivetranUpdateResponse {
        operation: Some(update_response::Operation::Task(Task {
            message: error.to_string(),
        })),
    }
}

fn error_to_task(
    error: anyhow::Error,
) -> ConnectorResult<<ConvexConnector as SourceConnector>::UpdateStream> {
    Ok(Response::new(
        futures::stream::once(async move { Ok(task_response(error)) }).boxed(),
    ))
}
