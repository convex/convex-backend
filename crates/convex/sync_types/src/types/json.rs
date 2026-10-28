use std::collections::BTreeMap;

use anyhow::{
    bail,
    Context,
};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
};
use serde_json::{
    json,
    value::RawValue,
    Value as JsonValue,
};

use crate::{
    types::{
        ClientEvent,
        ErrorPayload,
        SerializedArgs,
    },
    AuthenticationToken,
    ClientMessage,
    IdentityVersion,
    LogLinesMessage,
    Query,
    QueryId,
    QuerySetModification,
    QuerySetVersion,
    SerializedQueryJournal,
    ServerMessage,
    SessionRequestSeqNumber,
    StateModification,
    StateVersion,
    Timestamp,
    UserIdentifier,
    UserIdentityAttributes,
};

/// We implement custom deserialize and serialize to deliver u64s to
/// JavaScript. JavaScript's number type can only fit 52 bits of precision, so
/// u64s larger than 2^53-1 need to get stuffed in a BigInt instead. Sending
/// down a number in JSON would cause it to get decoded into a number
/// by default, with loss of precision.
///
/// e.g. (this number is 2^60)
///     > JSON.parse("{\"foo\": 1152921504606846976}")
///     { foo: 1152921504606847000 }
///
/// So instead we send it down as a string and unpack it ourselves.
fn u64_to_string(x: u64) -> String {
    base64::encode(x.to_le_bytes())
}

fn string_to_u64(s: &str) -> anyhow::Result<u64> {
    let bytes: [u8; 8] = base64::decode(s)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("u64 must be 8 bytes"))?;
    Ok(u64::from_le_bytes(bytes))
}

/// A custom deserializer for optional fields.
/// The outer `Option` represents the field being missing and the inner
/// `Option` represents null.
pub fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}
#[derive(Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
struct QueryJson {
    query_id: QueryId,
    udf_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "double_option")]
    journal: Option<SerializedQueryJournal>,

    #[serde(skip_serializing_if = "Option::is_none")]
    component_path: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct QuerySetModificationJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Box<RawValue>>,
    #[serde(flatten)]
    remaining: QuerySetModificationJsonInner,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type")]
enum QuerySetModificationJsonInner {
    Add(QueryJson),
    #[serde(rename_all = "camelCase")]
    Remove {
        query_id: QueryId,
    },
}

impl TryFrom<QuerySetModification> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(m: QuerySetModification) -> Result<Self, Self::Error> {
        let (modification_json, args) = match m {
            QuerySetModification::Add(q) => {
                let query_json = QueryJson {
                    query_id: q.query_id,
                    udf_path: String::from(q.udf_path),
                    journal: q.journal,
                    component_path: q.component_path,
                };
                (
                    QuerySetModificationJsonInner::Add(query_json),
                    Some(q.args.0),
                )
            },
            QuerySetModification::Remove { query_id } => {
                (QuerySetModificationJsonInner::Remove { query_id }, None)
            },
        };
        let outer = QuerySetModificationJson {
            args,
            remaining: modification_json,
        };
        Ok(serde_json::to_value(outer)?)
    }
}

impl TryFrom<JsonValue> for QuerySetModification {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        let QuerySetModificationJson { args, remaining } = serde_json::from_value(value)?;
        let result = match remaining {
            QuerySetModificationJsonInner::Add(q) => {
                let query = Query {
                    query_id: q.query_id,
                    udf_path: q.udf_path.parse()?,
                    args: SerializedArgs(args.unwrap_or_default()),
                    journal: q.journal,
                    component_path: q.component_path,
                };
                QuerySetModification::Add(query)
            },
            QuerySetModificationJsonInner::Remove { query_id } => {
                QuerySetModification::Remove { query_id }
            },
        };
        Ok(result)
    }
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(tag = "tokenType")]
enum AuthenticationTokenJson {
    Admin {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(alias = "impersonating")]
        acting_as: Option<JsonValue>,
    },
    User {
        value: String,
    },
    None,
}

