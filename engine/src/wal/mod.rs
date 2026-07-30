pub mod engine;

use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Lines, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing;

use crate::types::WalEntryType;

#[derive(Serialize, Deserialize)]
pub struct WalEntry {
    seq: u64,
    entry: WalEntryType,
}

impl WalEntry {
    pub fn new(seq: u64, entry: WalEntryType) -> Self {
        WalEntry { seq, entry }
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn entry(&self) -> &WalEntryType {
        &self.entry
    }
}

pub struct WalWriter {
    path: PathBuf,
    writer: BufWriter<File>,
    seq: u64,
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
        Ok(WalWriter {
            path,
            writer: BufWriter::new(file),
            seq: 0,
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

        let mut line =
            serde_json::to_vec(&entry).map_err(|e| format!("WAL serialize error: {e}"))?;
        line.push(b'\n');

        self.writer
            .write_all(&line)
            .map_err(|e| format!("WAL write error: {e}"))?;
        self.writer
            .flush()
            .map_err(|e| format!("WAL flush error: {e}"))?;
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
                        Ok(entry) => return Some(entry),
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
