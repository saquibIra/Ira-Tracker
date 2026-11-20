//! App log file for support: `<app data>/logs/ira-tracker.log`, rotated at 1 MB (one
//! previous file kept). Also printed to stderr when run from a terminal.
//!
//! Never log window titles, screenshots, descriptions or tokens.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 1024 * 1024;

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

struct FileLog {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

impl FileLog {
    fn open(path: &Path) -> Option<File> {
        OpenOptions::new().create(true).append(true).open(path).ok()
    }

    fn rotate_if_big(&self, file: &mut Option<File>) {
        let big = file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .is_some_and(|m| m.len() > MAX_BYTES);
        if big {
            *file = None;
            let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
            *file = Self::open(&self.path);
        }
    }
}

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info || m.target().starts_with("ira_tracker")
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}: {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            r.level(),
            r.target().trim_start_matches("ira_tracker_lib::"),
            r.args()
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        let mut guard = self.file.lock().unwrap_or_else(|p| p.into_inner());
        self.rotate_if_big(&mut guard);
        if let Some(f) = guard.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            let _ = f.flush();
        }
    }
}

/// Starts logging to the app's log folder. Safe to call once at start-up.
pub fn init(data_dir: &Path) {
    let d = dir(data_dir);
    let _ = std::fs::create_dir_all(&d);
    let path = d.join("ira-tracker.log");
    let logger = FileLog {
        file: Mutex::new(FileLog::open(&path)),
        path,
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log;

    #[test]
    fn rotates_when_the_file_gets_big() {
        let tmp = std::env::temp_dir().join(format!("ira-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("ira-tracker.log");
        std::fs::write(&path, vec![b'x'; (MAX_BYTES + 10) as usize]).unwrap();
        let logger = FileLog {
            file: Mutex::new(FileLog::open(&path)),
            path: path.clone(),
        };
        logger.log(
            &log::Record::builder()
                .args(format_args!("hello"))
                .level(log::Level::Info)
                .target("ira_tracker_lib::auto")
                .build(),
        );
        logger.flush();
        assert!(tmp.join("ira-tracker.log.1").exists());
        let fresh = std::fs::read_to_string(&path).unwrap();
        assert!(fresh.contains("INFO  auto: hello"), "{fresh}");
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