/// Workaround for a serde shortcoming around deserializing into `RawValue`.
/// Cannot use tagged enums with RawValue inside due to serde abstraction.
/// Instead, we lift the RawValue outside of the ClientMessageJsonInner - to
/// allow us to get the RawValue optimization.
#[derive(Deserialize, Serialize, Debug)]
struct ClientMessageJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modifications: Option<Vec<JsonValue>>,
    #[serde(flatten)]
    remaining: ClientMessageJsonInner,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(tag = "type")]
enum ClientMessageJsonInner {
    #[serde(rename_all = "camelCase")]
    Connect {
        session_id: String,
        connection_count: u32,

        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        last_close_reason: Option<String>,

        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        max_observed_timestamp: Option<String>,

        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        client_ts: Option<i64>,
    },
    #[serde(rename_all = "camelCase")]
    ModifyQuerySet {
        base_version: QuerySetVersion,
        new_version: QuerySetVersion,
    },
    #[serde(rename_all = "camelCase")]
    Mutation {
        request_id: u32,
        udf_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        component_path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Action {
        request_id: u32,
        udf_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        component_path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Authenticate {
        base_version: IdentityVersion,
        #[serde(flatten)]
        token: AuthenticationTokenJson,
    },
    #[serde(rename_all = "camelCase")]
    Event {
        event_type: String,
        event: JsonValue,
    },
}

impl TryFrom<ClientMessage> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(m: ClientMessage) -> Result<Self, Self::Error> {
        let (remaining, args, modifications): (
            ClientMessageJsonInner,
            Option<Box<RawValue>>,
            Option<Vec<JsonValue>>,
        ) = match m {
            ClientMessage::Connect {
                session_id,
                connection_count,
                last_close_reason,
                max_observed_timestamp,
                client_ts,
            } => (
                ClientMessageJsonInner::Connect {
                    session_id: format!("{}", session_id.as_hyphenated()),
                    connection_count,
                    last_close_reason: Some(last_close_reason),
                    max_observed_timestamp: max_observed_timestamp
                        .map(|ts| u64_to_string(ts.into())),
                    client_ts: client_ts.map(|ts| ts as i64),
                },
                None,
                None,
            ),
            ClientMessage::ModifyQuerySet {
                base_version,
                new_version,
                modifications,
            } => (
                ClientMessageJsonInner::ModifyQuerySet {
                    base_version,
                    new_version,
                },
                None,
                Some(
                    modifications
                        .into_iter()
                        .map(JsonValue::try_from)
                        .collect::<anyhow::Result<Vec<_>>>()?,
                ),
            ),
            ClientMessage::Mutation {
                request_id,
                udf_path,
                args,
                component_path,
            } => (
                ClientMessageJsonInner::Mutation {
                    request_id,
                    udf_path: String::from(udf_path),
                    component_path,
                },
                Some(args.0),
                None,
            ),
            ClientMessage::Action {
                request_id,
                udf_path,
                args,
                component_path,
            } => (
                ClientMessageJsonInner::Action {
                    request_id,
                    udf_path: String::from(udf_path),
                    component_path,
                },
                Some(args.0),
                None,
            ),
            ClientMessage::Authenticate {
                base_version,
                token: AuthenticationToken::Admin(value, acting_as),
            } => (
                ClientMessageJsonInner::Authenticate {
                    base_version,
                    token: AuthenticationTokenJson::Admin {
                        value,
                        acting_as: acting_as.map(|a| a.try_into()).transpose()?,
                    },
                },
                None,
                None,
            ),
            ClientMessage::Authenticate {
                base_version,
                token: AuthenticationToken::User(value),
            } => (
                ClientMessageJsonInner::Authenticate {
                    base_version,
                    token: AuthenticationTokenJson::User { value },
                },
                None,
                None,
            ),
            ClientMessage::Authenticate {
                base_version,
                token: AuthenticationToken::None,
            } => (
                ClientMessageJsonInner::Authenticate {
                    base_version,
                    token: AuthenticationTokenJson::None,
                },
                None,
                None,
            ),
            ClientMessage::Event(ClientEvent { event_type, event }) => (
                ClientMessageJsonInner::Event { event_type, event },
                None,
                None,
            ),
        };
        let s = ClientMessageJson {
            args,
            modifications,
            remaining,
        };
        let result = serde_json::to_value(&s)?;
        Ok(result)
    }
}

impl TryFrom<JsonValue> for ClientMessage {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        let ClientMessageJson {
            args,
            modifications,
            remaining,
        } = serde_json::from_value(value)?;
        let result = match remaining {
            ClientMessageJsonInner::Connect {
                session_id,
                connection_count,
                last_close_reason,
                max_observed_timestamp,
                client_ts,
            } => ClientMessage::Connect {
                session_id: session_id.parse()?,
                connection_count,
                last_close_reason: last_close_reason.unwrap_or_else(|| "unknown".to_string()),
                max_observed_timestamp: max_observed_timestamp
                    .map(|s| string_to_u64(&s))
                    .transpose()?
                    .map(Timestamp::try_from)
                    .transpose()?,
                client_ts: client_ts.map(|ts| ts as u64),
            },
            ClientMessageJsonInner::ModifyQuerySet {
                base_version,
                new_version,
            } => ClientMessage::ModifyQuerySet {
                base_version,
                new_version,
                modifications: modifications
                    .context("ModifyQuerySet lacks modifications")?
                    .into_iter()
                    .map(QuerySetModification::try_from)
                    .collect::<anyhow::Result<_>>()?,
            },
            ClientMessageJsonInner::Mutation {
                request_id,
                udf_path,
                component_path,
            } => ClientMessage::Mutation {
                request_id,
                udf_path: udf_path.parse()?,
                args: SerializedArgs(args.context("Mutation lacks args")?),
                component_path,
            },
            ClientMessageJsonInner::Action {
                request_id,
                udf_path,
                component_path,
            } => ClientMessage::Action {
                request_id,
                udf_path: udf_path.parse()?,
                args: SerializedArgs(args.context("Action lacks args")?),
                component_path,
            },
            ClientMessageJsonInner::Authenticate {
                base_version,
                token,
            } => ClientMessage::Authenticate {
                base_version,
                token: match token {
                    AuthenticationTokenJson::Admin { value, acting_as } => {
                        AuthenticationToken::Admin(
                            value,
                            acting_as.map(TryInto::try_into).transpose()?,
                        )
                    },
                    AuthenticationTokenJson::User { value } => AuthenticationToken::User(value),
                    AuthenticationTokenJson::None => AuthenticationToken::None,
                },
            },
            ClientMessageJsonInner::Event { event_type, event } => {
                ClientMessage::Event(ClientEvent { event_type, event })
            },
        };
        Ok(result)
    }
}

