//! Runtime artifact store (DESIGN.md §5.4, NATIVE.md §5.2).
//!
//! A carrier module source is `NATIVE_MAGIC` + one JSON header line + the
//! base64 of the cdylib. [`ArtifactStore::resolve`] decodes it, verifies the
//! sha256 and size against the header, writes the binary atomically (tmp file
//! plus rename, mode `0o500`) to `<RUST_NATIVE_DIR>/bin/<sha>.so|dylib` and
//! returns the [`Artifact`]. An existing file with the same sha is reused.
//! Results are cached in process by binary sha256 and, optionally, by the
//! caller's module-source key (`ModuleMetadata.sha256`) so a warm call never
//! touches the module source again.

use std::{
    collections::{
        BTreeMap,
        HashMap,
        HashSet,
    },
    io::Write as _,
    os::unix::fs::{
        MetadataExt,
        OpenOptionsExt,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
    },
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context as _;
use parking_lot::Mutex;
use serde::Deserialize;
use sha2::{
    Digest,
    Sha256,
};

use super::{
    Artifact,
    NATIVE_ABI_VERSION,
    NATIVE_MAGIC,
};

/// Library file extension of the host platform.
pub const LIB_EXTENSION: &str = if cfg!(target_os = "macos") { "dylib" } else { "so" };
/// Half-written files older than this are removed by [`ArtifactStore::gc`].
const STALE_TMP_AGE: Duration = Duration::from_secs(3600);

/// The JSON header line of a carrier (NATIVE.md §5.2 step 6).
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CarrierHeader {
    pub format: u32,
    pub native_abi: u32,
    pub abi: u32,
    pub triple: String,
    pub glibc_max: Option<String>,
    pub sha256: String,
    pub size: u64,
    /// `module:function` -> exported `convex_run:` symbol.
    pub functions: BTreeMap<String, String>,
}

/// A decoded carrier: header plus the verified binary.
#[derive(Debug)]
pub struct DecodedCarrier {
    pub header: CarrierHeader,
    pub binary: Vec<u8>,
}

pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// Decodes a carrier source and verifies format versions, size and sha256.
pub fn decode_carrier(source: &str) -> anyhow::Result<DecodedCarrier> {
    let rest = source
        .strip_prefix(NATIVE_MAGIC)
        .context("not a native carrier module (missing the `//convex-rust-native v1` line)")?;
    let (header_line, body) = rest
        .split_once('\n')
        .context("native carrier has no binary body")?;
    let header: CarrierHeader =
        serde_json::from_str(header_line).context("invalid native carrier header")?;
    anyhow::ensure!(header.format == 1, "unsupported carrier format {}", header.format);
    anyhow::ensure!(
        header.native_abi == NATIVE_ABI_VERSION,
        "unsupported native ABI {} (this backend runs {NATIVE_ABI_VERSION})",
        header.native_abi
    );
    anyhow::ensure!(header.abi == 1, "unsupported guest ABI {}", header.abi);
    anyhow::ensure!(
        header.sha256.len() == 64 && header.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "carrier sha256 is not a hex digest"
    );
    let compact: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let binary = base64::decode(compact.as_bytes()).context("invalid base64 in native carrier")?;
    anyhow::ensure!(
        binary.len() as u64 == header.size,
        "native carrier size mismatch: header says {} bytes, body has {}",
        header.size,
        binary.len()
    );
    let actual = sha256_hex(&binary);
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(&header.sha256),
        "native carrier sha256 mismatch: header says {}, body hashes to {actual}",
        header.sha256
    );
    let mut header = header;
    header.sha256 = actual;
    Ok(DecodedCarrier { header, binary })
}

