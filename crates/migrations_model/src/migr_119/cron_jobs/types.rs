use std::{
    borrow::Borrow,
    collections::BTreeMap,
    mem,
    ops::Deref,
    str::FromStr,
};

use anyhow::{
    bail,
    Context,
};
use common::{
    log_lines::RawLogLines,
    types::Timestamp,
};
use saffron::Cron;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
use sync_types::{
    CanonicalizedUdfPath,
    UdfPath,
};
use value::{
    codegen_convex_serialization,
    heap_size::HeapSize,
    id_v6::DeveloperDocumentId,
    json_deserialize,
    obj,
    ConvexArray,
    ConvexObject,
    ConvexValue,
};

#[derive(thiserror::Error, Debug, Clone)]
pub enum CronValidationError {
    #[error("Invalid JSON")]
    InvalidJson,
    #[error("Exactly one of (seconds, minutes, hours) should be specified")]
    SecondsMinutesHours,
    #[error("Interval must be an integer greater than 0")]
    InvalidIntervalValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CronJob {
    // Unique identifier of a cron
    pub name: CronIdentifier,

    // Cron-related metadata specified by the user and updated on pushes
    pub cron_spec: CronSpec,

    // Internally tracked metadata to execute the current run of the cron
    pub state: CronJobState,
    pub prev_ts: Option<Timestamp>,
    pub next_ts: Timestamp,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedCronJob {
    name: String,
    cron_spec: SerializedCronSpec,
    state: CronJobState,
    prev_ts: Option<i64>,
    next_ts: i64,
}

impl TryFrom<CronJob> for SerializedCronJob {
    type Error = anyhow::Error;

    fn try_from(job: CronJob) -> anyhow::Result<Self, Self::Error> {
        Ok(Self {
            name: job.name.to_string(),
            cron_spec: job.cron_spec.try_into()?,
            state: job.state,
            prev_ts: job.prev_ts.map(|ts| ts.into()),
            next_ts: job.next_ts.into(),
        })
    }
}

impl TryFrom<SerializedCronJob> for CronJob {
    type Error = anyhow::Error;

    fn try_from(value: SerializedCronJob) -> anyhow::Result<Self, Self::Error> {
        Ok(Self {
            name: value.name.parse()?,
            cron_spec: value.cron_spec.try_into()?,
            state: value.state,
            prev_ts: value.prev_ts.map(|ts| ts.try_into()).transpose()?,
            next_ts: value.next_ts.try_into()?,
        })
    }
}

codegen_convex_serialization!(CronJob, SerializedCronJob);

/// Check that a string can be used as a CronIdentifier.
pub fn check_valid_cron_identifier(s: &str) -> anyhow::Result<()> {
    for c in s.chars() {
        if !c.is_ascii() || c.is_ascii_control() {
            anyhow::bail!(
                "CronIdentifier {s} has invalid character '{c}': CronIdentifiers can only contain \
                 ASCII letters, numbers, spaces, underscores, dashes and apostrophes"
            );
        }
    }
    Ok(())
}

/// Identifiers of CronSpecs, names in CronJob.
#[derive(Hash, Eq, Ord, PartialEq, PartialOrd, Clone, Debug, derive_more::Display)]
pub struct CronIdentifier(String);

impl HeapSize for CronIdentifier {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

impl FromStr for CronIdentifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        check_valid_cron_identifier(s)?;
        Ok(Self(s.to_owned()))
    }
}

impl Deref for CronIdentifier {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Borrow<str> for CronIdentifier {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CronSpec {
    pub udf_path: CanonicalizedUdfPath,
    pub udf_args: ConvexArray,
    pub cron_schedule: CronSchedule,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedCronSpec {
    udf_path: String,
    #[serde(with = "serde_bytes")]
    udf_args: Option<Vec<u8>>,
    cron_schedule: SerializedCronSchedule,
}

impl TryFrom<CronSpec> for SerializedCronSpec {
    type Error = anyhow::Error;