impl From<StateVersion> for JsonValue {
    fn from(v: StateVersion) -> Self {
        serde_json::json!({
            "querySet": v.query_set,
            "identity": v.identity,
            "ts": u64_to_string(v.ts.into()),
        })
    }
}

impl TryFrom<JsonValue> for StateVersion {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct StateVersionJson {
            query_set: QuerySetVersion,
            identity: IdentityVersion,
            ts: String,
        }
        let s: StateVersionJson = serde_json::from_value(value)?;
        Ok(Self {
            query_set: s.query_set,
            identity: s.identity,
            ts: Timestamp::try_from(string_to_u64(&s.ts)?)?,
        })
    }
}

impl<V: Into<JsonValue>> From<StateModification<V>> for JsonValue {
    fn from(m: StateModification<V>) -> Self {
        match m {
            StateModification::QueryUpdated {
                query_id,
                value,
                log_lines,
                journal,
            } => {
                let jv: JsonValue = value.into();
                json!({
                    "type": "QueryUpdated",
                    "queryId": query_id,
                    "value": jv,
                    "logLines": log_lines,
                    "journal": journal
                })
            },
            StateModification::QueryFailed {
                query_id,
                error_message,
                log_lines,
                journal,
                error_data,
            } => {
                let mut response = json!({
                    "type": "QueryFailed",
                    "queryId": query_id,
                    "errorMessage": error_message,
                    "logLines": log_lines,
                    "journal": journal
                });

                if let Some(error_data) = error_data {
                    response["errorData"] = error_data.into();
                }
                response
            },
            StateModification::QueryRemoved { query_id } => json!({
                "type": "QueryRemoved",
                "queryId": query_id,
            }),
        }
    }
}

