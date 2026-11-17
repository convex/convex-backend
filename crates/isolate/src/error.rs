use common::{
    errors::{
        FrameData,
        JsError,
    },
    runtime::Runtime,
};
use deno_core::{
    v8,
    ModuleSpecifier,
};
use errors::ErrorMetadataAnyhowExt;
use sourcemap::SourceMap;
use url::Url;
use value::ConvexValue;

use crate::{
    environment::V8IsolateEnvironment,
    execution_scope::ExecutionScope,
    helpers::{
        deserialize_udf_custom_error,
        format_uncaught_error,
        get_property,
        source_map_from_slice,
        to_rust_string,
    },
    is_instance_of_error::is_instance_of_error,
    metrics,
};

impl<RT: Runtime, E: V8IsolateEnvironment<RT>> ExecutionScope<'_, '_, '_, RT, E> {
    pub fn format_traceback(&mut self, exception: v8::Local<v8::Value>) -> anyhow::Result<JsError> {
        // Check if we hit a system error or timeout and can't run any JavaScript now.
        // Abort with a system error here, and we'll (in the best case) pull out
        // the original system error stashed on the `ContextState`.
        self.handle().check_terminated()?;
        let error = match self.extract_source_mapped_error(exception) {
            Ok(err) => err,
            Err(e) => {
                let message = v8::Exception::create_message(self, exception);
                let message = message.get(self);
                let message = to_rust_string(self, &message)?;
                metrics::log_source_map_failure(&message, &e);
                JsError::from_message(message)
            },
        };
        Ok(error)
    }

    fn extract_source_mapped_error(
        &mut self,
        exception: v8::Local<v8::Value>,
    ) -> anyhow::Result<JsError> {
        let (message, frame_data, custom_data) = extract_source_mapped_error(self, exception)?;
        Ok(JsError::from_frames(
            message,
            frame_data,
            custom_data,
            |s| self.lookup_source_map(s),
        ))
    }

    pub fn lookup_source_map(
        &mut self,
        specifier: &ModuleSpecifier,
    ) -> anyhow::Result<Option<SourceMap>> {
        let module_map = self.module_map();
        let Some(module_id) = module_map.get_by_name(specifier) else {
            return Ok(None);
        };
        let Some(source_map) = module_map.source_map(module_id) else {
            return Ok(None);
        };
        Ok(source_map_from_slice(source_map.as_bytes()))
    }

    pub fn nicely_show_line_number_on_error(
        &mut self,
        name: &ModuleSpecifier,
        location: v8::Location,
        e: anyhow::Error,
    ) -> anyhow::Result<!> {
        let source_map = self.lookup_source_map(name)?;
        let orig_line = location.get_line_number();
        let orig_col = location.get_column_number();
        if let Some(ref source_map) = source_map
            && let Some(token) = source_map.lookup_token(orig_line as u32, orig_col as u32)
            && let (line, col) = token.get_src()
            && let Some(source_view) = token.get_source_view()
            && let Some(ctx) = source_view.get_line(line)
            && (col as usize) <= ctx.len()
        {
            Err(e.wrap_error_message(|m| {
                format!(
                    "{name}:{line}:{col}: {m}\n\n{ctx}\n{}{}",
                    " ".repeat(col as usize),
                    "~".repeat(ctx.len() - col as usize)
                )
            }))
        } else {
            Err(e.wrap_error_message(|m| format!("{name}:{orig_line}:{orig_col}: {m}")))
        }
    }
}

/// The source-mapped frames of a stack trace, rendered the way
/// `JsError::to_string` renders them. Both runtimes answer the `error/stack`
/// op through this, so their stacks stay formatted alike.
pub fn source_mapped_stack(
    frame_data: Vec<FrameData>,
    lookup_source_map: impl FnMut(&Url) -> anyhow::Result<Option<SourceMap>>,
) -> String {
    let js_error = JsError::from_frames(String::new(), frame_data, None, lookup_source_map);
    js_error
        .frames
        .expect("JsError::from_frames has frames=None")
        .to_string()
}

pub fn extract_source_mapped_error(
    scope: &v8::PinScope<'_, '_>,
    exception: v8::Local<'_, v8::Value>,
) -> anyhow::Result<(String, Vec<FrameData>, Option<ConvexValue>)> {
    if !is_instance_of_error(scope, exception) {
        anyhow::bail!("Exception wasn't an instance of `Error`");
    }
    let exception_obj: v8::Local<v8::Object> = exception.try_into()?;

    // Get the message by formatting error.name and error.message.
    let name = get_property(scope, exception_obj, "name")?
        .filter(|v| !v.is_undefined())
        .and_then(|m| m.to_string(scope))
        .map(|s| s.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "Error".to_string());
    let message_prop = get_property(scope, exception_obj, "message")?
        .filter(|v| !v.is_undefined())
        .and_then(|m| m.to_string(scope))
        .map(|s| s.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "".to_string());
    let message = format_uncaught_error(message_prop, name);

    // Access the `stack` property to ensure `prepareStackTrace` has been called.
    // NOTE if this is the first time accessing `stack`, it will call the op
    // `error/stack` which does a redundant source map lookup.
    let stack: v8::Local<v8::String> = get_property(scope, exception_obj, "stack")?
        .ok_or_else(|| anyhow::anyhow!("Exception was missing the `stack` property"))?
        .try_into()?;

    let frame_data = get_property(scope, exception_obj, "__frameData")?
        .ok_or_else(|| anyhow::anyhow!("Exception was missing the `__frameData` property"))?;

    // Sometimes the frame_data is undefined. What's that about?
    if frame_data.is_undefined() {
        anyhow::bail!(
            "Exception frame data was undefined, stack: {:?}",
            to_rust_string(scope, &stack)?
        );
    }

    let frame_data: v8::Local<v8::String> = frame_data.try_into()?;
    let frame_data = to_rust_string(scope, &frame_data)?;
    let frame_data: Vec<FrameData> = serde_json::from_str(&frame_data)?;

    // error[error.ConvexErrorSymbol] === true
    let convex_error_symbol = get_property(scope, exception_obj, "ConvexErrorSymbol")?;
    let is_convex_error = convex_error_symbol.is_some_and(|symbol| {
        exception_obj
            .get(scope, symbol)
            .is_some_and(|v| v.is_true())
    });

    let custom_data = if is_convex_error {
        let custom_data: v8::Local<v8::String> = get_property(scope, exception_obj, "data")?
            .ok_or_else(|| anyhow::anyhow!("The thrown ConvexError is missing `data` property"))?
            .try_into()?;
        Some(to_rust_string(scope, &custom_data)?)
    } else {
        None
    };
    let (message, custom_data) = deserialize_udf_custom_error(message, custom_data)?;
    Ok((message, frame_data, custom_data))
}