/// `RUST_NATIVE_DIR`.
pub fn native_dir_from_env() -> PathBuf {
    PathBuf::from(&*common::knobs::RUST_NATIVE_DIR)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GcReport {
    pub removed: Vec<String>,
    pub kept: usize,
    pub bytes_freed: u64,
}

#[derive(Default)]
struct Cache {
    by_sha: HashMap<String, Arc<Artifact>>,
    by_source_key: HashMap<Vec<u8>, Arc<Artifact>>,
}

/// On-disk plus in-process artifact store under one `RUST_NATIVE_DIR`.
pub struct ArtifactStore {
    root: PathBuf,
    cache: Mutex<Cache>,
    /// Serialises writes of the same store (distinct shas could write in
    /// parallel, but pushes are rare and this keeps tmp names trivial).
    write_lock: Mutex<()>,
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl ArtifactStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: Mutex::new(Cache::default()),
            write_lock: Mutex::new(()),
        }
    }

    pub fn from_env() -> Self {
        Self::new(native_dir_from_env())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// Per-worker scratch dirs live here (`SandboxPlan::tmp_dir`).
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn path_for(&self, sha256: &str) -> PathBuf {
        self.bin_dir().join(format!("{sha256}.{LIB_EXTENSION}"))
    }

    /// Cache lookup by binary sha256. A hit whose file vanished is dropped.
    pub fn cached(&self, sha256: &str) -> Option<Arc<Artifact>> {
        let a = self.cache.lock().by_sha.get(sha256).cloned()?;
        if a.path.exists() {
            Some(a)
        } else {
            self.forget(sha256);
            None
        }
    }

    /// Cache lookup by the caller's module-source key (`ModuleMetadata.sha256`).
    pub fn cached_by_source_key(&self, key: &[u8]) -> Option<Arc<Artifact>> {
        let a = self.cache.lock().by_source_key.get(key).cloned()?;
        self.cached(&a.sha256)
    }

    fn forget(&self, sha256: &str) {
        let mut c = self.cache.lock();
        c.by_sha.remove(sha256);
        c.by_source_key.retain(|_, a| a.sha256 != sha256);
    }

    /// Decodes, verifies and stores a carrier source; cached by sha.
    pub fn resolve(&self, carrier_source: &str) -> anyhow::Result<Arc<Artifact>> {
        self.resolve_inner(None, carrier_source)
    }

    /// As [`Self::resolve`], also remembering the artifact under `source_key`.
    pub fn resolve_with_key(
        &self,
        source_key: &[u8],
        carrier_source: &str,
    ) -> anyhow::Result<Arc<Artifact>> {
        self.resolve_inner(Some(source_key), carrier_source)
    }

    fn resolve_inner(
        &self,
        source_key: Option<&[u8]>,
        carrier_source: &str,
    ) -> anyhow::Result<Arc<Artifact>> {
        let decoded = decode_carrier(carrier_source)?;
        let sha = decoded.header.sha256.clone();
        let artifact = match self.cached(&sha) {
            Some(a) => a,
            None => {
                let path = self.install(&sha, &decoded.binary)?;
                let a = Arc::new(Artifact {
                    sha256: sha.clone(),
                    path,
                    size: decoded.binary.len() as u64,
                    functions: decoded.header.functions,
                });
                self.cache.lock().by_sha.insert(sha, a.clone());
                a
            },
        };
        if let Some(key) = source_key {
            self.cache
                .lock()
                .by_source_key
                .insert(key.to_vec(), artifact.clone());
        }
        Ok(artifact)
    }

    /// Writes `<bin>/<sha>.<ext>` unless a file with that sha is there.
    fn install(&self, sha: &str, binary: &[u8]) -> anyhow::Result<PathBuf> {
        let _w = self.write_lock.lock();
        let bin = self.bin_dir();
        create_private_dir(&self.root)?;
        create_private_dir(&bin)?;
        let dest = self.path_for(sha);
        if existing_matches(&dest, sha, binary.len() as u64) {
            return Ok(dest);
        }
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = bin.join(format!(".{sha}.{}.{n}.tmp", std::process::id()));
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o500)
                .open(&tmp)?;
            f.write_all(binary)?;
            f.sync_all()?;
            Ok(())
        };
        if let Err(e) = write() {
            let _ = std::fs::remove_file(&tmp);
            return Err(e).with_context(|| format!("cannot write {}", tmp.display()));
        }
        if let Err(e) = std::fs::rename(&tmp, &dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e).with_context(|| format!("cannot install {}", dest.display()));
        }
        Ok(dest)
    }

    /// Removes artifacts whose sha is not in `live`, and stale tmp files.
    /// Unlinking a library a worker has already `dlopen`ed is harmless, but a
    /// respawn of that pool would fail: pass every sha still referenced by a
    /// module or a live pool.
    pub fn gc(&self, live: &HashSet<String>) -> GcReport {
        let mut report = GcReport::default();
        let Ok(rd) = std::fs::read_dir(self.bin_dir()) else {
            return report;
        };
        // An `Arc<Artifact>` held outside the cache (a call between `resolve`
        // and its worker spawn, or a pool respawning) is in use even if no
        // module or pool key names it yet. The cache itself holds one `Arc`
        // in `by_sha` plus one per source key.
        let held: HashSet<String> = {
            let c = self.cache.lock();
            c.by_sha
                .iter()
                .filter(|(_, a)| {
                    let own = 1 + c.by_source_key.values().filter(|k| Arc::ptr_eq(k, a)).count();
                    Arc::strong_count(a) > own
                })
                .map(|(sha, _)| sha.clone())
                .collect()
        };
        let now = SystemTime::now();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            if name.starts_with('.') {
                let old = meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .is_some_and(|age| age > STALE_TMP_AGE);
                if old && std::fs::remove_file(&path).is_ok() {
                    report.bytes_freed += meta.size();
                }
                continue;
            }
            let stem = name.strip_suffix(&format!(".{LIB_EXTENSION}"));
            match stem {
                Some(sha) if live.contains(sha) || held.contains(sha) => report.kept += 1,
                Some(sha) => {
                    if std::fs::remove_file(&path).is_ok() {
                        report.bytes_freed += meta.size();
                        report.removed.push(sha.to_string());
                        self.forget(sha);
                    }
                },
                None => {},
            }
        }
        report
    }
}

