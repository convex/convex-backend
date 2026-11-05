// @generated - do not modify. Modify build.rs instead.
#![allow(clippy::match_single_binding)]
pub mod authentication_token;
pub mod document_id;
pub mod error_metadata;
pub mod field_path;
pub mod http;
pub mod user_identity_attributes;
pub mod common {
    include!(concat!(env!("OUT_DIR"), "/common.rs"));
}
pub mod convex_actions {
    include!(concat!(env!("OUT_DIR"), "/convex_actions.rs"));
}
pub mod convex_cursor {
    include!(concat!(env!("OUT_DIR"), "/convex_cursor.rs"));
}
pub mod convex_identity {
    include!(concat!(env!("OUT_DIR"), "/convex_identity.rs"));
}
pub mod convex_keys {
    include!(concat!(env!("OUT_DIR"), "/convex_keys.rs"));
}
pub mod convex_query_journal {
    include!(concat!(env!("OUT_DIR"), "/convex_query_journal.rs"));
}
pub mod errors {
    include!(concat!(env!("OUT_DIR"), "/errors.rs"));
}
pub mod managed_export {
    include!(concat!(env!("OUT_DIR"), "/managed_export.rs"));
}
pub mod outcome {
    include!(concat!(env!("OUT_DIR"), "/outcome.rs"));
}
pub mod searchlight {
    include!(concat!(env!("OUT_DIR"), "/searchlight.rs"));
}
pub mod storage {
    include!(concat!(env!("OUT_DIR"), "/storage.rs"));
}
pub mod usage {
    include!(concat!(env!("OUT_DIR"), "/usage.rs"));
}

include!(concat!(env!("OUT_DIR"), "/_extras.rs"));
use std::sync::LazyLock;

use prost_reflect::DescriptorPool;

const FILE_DESCRIPTOR_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/descriptors.bin"));
pub static DESCRIPTOR_POOL: LazyLock<DescriptorPool> =
    LazyLock::new(|| DescriptorPool::decode(FILE_DESCRIPTOR_BYTES).unwrap());