impl<V: TryFrom<JsonValue, Error = anyhow::Error>> TryFrom<JsonValue> for StateModification<V> {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        #[allow(clippy::enum_variant_names)]
        #[derive(Deserialize)]
        #[serde(tag = "type")]
        pub enum StateModificationJson {
            #[serde(rename_all = "camelCase")]
            QueryUpdated {
                query_id: QueryId,
                value: JsonValue,
                log_lines: LogLinesMessage,
                journal: SerializedQueryJournal,
            },
            #[serde(rename_all = "camelCase")]
            QueryFailed {
                query_id: QueryId,
                error_message: String,
                log_lines: LogLinesMessage,
                journal: SerializedQueryJournal,
                #[serde(default, deserialize_with = "deserialize_some")]
                error_data: Option<JsonValue>,
            },
            #[serde(rename_all = "camelCase")]
            QueryRemoved { query_id: QueryId },
        }
        let s: StateModificationJson = serde_json::from_value(value)?;
        let result = match s {
            StateModificationJson::QueryUpdated {
                query_id,
                value,
                log_lines,
                journal,
            } => StateModification::QueryUpdated {
                query_id,
                value: value.try_into()?,
                log_lines,
                journal,
            },
            StateModificationJson::QueryFailed {
                query_id,
                error_message,
                log_lines,
                journal,
                error_data,
            } => StateModification::QueryFailed {
                query_id,
                error_message,
                log_lines,
                journal,
                error_data: error_data
                    .map(|error_data| error_data.try_into())
                    .transpose()?,
            },
            StateModificationJson::QueryRemoved { query_id } => {
                StateModification::QueryRemoved { query_id }
            },
        };
        Ok(result)
    }
}

impl<V: Into<JsonValue>> From<ServerMessage<V>> for JsonValue {
    fn from(m: ServerMessage<V>) -> Self {
        match m {
            ServerMessage::Transition {
                start_version,
                end_version,
                modifications,
                client_clock_skew,
                server_ts,
            } => json!({
                "type": "Transition",
                "startVersion": JsonValue::from(start_version),
                "endVersion": JsonValue::from(end_version),
                "modifications": modifications.into_iter().map(JsonValue::from).collect::<Vec<JsonValue>>(),
                "clientClockSkew": JsonValue::from(client_clock_skew),
                "serverTs": JsonValue::from(server_ts),
            }),
            ServerMessage::MutationResponse {
                request_id,
                result: Ok(value),
                ts,
                log_lines,
            } => {
                let jv: JsonValue = value.into();
                json!({
                    "type": "MutationResponse",
                    "requestId": request_id,
                    "success": true,
                    "result": jv,
                    "ts": ts.map(|ts| u64_to_string(ts.into())),
                    "logLines": log_lines,
                })
            },
            ServerMessage::MutationResponse {
                request_id,
                result: Err(error_payload),
                ts,
                log_lines,
            } => {
                let mut response = json!({
                    "type": "MutationResponse",
                    "requestId": request_id,
                    "success": false,
                    "result": error_payload.get_message(),
                    "ts": ts.map(|ts| u64_to_string(ts.into())),
                    "logLines": log_lines,
                });
                if let ErrorPayload::ErrorData { data, .. } = error_payload {
                    response["errorData"] = data.into();
                }
                response
            },
            ServerMessage::ActionResponse {
                request_id,
                result: Ok(value),
                log_lines,
            } => {
                let jv: JsonValue = value.into();
                json!({
                    "type": "ActionResponse",
                    "requestId": request_id,
                    "success": true,
                    "result": jv,
                    "logLines": log_lines,
                })
            },
            ServerMessage::ActionResponse {
                request_id,
                result: Err(error_payload),
                log_lines,
            } => {
                let mut response = json!({
                    "type": "ActionResponse",
                    "requestId": request_id,
                    "success": false,
                    "result": error_payload.get_message(),
                    "logLines": log_lines,
                });
                if let ErrorPayload::ErrorData { data, .. } = error_payload {
                    response["errorData"] = data.into();
                }
                response
            },
            ServerMessage::AuthError {
                error_message,
                base_version,
                auth_update_attempted,
            } => {
                let mut response = json!({
                    "type": "AuthError",
                    "error": error_message,
                    "baseVersion": base_version,
                });
                // Only include authUpdateAttempted if it's present
                if let Some(auth_update_attempted) = auth_update_attempted {
                    response["authUpdateAttempted"] = auth_update_attempted.into();
                }
                response
            },
            ServerMessage::TransitionChunk {
                chunk,
                part_number,
                total_parts,
                transition_id,
            } => json!({
                "type": "TransitionChunk",
                "chunk": chunk,
                "partNumber": part_number,
                "totalParts": total_parts,
                "transitionId": transition_id,
            }),
            ServerMessage::FatalError { error_message } => json!({
                "type": "FatalError",
                "error": error_message,
            }),
            ServerMessage::Ping => json!({
                "type": "Ping"
            }),
        }
    }
}

