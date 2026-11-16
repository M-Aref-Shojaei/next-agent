//! The log files: `next-agent-YYYY-MM-DD[.N].jsonl` in one folder, rolled
//! daily (UTC) and at 10 MB, pruned past the retention age or total size.
//! Folder 0700, files 0600. Each line is written whole with one `write_all`
//! on an unbuffered file, so a crash loses at most the line being written.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A file is rolled once the next line would take it past this size.
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const PREFIX: &str = "next-agent-";
const SUFFIX: &str = ".jsonl";

pub(crate) struct Roller {
    dir: PathBuf,
    retention: Duration,
    max_total_bytes: u64,
    max_file_bytes: u64,
    file: Option<File>,
    current: PathBuf,
    date: String,
    index: u32,
    size: u64,
}

impl Roller {
    pub(crate) fn open(dir: &Path, retention_days: u32, max_total_bytes: u64) -> io::Result<Self> {
        create_private_dir(dir)?;
        let roller = Self {
            dir: dir.to_path_buf(),
            retention: Duration::from_secs(u64::from(retention_days) * 86_400),
            max_total_bytes,
            max_file_bytes: MAX_FILE_BYTES,
            file: None,
            current: PathBuf::new(),
            date: String::new(),
            index: 0,
            size: 0,
        };
        roller.prune();
        Ok(roller)
    }

    #[cfg(test)]
    pub(crate) fn with_max_file_bytes(mut self, bytes: u64) -> Self {
        self.max_file_bytes = bytes;
        self
    }

    /// Append one line (without its newline) to today's file.
    pub(crate) fn write_line(&mut self, line: &str, date: &str) -> io::Result<()> {
        let len = line.len() as u64 + 1;
        let full = self.size > 0 && self.size + len > self.max_file_bytes;
        if self.file.is_none() || self.date != date || full {
            self.roll(date)?;
        }
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        if let Some(file) = self.file.as_mut() {
            file.write_all(&bytes)?;
            self.size += len;
        }
        Ok(())
    }

    fn roll(&mut self, date: &str) -> io::Result<()> {
        if self.date == date && self.file.is_some() {
            self.index += 1;
        } else {
            self.date = date.to_string();
            self.index = 0;
        }
        // Reopening after a restart: skip today's files that are already full.
        loop {
            let path = self.dir.join(file_name(&self.date, self.index));
            let size = fs::metadata(&path).map_or(0, |m| m.len());
            if size < self.max_file_bytes {
                self.file = Some(open_private(&path)?);
                self.current = path;
                self.size = size;
                break;
            }
            self.index += 1;
        }
        self.prune();
        Ok(())
    }

    pub(crate) fn flush(&mut self) {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
            let _ = file.sync_data();
        }
    }

    /// Delete files past the retention age, then the oldest until the total
    /// fits. The file being written is never deleted.
    fn prune(&self) {
        let now = SystemTime::now();
        let mut kept = Vec::new();
        for (path, modified, size) in log_files(&self.dir) {
            let old = now.duration_since(modified).unwrap_or_default() > self.retention;
            if old && path != self.current {
                let _ = fs::remove_file(&path);
            } else {
                kept.push((path, size));
            }
        }
        let mut total: u64 = kept.iter().map(|(_, size)| size).sum();
        for (path, size) in kept {
            if total <= self.max_total_bytes {
                break;
            }
            if path != self.current && fs::remove_file(&path).is_ok() {
                total -= size;
            }
        }
    }
}

fn file_name(date: &str, index: u32) -> String {
    if index == 0 {
        format!("{PREFIX}{date}{SUFFIX}")
    } else {
        format!("{PREFIX}{date}.{index}{SUFFIX}")
    }
}

/// The log files in `dir`, oldest first: `(path, modified, size)`.
pub(crate) fn log_files(dir: &Path) -> Vec<(PathBuf, SystemTime, u64)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with(PREFIX) && name.ends_with(SUFFIX)
        })
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(fs::Metadata::is_file)?;
            Some((e.path(), meta.modified().ok()?, meta.len()))
        })
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    files
}

pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Open for appending, creating the file with mode 0600.
pub(crate) fn open_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let file = options.open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn rolls_daily_and_by_size() {
        let dir = tempfile::tempdir().unwrap();
        let mut roller = Roller::open(dir.path(), 14, u64::MAX)
            .unwrap()
            .with_max_file_bytes(20);
        roller.write_line("0123456789", "2026-11-14").unwrap();
        roller.write_line("0123456789", "2026-11-15").unwrap();
        roller.write_line("abc", "2026-11-15").unwrap();
        // 11 + 11 > 20: a new file.
        roller.write_line("0123456789", "2026-11-15").unwrap();
        assert_eq!(
            names(dir.path()),
            [
                "next-agent-2026-11-14.jsonl",
                "next-agent-2026-11-15.1.jsonl",
                "next-agent-2026-11-15.jsonl"
            ]
        );
        let first = fs::read_to_string(dir.path().join("next-agent-2026-11-15.jsonl")).unwrap();
        assert_eq!(first, "0123456789\nabc\n");
    }

    #[test]
    fn a_restart_appends_to_today_and_skips_full_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("next-agent-2026-11-15.jsonl"),
            "x".repeat(30),
        )
        .unwrap();
        let mut roller = Roller::open(dir.path(), 14, u64::MAX)
            .unwrap()
            .with_max_file_bytes(20);
        roller.write_line("line", "2026-11-15").unwrap();
        let next = fs::read_to_string(dir.path().join("next-agent-2026-11-15.1.jsonl")).unwrap();
        assert_eq!(next, "line\n");
    }

    #[test]
    fn prunes_by_age_then_total_size_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let make = |name: &str, bytes: usize, age_days: u64| {
            let path = dir.path().join(name);
            fs::write(&path, "x".repeat(bytes)).unwrap();
            let file = File::options().write(true).open(&path).unwrap();
            file.set_modified(now - Duration::from_secs(age_days * 86_400 + 60))
                .unwrap();
        };
        make("next-agent-2026-10-01.jsonl", 10, 30);
        make("next-agent-2026-11-10.jsonl", 40, 5);
        make("next-agent-2026-11-12.jsonl", 40, 3);
        make("next-agent-2026-11-14.jsonl", 40, 1);
        make("unrelated.txt", 500, 30);
        let _roller = Roller::open(dir.path(), 14, 100).unwrap();
        assert_eq!(
            names(dir.path()),
            [
                "next-agent-2026-11-12.jsonl",
                "next-agent-2026-11-14.jsonl",
                "unrelated.txt"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn folder_and_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("logs");
        let mut roller = Roller::open(&dir, 14, u64::MAX).unwrap();
        roller.write_line("{}", "2026-11-15").unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("next-agent-2026-11-15.jsonl")), 0o600);
    }
}