    fn try_from(spec: CronSpec) -> anyhow::Result<Self, Self::Error> {
        // Serialize the udf arguments as binary since we restrict what
        // field names can be used in a `Document`'s top-level object.
        let udf_args_bytes = spec.udf_args.json_serialize()?.into_bytes();
        Ok(Self {
            udf_path: String::from(spec.udf_path),
            udf_args: Some(udf_args_bytes),
            cron_schedule: spec.cron_schedule.try_into()?,
        })
    }
}

impl TryFrom<SerializedCronSpec> for CronSpec {
    type Error = anyhow::Error;

    fn try_from(value: SerializedCronSpec) -> anyhow::Result<Self, Self::Error> {
        let udf_path = value.udf_path.parse()?;
        let udf_args = match value.udf_args {
            Some(b) => {
                let udf_args_json: JsonValue = serde_json::from_slice(&b)?;
                udf_args_json.try_into()?
            },
            None => ConvexArray::try_from(vec![])?,
        };
        let cron_schedule = value.cron_schedule.try_into()?;
        Ok(Self {
            udf_path,
            udf_args,
            cron_schedule,
        })
    }
}

mod codegen_cron_spec {
    use value::codegen_convex_serialization;

    use super::{
        CronSpec,
        SerializedCronSpec,
    };

    codegen_convex_serialization!(CronSpec, SerializedCronSpec);
}

impl TryFrom<JsonValue> for CronSpec {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        enum DayOfWeek {
            Sunday,
            Monday,
            Tuesday,
            Wednesday,
            Thursday,
            Friday,
            Saturday,
        }

        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum ScheduleJson {
            #[serde(rename = "interval")]
            Interval {
                seconds: Option<i64>,
                minutes: Option<i64>,
                hours: Option<i64>,
            },
            #[serde(rename = "hourly")]
            Hourly {
                #[serde(rename = "minuteUTC")]
                minute_utc: i64,
            },
            #[serde(rename = "daily")]
            Daily {
                #[serde(rename = "minuteUTC")]
                minute_utc: i64,
                #[serde(rename = "hourUTC")]
                hour_utc: i64,
            },
            #[serde(rename_all = "camelCase")]
            #[serde(rename = "weekly")]
            Weekly {
                #[serde(rename = "minuteUTC")]
                minute_utc: i64,
                #[serde(rename = "hourUTC")]
                hour_utc: i64,
                day_of_week: DayOfWeek,
            },
            #[serde(rename_all = "camelCase")]
            #[serde(rename = "monthly")]
            Monthly {
                #[serde(rename = "minuteUTC")]
                minute_utc: i64,
                #[serde(rename = "hourUTC")]
                hour_utc: i64,
                day: i64,
            },
            #[serde(rename_all = "camelCase")]
            #[serde(rename = "cron")]
            Cron { cron: String },
        }

        // The JavaScript object produced by crons.export() uses different names:
        // name -> udf_path, schedule -> cron_schedule, args -> udf_args
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct CronSpecJson {
            name: String,
            args: JsonValue,
            schedule: ScheduleJson,
        }
        let j: CronSpecJson = serde_json::from_value(value.clone())
            .with_context(|| CronValidationError::InvalidJson)?;

