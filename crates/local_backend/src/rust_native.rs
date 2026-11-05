//! `GET /api/rust_native/info` (contracts/NATIVE.md §5.1). WN-07.

use axum::response::IntoResponse;
use common::http::{
    extract::Json,
    HttpResponseError,
};
use isolate::environment::rust::native::{
    global_runtime,
    target::{
        host_target,
        native_mode,
    },
};
use serde_json::json;

use crate::{
    admin::must_be_admin,
    authentication::ExtractIdentity,
};

pub async fn rust_native_info(
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    must_be_admin(&identity)?;
    let Some(mode) = native_mode() else {
        return Ok(Json(json!({"enabled": false})));
    };
    let mut info = json!({
        "enabled": true,
        "mode": mode,
        "nativeAbi": [1],
        "abi": [1],
        "target": host_target(),
        "sandbox": {"level": "off", "seccomp": false, "landlockAbi": null,
                    "netns": false, "cgroup": false, "dropUid": false},
        "pools": [],
    });
    // The runtime is built on the first Rust action; until then the
    // defaults above are all there is to report.
    if let (Some(rt), Some(obj)) = (global_runtime(), info.as_object_mut())
        && let Some(live) = rt.info().as_object()
    {
        obj.extend(live.clone());
    }
    Ok(Json(info))
}
