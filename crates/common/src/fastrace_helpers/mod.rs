use std::{
    collections::BTreeMap,
    future::Future,
    hash::{
        Hash,
        Hasher,
    },
    pin::Pin,
    str::FromStr,
    sync::LazyLock,
    task::{
        Context,
        Poll,
    },
};

use anyhow::Context as _;
use fastrace::{
    collector::SpanContext,
    Span,
};
use fnv::FnvHasher;
use parking_lot::Mutex;
use pin_project::pin_project;
use rand::Rng;
use regex::Regex;
use serde::Deserialize;

use crate::knobs::REQUEST_TRACE_SAMPLE_CONFIG;

pub mod interceptor;
pub mod layer;

static SAMPLING_CONFIG_FROM_LOADER: LazyLock<Mutex<Option<SamplingConfig>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Debug)]
pub struct EncodedSpan(pub Option<String>);

impl EncodedSpan {
    pub fn empty() -> Self {
        Self(None)
    }

    /// Encodes the current local parent `SpanContext`
    pub fn from_parent() -> Self {
        Self(SpanContext::current_local_parent().map(|ctx| ctx.encode_w3c_traceparent()))
    }
}

/// Given an instance name returns a span with the sample percentage specified
/// in `knobs.rs`
pub fn get_sampled_span<R: Rng>(deployment_name: &str, name: &str, rng: &mut R) -> Span {
    let sample_ratio = get_sampling_ratio(deployment_name, name);
    let should_sample = rng.random_bool(sample_ratio);
    match should_sample {
        true => Span::root(name.to_owned(), SpanContext::random())
            .with_property(|| ("dev.convex.instance_name", deployment_name.to_owned())),
        false => Span::noop(),
    }
}

/// Psuedorandomly sample a span based on `key`, deterministically making the
/// same decision each time this function is called with the same `key`.
pub fn get_keyed_sampled_span<K: Hash + std::fmt::Debug>(
    key: K,
    deployment_name: &str,
    name: &str,
    span_ctx: SpanContext,
) -> Span {
    let mut hasher = FnvHasher::default();
    key.hash(&mut hasher);
    let hash = hasher.finish() as u32;
    let sample_ratio = get_sampling_ratio(deployment_name, name);
    let threshold = ((u32::MAX as f64) * sample_ratio) as u32;
    if hash < threshold {
        tracing::info!("Sampling span for {key:?}: {name}");
        Span::root(name.to_owned(), span_ctx)
            .with_property(|| ("dev.convex.instance_name", deployment_name.to_owned()))
    } else {
        tracing::info!("Not sampling span for {key:?}: {name}");
        Span::noop()
    }
}

/// Sets the sampling configuration to be used by the `get_sampled_span`
/// function
pub fn set_sampling_config(config: SamplingConfig) {
    tracing::info!("Sampling config set to: {config:?}");
    *SAMPLING_CONFIG_FROM_LOADER.lock() = Some(config);
}

fn get_sampling_ratio(deployment_name: &str, name: &str) -> f64 {
    if SAMPLING_CONFIG_FROM_LOADER.lock().is_some() {
        SAMPLING_CONFIG_FROM_LOADER
            .lock()
            .as_ref()
            .unwrap()
            .sample_ratio(deployment_name, name)
    } else {
        REQUEST_TRACE_SAMPLE_CONFIG.sample_ratio(deployment_name, name)
    }
}

#[derive(Debug, Default, Clone)]
pub struct SamplingConfig {
    by_regex: Vec<(Option<String>, Regex, f64)>,
}

impl PartialEq for SamplingConfig {
    fn eq(&self, other: &Self) -> bool {
        if self.by_regex.len() != other.by_regex.len() {
            return false;
        }
        self.by_regex
            .iter()
            .zip(&other.by_regex)
            .all(|(a, b)| a.0 == b.0 && a.1.as_str() == b.1.as_str() && a.2 == b.2)
    }
}

impl SamplingConfig {
    fn sample_ratio(&self, deployment_name: &str, name: &str) -> f64 {
        self.by_regex
            .iter()
            .find_map(|(rule_deployment_name, name_regex, sample_ratio)| {
                if let Some(rule_instance_name) = rule_deployment_name
                    && rule_instance_name != deployment_name
                {
                    return None;
                }
                if name_regex.is_match(name) {
                    Some(*sample_ratio)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                [
                    ("/api/push_config", 0.1),
                    ("conductor/load-instance", 0.01),
                    ("usage_tracking_worker/send_usage", 0.01),
                    ("commit", 0.01),
                ]
                .into_iter()
                .find_map(|(route, fraction)| name.contains(route).then_some(fraction))
                .unwrap_or(0.00001)
            })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteOverride {
    route_regexp: String,
    fraction: f64,
}

// These are in priority order -- instance overrides take precedence over route
// overrides, which take precedence over the default fraction.
//
// When in doubt, write out a test case to verify the behavior.
// Technically the default fraction is redundant with `routeOverrides: [{
// "routeRegexp": ".*", "fraction": ... }]`, but it's pulled out for clarity.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SamplingConfigJson {
    instance_overrides: Option<BTreeMap<String, Vec<RouteOverride>>>,
    route_overrides: Option<Vec<RouteOverride>>,
    default_fraction: Option<f64>,
}

fn validate_fraction(value: f64, context: &str) -> anyhow::Result<f64> {
    if !(0.0..=1.0).contains(&value) {
        anyhow::bail!(
            "Invalid fraction {} in {}: clamping to [0.0, 1.0]",
            value,
            context
        );
    }
    Ok(value)
}

static DOT_STAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(".*").expect(".* is not a valid regex"));

impl TryFrom<SamplingConfigJson> for SamplingConfig {
    type Error = anyhow::Error;

