use std::time::Duration;

use deno_core::{
    serde_v8,
    v8::{
        self,
    },
};
use serde_json::value::Number as JsonNumber;

use super::{
    OpProvider,
    V8OpProvider,
};
use crate::environment::AsyncOpRequest;

pub fn async_op_sleep<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    args: v8::FunctionCallbackArguments,
    resolver: v8::Global<v8::PromiseResolver>,
) -> anyhow::Result<()> {
    // NOTE: name is only used for error messages.
    let name: String = serde_v8::from_v8(&mut provider.scope(), args.get(1))?;
    let millis: f64 = serde_v8::from_v8(&mut provider.scope(), args.get(2))?;
    let request = sleep_request(provider, name, millis)?;
    provider.start_async_op(request, resolver)
}

pub(super) fn sleep_request<P: OpProvider>(
    provider: &mut P,
    name: String,
    mut millis: f64,
) -> anyhow::Result<AsyncOpRequest> {
    // Negative, infinite and NaN delays fire immediately, as in browsers.
    if millis < 0.0 || !millis.is_finite() {
        millis = 0.0;
    }
    let duration = Duration::from_millis(millis as u64);

    let until = provider.unix_timestamp()? + duration;
    Ok(AsyncOpRequest::Sleep { name, until })
}

#[convex_macro::op]
pub fn op_now<P: OpProvider>(provider: &mut P) -> anyhow::Result<JsonNumber> {
    // NB: Date.now returns the current Unix timestamp in *milliseconds*. We round
    // to the nearest millisecond to match browsers. Browsers generally don't
    // provide sub-millisecond precision to protect against timing attacks:
    // https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date/now#reduced_time_precision
    let ms_since_epoch: u64 = provider.unix_timestamp()?.as_ms_since_epoch()?;
    let n = JsonNumber::from(ms_since_epoch);
    Ok(n)
}

#[convex_macro::op]
pub fn op_performance_now<P: OpProvider>(provider: &mut P) -> anyhow::Result<JsonNumber> {
    secs_as_dom_high_res_ms(provider.performance_now()?.as_secs_f64())
}

#[convex_macro::op]
pub fn op_performance_time_origin<P: OpProvider>(provider: &mut P) -> anyhow::Result<JsonNumber> {
    secs_as_dom_high_res_ms(provider.performance_time_origin()?.as_secs_f64())
}

fn secs_as_dom_high_res_ms(secs: f64) -> anyhow::Result<JsonNumber> {
    let ms = secs * 1000.0;
    // Round down to 100 µs (0.1 ms) to reduce timing precision and protect
    // against side-channel attacks.
    let rounded = (ms * 10.0).floor() / 10.0;
    JsonNumber::from_f64(rounded).ok_or_else(|| anyhow::anyhow!("timestamp produced NaN/inf"))
}
