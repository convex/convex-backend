pub use paste::paste;
pub use prometheus;

/// Register a histogram with the Convex metrics registry and store
/// in a static variable.
/// An optional third argument allows specifying labels for this metric.
/// The reported metric name will be the lower_snake_case version of the
/// declared variable name. An optional fourth argument sets the
/// eviction TTL for the labelled form.
///
/// Note the asymmetry with [`register_convex_gauge`]: labelled
/// histograms are auto-registered for inactivity eviction because
/// every `.observe()` refreshes the label-set's timestamp. Gauges
/// instead require opting in via [`register_convex_gauge_evictable`]
/// because "set once and leave it" is a valid usage pattern.
#[macro_export]
macro_rules! register_convex_histogram {
    ($VIS:vis $NAME:ident, $HELP:literal $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::VMHistogram> =
            std::sync::LazyLock::new(|| $crate::register_convex_histogram_owned!(
                $NAME,
                $HELP,
            ));
    };
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::VMHistogramVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_histogram_owned!(
                $NAME,
                $HELP,
                $LABELS,
            ));
    };
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr, $TTL:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::VMHistogramVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_histogram_owned!(
                $NAME,
                $HELP,
                $LABELS,
                $TTL,
            ));
    };
}

/// Register a histogram with the Convex metrics registry and return as
/// an expression.
/// An optional third argument allows specifying labels for this metric.
/// The reported metric name will be the lower_snake_case version of the
/// declared variable name.
#[macro_export]
macro_rules! register_convex_histogram_owned {
    ($NAME:ident, $HELP:literal $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        $crate::prometheus::register_vmhistogram_with_registry!(
            &*name,
            &*help,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed")
    }};
    ($NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {{
        $crate::register_convex_histogram_owned!($NAME, $HELP, $LABELS, $crate::ttl_from_env())
    }};
    ($NAME:ident, $HELP:literal, $LABELS:expr, $TTL:expr $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        let vec = $crate::prometheus::register_vmhistogram_vec_with_registry!(
            &*name,
            &*help,
            $LABELS,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed");
        $crate::register_evictable(&*name, std::sync::Arc::new(vec.clone()), $TTL);
        vec
    }};
}

/// Register an integer counter with the Convex metrics registry and store
/// in a static variable.
/// An optional third argument allows specifying labels for this metric.
/// The reported metric name will be the lower_snake_case version of the
/// declared variable name. An optional fourth argument sets the
/// eviction TTL for the labelled form.
///
/// Note the asymmetry with [`register_convex_gauge`]: labelled
/// counters are auto-registered for inactivity eviction because every
/// `.inc()` refreshes the label-set's timestamp. Gauges instead
/// require opting in via [`register_convex_gauge_evictable`] because
/// "set once and leave it" is a valid usage pattern.
#[macro_export]
macro_rules! register_convex_counter {
    ($VIS:vis $NAME:ident, $HELP:literal $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::IntCounter> =
            std::sync::LazyLock::new(|| $crate::register_convex_counter_owned!(
                $NAME,
                $HELP,
            ));
    };
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::IntCounterVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_counter_owned!(
                $NAME,
                $HELP,
                $LABELS,
            ));
    };
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr, $TTL:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::IntCounterVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_counter_owned!(
                $NAME,
                $HELP,
                $LABELS,
                $TTL,
            ));
    };
}

/// Register an integer counter with the Convex metrics registry and return
/// as an expression.
/// An optional third argument allows specifying labels for this metric.
/// The reported metric name will be the lower_snake_case version of the
/// declared variable name.
#[macro_export]
macro_rules! register_convex_counter_owned {
    ($NAME:ident, $HELP:literal $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        $crate::prometheus::register_int_counter_with_registry!(
            &*name,
            &*help,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed")
    }};
    ($NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {{
        $crate::register_convex_counter_owned!($NAME, $HELP, $LABELS, $crate::ttl_from_env())
    }};
    ($NAME:ident, $HELP:literal, $LABELS:expr, $TTL:expr $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        let vec = $crate::prometheus::register_int_counter_vec_with_registry!(
            &*name,
            &*help,
            $LABELS,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed");
        $crate::register_evictable(&*name, std::sync::Arc::new(vec.clone()), $TTL);
        vec
    }};
}

/// Register a floating-point gauge with the Convex metrics registry and
/// store in a static variable.
/// An optional third argument allows specifying labels for this metric.
/// The reported metric name will be the lower_snake_case version of the
/// declared variable name.
#[macro_export]
macro_rules! register_convex_gauge {
    ($VIS:vis $NAME:ident, $HELP:literal $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::Gauge> =
            std::sync::LazyLock::new(|| $crate::register_convex_gauge_owned!(
                $NAME,
                $HELP,
            ));
    };
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::GaugeVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_gauge_owned!(
                $NAME,
                $HELP,
                $LABELS,
            ));
    };
}

