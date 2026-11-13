//! IPC protocol between the host pump and a native worker (NATIVE.md §2.2).
//!
//! Every frame is `u32 len (LE, bytes after this field) | u8 kind | body`.
//! Integers are little endian. `bytes` is the rest of the frame; `str16` is
//! `u16 len + UTF-8`.

use std::io::{
    Read,
    Write,
};

use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
};

pub const KIND_READY: u8 = 0x01;
pub const KIND_CALL: u8 = 0x02;
pub const KIND_SUBMIT: u8 = 0x03;
pub const KIND_EVENT: u8 = 0x04;
pub const KIND_POLL: u8 = 0x05;
pub const KIND_NO_TASKS: u8 = 0x06;
pub const KIND_SYNC: u8 = 0x07;
pub const KIND_SYNC_RESULT: u8 = 0x08;
pub const KIND_LOG: u8 = 0x09;
pub const KIND_RESULT: u8 = 0x0A;
pub const KIND_SHUTDOWN: u8 = 0x0B;
/// H→W binary chunk event (proto 2): the body is the event, see
/// `core::new_chunk_event`.
pub const KIND_CHUNK: u8 = 0x0C;

/// Value of `Ready.proto`.
pub const PROTO_VERSION: u16 = 2;
/// Slack added to `max(max_task_request_bytes, max_result_bytes)`.
pub const FRAME_SLACK_BYTES: usize = 64 * 1024;

