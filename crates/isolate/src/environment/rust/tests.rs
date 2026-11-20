//! Drives the bench guest through the wasm core with an in-memory backend: no
//! `ActionRequest`, database or task executor involved.

use std::{
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use serde_json::{
    json,
    Value as JsonValue,
};

use super::core::{
    parse_envelope,
    CallFailure,
    CoreLimits,
    GuestErrorKind,
    GuestResult,
    HostBackend,
    InvalidModule,
    PollOutcome,
    PreparedModule,
    WasmCore,
};

#[derive(Default)]
struct TestBackend {
    logs: Vec<(i32, String)>,
    submitted: usize,
    /// `task_poll(block)` never resolves.
    park_on_poll: bool,
}

#[async_trait]
impl HostBackend for TestBackend {
    fn submit(&mut self, _request: JsonValue) -> Result<i64, String> {
        self.submitted += 1;
        Ok(self.submitted as i64 - 1)
    }

    fn outstanding_tasks(&self) -> usize {
        self.submitted
    }

    async fn poll(&mut self, block: bool) -> PollOutcome {
        if self.park_on_poll {
            std::future::pending::<()>().await;
        }
        if block {
            PollOutcome::NoTasks
        } else {
            PollOutcome::Empty
        }
    }

    fn sync_syscall(&mut self, op: &str, _args: JsonValue) -> JsonValue {
        match op {
            "now" => json!({"ok": 1.0}),
            _ => json!({"error": {"message": "unmocked"}}),
        }
    }

    fn log(&mut self, level: i32, message: String) {
        self.logs.push((level, message));
    }
}

fn bench_wasm() -> Option<Vec<u8>> {
    let path = std::env::var_os("BENCH_RUST_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                "/Users/tomasdomingos/convex-rs/target/wasm32-wasip1/release/bench_rust.wasm",
            )
        });
    std::fs::read(path).ok()
}

#[tokio::test]
async fn runs_pure_exports_end_to_end() -> anyhow::Result<()> {
    let Some(wasm) = bench_wasm() else {
        eprintln!("bench_rust.wasm not built; skipping");
        return Ok(());
    };
    let core = WasmCore::<TestBackend>::new()?;
    let prepared = core.prepare(&wasm)?;
    // Second prepare hits the LRU and returns the same compiled module.
    assert!(std::sync::Arc::ptr_eq(&prepared, &core.prepare(&wasm)?));

    let export = prepared
        .export_for("rust/base.js", "base_002")
        .expect("base_002 export")
        .to_string();
    let out = core
        .call(
            &prepared,
            &export,
            &json!({"seed": 1, "msg": "hello"}).to_string(),
            vec![("FOO".into(), "bar".into())],
            TestBackend::default(),
            Duration::from_secs(30),
            std::future::pending(),
        )
        .await;
    match out.result {
        Ok(GuestResult::Ok(v)) => assert_eq!(
            serde_json::from_str::<JsonValue>(v.get())?,
            json!({"seed": 1, "msg": "hello"})
        ),
        other => panic!("unexpected result {other:?}"),
    }

    let noop = prepared
        .export_for("rust/base", "base_001")
        .unwrap()
        .to_string();
    let out = core
        .call(
            &prepared,
            &noop,
            "{\"seed\":1}",
            vec![],
            TestBackend::default(),
            Duration::from_secs(30),
            std::future::pending(),
        )
        .await;
    assert!(matches!(out.result, Ok(GuestResult::Ok(v)) if v.get() == "null"));

    // Bad arguments surface as a guest error, not a host failure.
    let out = core
        .call(
            &prepared,
            &export,
            "{\"seed\":\"x\"}",
            vec![],
            TestBackend::default(),
            Duration::from_secs(30),
            std::future::pending(),
        )
        .await;
    assert!(matches!(out.result, Ok(GuestResult::Error { .. })));
    Ok(())
}