    fn try_from(json: SamplingConfigJson) -> anyhow::Result<Self> {
        let mut by_regex = Vec::new();
        if let Some(instance_overrides) = json.instance_overrides {
            for (deployment_name, route_overrides) in instance_overrides.iter() {
                for route_override in route_overrides {
                    by_regex.push((
                        Some(deployment_name.to_owned()),
                        Regex::new(&route_override.route_regexp).context("Invalid route regexp")?,
                        validate_fraction(route_override.fraction, deployment_name)?,
                    ));
                }
            }
        }
        if let Some(route_overrides) = json.route_overrides {
            for route_override in route_overrides {
                by_regex.push((
                    None,
                    Regex::new(&route_override.route_regexp).context("Invalid route regexp")?,
                    validate_fraction(route_override.fraction, &route_override.route_regexp)?,
                ));
            }
        }
        if let Some(default_fraction) = json.default_fraction {
            by_regex.push((
                None,
                DOT_STAR.clone(),
                validate_fraction(default_fraction, "default")?,
            ));
        }
        Ok(SamplingConfig { by_regex })
    }
}

impl FromStr for SamplingConfig {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let s = s.trim();
        if s.starts_with('{') {
            let json: SamplingConfigJson =
                serde_json::from_str(s).context("Failed to parse sampling config as JSON")?;
            return SamplingConfig::try_from(json);
        }

        let mut by_regex = Vec::new();
        for token in s.split(',') {
            let parts: Vec<_> = token.split(':').map(|s| s.trim()).collect();
            anyhow::ensure!(parts.len() <= 2, "Too many parts {}", token);
            let (deployment_name, token2) = if parts.len() == 2 {
                let instance_name = Some(parts[0].to_owned());
                (instance_name, parts[1])
            } else {
                (None, parts[0])
            };

            let parts: Vec<_> = token2.split('=').map(|s| s.trim()).collect();
            anyhow::ensure!(parts.len() <= 2, "Too many parts {}", token2);
            let (name_regex, rate) = if parts.len() == 2 {
                let regex = Regex::new(parts[0]).context("Failed to parse name regex")?;
                let rate: f64 = parts[1].parse().context("Failed to parse sampling rate")?;
                (regex, rate)
            } else {
                let rate: f64 = parts[0].parse().context("Failed to parse sampling rate")?;
                (DOT_STAR.clone(), rate)
            };
            by_regex.push((deployment_name, name_regex, rate));
        }
        Ok(SamplingConfig { by_regex })
    }
}

/// Creates a root span from an encoded parent trace
pub fn initialize_root_from_parent(span_name: &str, encoded_parent: EncodedSpan) -> Span {
    if let Some(p) = encoded_parent.0
        && let Some(ctx) = SpanContext::decode_w3c_traceparent(p.as_str())
    {
        return Span::root(span_name.to_string(), ctx);
    }
    Span::noop()
}

pub trait FutureExt: Sized {
    /// Create a fastrace span for this future starting on its first
    /// `Poll::Pending`. This avoids noise for futures that usually complete
    /// immediately.
    ///
    /// The future won't have a local span for its first poll, so if it itself
    /// uses fastrace it will probably create an unexpected span stack.
    fn trace_if_pending(self, name: &'static str) -> TraceIfPending<Self>;
}
impl<T: Future> FutureExt for T {
    fn trace_if_pending(self, name: &'static str) -> TraceIfPending<Self> {
        TraceIfPending {
            future: self,
            state: TraceIfPendingState::NotTracing(name),
        }
    }
}

enum TraceIfPendingState {
    NotTracing(&'static str),
    Tracing(Span),
    Done,
}

#[pin_project]
pub struct TraceIfPending<T> {
    #[pin]
    future: T,
    state: TraceIfPendingState,
}

impl<T: Future> Future for TraceIfPending<T> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let _guard;
        if let &mut TraceIfPendingState::Tracing(ref span) = this.state {
            _guard = span.set_local_parent();
        }
        let result = this.future.poll(cx);
        if result.is_ready() {
            *this.state = TraceIfPendingState::Done;
        } else if let TraceIfPendingState::NotTracing(name) = *this.state {
            *this.state = TraceIfPendingState::Tracing(Span::enter_with_local_parent(name));
        }
        result
    }
}