/// `RUST_NATIVE_MAX_FRAME_BYTES` from the two limits (NATIVE.md §2.2).
pub fn max_frame_len(max_task_request_bytes: usize, max_result_bytes: usize) -> usize {
    max_task_request_bytes.max(max_result_bytes) + FRAME_SLACK_BYTES
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// W→H
    Ready {
        proto: u16,
        pid: u32,
        resolved_exports: u32,
        init_us: u64,
        rss_bytes: u64,
    },
    /// H→W. `limits_json` is the raw limits JSON.
    Call {
        call_id: u64,
        limits_json: Vec<u8>,
        export: String,
        args_json: Vec<u8>,
    },
    /// W→H
    Submit { request_json: Vec<u8> },
    /// H→W
    Event { event_json: Vec<u8> },
    /// W→H
    Poll { seq: u32 },
    /// H→W
    NoTasks { seq: u32 },
    /// W→H
    Sync { op: String, args_json: Vec<u8> },
    /// H→W
    SyncResult { result_json: Vec<u8> },
    /// W→H
    Log { level: i32, message: Vec<u8> },
    /// W→H
    Result {
        call_id: u64,
        guest_us: u64,
        envelope_json: Vec<u8>,
    },
    /// H→W
    Shutdown,
    /// H→W. A stream chunk, raw: `0x01 | u8 id_len | stream id | bytes`.
    Chunk { event: Vec<u8> },
}

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("frame length {len} exceeds the maximum {max} (kind {kind:?})")]
    Oversize {
        len: usize,
        max: usize,
        /// Frame kind, when its byte had already been received.
        kind: Option<u8>,
    },
    #[error("empty frame (length 0, no kind byte)")]
    Empty,
    #[error("unknown frame kind {0:#04x}")]
    UnknownKind(u8),
    #[error("malformed frame (kind {kind:#04x}): {reason}")]
    Malformed { kind: u8, reason: &'static str },
    #[error("string field is {0} bytes, longer than u16::MAX")]
    StringTooLong(usize),
    #[error("frame is too large to encode ({0} bytes)")]
    EncodeTooLarge(usize),
    /// EOF in the middle of a frame.
    #[error("unexpected end of stream inside a frame")]
    Truncated,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl Frame {
    pub fn kind(&self) -> u8 {
        match self {
            Frame::Ready { .. } => KIND_READY,
            Frame::Call { .. } => KIND_CALL,
            Frame::Submit { .. } => KIND_SUBMIT,
            Frame::Event { .. } => KIND_EVENT,
            Frame::Poll { .. } => KIND_POLL,
            Frame::NoTasks { .. } => KIND_NO_TASKS,
            Frame::Sync { .. } => KIND_SYNC,
            Frame::SyncResult { .. } => KIND_SYNC_RESULT,
            Frame::Log { .. } => KIND_LOG,
            Frame::Result { .. } => KIND_RESULT,
            Frame::Shutdown => KIND_SHUTDOWN,
            Frame::Chunk { .. } => KIND_CHUNK,
        }
    }

    /// Appends the full wire encoding (`len | kind | body`) to `out`.
    pub fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), ProtoError> {
        let start = out.len();
        out.extend_from_slice(&[0; 4]);
        out.push(self.kind());
        let res = self.encode_body(out);
        if let Err(e) = res {
            out.truncate(start);
            return Err(e);
        }
        let len = out.len() - start - 4;
        let Ok(len32) = u32::try_from(len) else {
            out.truncate(start);
            return Err(ProtoError::EncodeTooLarge(len));
        };
        out[start..start + 4].copy_from_slice(&len32.to_le_bytes());
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtoError> {
        let mut v = Vec::new();
        self.encode_into(&mut v)?;
        Ok(v)
    }

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), ProtoError> {
        match self {
            Frame::Ready {
                proto,
                pid,
                resolved_exports,
                init_us,
                rss_bytes,
            } => {
                out.extend_from_slice(&proto.to_le_bytes());
                out.extend_from_slice(&pid.to_le_bytes());
                out.extend_from_slice(&resolved_exports.to_le_bytes());
                out.extend_from_slice(&init_us.to_le_bytes());
                out.extend_from_slice(&rss_bytes.to_le_bytes());
            },
            Frame::Call {
                call_id,
                limits_json,
                export,
                args_json,
            } => {
                let limits_len = u32::try_from(limits_json.len())
                    .map_err(|_| ProtoError::EncodeTooLarge(limits_json.len()))?;
                out.extend_from_slice(&call_id.to_le_bytes());
                out.extend_from_slice(&limits_len.to_le_bytes());
                out.extend_from_slice(limits_json);
                put_str16(out, export)?;
                out.extend_from_slice(args_json);
            },
            Frame::Submit { request_json } => out.extend_from_slice(request_json),
            Frame::Event { event_json } => out.extend_from_slice(event_json),
            Frame::Poll { seq } | Frame::NoTasks { seq } => {
                out.extend_from_slice(&seq.to_le_bytes())
            },
            Frame::Sync { op, args_json } => {
                put_str16(out, op)?;
                out.extend_from_slice(args_json);
            },
            Frame::SyncResult { result_json } => out.extend_from_slice(result_json),
            Frame::Log { level, message } => {
                out.extend_from_slice(&level.to_le_bytes());
                out.extend_from_slice(message);
            },
            Frame::Result {
                call_id,
                guest_us,
                envelope_json,
            } => {
                out.extend_from_slice(&call_id.to_le_bytes());
                out.extend_from_slice(&guest_us.to_le_bytes());
                out.extend_from_slice(envelope_json);
            },
            Frame::Shutdown => {},
            Frame::Chunk { event } => out.extend_from_slice(event),
        }
        Ok(())
    }

    /// Decodes `kind | body` (the bytes after the length field).
    pub fn decode_payload(payload: &[u8]) -> Result<Frame, ProtoError> {
        let (&kind, body) = payload.split_first().ok_or(ProtoError::Empty)?;
        let mut r = Cursor { kind, buf: body };
        let frame = match kind {
            KIND_READY => {
                let f = Frame::Ready {
                    proto: r.u16()?,
                    pid: r.u32()?,
                    resolved_exports: r.u32()?,
                    init_us: r.u64()?,
                    rss_bytes: r.u64()?,
                };
                r.finish()?;
                f
            },
            KIND_CALL => {
                let call_id = r.u64()?;
                let limits_len = r.u32()? as usize;
                let limits_json = r.take(limits_len)?.to_vec();
                let export = r.str16()?;
                Frame::Call {
                    call_id,
                    limits_json,
                    export,
                    args_json: r.rest(),
                }
            },
            KIND_SUBMIT => Frame::Submit {
                request_json: r.rest(),
            },
            KIND_EVENT => Frame::Event {
                event_json: r.rest(),
            },
            KIND_POLL => {
                let f = Frame::Poll { seq: r.u32()? };
                r.finish()?;
                f
            },
            KIND_NO_TASKS => {
                let f = Frame::NoTasks { seq: r.u32()? };
                r.finish()?;
                f
            },
            KIND_CHUNK => Frame::Chunk { event: r.rest() },
            KIND_SYNC => {
                let op = r.str16()?;
                Frame::Sync {
                    op,
                    args_json: r.rest(),
                }
            },
            KIND_SYNC_RESULT => Frame::SyncResult {
                result_json: r.rest(),
            },
            KIND_LOG => {
                let level = r.u32()? as i32;
                Frame::Log {
                    level,
                    message: r.rest(),
                }
            },
            KIND_RESULT => {
                let call_id = r.u64()?;
                let guest_us = r.u64()?;
                Frame::Result {
                    call_id,
                    guest_us,
                    envelope_json: r.rest(),
                }
            },
            KIND_SHUTDOWN => {
                r.finish()?;
                Frame::Shutdown
            },
            other => return Err(ProtoError::UnknownKind(other)),
        };
        Ok(frame)
    }
}

