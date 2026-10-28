use std::{
    hash::Hasher,
    io::Result,
    path::PathBuf,
};

use fxhash::FxHasher32;

// Make sure to select a rev off the `production` branch of the sdk
// https://github.com/fivetran/fivetran_sdk/tree/production
const REV: &str = "466a61bddfc0e541bfec3cb0cc6a3cf3704d64be";

const FILES: &[&str] = &[
    "common.proto",
    "connector_sdk.proto",
    "destination_sdk.proto",
];

// File hash to protect against accidental changes to the vendored files:
// Update this when updating `REV` above.
const FILE_HASH: u64 = 1411440539;

fn main() -> Result<()> {
    let protos_dir = PathBuf::from(format!("./fivetran_sdk/{REV}"));

    let mut proto_files = Vec::new();
    let mut hasher = FxHasher32::default();
    for file in FILES {
        let path = protos_dir.join(file);
        let contents = std::fs::read(&path)?;
        hasher.write(&contents);
        proto_files.push(path);
    }
    let hash = hasher.finish();
    #[cfg(not(windows))]
    if hash != FILE_HASH {
        panic!("Files have hash {hash}, expected {FILE_HASH}");
    }
    tonic_prost_build::configure()
        .btree_map(".")
        .compile_protos(&proto_files, &[protos_dir])?;

    Ok(())
}
