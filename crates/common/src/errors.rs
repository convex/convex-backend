use std::{
    backtrace::{
        Backtrace,
        BacktraceStatus,
    },
    borrow::Cow,
    collections::{
        btree_map::Entry,
        BTreeMap,
    },
    error::Error,
    fmt::{
        self,
        Debug,
        Display,
    },
    sync::LazyLock,
};

use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
pub use errors::{
    INTERNAL_SERVER_ERROR,
    INTERNAL_SERVER_ERROR_MSG,
};
use metrics::{
    log_counter,
    SERVICE_NAME,
};
use pb::common::{
    FrameData as FrameDataProto,
    JsError as JsErrorProto,
    JsFrames as JsFramesProto,
};
use rand::Rng;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use sourcemap::SourceMap;
use url::Url;
use uuid::Uuid;
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    ConvexValue,
};

use crate::{
    knobs::SHOW_PII_IN_ERRORS,
    metrics::log_errors_reported_total,
};

// Regex to match emails from https://colinhacks.com/essays/reasonable-email-regex
// This regex doesn’t match all valid email addresses (e.g. it skips emails
// in IP addresses) in order to avoid false positives for pnpm module paths.
// The negative lookaheads were removed because the `regex` crate doesn’t
// support them, which makes the regex less strict about the position of `.`s.
pub static EMAIL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([a-z0-9_'+\-\.]*)[a-z0-9_+-]@([a-z0-9][a-z0-9\-]*\.)+[a-z]{2,}"#).unwrap()
});

/// Replacers for PII in errors before reporting to thirdparty services
/// (sentry/datadog)
static PII_REPLACEMENTS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        // Regex to match PII where we show the object that doesn't match the
        // validator.
        (Regex::new(r"(?s)Object:.*Validator").unwrap(), "Validator"),
        (EMAIL_REGEX.clone(), "*****@*****.***"),
    ]
});

/// Limit error messages originating from user code to 64KiB
const MAX_ERROR_MESSAGE_LENGTH: usize = 1 << 16;

/// Return Result<(), MainError> from main functions to report returned errors
/// to Sentry.
pub struct MainError(anyhow::Error);
impl<T: Into<anyhow::Error>> From<T> for MainError {
    fn from(e: T) -> Self {
        let mut err: anyhow::Error = e.into();
        report_error_sync(&mut err);
        Self(err)
    }
}

impl Debug for MainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Just print the `Display` of the error rather than `Debug`, as `report_error`
        // above will already print the stack trace when `RUST_BACKTRACE` is set.
        write!(f, "{}", self.0)
    }
}

/// Redact PII from text that is about to be handed to a third party
/// (sentry/datadog).
pub fn redact_pii(s: &str) -> Cow<'_, str> {
    if *SHOW_PII_IN_ERRORS {
        return Cow::Borrowed(s);
    }
    let mut redacted = Cow::Borrowed(s);
    for (regex, replacement) in PII_REPLACEMENTS.iter() {
        if let Cow::Owned(owned) = regex.replace_all(&redacted, *replacement) {
            redacted = Cow::Owned(owned);
        }
    }
    redacted
}

/// Creates an error from `msg` with the backtrace from `err`. No other
/// properties of `err` are preserved, including its message or ErrorMetadata.
pub fn error_with_backtrace(msg: String, err: anyhow::Error) -> anyhow::Error {
    struct WithBacktrace(String, anyhow::Error);
    impl Error for WithBacktrace {
        fn provide<'a>(&'a self, request: &mut std::error::Request<'a>) {
            request.provide_ref(self.1.backtrace());
        }
    }
    impl Display for WithBacktrace {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl Debug for WithBacktrace {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }
    anyhow::Error::new(WithBacktrace(msg, err))
}

/// Log an error to Sentry.
///
/// Other parts of codebase should not use the `sentry_anyhow` crate directly!
pub async fn report_error(err: &mut anyhow::Error) -> Option<Uuid> {
    // Trace error before yield - since during shutdown, we won't be back.
    log_error(err);
    // Give shutdown cancellation a chance to complete before explicit reporting.
    tokio::task::yield_now().await;
    report_to_sentry(err, None)
}

/// Use the `pub async fn report_error` above if possible to log an error to
/// sentry. This is a synchronous version for use in sync contexts.
pub fn report_error_sync(err: &mut anyhow::Error) -> Option<Uuid> {
    log_error(err);
    report_to_sentry(err, None)
}

