use std::{
    fs::OpenOptions,
    io::{
        Read,
        Seek,
        SeekFrom,
        Write,
    },
    path::{
        Path,
        PathBuf,
    },
};

use async_trait::async_trait;
use parking_lot::Mutex;

use super::{
    BatchResult,
    Firehose,
};

pub struct LocalFileFirehose {
    path: PathBuf,
}

impl LocalFileFirehose {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl Firehose for LocalFileFirehose {
    async fn send_batch(&self, records: Vec<String>) -> anyhow::Result<BatchResult> {
        if !records.is_empty() {
            let path = self.path.clone();
            tokio::task::spawn_blocking(move || write_firehose_file(&path, &records)).await??;
        }
        Ok(BatchResult::default())
    }
}

/// Append local Firehose payloads as JSONL. A payload may contain multiple
/// lines.
fn write_firehose_file(path: &Path, records: &[String]) -> anyhow::Result<()> {
    // Deployment clients can target the same file; keep payloads and separators
    // together across all handles in this process.
    static WRITE_LOCK: Mutex<()> = Mutex::new(());
    let _guard = WRITE_LOCK.lock();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    if file.metadata()?.len() > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut last_byte = [0];
        file.read_exact(&mut last_byte)?;
        if last_byte[0] != b'\n' {
            file.write_all(b"\n")?;
        }
    }
    for record in records {
        writeln!(file, "{record}")?;
    }
    file.flush()?;
    Ok(())
}