#[test]
fn envelope_parsing() {
    assert!(matches!(
        parse_envelope(r#"{"ok": 3}"#),
        Ok(GuestResult::Ok(v)) if v.get().trim() == "3"
    ));
    match parse_envelope(r#"{"error": {"message": "m", "data": {"a": 1}, "kind": "convex"}}"#) {
        Ok(GuestResult::Error {
            message,
            data,
            kind,
        }) => {
            assert_eq!(message, "m");
            assert_eq!(data, Some(json!({"a": 1})));
            assert_eq!(kind, GuestErrorKind::Convex);
        },
        other => panic!("{other:?}"),
    }
    assert!(parse_envelope("{}").is_err());
    assert!(parse_envelope("nope").is_err());
}

#[test]
fn failure_types_are_distinct() {
    // Guards the mapping used by `execute_action`.
    assert!(matches!(CallFailure::Timeout, CallFailure::Timeout));
}

mod fixtures {
// @generated from the WAT in the comments above each fixture (wat 1.262).
    // (module
    //   (import "convex" "task_submit" (func $submit (param i32 i32) (result i64)))
    //   (import "convex" "log" (func $log (param i32 i32 i32)))
    //   (import "convex" "task_poll" (func $poll (param i32) (result i64)))
    //   (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
    //   (memory (export "memory") 2)
    //   (global $bump (mut i32) (i32.const 65536))
    //   (func (export "convex_alloc") (param $n i32) (result i32)
    //     (local $p i32)
    //     (local.set $p (global.get $bump))
    //     (global.set $bump (i32.add (global.get $bump) (local.get $n)))
    //     (local.get $p))
    //   (data (i32.const 1024) "{\22ok\22:\22rejected\22}")
    //   (data (i32.const 1100) "{\22ok\22:\22accepted\22}")
    //   (data (i32.const 2048) "{\22type\22:\22sleep\22,\22ms\22:1}")
    //   (data (i32.const 4000) "hello\0a")
    //   (data (i32.const 4100) "x")
    //   (@custom "convex:manifest" "{\22abiVersion\22:1,\22modulePath\22:\22app::t\22,\22name\22:\22f\22,\22visibility\22:\22public\22,\22args\22:null,\22returns\22:null}")
    //   (func (export "convex_run:app::t:submit_big") (param i32 i32) (result i64)
    //     (if (result i64) (i64.eq (call $submit (i32.const 0) (i32.const 100000000)) (i64.const -1)) (then (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 17))) (else (i64.or (i64.shl (i64.const 1100) (i64.const 32)) (i64.const 17)))))
    //   (func (export "convex_run:app::t:submit_neg") (param i32 i32) (result i64)
    //     (if (result i64) (i64.eq (call $submit (i32.const 0) (i32.const -1)) (i64.const -1)) (then (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 17))) (else (i64.or (i64.shl (i64.const 1100) (i64.const 32)) (i64.const 17)))))
    //   (func (export "convex_run:app::t:submit_many") (param i32 i32) (result i64)
    //     (drop (call $submit (i32.const 2048) (i32.const 23)))
    //     (drop (call $submit (i32.const 2048) (i32.const 23)))
    //     (if (result i64) (i64.eq (call $submit (i32.const 2048) (i32.const 23)) (i64.const -1)) (then (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 17))) (else (i64.or (i64.shl (i64.const 1100) (i64.const 32)) (i64.const 17)))))
    //   (func (export "convex_run:app::t:result_big") (param i32 i32) (result i64)
    //     (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 0x7fffffff)))
    //   (func (export "convex_run:app::t:log_big") (param i32 i32) (result i64)
    //     (call $log (i32.const 1) (i32.const 0) (i32.const 70000))
    //     (i64.or (i64.shl (i64.const 1100) (i64.const 32)) (i64.const 17)))
    //   (func (export "convex_run:app::t:grow") (param i32 i32) (result i64)
    //     (if (result i64) (i32.eq (memory.grow (i32.const 1000)) (i32.const -1)) (then (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 17))) (else (i64.or (i64.shl (i64.const 1100) (i64.const 32)) (i64.const 17)))))
    //   (func (export "convex_run:app::t:stdio_flood") (param i32 i32) (result i64)
    //     (local $i i32)
    //     ;; iovec at 3000: {ptr=4000,len=6}
    //     (i32.store (i32.const 3000) (i32.const 4000))
    //     (i32.store (i32.const 3004) (i32.const 6))
    //     (drop (call $fd_write (i32.const 1) (i32.const 3000) (i32.const 1) (i32.const 3100)))
    //     (call $log (i32.const 1) (i32.const 4100) (i32.const 1))
    //     ;; iovec {ptr=0,len=65536}, 400 writes = 25 MiB > the stdio capacity
    //     (i32.store (i32.const 3000) (i32.const 0))
    //     (i32.store (i32.const 3004) (i32.const 65536))
    //     (loop $l
    //       (drop (call $fd_write (i32.const 1) (i32.const 3000) (i32.const 1) (i32.const 3100)))
    //       (local.set $i (i32.add (local.get $i) (i32.const 1)))
    //       (br_if $l (i32.lt_u (local.get $i) (i32.const 400))))
    //     (i64.or (i64.shl (i64.const 1100) (i64.const 32)) (i64.const 17)))
    //   (func (export "convex_run:app::t:park") (param i32 i32) (result i64)
    //     (i32.store (i32.const 3000) (i32.const 4000))
    //     (i32.store (i32.const 3004) (i32.const 6))
    //     (drop (call $fd_write (i32.const 1) (i32.const 3000) (i32.const 1) (i32.const 3100)))
    //     (drop (call $poll (i32.const 1)))
    //     (i64.or (i64.shl (i64.const 1100) (i64.const 32)) (i64.const 17)))
    // )
    pub const HOSTILE: &[u8] = &[
        0, 97, 115, 109, 1, 0, 0, 0, 1, 31, 5, 96, 2, 127, 127, 1, 126, 96, 3, 127, 127, 127, 0, 96, 
        1, 127, 1, 126, 96, 4, 127, 127, 127, 127, 1, 127, 96, 1, 127, 1, 127, 2, 88, 4, 6, 99, 111, 110, 
        118, 101, 120, 11, 116, 97, 115, 107, 95, 115, 117, 98, 109, 105, 116, 0, 0, 6, 99, 111, 110, 118, 101, 120, 
        3, 108, 111, 103, 0, 1, 6, 99, 111, 110, 118, 101, 120, 9, 116, 97, 115, 107, 95, 112, 111, 108, 108, 0, 
        2, 22, 119, 97, 115, 105, 95, 115, 110, 97, 112, 115, 104, 111, 116, 95, 112, 114, 101, 118, 105, 101, 119, 49, 
        8, 102, 100, 95, 119, 114, 105, 116, 101, 0, 3, 3, 10, 9, 4, 0, 0, 0, 0, 0, 0, 0, 0, 5, 
        3, 1, 0, 2, 6, 8, 1, 127, 1, 65, 128, 128, 4, 11, 7, 132, 2, 10, 6, 109, 101, 109, 111, 114, 
        121, 2, 0, 12, 99, 111, 110, 118, 101, 120, 95, 97, 108, 108, 111, 99, 0, 4, 28, 99, 111, 110, 118, 101, 
        120, 95, 114, 117, 110, 58, 97, 112, 112, 58, 58, 116, 58, 115, 117, 98, 109, 105, 116, 95, 98, 105, 103, 0, 
        5, 28, 99, 111, 110, 118, 101, 120, 95, 114, 117, 110, 58, 97, 112, 112, 58, 58, 116, 58, 115, 117, 98, 109, 
        105, 116, 95, 110, 101, 103, 0, 6, 29, 99, 111, 110, 118, 101, 120, 95, 114, 117, 110, 58, 97, 112, 112, 58, 
        58, 116, 58, 115, 117, 98, 109, 105, 116, 95, 109, 97, 110, 121, 0, 7, 28, 99, 111, 110, 118, 101, 120, 95, 
        114, 117, 110, 58, 97, 112, 112, 58, 58, 116, 58, 114, 101, 115, 117, 108, 116, 95, 98, 105, 103, 0, 8, 25, 
        99, 111, 110, 118, 101, 120, 95, 114, 117, 110, 58, 97, 112, 112, 58, 58, 116, 58, 108, 111, 103, 95, 98, 105, 
        103, 0, 9, 22, 99, 111, 110, 118, 101, 120, 95, 114, 117, 110, 58, 97, 112, 112, 58, 58, 116, 58, 103, 114, 
        111, 119, 0, 10, 29, 99, 111, 110, 118, 101, 120, 95, 114, 117, 110, 58, 97, 112, 112, 58, 58, 116, 58, 115, 
        116, 100, 105, 111, 95, 102, 108, 111, 111, 100, 0, 11, 22, 99, 111, 110, 118, 101, 120, 95, 114, 117, 110, 58, 
        97, 112, 112, 58, 58, 116, 58, 112, 97, 114, 107, 0, 12, 10, 233, 2, 9, 17, 1, 1, 127, 35, 0, 33, 
        1, 35, 0, 32, 0, 106, 36, 0, 32, 1, 11, 36, 0, 65, 0, 65, 128, 194, 215, 47, 16, 0, 66, 127, 
        81, 4, 126, 66, 128, 8, 66, 32, 134, 66, 17, 132, 5, 66, 204, 8, 66, 32, 134, 66, 17, 132, 11, 11, 
        33, 0, 65, 0, 65, 127, 16, 0, 66, 127, 81, 4, 126, 66, 128, 8, 66, 32, 134, 66, 17, 132, 5, 66, 
        204, 8, 66, 32, 134, 66, 17, 132, 11, 11, 50, 0, 65, 128, 16, 65, 23, 16, 0, 26, 65, 128, 16, 65, 
        23, 16, 0, 26, 65, 128, 16, 65, 23, 16, 0, 66, 127, 81, 4, 126, 66, 128, 8, 66, 32, 134, 66, 17, 
        132, 5, 66, 204, 8, 66, 32, 134, 66, 17, 132, 11, 11, 15, 0, 66, 128, 8, 66, 32, 134, 66, 255, 255, 
        255, 255, 7, 132, 11, 21, 0, 65, 1, 65, 0, 65, 240, 162, 4, 16, 1, 66, 204, 8, 66, 32, 134, 66, 
        17, 132, 11, 32, 0, 65, 232, 7, 64, 0, 65, 127, 70, 4, 126, 66, 128, 8, 66, 32, 134, 66, 17, 132, 
        5, 66, 204, 8, 66, 32, 134, 66, 17, 132, 11, 11, 101, 1, 1, 127, 65, 184, 23, 65, 160, 31, 54, 2, 
        0, 65, 188, 23, 65, 6, 54, 2, 0, 65, 1, 65, 184, 23, 65, 1, 65, 156, 24, 16, 3, 26, 65, 1, 
        65, 132, 32, 65, 1, 16, 1, 65, 184, 23, 65, 0, 54, 2, 0, 65, 188, 23, 65, 128, 128, 4, 54, 2, 
        0, 3, 64, 65, 1, 65, 184, 23, 65, 1, 65, 156, 24, 16, 3, 26, 32, 2, 65, 1, 106, 33, 2, 32, 
        2, 65, 144, 3, 73, 13, 0, 11, 66, 204, 8, 66, 32, 134, 66, 17, 132, 11, 46, 0, 65, 184, 23, 65, 
        160, 31, 54, 2, 0, 65, 188, 23, 65, 6, 54, 2, 0, 65, 1, 65, 184, 23, 65, 1, 65, 156, 24, 16, 
        3, 26, 65, 1, 16, 2, 26, 66, 204, 8, 66, 32, 134, 66, 17, 132, 11, 11, 95, 5, 0, 65, 128, 8, 
        11, 17, 123, 34, 111, 107, 34, 58, 34, 114, 101, 106, 101, 99, 116, 101, 100, 34, 125, 0, 65, 204, 8, 11, 
        17, 123, 34, 111, 107, 34, 58, 34, 97, 99, 99, 101, 112, 116, 101, 100, 34, 125, 0, 65, 128, 16, 11, 23, 
        123, 34, 116, 121, 112, 101, 34, 58, 34, 115, 108, 101, 101, 112, 34, 44, 34, 109, 115, 34, 58, 49, 125, 0, 
        65, 160, 31, 11, 6, 104, 101, 108, 108, 111, 10, 0, 65, 132, 32, 11, 1, 120, 0, 114, 15, 99, 111, 110, 
        118, 101, 120, 58, 109, 97, 110, 105, 102, 101, 115, 116, 123, 34, 97, 98, 105, 86, 101, 114, 115, 105, 111, 110, 
        34, 58, 49, 44, 34, 109, 111, 100, 117, 108, 101, 80, 97, 116, 104, 34, 58, 34, 97, 112, 112, 58, 58, 116, 
        34, 44, 34, 110, 97, 109, 101, 34, 58, 34, 102, 34, 44, 34, 118, 105, 115, 105, 98, 105, 108, 105, 116, 121, 
        34, 58, 34, 112, 117, 98, 108, 105, 99, 34, 44, 34, 97, 114, 103, 115, 34, 58, 110, 117, 108, 108, 44, 34, 
        114, 101, 116, 117, 114, 110, 115, 34, 58, 110, 117, 108, 108, 125, 0, 70, 4, 110, 97, 109, 101, 1, 30, 4, 
        0, 6, 115, 117, 98, 109, 105, 116, 1, 3, 108, 111, 103, 2, 4, 112, 111, 108, 108, 3, 8, 102, 100, 95, 
        119, 114, 105, 116, 101, 2, 14, 2, 4, 2, 0, 1, 110, 1, 1, 112, 11, 1, 2, 1, 105, 3, 6, 1, 
        11, 1, 0, 1, 108, 7, 7, 1, 0, 4, 98, 117, 109, 112, 
    ];

    // (module (memory 1) (memory 1) (@custom "convex:manifest" "{\22abiVersion\22:1,\22modulePath\22:\22app::t\22,\22name\22:\22f\22,\22visibility\22:\22public\22,\22args\22:null,\22returns\22:null}"))
    pub const TWO_MEMORIES: &[u8] = &[
        0, 97, 115, 109, 1, 0, 0, 0, 5, 5, 2, 0, 1, 0, 1, 0, 114, 15, 99, 111, 110, 118, 101, 120, 
        58, 109, 97, 110, 105, 102, 101, 115, 116, 123, 34, 97, 98, 105, 86, 101, 114, 115, 105, 111, 110, 34, 58, 49, 
        44, 34, 109, 111, 100, 117, 108, 101, 80, 97, 116, 104, 34, 58, 34, 97, 112, 112, 58, 58, 116, 34, 44, 34, 
        110, 97, 109, 101, 34, 58, 34, 102, 34, 44, 34, 118, 105, 115, 105, 98, 105, 108, 105, 116, 121, 34, 58, 34, 
        112, 117, 98, 108, 105, 99, 34, 44, 34, 97, 114, 103, 115, 34, 58, 110, 117, 108, 108, 44, 34, 114, 101, 116, 
        117, 114, 110, 115, 34, 58, 110, 117, 108, 108, 125, 
    ];

    // (module (import "env" "nope" (func)) (memory (export "memory") 1) (@custom "convex:manifest" "{\22abiVersion\22:1,\22modulePath\22:\22app::t\22,\22name\22:\22f\22,\22visibility\22:\22public\22,\22args\22:null,\22returns\22:null}"))
    pub const BAD_IMPORT: &[u8] = &[
        0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 2, 12, 1, 3, 101, 110, 118, 4, 110, 111, 
        112, 101, 0, 0, 5, 3, 1, 0, 1, 7, 10, 1, 6, 109, 101, 109, 111, 114, 121, 2, 0, 0, 114, 15, 
        99, 111, 110, 118, 101, 120, 58, 109, 97, 110, 105, 102, 101, 115, 116, 123, 34, 97, 98, 105, 86, 101, 114, 115, 
        105, 111, 110, 34, 58, 49, 44, 34, 109, 111, 100, 117, 108, 101, 80, 97, 116, 104, 34, 58, 34, 97, 112, 112, 
        58, 58, 116, 34, 44, 34, 110, 97, 109, 101, 34, 58, 34, 102, 34, 44, 34, 118, 105, 115, 105, 98, 105, 108, 
        105, 116, 121, 34, 58, 34, 112, 117, 98, 108, 105, 99, 34, 44, 34, 97, 114, 103, 115, 34, 58, 110, 117, 108, 
        108, 44, 34, 114, 101, 116, 117, 114, 110, 115, 34, 58, 110, 117, 108, 108, 125, 
    ];

    // (module (table 200000 funcref) (memory (export "memory") 1) (func (export "convex_alloc") (param i32) (result i32) (i32.const 0)) (func (export "convex_run:app::t:f") (param i32 i32) (result i64) (i64.const 0)) (@custom "convex:manifest" "{\22abiVersion\22:1,\22modulePath\22:\22app::t\22,\22name\22:\22f\22,\22visibility\22:\22public\22,\22args\22:null,\22returns\22:null}"))
    pub const BIG_TABLE: &[u8] = &[
        0, 97, 115, 109, 1, 0, 0, 0, 1, 12, 2, 96, 1, 127, 1, 127, 96, 2, 127, 127, 1, 126, 3, 3, 
        2, 0, 1, 4, 6, 1, 112, 0, 192, 154, 12, 5, 3, 1, 0, 1, 7, 47, 3, 6, 109, 101, 109, 111, 
        114, 121, 2, 0, 12, 99, 111, 110, 118, 101, 120, 95, 97, 108, 108, 111, 99, 0, 0, 19, 99, 111, 110, 118, 
        101, 120, 95, 114, 117, 110, 58, 97, 112, 112, 58, 58, 116, 58, 102, 0, 1, 10, 11, 2, 4, 0, 65, 0, 
        11, 4, 0, 66, 0, 11, 0, 114, 15, 99, 111, 110, 118, 101, 120, 58, 109, 97, 110, 105, 102, 101, 115, 116, 
        123, 34, 97, 98, 105, 86, 101, 114, 115, 105, 111, 110, 34, 58, 49, 44, 34, 109, 111, 100, 117, 108, 101, 80, 
        97, 116, 104, 34, 58, 34, 97, 112, 112, 58, 58, 116, 34, 44, 34, 110, 97, 109, 101, 34, 58, 34, 102, 34, 
        44, 34, 118, 105, 115, 105, 98, 105, 108, 105, 116, 121, 34, 58, 34, 112, 117, 98, 108, 105, 99, 34, 44, 34, 
        97, 114, 103, 115, 34, 58, 110, 117, 108, 108, 44, 34, 114, 101, 116, 117, 114, 110, 115, 34, 58, 110, 117, 108, 
        108, 125, 
    ];

}

fn core_with(limits: CoreLimits) -> WasmCore<TestBackend> {
    WasmCore::with_limits(limits).unwrap()
}

async fn run(
    core: &WasmCore<TestBackend>,
    prepared: &PreparedModule<TestBackend>,
    function: &str,
    backend: TestBackend,
    timeout: Duration,
    wall_clock: impl std::future::Future<Output = ()>,
) -> super::core::CallOutput<TestBackend> {
    let export = format!("convex_run:app::t:{function}");
    core.call(prepared, &export, "{}", vec![], backend, timeout, wall_clock)
        .await
}

fn ok_str(result: &Result<GuestResult, CallFailure>) -> String {
    match result {
        Ok(GuestResult::Ok(v)) => v.get().trim_matches('"').to_string(),
        other => panic!("unexpected result {other:?}"),
    }
}

async fn call_hostile(function: &str, limits: CoreLimits) -> (String, TestBackend) {
    let core = core_with(limits);
    let prepared = core.prepare(fixtures::HOSTILE).unwrap();
    let out = run(
        &core,
        &prepared,
        function,
        TestBackend::default(),
        Duration::from_secs(30),
        std::future::pending(),
    )
    .await;
    (ok_str(&out.result), out.backend)
}

#[tokio::test]
async fn oversized_task_request_is_rejected_before_reading() {
    // 100 MB (and a negative i32, i.e. 4 GiB) lengths: -1, no trap, no submit.
    for f in ["submit_big", "submit_neg"] {
        let (r, backend) = call_hostile(f, CoreLimits::default()).await;
        assert_eq!(r, "rejected", "{f}");
        assert_eq!(backend.submitted, 0);
        assert!(backend.logs[0].1.contains("exceeds"), "{:?}", backend.logs);
    }
}

#[tokio::test]
async fn outstanding_tasks_are_capped() {
    let limits = CoreLimits {
        max_outstanding_tasks: 3,
        ..Default::default()
    };
    // Two accepted submits, the third (still under 3) is accepted too.
    let (r, backend) = call_hostile("submit_many", limits).await;
    assert_eq!(r, "accepted");
    assert_eq!(backend.submitted, 3);
    let limits = CoreLimits {
        max_outstanding_tasks: 2,
        ..Default::default()
    };
    let (r, backend) = call_hostile("submit_many", limits).await;
    assert_eq!(r, "rejected");
    assert_eq!(backend.submitted, 2);
    assert!(backend.logs[0].1.contains("outstanding"), "{:?}", backend.logs);
}

#[tokio::test]
async fn oversized_result_is_rejected_before_reading() {
    let core = core_with(CoreLimits::default());
    let prepared = core.prepare(fixtures::HOSTILE).unwrap();
    let out = run(
        &core,
        &prepared,
        "result_big",
        TestBackend::default(),
        Duration::from_secs(30),
        std::future::pending(),
    )
    .await;
    assert!(
        matches!(out.result, Err(CallFailure::ResultTooLarge { len: 0x7fffffff, .. })),
        "{:?}",
        out.result
    );
}

#[tokio::test]
async fn memory_is_capped() {
    // Growing by ~65 MB works under the default cap and fails (-1) under 4 MiB.
    let (r, _) = call_hostile("grow", CoreLimits::default()).await;
    assert_eq!(r, "accepted");
    let limits = CoreLimits {
        memory_bytes: 4 << 20,
        ..Default::default()
    };
    let (r, _) = call_hostile("grow", limits).await;
    assert_eq!(r, "rejected");
}

#[tokio::test]
async fn log_import_message_is_truncated() {
    let (r, backend) = call_hostile("log_big", CoreLimits::default()).await;
    assert_eq!(r, "accepted");
    assert_eq!(backend.logs.len(), 1);
    assert_eq!(backend.logs[0].1.len(), 32 * 1024);
}

#[tokio::test]
async fn stdio_overflow_is_truncated_not_trapped() {
    let (r, backend) = call_hostile("stdio_flood", CoreLimits::default()).await;
    assert_eq!(r, "accepted");
    // stdout line, then the `log` import line, in write order.
    assert_eq!(backend.logs[0], (1, "hello".to_string()));
    assert_eq!(backend.logs[1], (1, "x".to_string()));
    // The flood is page 0 repeated (it contains a few newlines); no line may
    // exceed the line limit, and a notice closes the output.
    assert!(backend.logs.len() > 3);
    assert!(backend.logs.iter().all(|(_, l)| l.len() <= 32 * 1024));
    assert!(backend.logs.last().unwrap().1.contains("truncated"));
}

#[tokio::test]
async fn timeout_while_parked_still_drains_stdout() {
    let core = core_with(CoreLimits::default());
    let prepared = core.prepare(fixtures::HOSTILE).unwrap();
    let out = run(
        &core,
        &prepared,
        "park",
        TestBackend {
            park_on_poll: true,
            ..Default::default()
        },
        Duration::from_secs(30),
        tokio::time::sleep(Duration::from_millis(200)),
    )
    .await;
    assert!(matches!(out.result, Err(CallFailure::Timeout)), "{:?}", out.result);
    assert_eq!(out.backend.logs, vec![(1, "hello".to_string())]);
}

#[test]
fn unusable_modules_are_invalid_module_errors() {
    let core = core_with(CoreLimits::default());
    // Two memories: the memory cap is per memory, so multi-memory is off.
    for (name, wasm) in [
        ("two memories", fixtures::TWO_MEMORIES),
        ("unknown import", fixtures::BAD_IMPORT),
        ("not wasm", b"definitely not wasm".as_slice()),
    ] {
        let e = core.prepare(wasm).err().unwrap_or_else(|| panic!("{name}"));
        assert!(e.downcast_ref::<InvalidModule>().is_some(), "{name}: {e:#}");
    }
    let e = core.prepare_base64("!!!").err().unwrap();
    assert!(e.downcast_ref::<InvalidModule>().is_some(), "{e:#}");
    // base64 path agrees with the byte path on a good module.
    let b64 = base64::encode(fixtures::HOSTILE);
    core.prepare_base64(&b64).unwrap();
}

#[tokio::test]
async fn oversized_table_traps() {
    let core = core_with(CoreLimits::default());
    let prepared = core.prepare(fixtures::BIG_TABLE).unwrap();
    let out = run(
        &core,
        &prepared,
        "f",
        TestBackend::default(),
        Duration::from_secs(30),
        std::future::pending(),
    )
    .await;
    assert!(matches!(out.result, Err(CallFailure::Trap(_))), "{:?}", out.result);
}

#[test]
fn concurrent_prepares_share_one_compile() {
    let core = Arc::new(core_with(CoreLimits::default()));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let core = core.clone();
            std::thread::spawn(move || core.prepare_base64(&base64::encode(fixtures::HOSTILE)))
        })
        .collect();
    let modules: Vec<_> = threads
        .into_iter()
        .map(|t| t.join().unwrap().unwrap())
        .collect();
    for m in &modules {
        assert!(Arc::ptr_eq(m, &modules[0]));
    }
}