/// Log `err`, redacting PII from the text that gets logged rather than from
/// `err` itself.
fn log_error(err: &anyhow::Error) {
    if let Some(label) = err.metric_server_error_label() {
        log_errors_reported_total(label);
    }

    let label = err.metric_status_label_value();
    let chain = format!("{err:#}");
    let redacted = redact_pii(&chain);
    let err_for_tracing = redacted.replace("\n", "\\n");
    let full_msg = format!(
        "Caught {label} error (RUST_BACKTRACE=1 RUST_LOG=info,{}=debug for full trace): \
         {err_for_tracing}",
        module_path!()
    );
    if err.metric_server_error_label().is_some() {
        tracing::error!("{full_msg}");
    } else {
        tracing::warn!("{full_msg}");
    }
    // Log backtraces separately because `(?s)Object:.*Validator` can match
    // across newlines and consume `validator` frames.
    tracing::debug!("{redacted}\n{}", err.backtrace());
}

/// This is the one point where we call into Sentry.
///
/// `recaptured_stack` is the stack where the error crossed into this context,
/// if any; it is reported as a second stacktrace on the event.
fn report_to_sentry(err: &mut anyhow::Error, recaptured_stack: Option<&Backtrace>) -> Option<Uuid> {
    if let Some(e) = err.downcast_mut::<ErrorMetadata>() {
        if let Some(counter) = e.custom_metric() {
            log_counter(counter, 1);
        }
        e.log_sampled_client_error();
        // Set the source of this error to the service name if it's not already set,
        // denoting that this error has been reported and downstream callers that
        // receive this error need not re-report it.
        match &e.source {
            Some(source) => {
                tracing::debug!("Not reporting above error: already reported by {source}");
                return None;
            },
            None => {
                e.source = Some(SERVICE_NAME.clone());
            },
        }
    }
    // Checked after the block above so the counters there still fire on a
    // repeat report, matching how the `source` guard behaves.
    if err.is::<AlreadyReported>() {
        tracing::debug!("Not reporting above error: already reported where it was recaptured");
        return None;
    }

    let Some(sentry_client) = sentry::Hub::current().client() else {
        tracing::error!("Not reporting above error: Sentry is not configured");
        return None;
    };
    if let Some((level, prob)) = err.should_report_to_sentry() {
        if let Some(prob) = prob
            && rand::rng().random::<f64>() > prob
        {
            tracing::debug!("Not reporting above error to sentry - due to sampling.");
            return None;
        }

        if !sentry_client.is_enabled() {
            tracing::debug!("Not reporting above error: SENTRY_DSN not set.");
            return None;
        }

        let mut event = event_from_error(err, recaptured_stack);
        // N.B.: we don't use `sentry::with_scope` because I think that is
        // non-thread-safe if the Hub itself is shared across threads; but we
        // can just attach data directly onto the event.
        event.level = level;
        event
            .tags
            .insert("short_msg".into(), redact_pii(err.short_msg()).into_owned());
        let event_id = sentry::capture_event(event);
        tracing::error!(
            "Reporting above error to sentry with event_id {}",
            event_id.simple()
        );
        Some(event_id)
    } else {
        tracing::debug!("Not reporting above error to sentry.");
        None
    }
}