        let schedule = match j.schedule {
            ScheduleJson::Interval {
                seconds,
                minutes,
                hours,
            } => {
                let num_time_fields =
                    seconds.is_some() as i64 + minutes.is_some() as i64 + hours.is_some() as i64;
                if num_time_fields != 1 {
                    anyhow::bail!(CronValidationError::SecondsMinutesHours);
                }
                if let Some(seconds) = seconds
                    && seconds <= 0
                {
                    anyhow::bail!(CronValidationError::InvalidIntervalValue)
                }
                if let Some(minutes) = minutes
                    && minutes <= 0
                {
                    anyhow::bail!(CronValidationError::InvalidIntervalValue)
                }
                if let Some(hours) = hours
                    && hours <= 0
                {
                    anyhow::bail!(CronValidationError::InvalidIntervalValue)
                }
                let seconds =
                    seconds.unwrap_or(0) + minutes.unwrap_or(0) * 60 + hours.unwrap_or(0) * 3600;

                CronSchedule::Interval { seconds }
            },
            ScheduleJson::Hourly { minute_utc } => {
                if !(0..=59).contains(&minute_utc) {
                    anyhow::bail!(
                        "minuteUTC must be 0-59 in {}",
                        serde_json::to_string_pretty(&value).unwrap()
                    );
                }
                CronSchedule::Hourly { minute_utc }
            },
            ScheduleJson::Daily {
                minute_utc,
                hour_utc,
            } => {
                if !(0..=59).contains(&minute_utc) {
                    anyhow::bail!(
                        "minuteUTC must be 0-59 in {}",
                        serde_json::to_string_pretty(&value).unwrap()
                    );
                }
                if !(0..=23).contains(&hour_utc) {
                    anyhow::bail!(
                        "hourUTC must be 0-23 in {}",
                        serde_json::to_string_pretty(&value).unwrap()
                    );
                }
                CronSchedule::Daily {
                    minute_utc,
                    hour_utc,
                }
            },
            ScheduleJson::Weekly {
                minute_utc,
                hour_utc,
                day_of_week,
            } => {
                if !(0..=59).contains(&minute_utc) {
                    anyhow::bail!(
                        "minuteUTC must be 0-59 in {}",
                        serde_json::to_string_pretty(&value).unwrap()
                    );
                }
                if !(0..=23).contains(&hour_utc) {
                    anyhow::bail!(
                        "hourUTC must be 0-23 in {}",
                        serde_json::to_string_pretty(&value).unwrap()
                    );
                }
                CronSchedule::Weekly {
                    minute_utc,
                    hour_utc,
                    day_of_week: match day_of_week {
                        DayOfWeek::Sunday => 0,
                        DayOfWeek::Monday => 1,
                        DayOfWeek::Tuesday => 2,
                        DayOfWeek::Wednesday => 3,
                        DayOfWeek::Thursday => 4,
                        DayOfWeek::Friday => 5,
                        DayOfWeek::Saturday => 6,
                    },
                }
            },
            ScheduleJson::Monthly {
                minute_utc,
                hour_utc,
                day,
            } => {
                if !(0..=59).contains(&minute_utc) {
                    anyhow::bail!(
                        "minuteUTC must be 0-59 in {}",
                        serde_json::to_string_pretty(&value).unwrap()
                    );
                }
                if !(0..=23).contains(&hour_utc) {
                    anyhow::bail!(
                        "hourUTC must be 0-23 in {}",
                        serde_json::to_string_pretty(&value).unwrap()
                    );
                }
                if !(1..=31).contains(&day) {
                    anyhow::bail!(
                        "day of month must be 1-31 in {}",
                        serde_json::to_string_pretty(&value).unwrap()
                    );
                }
                CronSchedule::Monthly {
                    day,
                    hour_utc,
                    minute_utc,
                }
            },
            ScheduleJson::Cron { cron } => {
                cron.parse::<saffron::Cron>()?;
                CronSchedule::Cron { cron_expr: cron }
            },
        };