/// Register a floating-point gauge with the Convex metrics registry and return
/// as an expression.
/// An optional third argument allows specifying labels for this metric.
/// The reported metric name will be the lower_snake_case version of the
/// declared variable name.
#[macro_export]
macro_rules! register_convex_gauge_owned {
    ($NAME:ident, $HELP:literal $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        $crate::prometheus::register_gauge_with_registry!(
            &*name,
            &*help,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed")
    }};
    ($NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        $crate::prometheus::register_gauge_vec_with_registry!(
            &*name,
            &*help,
            $LABELS,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed")
    }};
}

/// Like [`register_convex_gauge`] but the labelled form is swept by the
/// inactivity eviction loop. Not for "set once" gauges — label sets
/// that haven't been re-`set` within the TTL are dropped.
#[macro_export]
macro_rules! register_convex_gauge_evictable {
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::GaugeVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_gauge_evictable_owned!(
                $NAME,
                $HELP,
                $LABELS,
                $crate::ttl_from_env(),
            ));
    };
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr, $TTL:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::GaugeVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_gauge_evictable_owned!(
                $NAME,
                $HELP,
                $LABELS,
                $TTL,
            ));
    };
}

#[macro_export]
macro_rules! register_convex_gauge_evictable_owned {
    ($NAME:ident, $HELP:literal, $LABELS:expr, $TTL:expr $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        let vec = $crate::prometheus::register_gauge_vec_with_registry!(
            &*name,
            &*help,
            $LABELS,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed");
        $crate::register_evictable(&*name, std::sync::Arc::new(vec.clone()), $TTL);
        vec
    }};
}

/// Register an integer gauge with the Convex metrics registry and
/// store in a static variable.
/// An optional third argument allows specifying labels for this metric.
/// The reported metric name will be the lower_snake_case version of the
/// declared variable name.
#[macro_export]
macro_rules! register_convex_int_gauge {
    ($VIS:vis $NAME:ident, $HELP:literal $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::IntGauge> =
            std::sync::LazyLock::new(|| $crate::register_convex_int_gauge_owned!(
                $NAME,
                $HELP,
            ));
    };
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::IntGaugeVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_int_gauge_owned!(
                $NAME,
                $HELP,
                $LABELS,
            ));
    };
}

/// Register an integer gauge with the Convex metrics registry and return
/// as an expression.
/// An optional third argument allows specifying labels for this metric.
/// The reported metric name will be the lower_snake_case version of the
/// declared variable name.
#[macro_export]
macro_rules! register_convex_int_gauge_owned {
    ($NAME:ident, $HELP:literal $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        $crate::prometheus::register_int_gauge_with_registry!(
            &*name,
            &*help,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed")
    }};
    ($NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        $crate::prometheus::register_int_gauge_vec_with_registry!(
            &*name,
            &*help,
            $LABELS,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed")
    }};
}

/// See [`register_convex_gauge_evictable`].
#[macro_export]
macro_rules! register_convex_int_gauge_evictable {
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::IntGaugeVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_int_gauge_evictable_owned!(
                $NAME,
                $HELP,
                $LABELS,
                $crate::ttl_from_env(),
            ));
    };
    ($VIS:vis $NAME:ident, $HELP:literal, $LABELS:expr, $TTL:expr $(,)?) => {
        $VIS static $NAME: std::sync::LazyLock<$crate::prometheus::IntGaugeVec> =
            std::sync::LazyLock::new(|| $crate::register_convex_int_gauge_evictable_owned!(
                $NAME,
                $HELP,
                $LABELS,
                $TTL,
            ));
    };
}

#[macro_export]
macro_rules! register_convex_int_gauge_evictable_owned {
    ($NAME:ident, $HELP:literal, $LABELS:expr, $TTL:expr $(,)?) => {{
        $crate::paste! {
            let name = $crate::metric_name!(stringify!([<$NAME:lower>]));
        }
        let help = $crate::metric_help!($HELP);
        #[allow(clippy::disallowed_macros)]
        let vec = $crate::prometheus::register_int_gauge_vec_with_registry!(
            &*name,
            &*help,
            $LABELS,
            $crate::CONVEX_METRICS_REGISTRY,
        )
        .expect("Metric initialization failed");
        $crate::register_evictable(&*name, std::sync::Arc::new(vec.clone()), $TTL);
        vec
    }};
}