/// Construct a sentry `Event` from an `anyhow` error chain, while inserting
/// `ErrorMetadata`'s `short_msg` into the appropriate type.
///
/// `recaptured_stack` rides on `event.threads` so it stays out of Sentry's
/// grouping hash; every exception stacktrace contributes to that hash.
fn event_from_error(
    err: &anyhow::Error,
    recaptured_stack: Option<&Backtrace>,
) -> sentry::protocol::Event<'static> {
    let mut event = sentry::integrations::anyhow::event_from_error(err);
    if let Some(em) = err.downcast_ref::<ErrorMetadata>() {
        // hacky: we don't know where in the exception chain this
        // `ErrorMetadata` is; and if ErrorMetadata was added via `.context()`
        // (as opposed to being the root cause), the actual error type (as found
        // by `<dyn std::error::Error>::downcast`) won't be ErrorMetadata itself
        // but will be a private ContextError type.
        //
        // So we'll just find the matching exception by string quality :shrug:
        // This doesn't work if there are *multiple* ErrorMetadatas attached to
        // the error but we should generally try to avoid doing that.
        if let Some(exception) = event.exception.iter_mut().find(|e| {
            e.value.as_deref() == Some(&*em.msg) && (e.ty == "ErrorMetadata" || e.ty == "Error")
        }) {
            // N.B. the existing `exception.ty` is `ErrorMetadata` if it's the
            // root cause or `Error` otherwise.
            exception.ty = em.short_msg.to_string();
        }
    }
    if let Some(stack) = recaptured_stack
        && matches!(stack.status(), BacktraceStatus::Captured)
        && let Some(stacktrace) =
            sentry::integrations::backtrace::parse_stacktrace(&format!("{stack:#}"))
    {
        event.threads.values.push(sentry::protocol::Thread {
            name: Some("recapture_stacktrace".to_owned()),
            stacktrace: Some(stacktrace),
            ..Default::default()
        });
    }
    // Redaction runs last: the `short_msg` insertion above matches exceptions
    // against the unredacted `ErrorMetadata::msg`.
    for exception in event.exception.as_mut() {
        if let Cow::Owned(redacted) = redact_pii(&exception.ty) {
            exception.ty = redacted;
        }
        if let Some(value) = &exception.value
            && let Cow::Owned(redacted) = redact_pii(value)
        {
            exception.value = Some(redacted);
        }
    }
    event
}

/// Report `err` with the stack it is being handed off at attached as a second
/// stacktrace. Use this when an error crosses into a different context with a
/// different stack (eg from an async worker to a request), where the error's
/// own backtrace points at the wrong place.
///
/// Errors carrying `ErrorMetadata` are handed back unchanged and deduplicated
/// by its `source`. Other errors carry an `AlreadyReported` marker so upstream
/// callers do not report them again.
pub async fn recapture_stacktrace(mut err: anyhow::Error) -> anyhow::Error {
    let recaptured_stack = Backtrace::force_capture();
    log_error(&err);
    tokio::task::yield_now().await;
    report_to_sentry(&mut err, Some(&recaptured_stack));
    if err.is::<ErrorMetadata>() {
        err
    } else {
        anyhow::Error::new(AlreadyReported(format!("{err:#}")))
    }
}

pub fn recapture_stacktrace_noreport(err: &anyhow::Error) -> anyhow::Error {
    with_error_metadata_of(err, anyhow::anyhow!("Orig Error: {err:#}."))
}

fn with_error_metadata_of(err: &anyhow::Error, new_err: anyhow::Error) -> anyhow::Error {
    match err.downcast_ref::<ErrorMetadata>() {
        Some(em) => new_err.context(em.clone()),
        None => new_err,
    }
}

/// Root of the error `recapture_stacktrace` hands back. Its presence means the
/// original was already reported to Sentry, which is how errors that carry no
/// `ErrorMetadata` — and so have no `source` field to stamp — are deduped.
/// Confined to one process: `tonic::Status` carries only `ErrorMetadata`.
#[derive(thiserror::Error)]
#[error("Orig Error: {0}.")]
struct AlreadyReported(String);

impl Debug for AlreadyReported {
    // Sentry derives the exception type from `Debug` output, and only yields
    // "Error" when it renders as the quoted `Display` string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.to_string(), f)
    }
}

#[derive(Clone, Deserialize, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FrameData {
    pub type_name: Option<String>,
    pub function_name: Option<String>,
    pub method_name: Option<String>,
    pub file_name: Option<String>,
    pub line_number: Option<u32>,
    pub column_number: Option<u32>,
    pub eval_origin: Option<String>,
    pub is_top_level: Option<bool>,
    pub is_eval: bool,
    pub is_native: bool,
    pub is_constructor: bool,
    pub is_async: bool,
    pub is_promise_all: bool,
    pub promise_index: Option<u32>,
}

impl From<FrameData> for FrameDataProto {
    fn from(
        FrameData {
            type_name,
            function_name,
            method_name,
            file_name,
            line_number,
            column_number,
            eval_origin,
            is_top_level,
            is_eval,
            is_native,
            is_constructor,
            is_async,
            is_promise_all,
            promise_index,
        }: FrameData,
    ) -> Self {
        Self {
            type_name,
            function_name,
            method_name,
            file_name,
            line_number,
            column_number,
            eval_origin,
            is_top_level,
            is_eval: Some(is_eval),
            is_native: Some(is_native),
            is_constructor: Some(is_constructor),
            is_async: Some(is_async),
            is_promise_all: Some(is_promise_all),
            promise_index,
        }
    }
}