fn put_str16(out: &mut Vec<u8>, s: &str) -> Result<(), ProtoError> {
    let len = u16::try_from(s.len()).map_err(|_| ProtoError::StringTooLong(s.len()))?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

struct Cursor<'a> {
    kind: u8,
    buf: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn bad(&self, reason: &'static str) -> ProtoError {
        ProtoError::Malformed {
            kind: self.kind,
            reason,
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ProtoError> {
        if self.buf.len() < n {
            return Err(self.bad("body too short"));
        }
        let (a, b) = self.buf.split_at(n);
        self.buf = b;
        Ok(a)
    }

    fn u16(&mut self) -> Result<u16, ProtoError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, ProtoError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, ProtoError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn str16(&mut self) -> Result<String, ProtoError> {
        let n = self.u16()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| self.bad("invalid UTF-8 in str16"))
    }

    fn rest(&mut self) -> Vec<u8> {
        let v = self.buf.to_vec();
        self.buf = &[];
        v
    }

    fn finish(&self) -> Result<(), ProtoError> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(self.bad("trailing bytes"))
        }
    }
}

/// Incremental decoder: feed any chunking via [`push`](Self::push), then
/// drain frames with [`next_frame`](Self::next_frame).
#[derive(Debug)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    /// Consumed prefix of `buf`.
    pos: usize,
    max_len: usize,
}

impl FrameDecoder {
    pub fn new(max_len: usize) -> Self {
        Self {
            buf: Vec::new(),
            pos: 0,
            max_len,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        if self.pos > 0 && self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        } else if self.pos > 64 * 1024 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.extend_from_slice(data);
    }

    /// Bytes buffered but not yet returned as a frame.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Returns the next complete frame, `Ok(None)` if more bytes are needed.
    /// The length is checked as soon as the 4 length bytes are present, i.e.
    /// before the body arrives. After an error the stream is unusable.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, ProtoError> {
        let avail = &self.buf[self.pos..];
        if avail.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes(avail[..4].try_into().unwrap()) as usize;
        if len > self.max_len {
            return Err(ProtoError::Oversize {
                len,
                max: self.max_len,
                kind: avail.get(4).copied(),
            });
        }
        if len == 0 {
            return Err(ProtoError::Empty);
        }
        if avail.len() < 4 + len {
            // Size the buffer for the whole frame once, instead of letting
            // `push` double it (and copy it) while a multi-MB frame trickles in.
            let missing = 4 + len - avail.len();
            self.buf.reserve(missing);
            return Ok(None);
        }
        let frame = Frame::decode_payload(&avail[4..4 + len])?;
        self.pos += 4 + len;
        Ok(Some(frame))
    }
}

