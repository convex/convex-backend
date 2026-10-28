use std::fmt::{
    self,
    Display,
};

use common::{
    components::ComponentId,
    types::ObjectKey,
};
use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;
use value::codegen_convex_serialization;

#[derive(Clone, Debug, PartialEq)]
/// The export state machine. A new export starts as `Requested` and the valid
/// transitions are:
///
/// - Requested -> InProgress: the export worker started working on an export
///   the first time
/// - InProgress -> Completed: the export worker finished and created a zip
///   object
/// - InProgress -> Failed: not currently possible, but could be used in the
///   future if the export worker encounters an unrecoverable error
/// - Requested,InProgress -> Canceled: an admin canceled the export, which may
///   or may not have started.
///
/// Completed, Failed, and Canceled are terminal states.
pub enum Export {
    Requested {
        format: ExportFormat,
        component: ComponentId,
        requestor: ExportRequestor,
        /// Expiration timestamp in nanos
        expiration_ts: u64,
    },
    InProgress {
        /// Timestamp when the first attempt
        /// at the Export started.
        start_ts: Timestamp,
        format: ExportFormat,
        component: ComponentId,
        requestor: ExportRequestor,
        /// Expiration timestamp in nanos
        expiration_ts: u64,
        progress_message: Option<String>,
        resumption_token: Option<serde_json::Map<String, serde_json::Value>>,
    },
    Completed {
        /// Timestamp for the successful (final) attempt at Export.
        start_ts: Timestamp,
        /// Timestamp when the Export completed
        complete_ts: Timestamp,
        /// Expiration timestamp in nanos
        expiration_ts: u64,
        /// Object keys in S3
        zip_object_key: ObjectKey,
        /// Format of the export
        format: ExportFormat,
        component: ComponentId,
        requestor: ExportRequestor,
        size: u64,
    },
    Failed {
        /// Timestamp for the failed (final) attempt at Export.
        start_ts: Timestamp,
        /// Timestamp when the Export failed
        failed_ts: Timestamp,
        format: ExportFormat,
        component: ComponentId,
        requestor: ExportRequestor,
    },
    Canceled {
        /// When the Export first started, if at all
        start_ts: Option<Timestamp>,
        /// Timestamp when the Export was canceled
        canceled_ts: Timestamp,
        format: ExportFormat,
        component: ComponentId,
        requestor: ExportRequestor,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state")]
#[serde(rename_all = "snake_case")]
pub enum SerializedExport {
    Requested {
        format: SerializedExportFormat,
        component: Option<String>,
        requestor: String,
        expiration_ts: i64,
    },
    InProgress {
        start_ts: u64,
        format: SerializedExportFormat,
        component: Option<String>,
        requestor: String,
        expiration_ts: i64,
        progress_message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        resumption_token: Option<serde_json::Map<String, serde_json::Value>>,
    },
    Completed {
        start_ts: u64,
        complete_ts: u64,
        expiration_ts: i64,
        zip_object_key: String,
        format: SerializedExportFormat,
        component: Option<String>,
        requestor: String,
        #[serde(default)] // 0-default for backward compatibility
        size: i64,
    },
    Failed {
        start_ts: u64,
        failed_ts: u64,
        format: SerializedExportFormat,
        component: Option<String>,
        requestor: String,
    },
    #[serde(alias = "cancelled")]
    Canceled {
        start_ts: Option<u64>,
        #[serde(alias = "cancelled_ts")]
        canceled_ts: u64,
        format: SerializedExportFormat,
        component: Option<String>,
        requestor: String,
    },
}

impl TryFrom<Export> for SerializedExport {
    type Error = anyhow::Error;

    fn try_from(value: Export) -> Result<Self, Self::Error> {
        Ok(match value {
            Export::Requested {
                format,
                component,
                requestor,
                expiration_ts,
            } => SerializedExport::Requested {
                format: format.into(),
                component: component.serialize_to_string(),
                requestor: requestor.to_string(),
                expiration_ts: expiration_ts as i64,
            },
            Export::InProgress {
                start_ts,
                format,
                component,
                expiration_ts,
                requestor,
                progress_message,
                resumption_token,
            } => SerializedExport::InProgress {
                start_ts: start_ts.into(),
                format: format.into(),
                component: component.serialize_to_string(),
                requestor: requestor.to_string(),
                expiration_ts: expiration_ts as i64,
                progress_message,
                resumption_token,
            },
            Export::Completed {
                start_ts,
                complete_ts,
                expiration_ts,
                zip_object_key,
                format,
                component,
                requestor,
                size,
            } => SerializedExport::Completed {
                start_ts: start_ts.into(),
                complete_ts: complete_ts.into(),
                expiration_ts: expiration_ts as i64,
                zip_object_key: zip_object_key.to_string(),
                format: format.into(),
                component: component.serialize_to_string(),
                requestor: requestor.to_string(),
                size: size as i64,
            },
            Export::Failed {
                start_ts,
                failed_ts,
                format,
                component,
                requestor,
            } => SerializedExport::Failed {
                start_ts: start_ts.into(),
                failed_ts: failed_ts.into(),
                format: format.into(),
                component: component.serialize_to_string(),
                requestor: requestor.to_string(),
            },
            Export::Canceled {
                start_ts,
                canceled_ts,
                format,
                component,
                requestor,
            } => SerializedExport::Canceled {
                start_ts: start_ts.map(From::from),
                canceled_ts: canceled_ts.into(),
                format: format.into(),
                component: component.serialize_to_string(),
                requestor: requestor.to_string(),
            },
        })
    }
}

impl TryFrom<SerializedExport> for Export {
    type Error = anyhow::Error;

    fn try_from(value: SerializedExport) -> Result<Self, Self::Error> {
        Ok(match value {
            SerializedExport::Requested {
                format,
                component,
                requestor,
                expiration_ts,
            } => Export::Requested {
                format: format.into(),
                component: ComponentId::deserialize_from_string(component.as_deref())?,
                requestor: requestor.parse()?,
                expiration_ts: expiration_ts as u64,
            },
            SerializedExport::InProgress {
                start_ts,
                format,
                component,
                expiration_ts,
                requestor,
                progress_message,
                resumption_token,
            } => Export::InProgress {
                start_ts: start_ts.try_into()?,
                format: format.into(),
                component: ComponentId::deserialize_from_string(component.as_deref())?,
                requestor: requestor.parse()?,
                expiration_ts: expiration_ts as u64,
                progress_message,
                resumption_token,
            },
            SerializedExport::Completed {
                start_ts,
                complete_ts,
                expiration_ts,
                zip_object_key,
                format,
                component,
                requestor,
                size,
            } => Export::Completed {
                start_ts: start_ts.try_into()?,
                complete_ts: complete_ts.try_into()?,
                expiration_ts: expiration_ts as u64,
                zip_object_key: zip_object_key.try_into()?,
                format: format.into(),
                component: ComponentId::deserialize_from_string(component.as_deref())?,
                requestor: requestor.parse()?,
                size: size as u64,
            },
            SerializedExport::Failed {
                start_ts,
                failed_ts,
                format,
                component,
                requestor,
            } => Export::Failed {
                start_ts: start_ts.try_into()?,
                failed_ts: failed_ts.try_into()?,
                format: format.into(),
                component: ComponentId::deserialize_from_string(component.as_deref())?,
                requestor: requestor.parse()?,
            },
            SerializedExport::Canceled {
                start_ts,
                canceled_ts,
                format,
                component,
                requestor,
            } => Export::Canceled {
                start_ts: start_ts.map(Timestamp::try_from).transpose()?,
                canceled_ts: canceled_ts.try_into()?,
                format: format.into(),
                component: ComponentId::deserialize_from_string(component.as_deref())?,
                requestor: requestor.parse()?,
            },
        })
    }
}

codegen_convex_serialization!(Export, SerializedExport);

impl Export {
    pub fn format(&self) -> ExportFormat {
        match self {
            Export::Requested { format, .. }
            | Export::InProgress { format, .. }
            | Export::Completed { format, .. }
            | Export::Failed { format, .. }
            | Export::Canceled { format, .. } => *format,
        }
    }

    pub fn component(&self) -> ComponentId {
        match self {
            Export::Requested { component, .. }
            | Export::InProgress { component, .. }
            | Export::Completed { component, .. }
            | Export::Failed { component, .. }
            | Export::Canceled { component, .. } => *component,
        }
    }

    pub fn requestor(&self) -> ExportRequestor {
        match self {
            Export::Requested { requestor, .. }
            | Export::InProgress { requestor, .. }
            | Export::Completed { requestor, .. }
            | Export::Failed { requestor, .. }
            | Export::Canceled { requestor, .. } => *requestor,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ExportFormat {
    /// zip file containing a CleanJsonl for each table, and sidecar type info.
    Zip { include_storage: bool },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "format")]
#[serde(rename_all = "snake_case")]
pub enum SerializedExportFormat {
    Zip { include_storage: bool },
}

impl From<ExportFormat> for SerializedExportFormat {
    fn from(value: ExportFormat) -> Self {
        let ExportFormat::Zip { include_storage } = value;
        SerializedExportFormat::Zip { include_storage }
    }
}

impl From<SerializedExportFormat> for ExportFormat {
    fn from(value: SerializedExportFormat) -> Self {
        let SerializedExportFormat::Zip { include_storage } = value;
        ExportFormat::Zip { include_storage }
    }
}

impl ExportFormat {
    /// The format string recorded in deployment audit log events.
    pub fn audit_log_format(&self) -> &'static str {
        match self {
            ExportFormat::Zip {
                include_storage: true,
            } => "zip_with_storage",
            ExportFormat::Zip {
                include_storage: false,
            } => "zip",
        }
    }
}

codegen_convex_serialization!(ExportFormat, SerializedExportFormat);

#[derive(Copy, Clone, Debug, PartialEq, strum::EnumString, strum::Display)]
#[strum(serialize_all = "camelCase")]
pub enum ExportRequestor {
    /// The snapshot export feature in the CLI/Dashboard
    SnapshotExport,
    /// The team-level cloud backup feature
    CloudBackup,
}

impl ExportRequestor {
    pub fn usage_tag(&self) -> &'static str {
        match self {
            Self::SnapshotExport => "snapshot_export",
            Self::CloudBackup => "cloud_backup",
        }
    }
}

impl Export {
    pub fn requested(
        format: ExportFormat,
        component: ComponentId,
        requestor: ExportRequestor,
        expiration_ts: u64,
    ) -> Self {
        Self::Requested {
            format,
            component,
            requestor,
            expiration_ts,
        }
    }

    pub fn in_progress(self, ts: Timestamp) -> anyhow::Result<Export> {
        match self {
            Self::Requested {
                format,
                component,
                requestor,
                expiration_ts,
            } => Ok(Self::InProgress {
                start_ts: ts,
                format,
                component,
                requestor,
                expiration_ts,
                progress_message: None,
                resumption_token: None,
            }),
            Self::Completed { .. }
            | Self::InProgress { .. }
            | Self::Failed { .. }
            | Self::Canceled { .. } => Err(anyhow::anyhow!(
                "Can only begin an export that is requested"
            )),
        }
    }

    pub fn update_progress(mut self, msg: String) -> anyhow::Result<Export> {
        match self {
            Self::InProgress {
                ref mut progress_message,
                ..
            } => {
                *progress_message = Some(msg);
                Ok(self)
            },
            Self::Completed { .. }
            | Self::Requested { .. }
            | Self::Failed { .. }
            | Self::Canceled { .. } => Err(anyhow::anyhow!(
                "Can only update progress on an export that is InProgress"
            )),
        }
    }

    pub fn update_resumption_token(
        mut self,
        token: serde_json::Map<String, serde_json::Value>,
    ) -> anyhow::Result<Export> {
        match self {
            Self::InProgress {
                ref mut resumption_token,
                ..
            } => {
                *resumption_token = Some(token);
                Ok(self)
            },
            Self::Completed { .. }
            | Self::Requested { .. }
            | Self::Failed { .. }
            | Self::Canceled { .. } => Err(anyhow::anyhow!(
                "Can only set resumption token on an export that is InProgress"
            )),
        }
    }

    pub fn completed(
        self,
        snapshot_ts: Timestamp,
        complete_ts: Timestamp,
        zip_object_key: ObjectKey,
        size: u64,
    ) -> anyhow::Result<Export> {
        match self {
            Self::InProgress {
                format,
                component,
                requestor,
                expiration_ts,
                start_ts: _, // replace start_ts with the actual database TS
                progress_message: _,
                resumption_token: _,
            } => {
                anyhow::ensure!(snapshot_ts <= complete_ts);
                Ok(Self::Completed {
                    start_ts: snapshot_ts,
                    complete_ts,
                    expiration_ts,
                    zip_object_key,
                    format,
                    component,
                    requestor,
                    size,
                })
            },
            Self::Requested {
                format: _,
                component: _,
                requestor: _,
                expiration_ts: _,
            }
            | Self::Completed {
                start_ts: _,
                complete_ts: _,
                expiration_ts: _,
                zip_object_key: _,
                format: _,
                component: _,
                requestor: _,
                size: _,
            }
            | Self::Failed {
                start_ts: _,
                failed_ts: _,
                format: _,
                component: _,
                requestor: _,
            }
            | Self::Canceled {
                start_ts: _,
                canceled_ts: _,
                format: _,
                component: _,
                requestor: _,
            } => Err(anyhow::anyhow!(
                "Can only complete an export that is in_progress"
            )),
        }
    }

    pub fn failed(self, snapshot_ts: Timestamp, failed_ts: Timestamp) -> anyhow::Result<Export> {
        match self {
            Self::InProgress {
                format,
                component,
                requestor,
                ..
            } => {
                anyhow::ensure!(snapshot_ts <= failed_ts);
                Ok(Self::Failed {
                    start_ts: snapshot_ts,
                    failed_ts,
                    format,
                    component,
                    requestor,
                })
            },
            Self::Requested {
                format: _,
                component: _,
                requestor: _,
                expiration_ts: _,
            }
            | Self::Completed {
                start_ts: _,
                complete_ts: _,
                expiration_ts: _,
                zip_object_key: _,
                format: _,
                component: _,
                requestor: _,
                size: _,
            }
            | Self::Failed {
                start_ts: _,
                failed_ts: _,
                format: _,
                component: _,
                requestor: _,
            }
            | Self::Canceled {
                start_ts: _,
                canceled_ts: _,
                format: _,
                component: _,
                requestor: _,
            } => Err(anyhow::anyhow!(
                "Can only fail an export that is in_progress"
            )),
        }
    }

    pub fn canceled(self, canceled_ts: Timestamp) -> anyhow::Result<Export> {
        match self {
            Self::InProgress {
                format,
                component,
                requestor,
                start_ts,
                ..
            } => Ok(Self::Canceled {
                start_ts: Some(start_ts),
                canceled_ts,
                format,
                component,
                requestor,
            }),
            Self::Requested {
                format,
                component,
                requestor,
                ..
            } => Ok(Self::Canceled {
                start_ts: None,
                canceled_ts,
                format,
                component,
                requestor,
            }),
            Self::Completed { .. } | Self::Failed { .. } | Self::Canceled { .. } => Err(
                anyhow::anyhow!("Can only cancel an export that hasn't completed or failed"),
            ),
        }
    }
}

impl Display for Export {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Requested { .. } => write!(f, "requested"),
            Self::InProgress { .. } => write!(f, "in_progress"),
            Self::Completed { .. } => write!(f, "completed"),
            Self::Failed { .. } => write!(f, "failed"),
            Self::Canceled { .. } => write!(f, "canceled"),
        }
    }
}