impl From<FrameData> for sentry::protocol::Frame {
    fn from(frame: FrameData) -> Self {
        let function = match frame.function_name {
            Some(f) => f,
            None => match frame.method_name {
                Some(m) => m,
                None => "<anonymous>".to_string(),
            },
        };

        Self {
            function: Some(function),
            filename: frame.file_name.clone(),
            lineno: frame.line_number.map(|l| l as u64),
            colno: frame.column_number.map(|c| c as u64),
            module: None,
            package: None,
            abs_path: None,
            pre_context: vec![],
            context_line: None,
            post_context: vec![],
            in_app: Some(
                frame
                    .file_name
                    .map(|f| !f.contains("node_modules"))
                    .unwrap_or(false),
            ),
            vars: BTreeMap::new(),
            image_addr: None,
            instruction_addr: None,
            symbol_addr: None,
            addr_mode: None,
            symbol: None,
        }
    }
}

impl TryFrom<FrameDataProto> for FrameData {
    type Error = anyhow::Error;

    fn try_from(
        FrameDataProto {
            type_name,
            function_name,
            method_name,
            file_name,
            line_number,
            column_number,
            eval_origin,
            is_top_level,
            is_eval,
            is_native,
            is_constructor,
            is_async,
            is_promise_all,
            promise_index,
        }: FrameDataProto,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            type_name,
            function_name,
            method_name,
            file_name,
            line_number,
            column_number,
            eval_origin,
            is_top_level,
            is_eval: is_eval.ok_or_else(|| anyhow::anyhow!("Missing is_eval"))?,
            is_native: is_native.ok_or_else(|| anyhow::anyhow!("Missing is_native"))?,
            is_constructor: is_constructor
                .ok_or_else(|| anyhow::anyhow!("Missing is_constructor"))?,
            is_async: is_async.ok_or_else(|| anyhow::anyhow!("Missing is_async"))?,
            is_promise_all: is_promise_all
                .ok_or_else(|| anyhow::anyhow!("Missing is_promise_all"))?,
            promise_index,
        })
    }
}

impl HeapSize for FrameData {
    fn heap_size(&self) -> usize {
        self.type_name.heap_size()
            + self.function_name.heap_size()
            + self.method_name.heap_size()
            + self.file_name.heap_size()
            + self.line_number.heap_size()
            + self.column_number.heap_size()
            + self.eval_origin.heap_size()
            + self.is_top_level.heap_size()
            + self.is_eval.heap_size()
            + self.is_native.heap_size()
            + self.is_constructor.heap_size()
            + self.is_async.heap_size()
            + self.is_promise_all.heap_size()
            + self.promise_index.heap_size()
    }
}

pub type MappedFrame = FrameData;

impl FrameData {
    fn is_omittable_internal_frame(&self) -> bool {
        let Some(ref f) = self.file_name else {
            return false;
        };
        f.contains("udf-runtime/src") || f.contains("convex/src/server/impl")
    }
}

impl fmt::Display for FrameData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "    at ")?;
        if self.is_async {
            write!(f, "async ")?;
        }
        if self.is_promise_all
            && let Some(promise_index) = self.promise_index
        {
            // V8's `SerializeJSStackFrame` renders a `Promise.all` frame as
            // just its element index, with no function name or location.
            return write!(f, "Promise.all (index {promise_index})");
        }
        let is_method_call = !(self.is_top_level == Some(true) || self.is_constructor);
        if is_method_call {
            if let Some(ref function_name) = self.function_name {
                if let Some(ref type_name) = self.type_name
                    && function_name.starts_with(type_name)
                {
                    write!(f, "{type_name}.")?;
                }
                write!(f, "{function_name}")?;
                if let Some(ref method_name) = self.method_name
                    && function_name.ends_with(method_name)
                {
                    write!(f, " [as {method_name}]")?;
                }
            } else {
                if let Some(ref type_name) = self.type_name {
                    write!(f, "{type_name}.")?;
                }
                if let Some(ref method_name) = self.method_name {
                    write!(f, "{method_name}")?;
                } else {
                    write!(f, "<anonymous>")?;
                }
            }
        } else if self.is_constructor {
            write!(f, "new ")?;
            if let Some(ref function_name) = self.function_name {
                write!(f, "{function_name}")?;
            } else {
                write!(f, "<anonymous>")?;
            }
        } else if let Some(ref function_name) = self.function_name {
            write!(f, "{function_name}")?;
        } else {
            format_location(f, self)?;
            return Ok(());
        }
        write!(f, " (")?;
        format_location(f, self)?;
        write!(f, ")")?;
        Ok(())
    }
}