        let udf_path: UdfPath = j.name.parse()?;
        let udf_path_canonicalized = udf_path.canonicalize();
        Ok(Self {
            udf_path: udf_path_canonicalized,
            udf_args: ConvexArray::try_from(j.args)?,
            cron_schedule: schedule,
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum CronJobState {
    // Yet to be attempted.
    Pending,
    // Started but not completed yet. Used to make actions execute at most once.
    InProgress,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CronSchedule {
    Interval {
        seconds: i64,
    },
    Hourly {
        minute_utc: i64,
    },
    Daily {
        hour_utc: i64,
        minute_utc: i64,
    },
    Weekly {
        day_of_week: i64,
        hour_utc: i64,
        minute_utc: i64,
    },
    Monthly {
        day: i64,
        hour_utc: i64,
        minute_utc: i64,
    },
    Cron {
        cron_expr: String,
    },
}

impl HeapSize for CronSchedule {
    fn heap_size(&self) -> usize {
        match self {
            CronSchedule::Interval { .. } => mem::size_of::<i64>(),
            CronSchedule::Hourly { .. } => mem::size_of::<i64>(),
            CronSchedule::Daily { .. } => 2 * mem::size_of::<i64>(),
            CronSchedule::Monthly { .. } | CronSchedule::Weekly { .. } => 3 * mem::size_of::<i64>(),
            CronSchedule::Cron { cron_expr } => cron_expr.heap_size(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SerializedCronSchedule {
    Interval {
        seconds: i64,
    },
    Hourly {
        #[serde(rename = "minuteUTC")]
        minute_utc: i64,
    },
    Daily {
        #[serde(rename = "hourUTC")]
        hour_utc: i64,
        #[serde(rename = "minuteUTC")]
        minute_utc: i64,
    },
    Weekly {
        #[serde(rename = "dayOfWeek")]
        day_of_week: i64,
        #[serde(rename = "hourUTC")]
        hour_utc: i64,
        #[serde(rename = "minuteUTC")]
        minute_utc: i64,
    },
    Monthly {
        day: i64,
        #[serde(rename = "hourUTC")]
        hour_utc: i64,
        #[serde(rename = "minuteUTC")]
        minute_utc: i64,
    },
    #[serde(rename_all = "camelCase")]
    Cron {
        cron_expr: String,
    },
}

impl TryFrom<CronSchedule> for SerializedCronSchedule {
    type Error = anyhow::Error;

    fn try_from(schedule: CronSchedule) -> anyhow::Result<Self, Self::Error> {
        match schedule {
            CronSchedule::Interval { seconds } => Ok(Self::Interval { seconds }),
            CronSchedule::Hourly { minute_utc } => Ok(Self::Hourly { minute_utc }),
            CronSchedule::Daily {
                hour_utc,
                minute_utc,
            } => Ok(Self::Daily {
                hour_utc,
                minute_utc,
            }),
            CronSchedule::Weekly {
                day_of_week,
                hour_utc,
                minute_utc,
            } => Ok(Self::Weekly {
                day_of_week,
                hour_utc,
                minute_utc,
            }),
            CronSchedule::Monthly {
                day,
                hour_utc,
                minute_utc,
            } => Ok(Self::Monthly {
                day,
                hour_utc,
                minute_utc,
            }),
            CronSchedule::Cron { cron_expr } => Ok(Self::Cron { cron_expr }),
        }
    }
}

impl TryFrom<SerializedCronSchedule> for CronSchedule {
    type Error = anyhow::Error;

    fn try_from(value: SerializedCronSchedule) -> anyhow::Result<Self, Self::Error> {
        match value {
            SerializedCronSchedule::Interval { seconds } => Ok(CronSchedule::Interval { seconds }),
            SerializedCronSchedule::Hourly { minute_utc } => {
                Ok(CronSchedule::Hourly { minute_utc })
            },
            SerializedCronSchedule::Daily {
                hour_utc,
                minute_utc,
            } => Ok(CronSchedule::Daily {
                hour_utc,
                minute_utc,
            }),
            SerializedCronSchedule::Weekly {
                day_of_week,
                hour_utc,
                minute_utc,
            } => Ok(CronSchedule::Weekly {
                day_of_week,
                hour_utc,
                minute_utc,
            }),
            SerializedCronSchedule::Monthly {
                day,
                hour_utc,
                minute_utc,
            } => Ok(CronSchedule::Monthly {
                day,
                hour_utc,
                minute_utc,
            }),
            SerializedCronSchedule::Cron { cron_expr } => Ok(CronSchedule::Cron { cron_expr }),
        }
    }
}

mod codegen_cron_schedule {
    use value::codegen_convex_serialization;

    use super::{
        CronSchedule,
        SerializedCronSchedule,
    };

    codegen_convex_serialization!(CronSchedule, SerializedCronSchedule);
}

#[derive(Debug, Serialize)]
#[allow(dead_code)] // TODO: remove
pub enum CronScheduleProductAnalysis {
    Interval {
        seconds: i64,
    },
    Hourly {
        minute_utc: i64,
    },
    Daily {
        hour_utc: i64,
        minute_utc: i64,
    },
    Weekly {
        day_of_week: i64,
        hour_utc: i64,
        minute_utc: i64,
    },
    Monthly {
        day: i64,
        hour_utc: i64,
        minute_utc: i64,
    },
    Cron {
        cron_expr: String,
    },
}

impl From<CronSchedule> for CronScheduleProductAnalysis {
    fn from(schedule: CronSchedule) -> Self {
        match schedule {
            CronSchedule::Interval { seconds } => Self::Interval { seconds },
            CronSchedule::Hourly { minute_utc } => Self::Hourly { minute_utc },
            CronSchedule::Daily {
                hour_utc,
                minute_utc,
            } => Self::Daily {
                hour_utc,
                minute_utc,
            },
            CronSchedule::Weekly {
                day_of_week,
                hour_utc,
                minute_utc,
            } => Self::Weekly {
                day_of_week,
                hour_utc,
                minute_utc,
            },
            CronSchedule::Monthly {
                day,
                hour_utc,
                minute_utc,
            } => Self::Monthly {
                day,
                hour_utc,
                minute_utc,
            },
            CronSchedule::Cron { cron_expr } => Self::Cron { cron_expr },
        }
    }
}

impl CronSchedule {
    pub fn validate_format(&self) -> anyhow::Result<()> {
        let _: Cron = match self.clone() {
            CronSchedule::Interval { seconds } => {
                if seconds <= 0 {
                    bail!("CronSchedule intervals must have a positive duration.");
                }
                return Ok(());
            },
            CronSchedule::Hourly { minute_utc } => format!("{minute_utc} * * * *")
                .parse()
                .context("Hourly Schedule: Cron parsing from Saffron failed")?,
            CronSchedule::Daily {
                hour_utc,
                minute_utc,
            } => format!("{minute_utc} {hour_utc} * * *")
                .parse()
                .context("Daily Schedule: Cron parsing from Saffron failed")?,
            CronSchedule::Weekly {
                day_of_week,
                hour_utc,
                minute_utc,
            } => format!("{minute_utc} {hour_utc} * * {day_of_week}")
                .parse()
                .context("Weekly Schedule: Cron parsing from Saffron failed")?,
            CronSchedule::Monthly {
                day,
                hour_utc,
                minute_utc,
            } => format!("{minute_utc} {hour_utc} {day} * *")
                .parse()
                .context("Monthly Schedule: Cron parsing from Saffron failed")?,
            CronSchedule::Cron { cron_expr } => cron_expr
                .parse()
                .context("Cron Schedule: Cron parsing from Saffron failed")?,
        };
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CronJobLog {
    pub name: CronIdentifier,
    pub ts: Timestamp,
    pub udf_path: CanonicalizedUdfPath,
    pub udf_args: ConvexArray,
    pub status: CronJobStatus,
    pub log_lines: CronJobLogLines,
    pub execution_time: f64,
}

impl TryFrom<CronJobLog> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(log: CronJobLog) -> anyhow::Result<Self, Self::Error> {
        // Serialize the udf arguments as binary since we restrict what
        // field names can be used in a `Document`'s top-level object.
        let udf_args_bytes = log.udf_args.json_serialize()?.into_bytes();

        obj!(
            "name" => log.name.to_string(),
            "ts" => ConvexValue::Int64(log.ts.into()),
            "udfPath" => String::from(log.udf_path),
            "udfArgs" => udf_args_bytes,
            "status" => ConvexValue::Object(log.status.try_into()?),
            "logLines" => ConvexValue::Object(log.log_lines.try_into()?),
            "executionTime" => log.execution_time,
        )
    }
}

impl TryFrom<ConvexObject> for CronJobLog {
    type Error = anyhow::Error;

    fn try_from(value: ConvexObject) -> anyhow::Result<Self, Self::Error> {
        let mut fields: BTreeMap<_, _> = value.into();

        let name = match fields.remove("name") {
            Some(ConvexValue::String(s)) => CronIdentifier::from_str(s.to_string().as_str())?,
            _ => anyhow::bail!(
                "Missing or invalid `name` field for CronJobLog: {:?}",
                fields
            ),
        };
        let ts = match fields.remove("ts") {
            Some(ConvexValue::Int64(ts)) => ts.try_into()?,
            _ => anyhow::bail!("Missing or invalid `ts` field for CronJobLog: {:?}", fields),
        };
        let udf_path = match fields.remove("udfPath") {
            Some(ConvexValue::String(s)) => s,
            _ => anyhow::bail!(
                "Missing or invalid `udfPath` field for CronJobLog: {:?}",
                fields
            ),
        };
        let udf_path: CanonicalizedUdfPath = udf_path
            .parse()
            .context(format!("Failed to deserialize udf_path {udf_path}"))?;
        let udf_args = match fields.remove("udfArgs") {
            Some(ConvexValue::Bytes(b)) => {
                let udf_args_json: JsonValue = serde_json::from_slice(&b)?;
                udf_args_json.try_into()?
            },
            _ => anyhow::bail!(
                "Missing or invalid `udfArgs` field for CronJobLog: {:?}",
                fields
            ),
        };
        let status = match fields.remove("status") {
            Some(ConvexValue::Object(o)) => o.try_into()?,
            _ => anyhow::bail!(
                "Missing or invalid `status` field for CronJobLog: {:?}",
                fields
            ),
        };
        let log_lines = match fields.remove("logLines") {
            Some(ConvexValue::Object(o)) => o.try_into()?,
            _ => anyhow::bail!(
                "Missing or invalid `logLines` field for CronJobLog: {:?}",
                fields
            ),
        };
        let execution_time = match fields.remove("executionTime") {
            Some(ConvexValue::Float64(ts)) => ts,
            _ => anyhow::bail!(
                "Missing or invalid `executionTime` field for CronJobLog: {:?}",
                fields
            ),
        };

        Ok(Self {
            name,
            ts,
            udf_path,
            udf_args,
            status,
            log_lines,
            execution_time,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CronJobStatus {
    Success(CronJobResult),
    Err(String),
    Canceled { num_canceled: i64 },
}

impl TryFrom<CronJobStatus> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(status: CronJobStatus) -> anyhow::Result<Self, Self::Error> {
        match status {
            CronJobStatus::Success(r) => {
                obj!(
                    "type" => "success",
                    "result" => ConvexValue::Object(r.try_into()?),
                )
            },
            CronJobStatus::Err(e) => {
                obj!("type" => "err", "error" => e)
            },
            CronJobStatus::Canceled { num_canceled } => {
                obj!("type" => "canceled", "num_canceled" => num_canceled)
            },
        }
    }
}

impl TryFrom<ConvexObject> for CronJobStatus {
    type Error = anyhow::Error;

    fn try_from(value: ConvexObject) -> anyhow::Result<Self, Self::Error> {
        let mut fields: BTreeMap<_, _> = value.into();
        let status_t = match fields.remove("type") {
            Some(ConvexValue::String(s)) => s,
            _ => anyhow::bail!(
                "Missing or invalid `type` field for CronJobStatus: {:?}",
                fields
            ),
        };

        match status_t.as_ref() {
            "success" => {
                let result = match fields.remove("result") {
                    Some(ConvexValue::Object(o)) => o.try_into()?,
                    _ => anyhow::bail!(
                        "Missing or invalid `value` field for CronJobStatus: {:?}",
                        fields
                    ),
                };
                Ok(CronJobStatus::Success(result))
            },
            "err" => {
                let err = match fields.remove("error") {
                    Some(ConvexValue::String(s)) => s,
                    _ => anyhow::bail!(
                        "Missing or invalid `error` field for CronJobStatus: {:?}",
                        fields
                    ),
                };
                Ok(CronJobStatus::Err(err.into()))
            },
            "canceled" => {
                let num_canceled = match fields.remove("num_canceled") {
                    Some(ConvexValue::Int64(i)) => i,
                    _ => anyhow::bail!(
                        "Missing or invalid `num_canceled` field for CronJobStatus: {:?}",
                        fields
                    ),
                };
                Ok(CronJobStatus::Canceled { num_canceled })
            },
            _ => anyhow::bail!("Invalid CronJobStatus `type`: {}", status_t),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CronJobResult {
    Default(ConvexValue),
    Truncated(String),
}

impl TryFrom<CronJobResult> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(result: CronJobResult) -> anyhow::Result<Self, Self::Error> {
        match result {
            CronJobResult::Default(v) => {
                obj!(
                    "type" => "default",
                    "value" => v.json_serialize()?,
                )
            },
            CronJobResult::Truncated(s) => {
                obj!("type" => "truncated", "truncated_log" => s)
            },
        }
    }
}

impl TryFrom<ConvexObject> for CronJobResult {
    type Error = anyhow::Error;

    fn try_from(value: ConvexObject) -> anyhow::Result<Self, Self::Error> {
        let mut fields: BTreeMap<_, _> = value.into();
        let result_t = match fields.remove("type") {
            Some(ConvexValue::String(s)) => s,
            _ => anyhow::bail!(
                "Missing or invalid `type` field for CronJobResult: {:?}",
                fields
            ),
        };

        match result_t.as_ref() {
            "default" => {
                let value = match fields.remove("value") {
                    Some(ConvexValue::String(s)) => json_deserialize(&s)?,
                    _ => anyhow::bail!(
                        "Missing or invalid `value` field for CronJobResult: {:?}",
                        fields
                    ),
                };
                Ok(CronJobResult::Default(value))
            },
            "truncated" => {
                let truncated_log = match fields.remove("truncated_log") {
                    Some(ConvexValue::String(s)) => s,
                    _ => anyhow::bail!(
                        "Missing or invalid `truncated_log` field for CronJobResult: {:?}",
                        fields
                    ),
                };
                Ok(CronJobResult::Truncated(truncated_log.into()))
            },
            _ => anyhow::bail!("Invalid CronJobResult `type`: {}", result_t),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CronJobLogLines {
    pub log_lines: RawLogLines,
    pub is_truncated: bool,
}

impl TryFrom<CronJobLogLines> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(cron_log: CronJobLogLines) -> anyhow::Result<Self, Self::Error> {
        let log_lines: anyhow::Result<Vec<ConvexValue>> = cron_log
            .log_lines
            .into_iter()
            .map(ConvexValue::try_from)
            .collect();
        obj!(
            "logLines" => ConvexValue::Array(log_lines?.try_into()?),
            "isTruncated" => ConvexValue::Boolean(cron_log.is_truncated),
        )
    }
}

impl TryFrom<ConvexObject> for CronJobLogLines {
    type Error = anyhow::Error;

    fn try_from(value: ConvexObject) -> anyhow::Result<Self, Self::Error> {
        let mut fields: BTreeMap<_, _> = value.into();
        let log_lines: RawLogLines = match fields.remove("logLines") {
            Some(ConvexValue::Array(a)) => a
                .into_iter()
                .map(String::try_from)
                .collect::<anyhow::Result<RawLogLines>>()?,
            _ => anyhow::bail!(
                "Missing or invalid `logLines` field for CronJobLogLines: {:?}",
                fields
            ),
        };
        let is_truncated = match fields.remove("isTruncated") {
            Some(ConvexValue::Boolean(b)) => b,
            _ => anyhow::bail!(
                "Missing or invalid `isTruncated` field for CronJobLogLines: {:?}",
                fields
            ),
        };
        Ok(CronJobLogLines {
            log_lines,
            is_truncated,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CronNextRun {
    // Internally tracked metadata to execute the current run of the cron
    pub cron_job_id: DeveloperDocumentId,
    pub state: CronJobState,
    pub prev_ts: Option<Timestamp>,
    pub next_ts: Timestamp,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedCronNextRun {
    cron_job_id: String,
    state: CronJobState,
    prev_ts: Option<i64>,
    next_ts: i64,
}

impl From<CronNextRun> for SerializedCronNextRun {
    fn from(run: CronNextRun) -> Self {
        Self {
            state: run.state,
            prev_ts: run.prev_ts.map(|ts| ts.into()),
            next_ts: run.next_ts.into(),
            cron_job_id: run.cron_job_id.encode(),
        }
    }
}

impl TryFrom<SerializedCronNextRun> for CronNextRun {
    type Error = anyhow::Error;

    fn try_from(value: SerializedCronNextRun) -> anyhow::Result<Self, Self::Error> {
        Ok(Self {
            cron_job_id: DeveloperDocumentId::decode(&value.cron_job_id)?,
            state: value.state,
            prev_ts: value.prev_ts.map(|ts| ts.try_into()).transpose()?,
            next_ts: value.next_ts.try_into()?,
        })
    }
}

codegen_convex_serialization!(CronNextRun, SerializedCronNextRun);