fn check_len(len: usize, max_len: usize) -> Result<(), ProtoError> {
    if len > max_len {
        return Err(ProtoError::Oversize {
            len,
            max: max_len,
            kind: None,
        });
    }
    if len == 0 {
        return Err(ProtoError::Empty);
    }
    Ok(())
}

/// Reads one frame. `Ok(None)` is a clean EOF at a frame boundary. The length
/// is validated before the body is read (an oversize frame is never buffered).
pub fn read_frame<R: Read>(r: &mut R, max_len: usize) -> Result<Option<Frame>, ProtoError> {
    let mut hdr = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut hdr[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(ProtoError::Truncated),
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {},
            Err(e) => return Err(e.into()),
        }
    }
    let len = u32::from_le_bytes(hdr) as usize;
    check_len(len, max_len)?;
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).map_err(eof_to_truncated)?;
    Frame::decode_payload(&payload).map(Some)
}

/// Encodes and writes one frame with a single `write_all`.
pub fn write_frame<W: Write>(w: &mut W, frame: &Frame) -> Result<(), ProtoError> {
    let bytes = frame.encode()?;
    w.write_all(&bytes)?;
    Ok(())
}

/// Async [`read_frame`]. Not cancel-safe mid-frame: drop the stream after a
/// cancelled read (the pump kills the worker in that case anyway).
pub async fn read_frame_async<R: AsyncRead + Unpin>(
    r: &mut R,
    max_len: usize,
) -> Result<Option<Frame>, ProtoError> {
    let mut hdr = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        let n = r.read(&mut hdr[got..]).await?;
        if n == 0 {
            return if got == 0 {
                Ok(None)
            } else {
                Err(ProtoError::Truncated)
            };
        }
        got += n;
    }
    let len = u32::from_le_bytes(hdr) as usize;
    check_len(len, max_len)?;
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)
        .await
        .map_err(eof_to_truncated)?;
    Frame::decode_payload(&payload).map(Some)
}

/// Async [`write_frame`]; does not flush.
pub async fn write_frame_async<W: AsyncWrite + Unpin>(
    w: &mut W,
    frame: &Frame,
) -> Result<(), ProtoError> {
    let bytes = frame.encode()?;
    w.write_all(&bytes).await?;
    Ok(())
}

