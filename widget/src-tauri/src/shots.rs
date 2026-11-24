//! Screenshots kept on this computer so the engineer can check their day in the
//! review. Never uploaded for storage (the Autotrack service only sees a screenshot while
//! classifying it). Files live in `<app data>/captures/` with owner-only permissions
//! and are deleted after `keep_screenshots_days`, on sign-out, or by the user.

use crate::capture::Shot;
use crate::{AppState, db, fmt_time};
use base64::Engine;
use chrono::{Duration as Span, Utc};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("captures")
}

/// Ids are UUIDs we generate; anything else is refused (no path tricks).
pub fn valid_id(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn file(data_dir: &Path, id: &str, thumb: bool) -> PathBuf {
    dir(data_dir).join(if thumb {
        format!("{id}.thumb.jpg")
    } else {
        format!("{id}.jpg")
    })
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(bytes)
}

fn ensure_dir(data_dir: &Path) -> std::io::Result<PathBuf> {
    let d = dir(data_dir);
    std::fs::create_dir_all(&d)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(d)
}

pub fn save(st: &AppState, row: &db::CaptureRow, shot: &Shot) -> Result<(), String> {
    ensure_dir(&st.data_dir).map_err(|e| e.to_string())?;
    let full = file(&st.data_dir, &row.id, false);
    let thumb = file(&st.data_dir, &row.id, true);
    let written = write_private(&full, &shot.jpeg).and_then(|_| write_private(&thumb, &shot.thumb));
    if let Err(e) = written
        .map_err(|e| e.to_string())
        .and_then(|_| db::add_capture(&st.db.lock().unwrap(), row).map_err(|e| e.to_string()))
    {
        let _ = std::fs::remove_file(&full);
        let _ = std::fs::remove_file(&thumb);
        return Err(e);
    }
    Ok(())
}

/// An activity record without a picture (Screen Recording not allowed, or the capture
/// failed): the review still shows the window and the classification.
pub fn save_record(st: &AppState, row: &db::CaptureRow) -> Result<(), String> {
    db::add_capture(&st.db.lock().unwrap(), row).map_err(|e| e.to_string())
}

pub fn data_url(data_dir: &Path, id: &str, thumb: bool) -> Option<String> {
    if !valid_id(id) {
        return None;
    }
    let bytes = std::fs::read(file(data_dir, id, thumb)).ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

fn remove_files(data_dir: &Path, id: &str) {
    let _ = std::fs::remove_file(file(data_dir, id, false));
    let _ = std::fs::remove_file(file(data_dir, id, true));
}

pub fn delete(st: &AppState, id: &str) -> Result<bool, String> {
    if !valid_id(id) {
        return Err("Unknown screenshot".into());
    }
    let found = db::delete_capture(&st.db.lock().unwrap(), id).map_err(|e| e.to_string())?;
    remove_files(&st.data_dir, id);
    Ok(found)
}

/// Deletes screenshots older than `keep_days` (all of them when 0) and any stray file
/// that has no record (e.g. after a crash).
pub fn prune(st: &AppState, keep_days: i64) {
    let before = if keep_days <= 0 {
        "9999".to_string()
    } else {
        fmt_time(Utc::now() - Span::days(keep_days))
    };
    let (old, known) = {
        let db = st.db.lock().unwrap();
        (
            db::prune_captures(&db, &before).unwrap_or_default(),
            db::capture_ids(&db).unwrap_or_default(),
        )
    };
    for id in &old {
        remove_files(&st.data_dir, id);
    }
    let Ok(entries) = std::fs::read_dir(dir(&st.data_dir)) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let id = name.split('.').next().unwrap_or_default();
        if !known.iter().any(|k| k == id) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Sign-out: every screenshot goes.
pub fn delete_all(st: &AppState) {
    let _ = std::fs::remove_dir_all(dir(&st.data_dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_generated_ids_are_accepted() {
        assert!(valid_id("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert!(!valid_id("../../etc/passwd"));
        assert!(!valid_id("0f8fad5b-d9cb-469f-a165-70867728950e/../x"));
        assert!(!valid_id(""));
    }

    #[test]
    fn files_are_private_and_readable_as_data_urls() {
        let tmp = std::env::temp_dir().join(format!("timepilot-shots-{}", uuid::Uuid::new_v4()));
        let d = ensure_dir(&tmp).unwrap();
        assert!(d.is_dir());
        let id = uuid::Uuid::new_v4().to_string();
        write_private(&file(&tmp, &id, true), &[0xFF, 0xD8, 0xFF]).unwrap();
        let url = data_url(&tmp, &id, true).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,/9j/"));
        assert_eq!(data_url(&tmp, "../secret", true), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file(&tmp, &id, true))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
            assert_eq!(
                std::fs::metadata(&d).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
