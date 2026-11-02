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
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;
         CREATE TABLE IF NOT EXISTS entry(
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
    )?;
    Ok(c)
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
}

const COLS: &str =
    "local_id, remote_id, project_id, task_id, description, start, end, billable, deleted, rev";

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
            if e.remote_id.is_none() {
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