fn create_private_dir(dir: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("cannot create {}", dir.display()))
}

/// The file exists with the right size and content hash.
fn existing_matches(path: &Path, sha: &str, size: u64) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() != size {
        return false;
    }
    match std::fs::read(path) {
        Ok(data) => sha256_hex(&data) == sha,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn tmp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rn-artifact-{tag}-{}-{}",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    pub(crate) fn carrier_for(binary: &[u8], functions: &[(&str, &str)]) -> String {
        let fns: BTreeMap<_, _> = functions.iter().map(|(a, b)| (*a, *b)).collect();
        let header = serde_json::json!({
            "format": 1, "nativeAbi": 1, "abi": 1, "triple": "x", "glibcMax": null,
            "sha256": sha256_hex(binary), "size": binary.len(), "functions": fns, "sdk": "t",
        });
        format!("{NATIVE_MAGIC}{header}\n{}", base64::encode(binary))
    }

    #[test]
    fn decode_verifies_sha_and_size() {
        let src = carrier_for(b"hello", &[("m:f", "convex_run:m:f")]);
        let d = decode_carrier(&src).unwrap();
        assert_eq!(d.binary, b"hello");
        assert_eq!(d.header.functions["m:f"], "convex_run:m:f");

        let bad_sha = src.replace(&sha256_hex(b"hello"), &sha256_hex(b"other"));
        assert!(decode_carrier(&bad_sha).unwrap_err().to_string().contains("sha256 mismatch"));
        let bad_size = src.replace("\"size\":5", "\"size\":6");
        assert!(decode_carrier(&bad_size).unwrap_err().to_string().contains("size mismatch"));
        assert!(decode_carrier("//other\n{}").is_err());
        assert!(decode_carrier(&format!("{NATIVE_MAGIC}{{}}\nAAAA")).is_err());
        let abi2 = src.replace("\"nativeAbi\":1", "\"nativeAbi\":2");
        assert!(decode_carrier(&abi2).unwrap_err().to_string().contains("native ABI"));
    }

    #[test]
    fn resolve_writes_atomically_with_mode_and_reuses() {
        let root = tmp_root("resolve");
        let store = ArtifactStore::new(root.clone());
        let src = carrier_for(b"binary-bytes", &[("m:f", "convex_run:m:f")]);
        let a = store.resolve_with_key(b"key1", &src).unwrap();
        assert_eq!(a.path, root.join("bin").join(format!("{}.{LIB_EXTENSION}", a.sha256)));
        assert_eq!(std::fs::read(&a.path).unwrap(), b"binary-bytes");
        assert_eq!(std::fs::metadata(&a.path).unwrap().permissions().mode() & 0o777, 0o500);
        assert_eq!(a.size, 12);
        // No tmp files left behind.
        let leftovers = std::fs::read_dir(root.join("bin"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .count();
        assert_eq!(leftovers, 0);

        // In-process cache: same Arc, by sha and by source key.
        let b = store.resolve(&src).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(Arc::ptr_eq(&a, &store.cached_by_source_key(b"key1").unwrap()));
        assert!(store.cached_by_source_key(b"nope").is_none());

        // A fresh store (a restart) reuses the file on disk without rewriting it.
        let inode = std::fs::metadata(&a.path).unwrap().ino();
        let store2 = ArtifactStore::new(root.clone());
        let c = store2.resolve(&src).unwrap();
        assert_eq!(std::fs::metadata(&c.path).unwrap().ino(), inode);

        // A corrupt file with the right name is replaced.
        let mut p = std::fs::metadata(&a.path).unwrap().permissions();
        p.set_mode(0o600);
        std::fs::set_permissions(&a.path, p).unwrap();
        std::fs::write(&a.path, b"corrupt").unwrap();
        let store3 = ArtifactStore::new(root.clone());
        let d = store3.resolve(&src).unwrap();
        assert_eq!(std::fs::read(&d.path).unwrap(), b"binary-bytes");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cache_drops_entries_whose_file_vanished() {
        let root = tmp_root("vanish");
        let store = ArtifactStore::new(root.clone());
        let src = carrier_for(b"abc", &[]);
        let a = store.resolve(&src).unwrap();
        std::fs::remove_file(&a.path).unwrap();
        assert!(store.cached(&a.sha256).is_none());
        // Resolving again rewrites it.
        let b = store.resolve(&src).unwrap();
        assert!(b.path.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn gc_removes_unreferenced_artifacts_only() {
        let root = tmp_root("gc");
        let store = ArtifactStore::new(root.clone());
        let a = store.resolve(&carrier_for(b"one", &[])).unwrap();
        let (b, c) = {
            let b = store.resolve(&carrier_for(b"two", &[])).unwrap();
            let c = store.resolve(&carrier_for(b"three", &[])).unwrap();
            ((b.sha256.clone(), b.path.clone()), (c.sha256.clone(), c.path.clone()))
        };
        // A fresh tmp file is kept, a stale one removed.
        let fresh = root.join("bin").join(".fresh.tmp");
        let stale = root.join("bin").join(".stale.tmp");
        std::fs::write(&fresh, b"x").unwrap();
        std::fs::write(&stale, b"x").unwrap();
        let old = SystemTime::now() - Duration::from_secs(7200);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();

        let live: HashSet<String> = [a.sha256.clone()].into();
        let report = store.gc(&live);
        assert_eq!(report.kept, 1);
        let mut removed = report.removed.clone();
        removed.sort();
        let mut want = vec![b.0.clone(), c.0.clone()];
        want.sort();
        assert_eq!(removed, want);
        assert!(a.path.exists() && !b.1.exists() && !c.1.exists());
        assert!(fresh.exists() && !stale.exists());
        // Removed entries left the cache too.
        assert!(store.cached(&b.0).is_none());
        assert!(store.cached(&a.sha256).is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    /// N-T5: an artifact still held by an in-flight call (not in `live`)
    /// survives gc; once released it goes.
    #[test]
    fn gc_keeps_artifacts_held_by_in_flight_calls() {
        let root = tmp_root("gc-held");
        let store = ArtifactStore::new(root.clone());
        let held = store.resolve_with_key(b"k", &carrier_for(b"held", &[])).unwrap();
        let none = HashSet::new();
        let report = store.gc(&none);
        assert!(report.removed.is_empty() && held.path.exists());
        let (sha, path) = (held.sha256.clone(), held.path.clone());
        drop(held);
        let report = store.gc(&none);
        assert_eq!(report.removed, vec![sha]);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