#[test]
fn keyed_cache_hit_needs_no_source() {
    let core = core_with(CoreLimits::default());
    let key = [7u8; 32];
    assert!(core.lookup(&key).is_none());
    let b64 = base64::encode(fixtures::HOSTILE);
    let first = core.prepare_base64_keyed(key, &b64).unwrap();
    assert!(Arc::ptr_eq(&first, &core.lookup(&key).unwrap()));
    // A hit never looks at the source, so even garbage is fine.
    assert!(Arc::ptr_eq(&first, &core.prepare_base64_keyed(key, "!!!").unwrap()));
}

#[test]
fn disk_cache_survives_a_new_core() {
    let dir = std::env::temp_dir().join(format!("rust-wasm-cache-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let limits = || CoreLimits {
        disk_cache_dir: Some(dir.clone()),
        ..Default::default()
    };
    let key = [9u8; 32];
    core_with(limits())
        .prepare_base64_keyed(key, &base64::encode(fixtures::HOSTILE))
        .unwrap();
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with(".cwasm") && files[0].starts_with(&"09".repeat(32)));
    // A fresh core ("restart") loads it from disk: an unusable source proves
    // the wasm was not recompiled from it.
    core_with(limits()).prepare_base64_keyed(key, "!!!").unwrap();
    // A corrupt file is a miss, not an error.
    std::fs::write(dir.join(&files[0]), b"garbage").unwrap();
    core_with(limits())
        .prepare_base64_keyed(key, &base64::encode(fixtures::HOSTILE))
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