/// An Error emitted from a Convex Function execution.
#[derive(Clone)]
pub struct JsError {
    pub message: String,
    pub custom_data: Option<ConvexValue>,
    pub frames: Option<JsFrames>,
}

impl From<JsError> for anyhow::Error {
    fn from(js_error: JsError) -> anyhow::Error {
        let msg = js_error.to_string();
        anyhow::anyhow!(ErrorMetadata::bad_request("Error", msg)).context(js_error)
    }
}

impl TryFrom<JsError> for JsErrorProto {
    type Error = anyhow::Error;

    fn try_from(
        JsError {
            message,
            custom_data,
            frames,
        }: JsError,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            message: Some(message),
            custom_data: custom_data
                .map(|v| anyhow::Ok(v.json_serialize()?.into_bytes()))
                .transpose()?,
            frames: frames.map(JsFramesProto::from),
        })
    }
}

impl TryFrom<JsErrorProto> for JsError {
    type Error = anyhow::Error;

    fn try_from(
        JsErrorProto {
            message,
            custom_data,
            frames,
        }: JsErrorProto,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            message: message.ok_or_else(|| anyhow::anyhow!("Missing message"))?,
            custom_data: custom_data
                .map(|bytes| {
                    let json: JsonValue = serde_json::from_slice(&bytes)?;
                    let developer_value = json.try_into()?;
                    anyhow::Ok::<ConvexValue>(developer_value)
                })
                .transpose()?,
            frames: frames.map(JsFrames::try_from).transpose()?,
        })
    }
}

impl HeapSize for JsError {
    fn heap_size(&self) -> usize {
        self.message.heap_size() + self.frames.heap_size()
    }
}

#[derive(Clone)]
pub struct JsFrames(pub WithHeapSize<Vec<MappedFrame>>);

impl From<JsFrames> for JsFramesProto {
    fn from(JsFrames(frames): JsFrames) -> Self {
        Self {
            frames: frames.into_iter().map(FrameDataProto::from).collect(),
        }
    }
}

impl TryFrom<JsFramesProto> for JsFrames {
    type Error = anyhow::Error;

    fn try_from(JsFramesProto { frames }: JsFramesProto) -> anyhow::Result<Self> {
        Ok(Self(
            frames.into_iter().map(FrameData::try_from).try_collect()?,
        ))
    }
}

impl HeapSize for JsFrames {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

impl JsError {
    pub fn from_error(e: anyhow::Error) -> Self {
        match e.downcast::<Self>() {
            Ok(js_error) => js_error,
            Err(e) => Self::from_message(e.user_facing_message()),
        }
    }

    pub fn from_error_ref(e: &anyhow::Error) -> Self {
        match e.downcast_ref::<Self>() {
            Some(js_error) => js_error.clone(),
            None => Self::from_message(e.user_facing_message()),
        }
    }

    pub fn from_message(message: String) -> Self {
        Self {
            message,
            custom_data: None,
            frames: None,
        }
    }

    pub fn convex_error(message: String, data: ConvexValue) -> Self {
        Self {
            message,
            custom_data: Some(data),
            frames: None,
        }
    }

