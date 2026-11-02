//! Local SQLite store: the offline queue of time entries plus a small key/value table.
//!
//! The local database is the widget's source of truth. Every user action writes here
//! first (instant), marks the row dirty and bumps `rev`; the sync loop pushes dirty rows
//! to solidtime and clears `dirty` only if `rev` is unchanged since it read the row.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use std::path::Path;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    migrate(&c)?;
    Ok(c)
}

/// Schema migrations, applied in order and tracked in `PRAGMA user_version`.
/// Never edit a released step; append a new one.
const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    "CREATE TABLE IF NOT EXISTS entry(
            local_id    TEXT PRIMARY KEY,
            remote_id   TEXT UNIQUE,
            project_id  TEXT,
            task_id     TEXT,
            description TEXT NOT NULL DEFAULT '',
            start       TEXT NOT NULL,
            end         TEXT,
            billable    INTEGER NOT NULL,
            deleted     INTEGER NOT NULL DEFAULT 0,
            dirty       INTEGER NOT NULL DEFAULT 1,
            rev         INTEGER NOT NULL DEFAULT 1,
            error       TEXT,
            error_rev   INTEGER
         );
         CREATE INDEX IF NOT EXISTS entry_start ON entry(start);
         CREATE TABLE IF NOT EXISTS kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);",
    // 2: remember that a create was sent, so a crash before storing the server id
    //    cannot produce a duplicate on retry
    "ALTER TABLE entry ADD COLUMN create_attempted INTEGER NOT NULL DEFAULT 0;",
];

fn migrate(c: &Connection) -> rusqlite::Result<()> {
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, step) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        c.execute_batch(&format!(
            "BEGIN; {step} PRAGMA user_version = {}; COMMIT;",
            i + 1
        ))?;
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub local_id: String,
    pub remote_id: Option<String>,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub description: String,
    pub start: String,
    pub end: Option<String>,
    pub billable: bool,
    pub deleted: bool,
    pub rev: i64,
    pub create_attempted: bool,
}

const COLS: &str = "local_id, remote_id, project_id, task_id, description, start, end, billable, \
                    deleted, rev, create_attempted";

fn row(r: &Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        local_id: r.get(0)?,
        remote_id: r.get(1)?,
        project_id: r.get(2)?,
        task_id: r.get(3)?,
        description: r.get(4)?,
        start: r.get(5)?,
        end: r.get(6)?,
        billable: r.get(7)?,
        deleted: r.get(8)?,
        rev: r.get(9)?,
        create_attempted: r.get(10)?,
    })
}

pub fn kv_get(c: &Connection, k: &str) -> Option<String> {
    c.query_row("SELECT v FROM kv WHERE k=?", [k], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
}

pub fn kv_set(c: &Connection, k: &str, v: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO kv(k, v) VALUES(?, ?) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        [k, v],
    )?;
    Ok(())
}

pub fn kv_del(c: &Connection, k: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM kv WHERE k=?", [k])?;
    Ok(())
}

pub fn running(c: &Connection) -> rusqlite::Result<Option<Entry>> {
    c.query_row(
        &format!(
            "SELECT {COLS} FROM entry WHERE end IS NULL AND deleted=0 ORDER BY start DESC LIMIT 1"
        ),
        [],
        row,
    )
    .optional()
}

/// Ends the running entry at `now`. A zero-length entry is dropped instead.
fn close_running(c: &Connection, now: &str) -> rusqlite::Result<()> {
    if let Some(e) = running(c)? {
        if e.start.as_str() >= now {
            if e.remote_id.is_none() && !e.create_attempted {
                c.execute("DELETE FROM entry WHERE local_id=?", [&e.local_id])?;
            } else {
                c.execute(
                    "UPDATE entry SET deleted=1, dirty=1, rev=rev+1 WHERE local_id=?",
                    [&e.local_id],
                )?;
            }
        } else {
            c.execute(
                "UPDATE entry SET end=?, dirty=1, rev=rev+1 WHERE local_id=?",
                [now, &e.local_id],
            )?;
        }
    }
    Ok(())
}