fn eof_to_truncated(e: std::io::Error) -> ProtoError {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        ProtoError::Truncated
    } else {
        ProtoError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const MAX: usize = 1 << 20;

    /// One example per kind. The golden files in `contracts/native/frames/`
    /// are hand-derived from the NATIVE.md table and must equal these.
    fn examples() -> Vec<(&'static str, Frame)> {
        vec![
            (
                "01_ready",
                Frame::Ready {
                    proto: 1,
                    pid: 4242,
                    resolved_exports: 3,
                    init_us: 1500,
                    rss_bytes: 12_345_678,
                },
            ),
            (
                "02_call",
                Frame::Call {
                    call_id: 7,
                    limits_json: br#"{"maxResultBytes":16777216,"maxTaskRequestBytes":67108864,"maxOutstandingTasks":64,"maxLogMessageBytes":4096}"#.to_vec(),
                    export: "hello:greet".into(),
                    args_json: br#"{"name":"x"}"#.to_vec(),
                },
            ),
            (
                "03_submit",
                Frame::Submit {
                    request_json: br#"{"type":"fetch","url":"https://e.co"}"#.to_vec(),
                },
            ),
            (
                "04_event",
                Frame::Event {
                    event_json: br#"{"type":"done","taskId":0,"value":1}"#.to_vec(),
                },
            ),
            ("05_poll", Frame::Poll { seq: 9 }),
            ("06_no_tasks", Frame::NoTasks { seq: 9 }),
            (
                "07_sync",
                Frame::Sync {
                    op: "now".into(),
                    args_json: b"[]".to_vec(),
                },
            ),
            (
                "08_sync_result",
                Frame::SyncResult {
                    result_json: br#"{"ok":1700000000000}"#.to_vec(),
                },
            ),
            (
                "09_log",
                Frame::Log {
                    level: 2,
                    message: "hello log".as_bytes().to_vec(),
                },
            ),
            (
                "0a_result",
                Frame::Result {
                    call_id: 7,
                    guest_us: 250,
                    envelope_json: br#"{"ok":"hi"}"#.to_vec(),
                },
            ),
            ("0b_shutdown", Frame::Shutdown),
            (
                "0c_chunk",
                Frame::Chunk {
                    event: vec![1, 2, b's', b'1', b'h', b'i'],
                },
            ),
        ]
    }

    fn frames_dir() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // crates/isolate -> convex-backend -> vendor -> convex-rs
        let p = root.join("../../../../contracts/native/frames");
        if p.is_dir() {
            return p;
        }
        PathBuf::from("/Users/tomasdomingos/convex-rs/contracts/native/frames")
    }

    fn parse_hex(s: &str) -> Vec<u8> {
        let h: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(h.len() % 2 == 0, "odd hex length");
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect()
    }

    fn decode_all(bytes: &[u8], chunk: usize) -> Vec<Frame> {
        let mut d = FrameDecoder::new(MAX);
        let mut out = vec![];
        for c in bytes.chunks(chunk) {
            d.push(c);
            while let Some(f) = d.next_frame().unwrap() {
                out.push(f);
            }
        }
        assert_eq!(d.buffered(), 0);
        out
    }

    #[test]
    fn every_frame_round_trips() {
        for (name, f) in examples() {
            let bytes = f.encode().unwrap();
            let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
            assert_eq!(len, bytes.len() - 4, "{name}");
            assert_eq!(bytes[4], f.kind(), "{name}");
            assert_eq!(Frame::decode_payload(&bytes[4..]).unwrap(), f, "{name}");
            assert_eq!(decode_all(&bytes, usize::MAX), vec![f.clone()], "{name}");
            // sync io
            let mut rd = &bytes[..];
            assert_eq!(read_frame(&mut rd, MAX).unwrap(), Some(f.clone()));
            assert_eq!(read_frame(&mut rd, MAX).unwrap(), None);
            let mut w = Vec::new();
            write_frame(&mut w, &f).unwrap();
            assert_eq!(w, bytes);
        }
    }

    #[test]
    fn golden_fixtures() {
        let dir = frames_dir();
        for (name, f) in examples() {
            let path = dir.join(format!("{name}.hex"));
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let bytes = parse_hex(&text);
            let decoded = decode_all(&bytes, usize::MAX);
            assert_eq!(decoded, vec![f.clone()], "{name} decode");
            assert_eq!(decoded[0].encode().unwrap(), bytes, "{name} re-encode");
        }
    }

    #[test]
    fn one_byte_chunks_and_back_to_back() {
        let all: Vec<(&str, Frame)> = examples();
        let mut stream = vec![];
        for (_, f) in &all {
            f.encode_into(&mut stream).unwrap();
        }
        let want: Vec<Frame> = all.into_iter().map(|(_, f)| f).collect();
        assert_eq!(decode_all(&stream, 1), want);
        assert_eq!(decode_all(&stream, 3), want);
        assert_eq!(decode_all(&stream, usize::MAX), want);
    }

    #[test]
    fn oversize_rejected_before_body() {
        let mut d = FrameDecoder::new(100);
        // header only, claims 101 bytes
        d.push(&101u32.to_le_bytes());
        assert!(matches!(
            d.next_frame(),
            Err(ProtoError::Oversize {
                len: 101,
                max: 100,
                kind: None
            })
        ));
        let mut d = FrameDecoder::new(100);
        d.push(&101u32.to_le_bytes());
        d.push(&[KIND_RESULT]);
        assert!(matches!(
            d.next_frame(),
            Err(ProtoError::Oversize {
                kind: Some(KIND_RESULT),
                ..
            })
        ));
        // exactly at the limit is fine
        let f = Frame::Submit {
            request_json: vec![b'x'; 99],
        };
        let bytes = f.encode().unwrap();
        assert_eq!(bytes.len() - 4, 100);
        let mut d = FrameDecoder::new(100);
        d.push(&bytes);
        assert_eq!(d.next_frame().unwrap(), Some(f));
        // io helpers reject without reading the body
        let hdr = 1000u32.to_le_bytes();
        assert!(matches!(
            read_frame(&mut &hdr[..], 100),
            Err(ProtoError::Oversize { len: 1000, .. })
        ));
    }

    #[test]
    fn unknown_kind_and_malformed() {
        for k in [0x00u8, 0x0D, 0xFF] {
            let mut d = FrameDecoder::new(MAX);
            d.push(&[1, 0, 0, 0, k]);
            assert!(matches!(d.next_frame(), Err(ProtoError::UnknownKind(x)) if x == k));
        }
        let mut d = FrameDecoder::new(MAX);
        d.push(&[0, 0, 0, 0]);
        assert!(matches!(d.next_frame(), Err(ProtoError::Empty)));
        // Poll with a short body, and Shutdown with a body
        let mut d = FrameDecoder::new(MAX);
        d.push(&[3, 0, 0, 0, KIND_POLL, 1, 2]);
        assert!(matches!(d.next_frame(), Err(ProtoError::Malformed { .. })));
        let mut d = FrameDecoder::new(MAX);
        d.push(&[2, 0, 0, 0, KIND_SHUTDOWN, 0]);
        assert!(matches!(d.next_frame(), Err(ProtoError::Malformed { .. })));
        // Call whose limits_len runs past the end
        let mut body = vec![KIND_CALL];
        body.extend_from_slice(&1u64.to_le_bytes());
        body.extend_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            Frame::decode_payload(&body),
            Err(ProtoError::Malformed { .. })
        ));
        // bad UTF-8 in str16
        let body = [KIND_SYNC, 1, 0, 0xFF];
        assert!(matches!(
            Frame::decode_payload(&body),
            Err(ProtoError::Malformed { .. })
        ));
    }

    #[test]
    fn truncated_stream() {
        let bytes = Frame::Poll { seq: 1 }.encode().unwrap();
        assert!(matches!(
            read_frame(&mut &bytes[..bytes.len() - 1], MAX),
            Err(ProtoError::Truncated)
        ));
        assert!(matches!(
            read_frame(&mut &bytes[..2], MAX),
            Err(ProtoError::Truncated)
        ));
    }

    #[test]
    fn str16_too_long() {
        let f = Frame::Sync {
            op: "a".repeat(70_000),
            args_json: vec![],
        };
        let mut out = vec![1, 2, 3];
        assert!(matches!(
            f.encode_into(&mut out),
            Err(ProtoError::StringTooLong(70_000))
        ));
        assert_eq!(out, vec![1, 2, 3]);
    }

    #[test]
    fn max_frame_len_formula() {
        assert_eq!(
            max_frame_len(64 << 20, 16 << 20),
            (64 << 20) + 64 * 1024
        );
    }

    #[tokio::test]
    async fn async_round_trip_over_duplex() {
        let (mut a, mut b) = tokio::io::duplex(16);
        let frames: Vec<Frame> = examples().into_iter().map(|(_, f)| f).collect();
        let to_send = frames.clone();
        let writer = tokio::spawn(async move {
            for f in &to_send {
                write_frame_async(&mut a, f).await.unwrap();
            }
            a.shutdown().await.unwrap();
        });
        let mut got = vec![];
        while let Some(f) = read_frame_async(&mut b, MAX).await.unwrap() {
            got.push(f);
        }
        writer.await.unwrap();
        assert_eq!(got, frames);
    }

    #[tokio::test]
    async fn async_oversize_and_truncated() {
        let hdr = 5000u32.to_le_bytes();
        assert!(matches!(
            read_frame_async(&mut &hdr[..], 100).await,
            Err(ProtoError::Oversize { len: 5000, .. })
        ));
        let bytes = Frame::Poll { seq: 1 }.encode().unwrap();
        assert!(matches!(
            read_frame_async(&mut &bytes[..6], MAX).await,
            Err(ProtoError::Truncated)
        ));
    }
}
