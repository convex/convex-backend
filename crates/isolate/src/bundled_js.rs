use value::sha256::Sha256Digest;

/// This file includes the generated table of all of the system JS
/// files, which we mount under "_system/" in the module namespace.
mod system_udf_js_data {
    include!(concat!(env!("OUT_DIR"), "/system_udf_js_data.rs"));
}

/// This file includes the generated table of all of the system JS
/// files, which we mount under "_system/" in the module namespace.
mod node_executor_js_data {
    include!(concat!(env!("OUT_DIR"), "/node_executor_js_data.rs"));
}

/// Source and sourcemap
pub type BundledJsFile = (&'static str, Option<&'static str>);

pub fn system_udf_file(path: &str) -> Option<BundledJsFile> {
    system_udf_js_data::FILES.get(path).copied()
}
pub fn system_udf_files_sha256() -> Sha256Digest {
    system_udf_js_data::FILES_SHA256.into()
}
pub fn node_executor_file(path: &str) -> Option<BundledJsFile> {
    node_executor_js_data::FILES.get(path).copied()
}
pub fn node_executor_files_sha256() -> Sha256Digest {
    node_executor_js_data::FILES_SHA256.into()
}