pub fn start(
    c: &mut Connection,
    project_id: &str,
    task_id: Option<&str>,
    description: &str,
    billable: bool,
    now: &str,
) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    close_running(&tx, now)?;
    tx.execute(
        "INSERT INTO entry(local_id, project_id, task_id, description, start, billable)
         VALUES(?, ?, ?, ?, ?, ?)",
        params![
            uuid::Uuid::new_v4().to_string(),
            project_id,
            task_id,
            description,
            now,
            billable
        ],
    )?;
    tx.commit()
}

pub fn stop(c: &mut Connection, now: &str) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    close_running(&tx, now)?;
    tx.commit()
}

/// W6: takes the away period `[from, to)` out of the running entry `local_id`.
/// The entry ends at `from` and resumes as a new running entry at `to`. With
/// `assign`, the away period becomes its own entry on that project/task.
/// Returns false if that entry is no longer the running one.
pub fn cut_running(
    c: &mut Connection,
    local_id: &str,
    from: &str,
    to: &str,
    assign: Option<(&str, Option<&str>, bool)>,
) -> rusqlite::Result<bool> {
    let tx = c.transaction()?;
    let Some(r) = running(&tx)?.filter(|r| r.local_id == local_id) else {
        return Ok(false);
    };
    let from = from.max(r.start.as_str()).to_string();
    close_running(&tx, &from)?;
    if let Some((project_id, task_id, billable)) = assign
        && from.as_str() < to
    {
        tx.execute(
            "INSERT INTO entry(local_id, project_id, task_id, start, end, billable)
             VALUES(?, ?, ?, ?, ?, ?)",
            params![
                uuid::Uuid::new_v4().to_string(),
                project_id,
                task_id,
                from,
                to,
                billable
            ],
        )?;
    }
    tx.execute(
        "INSERT INTO entry(local_id, project_id, task_id, description, start, billable)
         VALUES(?, ?, ?, ?, ?, ?)",
        params![
            uuid::Uuid::new_v4().to_string(),
            r.project_id,
            r.task_id,
            r.description,
            to,
            r.billable
        ],
    )?;
    tx.commit()?;
    Ok(true)
}

/// Disk budget: drops synced, finished entries that started before `before`.
pub fn prune(c: &Connection, before: &str) -> rusqlite::Result<()> {
    c.execute(
        "DELETE FROM entry WHERE dirty=0 AND end IS NOT NULL AND start < ?",
        [before],
    )?;
    Ok(())
}

pub fn set_running_description(c: &Connection, description: &str) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE entry SET description=?, dirty=1, rev=rev+1
         WHERE end IS NULL AND deleted=0",
        [description],
    )?;
    Ok(())
}

/// Most recently used distinct project/task pairs.
pub fn recent(c: &Connection, n: usize) -> rusqlite::Result<Vec<(String, Option<String>)>> {
    let mut s = c.prepare(
        "SELECT project_id, task_id FROM entry
         WHERE deleted=0 AND project_id IS NOT NULL
         GROUP BY project_id, task_id ORDER BY MAX(start) DESC LIMIT ?",
    )?;
    let rows = s.query_map([n as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// Dirty rows to push, oldest first. Rows the server rejected are skipped until edited again.
pub fn dirty(c: &Connection) -> rusqlite::Result<Vec<Entry>> {
    let mut s = c.prepare(&format!(
        "SELECT {COLS} FROM entry
         WHERE dirty=1 AND (error_rev IS NULL OR error_rev != rev)
         ORDER BY start"
    ))?;
    let rows = s.query_map([], row)?;
    rows.collect()
}

pub fn pending_count(c: &Connection) -> i64 {
    c.query_row("SELECT COUNT(*) FROM entry WHERE dirty=1", [], |r| r.get(0))
        .unwrap_or(0)
}

/// Called just before a create is sent to the server.
pub fn mark_create_attempted(c: &Connection, local_id: &str) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE entry SET create_attempted=1 WHERE local_id=?",
        [local_id],
    )?;
    Ok(())
}

pub fn mark_synced(
    c: &Connection,
    local_id: &str,
    remote_id: &str,
    rev: i64,
) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE entry SET remote_id=?, error=NULL, error_rev=NULL,
                dirty = CASE WHEN rev=? THEN 0 ELSE 1 END
         WHERE local_id=?",
        params![remote_id, rev, local_id],
    )?;
    Ok(())
}

