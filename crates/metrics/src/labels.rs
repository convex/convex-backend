use std::borrow::Cow;

/// Type alias for when you need to make a `MetricLabel` from an unowned string,
/// and you'd rather just make it owned than propagate lifetimes.
pub type StaticMetricLabel = MetricLabel<'static>;

pub type Labels<'a> = Vec<MetricLabel<'a>>;

#[derive(PartialOrd, Ord, PartialEq, Eq, Clone, Debug)]
pub struct MetricLabel<'a> {
    pub key: &'static str,
    // Allows passing in an owned `String` or an `&'a str` -- the latter is useful if you have a
    // `&str` and need to create the `MetricLabel` to immediately report a metric, such that the
    // label doesn't need to outlive the current scope.
    pub value: Cow<'a, str>,
}

impl<'a> MetricLabel<'a> {
    pub const STATUS_AUTH_UPDATE_FAILED_ERROR: StaticMetricLabel = MetricLabel {
        key: "status",
        value: Cow::Borrowed("auth_update_failed_error"),
    };
    pub const STATUS_CANCELED: StaticMetricLabel = MetricLabel {
        key: "status",
        value: Cow::Borrowed("canceled"),
    };
    pub const STATUS_DEVELOPER_ERROR: StaticMetricLabel = MetricLabel {
        key: "status",
        value: Cow::Borrowed("developer_error"),
    };
    pub const STATUS_ERROR: StaticMetricLabel = MetricLabel {
        key: "status",
        value: Cow::Borrowed("error"),
    };
    pub const STATUS_SUCCESS: StaticMetricLabel = MetricLabel {
        key: "status",
        value: Cow::Borrowed("success"),
    };
    pub const STATUS_UNAUTHENTICATED_ERROR: StaticMetricLabel = MetricLabel {
        key: "status",
        value: Cow::Borrowed("unauthenticated_error"),
    };

    pub fn new(key: &'static str, value: impl Into<Cow<'a, str>>) -> Self {
        Self {
            key,
            value: value.into(),
        }
    }

    pub const fn new_const(key: &'static str, value: &'static str) -> StaticMetricLabel {
        MetricLabel {
            key,
            value: Cow::Borrowed(value),
        }
    }

    /// Common labels. Use these instead of custom defined ones when possible.
    pub fn status(is_ok: bool) -> MetricLabel<'static> {
        if is_ok {
            Self::STATUS_SUCCESS
        } else {
            Self::STATUS_ERROR
        }
    }

    pub fn split_key_value(&self) -> (&str, &str) {
        (self.key, &self.value)
    }
}

pub const STATUS_LABEL: [&str; 1] = ["status"];

pub trait IntoLabel {
    fn as_label(&self) -> &'static str;
}

impl IntoLabel for bool {
    fn as_label(&self) -> &'static str {
        if *self {
            "true"
        } else {
            "false"
        }
    }
}

pub const CLUSTER_LABEL: &str = "cluster";
pub fn cluster_label(cluster: &'static str) -> StaticMetricLabel {
    StaticMetricLabel::new(CLUSTER_LABEL, cluster)
}

pub const METHOD_LABEL: &str = "method";
/// Maps an arbitrary (potentially client-controlled) HTTP method to a bounded
/// set of static label values so metric cardinality can't be inflated by
/// unusual/custom method tokens.
pub fn method_label(method: &str) -> StaticMetricLabel {
    let value = match method {
        "GET" => "GET",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "PATCH" => "PATCH",
        "HEAD" => "HEAD",
        "OPTIONS" => "OPTIONS",
        _ => "other",
    };
    StaticMetricLabel::new(METHOD_LABEL, value)
}
