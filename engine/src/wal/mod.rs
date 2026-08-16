pub mod engine;

use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Lines, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing;

use crate::types::WalEntryType;

/// On-disk format version of WAL entries. Bump whenever the entry layout
/// changes so old logs can be detected (and rejected) rather than misread.
pub const WAL_VERSION: u8 = 1;

/// FNV-1a 64-bit hash, used for per-entry checksums. Chosen for its small,
/// dependency-free implementation; entries are short, so speed is irrelevant.
fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// A single append-only log entry. Each line in the WAL file is one entry.
///
/// `version` and `checksum` use `#[serde(default)]` so logs written by older
/// versions (before these fields existed) still deserialize — and, lacking a
/// checksum, are trusted as-is.
#[derive(Serialize, Deserialize)]
pub struct WalEntry {
    seq: u64,
    #[serde(default)]
    version: u8,
    #[serde(default)]
    checksum: Option<String>,
    entry: WalEntryType,
}

impl WalEntry {
    pub fn new(seq: u64, entry: WalEntryType) -> Self {
        WalEntry {
            seq,
            version: WAL_VERSION,
            checksum: None,
            entry,
        }
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn checksum(&self) -> Option<&str> {
        self.checksum.as_deref()
    }

    pub fn entry(&self) -> &WalEntryType {
        &self.entry
    }

    /// Checksum over the entry's logical contents `(seq, entry)`, computed
    /// independently of the stored `checksum` field so it can be re-derived
    /// for verification.
    fn compute_checksum(&self) -> String {
        let body = serde_json::to_vec(&(self.seq, &self.entry)).expect("WAL serialization");
        format!("{:016x}", fnv1a64(&body))
    }

    /// Returns `true` if the stored checksum matches the entry's contents.
    /// Entries without a checksum (legacy logs) are trusted and return `true`.
    pub fn verify_checksum(&self) -> bool {
        match &self.checksum {
            Some(expected) => &self.compute_checksum() == expected,
            None => true,
        }
    }
}

pub struct WalWriter {
    path: PathBuf,
    writer: BufWriter<File>,
    seq: u64,
    /// When `true`, each entry is `fsync`ed to disk before `write` returns.
    /// Set via the `WAL_SYNC=true` env var; off by default for throughput.
    sync: bool,
}

impl WalWriter {
    // Opens (or creates) the WAL file at `path` for appending
    ///
    /// If the file already exists, new entries are appended after the
    /// existing content. The initial sequence number is `0` — the caller
    /// (typically [`WalEngine`](crate::wal::WalEngine)) is responsible for
    /// scanning existing entries and setting the correct starting `seq`.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Failed to open WAL file {}: {e}", path.display()))?;
        let sync = std::env::var("WAL_SYNC")
            .map(|v| v == "true")
            .unwrap_or(false);
        Ok(WalWriter {
            path,
            writer: BufWriter::new(file),
            seq: 0,
            sync,
        })
    }

    /// Returns the current sequence number
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Sets the sequence number (used during recovery to resume from the last known entry)
    pub fn set_seq(&mut self, seq: u64) {
        self.seq = seq;
    }

    pub fn write(&mut self, mut entry: WalEntry) -> Result<u64, String> {
        self.seq += 1;
        entry.seq = self.seq;
        entry.version = WAL_VERSION;
        entry.checksum = Some(entry.compute_checksum());

        let mut line =
            serde_json::to_vec(&entry).map_err(|e| format!("WAL serialize error: {e}"))?;
        line.push(b'\n');

        self.writer
            .write_all(&line)
            .map_err(|e| format!("WAL write error: {e}"))?;
        self.writer
            .flush()
            .map_err(|e| format!("WAL flush error: {e}"))?;
        if self.sync {
            self.writer
                .get_ref()
                .sync_all()
                .map_err(|e| format!("WAL fsync error: {e}"))?;
        }
        Ok(self.seq)
    }

    // Return path to the WAL file
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Reads and deserializes the WAL entries from a file, one JSON line at a time.
/// Used during recovery.
pub struct WalReader {
    lines: Lines<BufReader<File>>,
}

impl WalReader {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, String> {
        let file = File::open(path.as_ref())
            .map_err(|e| format!("Failed to open WAL file for reading: {e}"))?;
        Ok(WalReader {
            lines: BufReader::new(file).lines(),
        })
    }
}

impl Iterator for WalReader {
    type Item = WalEntry;
    fn next(&mut self) -> Option<Self::Item> {
        for line in &mut self.lines {
            match line {
                Ok(line) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<WalEntry>(line) {
                        Ok(entry) => {
                            if entry.verify_checksum() {
                                return Some(entry);
                            }
                            tracing::error!(
                                seq = entry.seq(),
                                "WAL: checksum mismatch on entry {} — skipping; log may be corrupt",
                                entry.seq()
                            );
                        }
                        Err(e) => {
                            // skip malformed lines
                            tracing::warn!("WAL: skipping malformed entry: {e}");
                            continue;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("WAL: read error: {e}");
                    return None;
                }
            }
        }
        None
    }
}