pub fn mark_rejected(
    c: &Connection,
    local_id: &str,
    rev: i64,
    error: &str,
) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE entry SET error=?, error_rev=? WHERE local_id=?",
        params![error, rev, local_id],
    )?;
    Ok(())
}

pub fn remove(c: &Connection, local_id: &str, rev: i64) -> rusqlite::Result<()> {
    c.execute(
        "DELETE FROM entry WHERE local_id=? AND rev=?",
        params![local_id, rev],
    )?;
    Ok(())
}

/// Server-side entry as returned by solidtime.
#[derive(serde::Deserialize, Debug)]
pub struct RemoteEntry {
    pub id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub description: Option<String>,
    pub start: String,
    pub end: Option<String>,
    pub billable: bool,
}

/// Merges entries pulled from the server. Local unsynced edits always win.
pub fn merge_remote(c: &mut Connection, entries: &[RemoteEntry]) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    let local_running = running(&tx)?.map(|e| e.remote_id);
    for e in entries {
        let existing: Option<i64> = tx
            .query_row("SELECT dirty FROM entry WHERE remote_id=?", [&e.id], |r| {
                r.get(0)
            })
            .optional()?;
        let desc = e.description.clone().unwrap_or_default();
        match existing {
            Some(0) => {
                tx.execute(
                    "UPDATE entry SET project_id=?, task_id=?, description=?, start=?, end=?, billable=?
                     WHERE remote_id=?",
                    params![e.project_id, e.task_id, desc, e.start, e.end, e.billable, e.id],
                )?;
            }
            Some(_) => {}
            None => {
                // Never import a second running timer next to a local one.
                if e.end.is_none()
                    && matches!(local_running, Some(ref r) if r.as_deref() != Some(e.id.as_str()))
                {
                    continue;
                }
                tx.execute(
                    "INSERT INTO entry(local_id, remote_id, project_id, task_id, description, start, end, billable, dirty)
                     VALUES(?, ?, ?, ?, ?, ?, ?, ?, 0)",
                    params![uuid::Uuid::new_v4().to_string(), e.id, e.project_id, e.task_id, desc, e.start, e.end, e.billable],
                )?;
            }
        }
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    fn all(c: &Connection) -> Vec<Entry> {
        let mut s = c
            .prepare(&format!("SELECT {COLS} FROM entry ORDER BY start"))
            .unwrap();
        s.query_map([], row).unwrap().map(|r| r.unwrap()).collect()
    }

    #[test]
    fn switching_closes_previous_entry() {
        let mut c = mem();
        start(&mut c, "p1", None, "a", true, "2026-11-02T09:00:00Z").unwrap();
        start(&mut c, "p2", Some("t2"), "", false, "2026-11-02T10:00:00Z").unwrap();
        let e = all(&c);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].end.as_deref(), Some("2026-11-02T10:00:00Z"));
        assert_eq!(e[1].end, None);
        assert_eq!(
            running(&c).unwrap().unwrap().project_id.as_deref(),
            Some("p2")
        );
    }

    #[test]
    fn zero_length_entry_is_dropped() {
        let mut c = mem();
        start(&mut c, "p1", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        start(&mut c, "p2", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        let e = all(&c);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].project_id.as_deref(), Some("p2"));
    }

    #[test]
    fn discard_away_time_splits_running_entry() {
        let mut c = mem();
        start(
            &mut c,
            "p1",
            Some("t1"),
            "work",
            true,
            "2026-11-02T09:00:00Z",
        )
        .unwrap();
        let id = running(&c).unwrap().unwrap().local_id;
        let ok = cut_running(
            &mut c,
            &id,
            "2026-11-02T10:00:00Z",
            "2026-11-02T10:30:00Z",
            None,
        )
        .unwrap();
        assert!(ok);
        let e = all(&c);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].end.as_deref(), Some("2026-11-02T10:00:00Z"));
        assert_eq!(e[1].start, "2026-11-02T10:30:00Z");
        assert_eq!(e[1].end, None);
        assert_eq!(
            (e[1].project_id.as_deref(), e[1].task_id.as_deref()),
            (Some("p1"), Some("t1"))
        );
        assert_eq!(e[1].description, "work");
    }

    #[test]
    fn assign_away_time_creates_entry_on_other_project() {
        let mut c = mem();
        start(&mut c, "p1", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        let id = running(&c).unwrap().unwrap().local_id;
        cut_running(
            &mut c,
            &id,
            "2026-11-02T10:00:00Z",
            "2026-11-02T10:30:00Z",
            Some(("meet", None, false)),
        )
        .unwrap();
        let e = all(&c);
        assert_eq!(e.len(), 3);
        assert_eq!(e[1].project_id.as_deref(), Some("meet"));
        assert_eq!(
            (e[1].start.as_str(), e[1].end.as_deref()),
            ("2026-11-02T10:00:00Z", Some("2026-11-02T10:30:00Z"))
        );
        assert!(!e[1].billable);
        assert_eq!(e[2].project_id.as_deref(), Some("p1"));
        assert_eq!(e[2].end, None);
    }

    #[test]
    fn cut_ignores_stale_entry() {
        let mut c = mem();
        start(&mut c, "p1", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        let old = running(&c).unwrap().unwrap().local_id;
        start(&mut c, "p2", None, "", true, "2026-11-02T09:30:00Z").unwrap();
        let ok = cut_running(
            &mut c,
            &old,
            "2026-11-02T10:00:00Z",
            "2026-11-02T10:30:00Z",
            None,
        )
        .unwrap();
        assert!(!ok);
        assert_eq!(all(&c).len(), 2);
    }

    #[test]
    fn edit_during_sync_keeps_row_dirty() {
        let mut c = mem();
        start(&mut c, "p1", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        let snapshot = dirty(&c).unwrap().remove(0);
        set_running_description(&c, "changed while syncing").unwrap();
        mark_synced(&c, &snapshot.local_id, "remote-1", snapshot.rev).unwrap();
        let again = dirty(&c).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].remote_id.as_deref(), Some("remote-1"));
    }

    #[test]
    fn migrations_are_idempotent_and_versioned() {
        let c = mem();
        migrate(&c).unwrap();
        let v: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
    }

    #[test]
    fn dropped_entry_is_kept_for_deletion_if_create_was_sent() {
        let mut c = mem();
        start(&mut c, "p1", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        let e = running(&c).unwrap().unwrap();
        mark_create_attempted(&c, &e.local_id).unwrap();
        // switching in the same second makes the first entry zero-length
        start(&mut c, "p2", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        let kept = all(&c)
            .into_iter()
            .find(|x| x.local_id == e.local_id)
            .unwrap();
        assert!(
            kept.deleted,
            "must stay queued so the server copy can be deleted"
        );
    }

    #[test]
    fn rejected_row_is_skipped_until_edited() {
        let mut c = mem();
        start(&mut c, "p1", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        let e = dirty(&c).unwrap().remove(0);
        mark_rejected(&c, &e.local_id, e.rev, "HTTP 422").unwrap();
        assert!(dirty(&c).unwrap().is_empty());
        set_running_description(&c, "fixed").unwrap();
        assert_eq!(dirty(&c).unwrap().len(), 1);
    }
}