impl<V: TryFrom<JsonValue, Error = anyhow::Error>> TryFrom<JsonValue> for ServerMessage<V> {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type")]
        pub enum ServerMessageJson {
            #[serde(rename_all = "camelCase")]
            Transition {
                start_version: JsonValue,
                end_version: JsonValue,
                modifications: Vec<JsonValue>,
                client_clock_skew: Option<i64>,
                server_ts: Option<i64>,
            },
            #[serde(rename_all = "camelCase")]
            MutationResponse {
                request_id: SessionRequestSeqNumber,
                success: bool,
                result: JsonValue,
                ts: Option<String>,
                log_lines: LogLinesMessage,
                #[serde(default, deserialize_with = "deserialize_some")]
                error_data: Option<JsonValue>,
            },
            #[serde(rename_all = "camelCase")]
            ActionResponse {
                request_id: SessionRequestSeqNumber,
                success: bool,
                result: JsonValue,
                log_lines: LogLinesMessage,
                #[serde(default, deserialize_with = "deserialize_some")]
                error_data: Option<JsonValue>,
            },
            #[serde(rename_all = "camelCase")]
            FatalError { error: String },
            #[serde(rename_all = "camelCase")]
            AuthError {
                error: String,
                base_version: Option<IdentityVersion>,
                auth_update_attempted: Option<bool>,
            },
            #[serde(rename_all = "camelCase")]
            TransitionChunk {
                chunk: String,
                part_number: u32,
                total_parts: u32,
                transition_id: String,
            },
            #[serde(rename_all = "camelCase")]
            Ping {},
        }
        let s: ServerMessageJson = serde_json::from_value(value)?;
        let result = match s {
            ServerMessageJson::Transition {
                start_version,
                end_version,
                modifications,
                client_clock_skew,
                server_ts,
            } => ServerMessage::Transition {
                start_version: start_version.try_into()?,
                end_version: end_version.try_into()?,
                modifications: modifications
                    .into_iter()
                    .map(|sm: JsonValue| sm.try_into())
                    .collect::<anyhow::Result<Vec<StateModification<V>>>>()?,
                client_clock_skew,
                server_ts: server_ts.map(Timestamp::try_from).transpose()?,
            },
            ServerMessageJson::MutationResponse {
                request_id,
                success,
                result,
                ts,
                log_lines,
                error_data,
            } => {
                let result = if success {
                    Ok(result.try_into()?)
                } else {
                    let msg: String = serde_json::from_value(result)?;
                    Err(if let Some(data) = error_data {
                        ErrorPayload::ErrorData {
                            message: msg,
                            data: data.try_into()?,
                        }
                    } else {
                        ErrorPayload::Message(msg)
                    })
                };
                ServerMessage::MutationResponse {
                    request_id,
                    result,
                    ts: ts
                        .map(|s| string_to_u64(&s))
                        .transpose()?
                        .map(Timestamp::try_from)
                        .transpose()?,
                    log_lines,
                }
            },
            ServerMessageJson::ActionResponse {
                request_id,
                success,
                result,
                log_lines,
                error_data,
            } => {
                let result = if success {
                    Ok(result.try_into()?)
                } else {
                    let msg: String = serde_json::from_value(result)?;
                    Err(if let Some(data) = error_data {
                        ErrorPayload::ErrorData {
                            message: msg,
                            data: data.try_into()?,
                        }
                    } else {
                        ErrorPayload::Message(msg)
                    })
                };
                ServerMessage::ActionResponse {
                    request_id,
                    result,
                    log_lines,
                }
            },
            ServerMessageJson::FatalError { error } => ServerMessage::FatalError {
                error_message: error,
            },
            ServerMessageJson::AuthError {
                error,
                base_version,
                auth_update_attempted,
            } => ServerMessage::AuthError {
                error_message: error,
                base_version,
                auth_update_attempted,
            },
            ServerMessageJson::TransitionChunk {
                chunk,
                part_number,
                total_parts,
                transition_id,
            } => ServerMessage::TransitionChunk {
                chunk,
                part_number,
                total_parts,
                transition_id,
            },
            ServerMessageJson::Ping {} => ServerMessage::Ping {},
        };
        Ok(result)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserIdentityAttributesJson {
    // Always exists when serializing
    pub token_identifier: Option<UserIdentifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birthday: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub custom_claims: Option<BTreeMap<String, JsonValue>>,
}

impl TryFrom<JsonValue> for UserIdentityAttributes {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        let raw: UserIdentityAttributesJson = serde_json::from_value(value)?;
        let token_identifier = if let Some(token_identifier) = raw.token_identifier {
            token_identifier
        } else if let (Some(issuer), Some(subject)) = (&raw.issuer, &raw.subject) {
            UserIdentifier::construct(issuer, subject)
        } else {
            bail!("Either \"tokenIdentifier\" or \"issuer\" and \"subject\" must be set")
        };
        let custom_claims = raw
            .custom_claims
            .context("expected custom claims to be set")?;
        let custom_claims_string = custom_claims
            .into_iter()
            .map(|(key, value)| {
                let value_string = serde_json::to_string(&value)?;
                Ok((key, value_string))
            })
            .collect::<anyhow::Result<_>>()?;

