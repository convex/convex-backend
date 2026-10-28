use std::{
    ffi::{
        c_char,
        c_void,
        CStr,
        CString,
    },
    slice,
};

use anyhow::Context as _;
use axum::debug_handler;
use common::http::HttpResponseError;
use serde::Deserialize;
use tikv_jemalloc_sys::malloc_stats_print;
use tikv_jemallocator::Jemalloc;

use crate::{
    metrics::log_process_level_stats,
    performance::{
        JemallocStats,
        JEMALLOC_STATS_REPORTER,
    },
};

// Configure jemalloc as Rust's global allocator. Based on:
// 1. https://www.polarsignals.com/blog/posts/2023/12/20/rust-memory-profiling
// 2. https://github.com/jemalloc/jemalloc/blob/dev/TUNING.md
#[global_allocator]
static ALLOC: Jemalloc = Jemalloc;

#[allow(non_upper_case_globals)]
#[unsafe(export_name = "malloc_conf")]
pub static malloc_conf: &[u8] =
    b"prof:true,prof_active:true,lg_prof_sample:19,background_thread:true\0";

const MAX_STATS_SIZE: usize = 4096;

#[derive(Deserialize, Debug)]
struct JemallocJson {
    jemalloc: JemallocReport,
}

#[derive(Deserialize, Debug)]
struct JemallocReport {
    stats: JemallocStats,
}

unsafe extern "C" fn stats_write_cb(ctx: *mut c_void, buf: *const c_char) {
    unsafe {
        let slice = slice::from_raw_parts_mut(ctx as *mut u8, MAX_STATS_SIZE);
        let message = CStr::from_ptr(buf);

        // Copy over the message buffer, ensuring there's a null terminator.
        let message_len = message.to_bytes().len();
        slice[..message_len].copy_from_slice(message.to_bytes());
        slice[MAX_STATS_SIZE - 1] = 0;
    }
}

pub fn load_jemalloc_stats() -> anyhow::Result<JemallocStats> {
    let mut buf = vec![0u8; MAX_STATS_SIZE];
    // Just get the bare minimum stats from jemalloc:
    // https://github.com/tikv/jemallocator/blob/main/jemalloc-sys/src/lib.rs#L526
    let opts = CString::new("Jgmdablx")?;
    unsafe {
        malloc_stats_print(
            Some(stats_write_cb),
            buf.as_mut_ptr() as *mut c_void,
            opts.as_ptr(),
        );
    }
    let s = CStr::from_bytes_until_nul(&buf)?.to_str()?;
    let top_level: JemallocJson = serde_json::from_str(s)?;
    Ok(top_level.jemalloc.stats)
}

pub fn install_jemalloc_reporter() {
    *JEMALLOC_STATS_REPORTER.lock() = Some(load_jemalloc_stats);
}

#[cfg(target_os = "linux")]
async fn collect_profile() -> anyhow::Result<Vec<u8>> {
    let mut prof_ctl = jemalloc_pprof::PROF_CTL
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Failed to load jemalloc profiler"))?
        .lock()
        .await;
    anyhow::ensure!(prof_ctl.activated(), "Heap profiling not activated");
    prof_ctl.dump_pprof()
}

#[cfg(not(target_os = "linux"))]
async fn collect_profile() -> anyhow::Result<Vec<u8>> {
    Err(anyhow::Error::new(errors::ErrorMetadata::bad_request(
        "UnsupportedPlatform",
        format!("Heap profiling unsupported on {}", std::env::consts::OS),
    )))
}

#[debug_handler]
pub async fn heap_profile() -> Result<impl axum::response::IntoResponse, HttpResponseError> {
    Ok(collect_profile().await?)
}

pub fn log_process_memory_stats() -> anyhow::Result<()> {
    let jemalloc_stats = load_jemalloc_stats()?;
    let process_stats = memory_stats::memory_stats().context("failed to get memory stats")?;
    log_process_level_stats(process_stats.physical_mem, &jemalloc_stats);
    Ok(())
}