    pub fn from_frames(
        mut message: String,
        frame_data: Vec<FrameData>,
        custom_data: Option<ConvexValue>,
        mut lookup_source_map: impl FnMut(&Url) -> anyhow::Result<Option<SourceMap>>,
    ) -> Self {
        let mut source_maps = BTreeMap::new();
        let mut mapped_frames = Vec::with_capacity(frame_data.len());
        for mut frame in frame_data {
            if let FrameData {
                file_name: Some(ref f),
                line_number: Some(l),
                column_number: Some(c),
                ..
            } = frame
            {
                let Ok(specifier) = Url::parse(f) else {
                    // We expect the file_name to be fully qualified URL but seems
                    // this is not always the case. Lets log warning here.
                    tracing::warn!("Skipping frame with invalid file_name: {f}");
                    continue;
                };
                let source_map = match source_maps.entry(specifier) {
                    Entry::Vacant(e) => {
                        let maybe_source_map = match lookup_source_map(e.key()) {
                            Ok(maybe_source_map) => maybe_source_map,
                            Err(err) => {
                                // This is not expected so report an error.
                                let mut err = err
                                    .context(ErrorMetadata::operational_internal_server_error())
                                    .context("Failed to lookup source_map");
                                report_error_sync(&mut err);
                                continue;
                            },
                        };
                        let Some(source_map) = maybe_source_map else {
                            tracing::debug!("Missing source map for {}", e.key());
                            continue;
                        };
                        e.insert(source_map)
                    },
                    Entry::Occupied(e) => e.into_mut(),
                };
                if let Some(token) = source_map.lookup_token(l, c) {
                    if let Some(mapped_name) = token.get_source() {
                        frame.file_name = Some(mapped_name.to_string());
                    }
                    frame.line_number = Some(token.get_src_line());
                    frame.column_number = Some(token.get_src_col());
                } else {
                    tracing::debug!("Failed to find token for {f}:{l}:{c}");
                }
            } else {
                tracing::debug!("Skipping incomplete frame: {frame:?}");
            }

            // Omit leading frames inside of our own UDF harness code. This stuff is not
            // helpful to Convex developers - they want to see their own code.
            if mapped_frames.is_empty() && frame.is_omittable_internal_frame() {
                continue;
            }
            mapped_frames.push(frame);
        }

        // Omit trailing frames inside our own UDF harness code as well.
        while let Some(f) = mapped_frames.last()
            && f.is_omittable_internal_frame()
        {
            mapped_frames.pop();
        }

        message.truncate(message.floor_char_boundary(MAX_ERROR_MESSAGE_LENGTH));

        JsError {
            message,
            custom_data,
            frames: Some(JsFrames(mapped_frames.into())),
        }
    }

}

// Based on deno's `02_error.js:formatLocation`.
fn format_location(f: &mut fmt::Formatter<'_>, frame: &MappedFrame) -> fmt::Result {
    if frame.is_native {
        return write!(f, "native");
    }
    if let Some(ref file_name) = frame.file_name {
        write!(f, "{file_name}")?;
    } else {
        if frame.is_eval
            && let Some(ref eval_origin) = frame.eval_origin
        {
            write!(f, "{eval_origin}, ")?;
        }
        write!(f, "<anonymous>")?;
    }
    if let Some(line_number) = frame.line_number {
        write!(f, ":{line_number}")?;
        if let Some(column_number) = frame.column_number {
            write!(f, ":{column_number}")?;
        }
    }
    Ok(())
}

impl fmt::Debug for JsFrames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for frame in self.0.iter() {
            writeln!(f, "{frame}")?;
        }
        Ok(())
    }
}

impl fmt::Display for JsFrames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl fmt::Debug for JsError {
    // Based on deno's `02_error.js:formatCallsite`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.message)?;
        if let Some(ref frames) = self.frames {
            write!(f, "{frames}")?;
        }
        Ok(())
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Lease Lost")]
pub struct LeaseLostError;
pub fn lease_lost_error() -> anyhow::Error {
    anyhow::anyhow!(LeaseLostError).context(ErrorMetadata::operational_internal_server_error())
}

#[derive(thiserror::Error, Debug)]
#[error("Database Timeout ({0})")]
pub struct DatabaseTimeoutError(&'static str);
pub fn database_timeout_error(db_type: &'static str) -> anyhow::Error {
    anyhow::anyhow!(DatabaseTimeoutError(db_type))
        .context(ErrorMetadata::operational_internal_server_error())
}

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct DatabaseOperationalError(anyhow::Error);
pub fn database_operational_error(error: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!(DatabaseOperationalError(error))
        .context(ErrorMetadata::operational_internal_server_error())
}

/// Not a [`DatabaseOperationalError`]: those are retried per statement, but a
/// deadlock rolls back the whole transaction.
#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct DatabaseDeadlockError(anyhow::Error);
pub fn database_deadlock_error(error: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!(DatabaseDeadlockError(error))
}

/// True if `e` (or anything in its anyhow context chain) is a transient
/// database error.
pub fn is_transient_db_error(e: &anyhow::Error) -> bool {
    e.is::<DatabaseTimeoutError>() || e.is::<DatabaseOperationalError>()
}

pub fn is_db_deadlock_error(e: &anyhow::Error) -> bool {
    e.is::<DatabaseDeadlockError>()
}

pub const AUTH_ERROR: &str = "AuthError";