        Ok(UserIdentityAttributes {
            token_identifier,
            issuer: raw.issuer,
            subject: raw.subject,
            name: raw.name,
            given_name: raw.given_name,
            family_name: raw.family_name,
            nickname: raw.nickname,
            preferred_username: raw.preferred_username,
            profile_url: raw.profile_url,
            picture_url: raw.picture_url,
            website_url: raw.website_url,
            email: raw.email,
            email_verified: raw.email_verified,
            gender: raw.gender,
            birthday: raw.birthday,
            timezone: raw.timezone,
            language: raw.language,
            phone_number: raw.phone_number,
            phone_number_verified: raw.phone_number_verified,
            address: raw.address,
            updated_at: raw.updated_at,
            custom_claims: custom_claims_string,
        })
    }
}

impl TryFrom<UserIdentityAttributes> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(value: UserIdentityAttributes) -> Result<Self, Self::Error> {
        let custom_claims_json = value
            .custom_claims
            .into_iter()
            .map(|(key, value)| {
                let value_json = serde_json::from_str(&value)?;
                Ok((key, value_json))
            })
            .collect::<anyhow::Result<_>>()?;
        let raw = UserIdentityAttributesJson {
            token_identifier: Some(value.token_identifier),
            issuer: value.issuer,
            subject: value.subject,
            name: value.name,
            given_name: value.given_name,
            family_name: value.family_name,
            nickname: value.nickname,
            preferred_username: value.preferred_username,
            profile_url: value.profile_url,
            picture_url: value.picture_url,
            website_url: value.website_url,
            email: value.email,
            email_verified: value.email_verified,
            gender: value.gender,
            birthday: value.birthday,
            timezone: value.timezone,
            language: value.language,
            phone_number: value.phone_number,
            phone_number_verified: value.phone_number_verified,
            address: value.address,
            updated_at: value.updated_at,
            custom_claims: Some(custom_claims_json),
        };
        Ok(serde_json::to_value(raw)?)
    }
}

// Make sure that `null` is `Some(JsonValue::Null)`, not `None`
fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}
