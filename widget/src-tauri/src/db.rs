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
    // 3: auto-tracking. `source` is 'manual' or 'auto'; `context` is the window
    //    fingerprint that produced an auto entry (local only, never synced), so a
    //    correction in the day review can teach the cache. `context_cache` maps window
    //    fingerprints to a project (learned from the AI or from the user).
    "ALTER TABLE entry ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
     ALTER TABLE entry ADD COLUMN context TEXT;
     CREATE TABLE IF NOT EXISTS context_cache(
            fp          TEXT PRIMARY KEY,
            kind        TEXT NOT NULL,
            project_id  TEXT,
            task_id     TEXT,
            note        TEXT NOT NULL DEFAULT '',
            source      TEXT NOT NULL,
            updated_at  TEXT NOT NULL
     );",
    // 4: screenshots kept on this computer for the day review (metadata only; the
    //    images are files in the captures folder). `kind`: project / review / neutral
    //    / pending; `source`: how the project was decided (ai, keywords, cache).
    "CREATE TABLE IF NOT EXISTS capture(
            id          TEXT PRIMARY KEY,
            taken_at    TEXT NOT NULL,
            app         TEXT NOT NULL,
            title       TEXT NOT NULL,
            kind        TEXT NOT NULL,
            project_id  TEXT,
            task_id     TEXT,
            note        TEXT NOT NULL DEFAULT '',
            confidence  REAL,
            source      TEXT NOT NULL
     );
     CREATE INDEX IF NOT EXISTS capture_taken ON capture(taken_at);",
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
    pub source: String,
    pub context: Option<String>,
    pub error: Option<String>,
}

const COLS: &str = "local_id, remote_id, project_id, task_id, description, start, end, billable, \
                    deleted, rev, create_attempted, source, context, \
                    CASE WHEN error_rev = rev THEN error END";

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
        source: r.get(11)?,
        context: r.get(12)?,
        error: r.get(13)?,
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

/// A timer to start. `project_id: None` is a "Needs review" block (auto-tracking could
/// not tell the project).
#[derive(Clone, Debug, Default)]
pub struct NewEntry<'a> {
    pub project_id: Option<&'a str>,
    pub task_id: Option<&'a str>,
    pub description: &'a str,
    pub billable: bool,
    pub auto: bool,
    pub context: Option<&'a str>,
}

impl<'a> NewEntry<'a> {
    pub fn manual(
        project_id: &'a str,
        task_id: Option<&'a str>,
        description: &'a str,
        billable: bool,
    ) -> Self {
        NewEntry {
            project_id: Some(project_id),
            task_id,
            description,
            billable,
            ..Default::default()
        }
    }
}

/// Starts a timer at `at`, closing the running one at the same moment (no overlap).
/// Auto-tracking passes a backdated `at`; it is never earlier than the running entry's
/// start, so finished entries are never touched.
pub fn start(c: &mut Connection, e: &NewEntry, at: &str) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    close_running(&tx, at)?;
    tx.execute(
        "INSERT INTO entry(local_id, project_id, task_id, description, start, billable, source, context)
         VALUES(?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            uuid::Uuid::new_v4().to_string(),
            e.project_id,
            e.task_id,
            e.description,
            at,
            e.billable,
            if e.auto { "auto" } else { "manual" },
            e.context,
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
        "INSERT INTO entry(local_id, project_id, task_id, description, start, billable, source, context)
         VALUES(?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            uuid::Uuid::new_v4().to_string(),
            r.project_id,
            r.task_id,
            r.description,
            to,
            r.billable,
            r.source,
            r.context
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

// ---------- day review (W8) ----------

/// Entries overlapping `[from, to)`, oldest first (includes the running one).
pub fn day_entries(c: &Connection, from: &str, to: &str) -> rusqlite::Result<Vec<Entry>> {
    let mut s = c.prepare(&format!(
        "SELECT {COLS} FROM entry
         WHERE deleted=0 AND start < ? AND (end IS NULL OR end > ?)
         ORDER BY start"
    ))?;
    let rows = s.query_map([to, from], row)?;
    rows.collect()
}

pub fn entry(c: &Connection, local_id: &str) -> rusqlite::Result<Option<Entry>> {
    c.query_row(
        &format!("SELECT {COLS} FROM entry WHERE local_id=? AND deleted=0"),
        [local_id],
        row,
    )
    .optional()
}

/// Reassigns an entry (fixing a "Needs review" block or a wrong guess).
pub fn set_entry_project(
    c: &Connection,
    local_id: &str,
    project_id: &str,
    task_id: Option<&str>,
    billable: bool,
) -> rusqlite::Result<bool> {
    let n = c.execute(
        "UPDATE entry SET project_id=?, task_id=?, billable=?, dirty=1, rev=rev+1
         WHERE local_id=? AND deleted=0",
        params![project_id, task_id, billable, local_id],
    )?;
    Ok(n == 1)
}

pub fn set_entry_description(
    c: &Connection,
    local_id: &str,
    description: &str,
) -> rusqlite::Result<bool> {
    let n = c.execute(
        "UPDATE entry SET description=?, dirty=1, rev=rev+1 WHERE local_id=? AND deleted=0",
        params![description, local_id],
    )?;
    Ok(n == 1)
}

/// Removes an entry. One the server may already have stays queued as a deletion.
pub fn delete_entry(c: &Connection, local_id: &str) -> rusqlite::Result<bool> {
    let n = c.execute(
        "DELETE FROM entry WHERE local_id=? AND remote_id IS NULL AND create_attempted=0",
        [local_id],
    )?;
    if n == 1 {
        return Ok(true);
    }
    let n = c.execute(
        "UPDATE entry SET deleted=1, dirty=1, rev=rev+1,
                end = COALESCE(end, start)
         WHERE local_id=? AND deleted=0",
        [local_id],
    )?;
    Ok(n == 1)
}

// ---------- learned window → project cache (auto-tracking) ----------

#[derive(Clone, Debug, PartialEq)]
pub struct Learned {
    /// "project" or "neutral" (chat, lock screen: keep the current project).
    pub kind: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub note: String,
    /// "ai" or "user".
    pub source: String,
}

/// A cached answer for this window, if younger than its source's lifetime: AI answers
/// for `ai_max_age` (the same title can mean different work on another day), the
/// user's own corrections for `user_max_age`.
pub fn learned(
    c: &Connection,
    fp: &str,
    ai_since: &str,
    user_since: &str,
) -> rusqlite::Result<Option<Learned>> {
    c.query_row(
        "SELECT kind, project_id, task_id, note, source FROM context_cache
         WHERE fp=? AND updated_at >= CASE source WHEN 'user' THEN ? ELSE ? END",
        params![fp, user_since, ai_since],
        |r| {
            Ok(Learned {
                kind: r.get(0)?,
                project_id: r.get(1)?,
                task_id: r.get(2)?,
                note: r.get(3)?,
                source: r.get(4)?,
            })
        },
    )
    .optional()
}

/// Stores an answer. An AI answer never overwrites a fresh correction by the user.
pub fn learn(
    c: &Connection,
    fp: &str,
    l: &Learned,
    now: &str,
    user_since: &str,
) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO context_cache(fp, kind, project_id, task_id, note, source, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(fp) DO UPDATE SET
           kind=excluded.kind, project_id=excluded.project_id, task_id=excluded.task_id,
           note=excluded.note, source=excluded.source, updated_at=excluded.updated_at
         WHERE ?6 = 'user' OR context_cache.source != 'user' OR context_cache.updated_at < ?8",
        params![
            fp,
            l.kind,
            l.project_id,
            l.task_id,
            l.note,
            l.source,
            now,
            user_since
        ],
    )?;
    Ok(())
}

/// Disk budget and privacy: window titles are kept only as long as they are useful.
pub fn prune_learned(c: &Connection, before: &str, max_rows: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM context_cache WHERE updated_at < ?", [before])?;
    c.execute(
        "DELETE FROM context_cache WHERE fp NOT IN
           (SELECT fp FROM context_cache ORDER BY updated_at DESC LIMIT ?)",
        [max_rows],
    )?;
    Ok(())
}

/// Sign-out: forget everything learned about this user's windows.
pub fn forget_all(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch("DELETE FROM entry; DELETE FROM context_cache; DELETE FROM capture;")
}

/// Settings that belong to the signed-in account and its server. The Autotrack service
/// address and its policy are part of the server: signing in somewhere else must
/// not keep talking to the old service.
pub const ACCOUNT_KEYS: &[&str] = &[
    "base_url",
    "org_id",
    "member_id",
    "catalog",
    "auto_enabled",
    "consent_version",
    "paused_until",
    "ai_url",
    "ai_policy",
    "ai_hints",
    "ai_hints_etag",
    "nudged",
];

/// Settings learned from an Autotrack service. Kept only while they belong to the service of
/// the server the user is signed in to.
const SERVICE_KEYS: &[&str] = &["ai_url", "ai_policy", "ai_hints", "ai_hints_etag"];

/// Drops the policy and hints of another server's Autotrack service (e.g. a test server used
/// before switching to production with an older version). Returns whether it did.
pub fn forget_other_service(c: &Connection, expected: Option<&str>) -> rusqlite::Result<bool> {
    let stored = kv_get(c, "ai_url");
    if stored.is_none() || stored.as_deref() == expected {
        return Ok(false);
    }
    let tx = c.unchecked_transaction()?;
    for k in SERVICE_KEYS {
        kv_del(&tx, k)?;
    }
    tx.commit()?;
    Ok(true)
}

/// Sign-out, in one transaction: entries, learned windows, screenshot records and the
/// account's settings (incl. which days were submitted).
pub fn forget_account(c: &Connection) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    forget_all(&tx)?;
    for k in ACCOUNT_KEYS {
        kv_del(&tx, k)?;
    }
    tx.execute("DELETE FROM kv WHERE k LIKE 'submitted:%'", [])?;
    tx.commit()
}

// ---------- editing times (W8 review) ----------

fn other_overlaps(
    c: &Connection,
    start: &str,
    end: Option<&str>,
    skip: &[&str],
) -> rusqlite::Result<bool> {
    let mut s = c.prepare(
        "SELECT local_id FROM entry
         WHERE deleted=0 AND start < COALESCE(?1, '9999') AND (end IS NULL OR end > ?2)",
    )?;
    let ids = s.query_map(params![end, start], |r| r.get::<_, String>(0))?;
    for id in ids {
        if !skip.contains(&id?.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn touch(c: &Connection, sql: &str, value: &str, local_id: &str) -> rusqlite::Result<()> {
    c.execute(sql, params![value, local_id])?;
    Ok(())
}

/// Moves an entry's start (and end, unless it is running). An adjacent block (one that
/// ended exactly where this one started, or started exactly where it ended) moves with
/// the boundary, so fixing a switch time leaves no gap or overlap. Anything else that
/// would overlap is refused. Times are UTC in solidtime format.
pub fn set_entry_times(
    c: &mut Connection,
    local_id: &str,
    start: &str,
    end: Option<&str>,
) -> Result<(), String> {
    let db = |e: rusqlite::Error| e.to_string();
    let tx = c.transaction().map_err(db)?;
    let e = entry(&tx, local_id)
        .map_err(db)?
        .ok_or("This entry no longer exists")?;
    let end = match (&e.end, end) {
        (None, _) => None, // running: only the start can move
        (Some(_), Some(x)) => Some(x),
        (Some(_), None) => return Err("End time is required".into()),
    };
    if end.is_some_and(|x| x <= start) {
        return Err("The end must be after the start".into());
    }
    let prev: Option<Entry> = tx
        .query_row(
            &format!(
                "SELECT {COLS} FROM entry WHERE deleted=0 AND end=? AND local_id != ? LIMIT 1"
            ),
            params![e.start, local_id],
            row,
        )
        .optional()
        .map_err(db)?;
    let next: Option<Entry> = match &e.end {
        Some(old_end) => tx
            .query_row(
                &format!(
                    "SELECT {COLS} FROM entry WHERE deleted=0 AND start=? AND local_id != ? LIMIT 1"
                ),
                params![old_end, local_id],
                row,
            )
            .optional()
            .map_err(db)?,
        None => None,
    };
    let mut skip = vec![local_id];
    if let Some(p) = prev.as_ref().filter(|_| start != e.start) {
        if start <= p.start.as_str() {
            return Err("That would remove the previous block entirely; delete it instead".into());
        }
        touch(
            &tx,
            "UPDATE entry SET end=?, dirty=1, rev=rev+1 WHERE local_id=?",
            start,
            &p.local_id,
        )
        .map_err(db)?;
        skip.push(&p.local_id);
    }
    if let (Some(n), Some(new_end)) = (next.as_ref(), end)
        && Some(new_end) != e.end.as_deref()
    {
        if n.end.as_deref().is_some_and(|ne| new_end >= ne) {
            return Err("That would remove the next block entirely; delete it instead".into());
        }
        touch(
            &tx,
            "UPDATE entry SET start=?, dirty=1, rev=rev+1 WHERE local_id=?",
            new_end,
            &n.local_id,
        )
        .map_err(db)?;
        skip.push(&n.local_id);
    }
    if other_overlaps(&tx, start, end, &skip).map_err(db)? {
        return Err("These times overlap another block".into());
    }
    tx.execute(
        "UPDATE entry SET start=?, end=?, dirty=1, rev=rev+1 WHERE local_id=?",
        params![start, end, local_id],
    )
    .map_err(db)?;
    tx.commit().map_err(db)
}

/// Splits an entry at `at` into two blocks with the same project; returns the id of the
/// second part (the running one, if the entry was running).
pub fn split_entry(c: &mut Connection, local_id: &str, at: &str) -> Result<String, String> {
    let db = |e: rusqlite::Error| e.to_string();
    let tx = c.transaction().map_err(db)?;
    let e = entry(&tx, local_id)
        .map_err(db)?
        .ok_or("This entry no longer exists")?;
    if at <= e.start.as_str() || e.end.as_deref().is_some_and(|x| at >= x) {
        return Err("Pick a time inside the block".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "UPDATE entry SET end=?, dirty=1, rev=rev+1 WHERE local_id=?",
        params![at, local_id],
    )
    .map_err(db)?;
    tx.execute(
        "INSERT INTO entry(local_id, project_id, task_id, description, start, end, billable, source, context)
         VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![id, e.project_id, e.task_id, e.description, at, e.end, e.billable, e.source, e.context],
    )
    .map_err(db)?;
    tx.commit().map_err(db)?;
    Ok(id)
}

// ---------- screenshots kept for the review ----------

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRow {
    pub id: String,
    pub taken_at: String,
    pub app: String,
    pub title: String,
    pub kind: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub note: String,
    pub confidence: Option<f64>,
    pub source: String,
}

pub fn add_capture(c: &Connection, r: &CaptureRow) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO capture(id, taken_at, app, title, kind, project_id, task_id, note, confidence, source)
         VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![r.id, r.taken_at, r.app, r.title, r.kind, r.project_id, r.task_id, r.note, r.confidence, r.source],
    )?;
    Ok(())
}

pub fn captures_between(c: &Connection, from: &str, to: &str) -> rusqlite::Result<Vec<CaptureRow>> {
    let mut s = c.prepare(
        "SELECT id, taken_at, app, title, kind, project_id, task_id, note, confidence, source
         FROM capture WHERE taken_at >= ? AND taken_at < ? ORDER BY taken_at",
    )?;
    let rows = s.query_map([from, to], |r| {
        Ok(CaptureRow {
            id: r.get(0)?,
            taken_at: r.get(1)?,
            app: r.get(2)?,
            title: r.get(3)?,
            kind: r.get(4)?,
            project_id: r.get(5)?,
            task_id: r.get(6)?,
            note: r.get(7)?,
            confidence: r.get(8)?,
            source: r.get(9)?,
        })
    })?;
    rows.collect()
}

pub fn delete_capture(c: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(c.execute("DELETE FROM capture WHERE id=?", [id])? == 1)
}

/// Removes captures taken before `before` and returns their ids (to delete the files).
pub fn prune_captures(c: &Connection, before: &str) -> rusqlite::Result<Vec<String>> {
    let ids: Vec<String> = {
        let mut s = c.prepare("SELECT id FROM capture WHERE taken_at < ?")?;
        s.query_map([before], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    c.execute("DELETE FROM capture WHERE taken_at < ?", [before])?;
    Ok(ids)
}

pub fn capture_ids(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut s = c.prepare("SELECT id FROM capture")?;
    s.query_map([], |r| r.get(0))?.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(
        c: &mut Connection,
        p: &str,
        t: Option<&str>,
        d: &str,
        b: bool,
        at: &str,
    ) -> rusqlite::Result<()> {
        super::start(c, &NewEntry::manual(p, t, d, b), at)
    }

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

    fn auto<'a>(p: Option<&'a str>, note: &'a str, fp: &'a str) -> NewEntry<'a> {
        NewEntry {
            project_id: p,
            description: note,
            auto: true,
            context: Some(fp),
            ..Default::default()
        }
    }

    #[test]
    fn migration_from_v2_keeps_rows_as_manual() {
        let c = Connection::open_in_memory().unwrap();
        for step in &MIGRATIONS[..2] {
            c.execute_batch(step).unwrap();
        }
        c.execute_batch(
            "PRAGMA user_version = 2;
             INSERT INTO entry(local_id, project_id, start, billable) VALUES('old', 'p1', '2026-11-02T09:00:00Z', 1);",
        )
        .unwrap();
        migrate(&c).unwrap();
        let e = all(&c).remove(0);
        assert_eq!((e.source.as_str(), e.context), ("manual", None));
    }

    #[test]
    fn auto_switch_is_backdated_and_needs_review_has_no_project() {
        let mut c = mem();
        super::start(
            &mut c,
            &auto(None, "Unclear", "fp-a"),
            "2026-11-02T09:00:00Z",
        )
        .unwrap();
        super::start(
            &mut c,
            &auto(Some("p1"), "Coding", "fp-b"),
            "2026-11-02T09:20:00Z",
        )
        .unwrap();
        let e = all(&c);
        assert_eq!(e[0].project_id, None);
        assert_eq!(e[0].end.as_deref(), Some("2026-11-02T09:20:00Z"));
        assert_eq!(
            (e[1].source.as_str(), e[1].context.as_deref()),
            ("auto", Some("fp-b"))
        );
    }

    #[test]
    fn idle_cut_keeps_auto_source() {
        let mut c = mem();
        super::start(&mut c, &auto(Some("p1"), "", "fp"), "2026-11-02T09:00:00Z").unwrap();
        let id = running(&c).unwrap().unwrap().local_id;
        cut_running(
            &mut c,
            &id,
            "2026-11-02T10:00:00Z",
            "2026-11-02T10:30:00Z",
            None,
        )
        .unwrap();
        let r = running(&c).unwrap().unwrap();
        assert_eq!(
            (r.source.as_str(), r.context.as_deref()),
            ("auto", Some("fp"))
        );
    }

    #[test]
    fn day_entries_returns_overlapping_entries_only() {
        let mut c = mem();
        start(&mut c, "p1", None, "", true, "2026-11-01T23:00:00Z").unwrap();
        start(&mut c, "p2", None, "", true, "2026-11-02T01:00:00Z").unwrap();
        stop(&mut c, "2026-11-02T02:00:00Z").unwrap();
        start(&mut c, "p3", None, "", true, "2026-11-03T09:00:00Z").unwrap();
        let day = day_entries(&c, "2026-11-02T00:00:00Z", "2026-11-03T00:00:00Z").unwrap();
        let ids: Vec<_> = day
            .iter()
            .map(|e| e.project_id.as_deref().unwrap())
            .collect();
        assert_eq!(
            ids,
            ["p1", "p2"],
            "an entry crossing midnight belongs to both days"
        );
    }

    #[test]
    fn review_edits_mark_entry_dirty() {
        let mut c = mem();
        super::start(&mut c, &auto(None, "Unclear", "fp"), "2026-11-02T09:00:00Z").unwrap();
        let e = running(&c).unwrap().unwrap();
        mark_synced(&c, &e.local_id, "r1", e.rev).unwrap();
        assert!(set_entry_project(&c, &e.local_id, "p9", Some("t9"), true).unwrap());
        assert!(set_entry_description(&c, &e.local_id, "Fixed").unwrap());
        let d = dirty(&c).unwrap().remove(0);
        assert_eq!(
            (
                d.project_id.as_deref(),
                d.task_id.as_deref(),
                d.billable,
                d.description.as_str()
            ),
            (Some("p9"), Some("t9"), true, "Fixed")
        );
        assert!(!set_entry_project(&c, "missing", "p", None, false).unwrap());
    }

    #[test]
    fn delete_removes_local_rows_and_queues_synced_ones() {
        let mut c = mem();
        start(&mut c, "p1", None, "", true, "2026-11-02T09:00:00Z").unwrap();
        start(&mut c, "p2", None, "", true, "2026-11-02T10:00:00Z").unwrap();
        let e = all(&c);
        mark_synced(&c, &e[1].local_id, "r2", e[1].rev).unwrap();
        assert!(delete_entry(&c, &e[0].local_id).unwrap());
        assert!(delete_entry(&c, &e[1].local_id).unwrap());
        let left = all(&c);
        assert_eq!(left.len(), 1, "never-sent row is gone");
        assert!(left[0].deleted && left[0].end.is_some());
        assert!(
            running(&c).unwrap().is_none(),
            "deleting the running entry stops it"
        );
        assert_eq!(dirty(&c).unwrap().len(), 1, "server copy will be deleted");
    }

    fn learned_row(kind: &str, p: Option<&str>, source: &str) -> Learned {
        Learned {
            kind: kind.into(),
            project_id: p.map(String::from),
            task_id: None,
            note: "n".into(),
            source: source.into(),
        }
    }

    #[test]
    fn learned_answers_expire_by_source() {
        let c = mem();
        learn(
            &c,
            "fp",
            &learned_row("project", Some("p1"), "ai"),
            "2026-11-02T09:00:00Z",
            "2026-10-01T00:00:00Z",
        )
        .unwrap();
        let fresh = learned(&c, "fp", "2026-11-02T08:00:00Z", "2026-10-01T00:00:00Z").unwrap();
        assert_eq!(fresh.unwrap().project_id.as_deref(), Some("p1"));
        let stale = learned(&c, "fp", "2026-11-02T10:00:00Z", "2026-10-01T00:00:00Z").unwrap();
        assert_eq!(stale, None, "AI answers are only reused for a few hours");
    }

    #[test]
    fn ai_never_overwrites_a_fresh_user_correction() {
        let c = mem();
        let user_since = "2026-10-01T00:00:00Z";
        learn(
            &c,
            "fp",
            &learned_row("project", Some("mine"), "user"),
            "2026-11-02T09:00:00Z",
            user_since,
        )
        .unwrap();
        learn(
            &c,
            "fp",
            &learned_row("project", Some("ai"), "ai"),
            "2026-11-02T10:00:00Z",
            user_since,
        )
        .unwrap();
        let hit = learned(&c, "fp", "2026-11-02T00:00:00Z", user_since)
            .unwrap()
            .unwrap();
        assert_eq!(
            (hit.project_id.as_deref(), hit.source.as_str()),
            (Some("mine"), "user")
        );
        // the user may change their mind
        learn(
            &c,
            "fp",
            &learned_row("project", Some("other"), "user"),
            "2026-11-02T11:00:00Z",
            user_since,
        )
        .unwrap();
        let hit = learned(&c, "fp", "2026-11-02T00:00:00Z", user_since)
            .unwrap()
            .unwrap();
        assert_eq!(hit.project_id.as_deref(), Some("other"));
    }

    fn closed(c: &mut Connection, p: &str, from: &str, to: &str) -> String {
        start(c, p, None, "", true, from).unwrap();
        let id = running(c).unwrap().unwrap().local_id;
        stop(c, to).unwrap();
        id
    }

    fn times(c: &Connection, id: &str) -> (String, Option<String>) {
        let e = entry(c, id).unwrap().unwrap();
        (e.start, e.end)
    }

    #[test]
    fn moving_a_boundary_moves_the_adjacent_block() {
        let mut c = mem();
        let a = closed(&mut c, "A", "2026-11-19T09:00:00Z", "2026-11-19T10:00:00Z");
        start(&mut c, "B", None, "", true, "2026-11-19T10:00:00Z").unwrap();
        stop(&mut c, "2026-11-19T11:00:00Z").unwrap();
        let b = all(&c)[1].local_id.clone();
        // the switch really happened at 10:20
        set_entry_times(
            &mut c,
            &a,
            "2026-11-19T09:00:00Z",
            Some("2026-11-19T10:20:00Z"),
        )
        .unwrap();
        assert_eq!(times(&c, &a).1.as_deref(), Some("2026-11-19T10:20:00Z"));
        assert_eq!(times(&c, &b).0, "2026-11-19T10:20:00Z");
        // and earlier than first thought
        set_entry_times(
            &mut c,
            &b,
            "2026-11-19T09:40:00Z",
            Some("2026-11-19T11:00:00Z"),
        )
        .unwrap();
        assert_eq!(times(&c, &a).1.as_deref(), Some("2026-11-19T09:40:00Z"));
        assert_eq!(dirty(&c).unwrap().len(), 2);
    }

    #[test]
    fn time_edits_never_create_overlaps_or_empty_blocks() {
        let mut c = mem();
        let a = closed(&mut c, "A", "2026-11-19T09:00:00Z", "2026-11-19T10:00:00Z");
        let b = closed(&mut c, "B", "2026-11-19T10:30:00Z", "2026-11-19T11:00:00Z");
        // not adjacent: overlapping B is refused
        assert!(
            set_entry_times(
                &mut c,
                &a,
                "2026-11-19T09:00:00Z",
                Some("2026-11-19T10:45:00Z")
            )
            .is_err()
        );
        assert!(
            set_entry_times(
                &mut c,
                &a,
                "2026-11-19T09:30:00Z",
                Some("2026-11-19T09:10:00Z")
            )
            .is_err()
        );
        // adjacent case: swallowing the neighbour entirely is refused
        let x = closed(&mut c, "X", "2026-11-19T12:00:00Z", "2026-11-19T12:30:00Z");
        let y = closed(&mut c, "Y", "2026-11-19T12:30:00Z", "2026-11-19T13:00:00Z");
        assert!(
            set_entry_times(
                &mut c,
                &x,
                "2026-11-19T12:00:00Z",
                Some("2026-11-19T13:00:00Z")
            )
            .is_err()
        );
        assert_eq!(
            times(&c, &y).0,
            "2026-11-19T12:30:00Z",
            "refused edit changes nothing"
        );
        assert_eq!(times(&c, &b).0, "2026-11-19T10:30:00Z");
    }

    #[test]
    fn running_entry_start_can_move_but_end_stays_open() {
        let mut c = mem();
        let a = closed(&mut c, "A", "2026-11-19T09:00:00Z", "2026-11-19T10:00:00Z");
        start(&mut c, "B", None, "", true, "2026-11-19T10:00:00Z").unwrap();
        let b = running(&c).unwrap().unwrap().local_id;
        set_entry_times(
            &mut c,
            &b,
            "2026-11-19T09:50:00Z",
            Some("2026-11-19T12:00:00Z"),
        )
        .unwrap();
        assert_eq!(times(&c, &b), ("2026-11-19T09:50:00Z".to_string(), None));
        assert_eq!(times(&c, &a).1.as_deref(), Some("2026-11-19T09:50:00Z"));
    }

    #[test]
    fn split_makes_two_blocks_with_the_same_project() {
        let mut c = mem();
        let a = closed(&mut c, "A", "2026-11-19T09:00:00Z", "2026-11-19T11:00:00Z");
        let second = split_entry(&mut c, &a, "2026-11-19T10:15:00Z").unwrap();
        assert_eq!(times(&c, &a).1.as_deref(), Some("2026-11-19T10:15:00Z"));
        assert_eq!(
            times(&c, &second),
            (
                "2026-11-19T10:15:00Z".into(),
                Some("2026-11-19T11:00:00Z".into())
            )
        );
        assert_eq!(
            entry(&c, &second).unwrap().unwrap().project_id.as_deref(),
            Some("A")
        );
        assert!(
            split_entry(&mut c, &a, "2026-11-19T09:00:00Z").is_err(),
            "edge is not inside"
        );
        // splitting the running entry keeps the second part running
        start(&mut c, "R", None, "", true, "2026-11-19T12:00:00Z").unwrap();
        let r = running(&c).unwrap().unwrap().local_id;
        let tail = split_entry(&mut c, &r, "2026-11-19T12:30:00Z").unwrap();
        assert_eq!(running(&c).unwrap().unwrap().local_id, tail);
    }

    fn cap(id: &str, at: &str) -> CaptureRow {
        CaptureRow {
            id: id.into(),
            taken_at: at.into(),
            app: "Code".into(),
            title: "index.ts — mmcg-platform".into(),
            kind: "project".into(),
            project_id: Some("A".into()),
            task_id: None,
            note: String::new(),
            confidence: None,
            source: "keywords".into(),
        }
    }

    #[test]
    fn captures_are_listed_by_time_pruned_and_forgotten() {
        let c = mem();
        add_capture(&c, &cap("1", "2026-11-17T09:00:00Z")).unwrap();
        add_capture(&c, &cap("2", "2026-11-19T09:00:00Z")).unwrap();
        add_capture(&c, &cap("3", "2026-11-19T09:05:00Z")).unwrap();
        let day = captures_between(&c, "2026-11-19T00:00:00Z", "2026-11-20T00:00:00Z").unwrap();
        assert_eq!(
            day.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["2", "3"]
        );
        assert_eq!(prune_captures(&c, "2026-11-18T00:00:00Z").unwrap(), ["1"]);
        assert!(delete_capture(&c, "2").unwrap());
        assert_eq!(capture_ids(&c).unwrap(), ["3"]);
        forget_all(&c).unwrap();
        assert!(capture_ids(&c).unwrap().is_empty());
    }

    #[test]
    fn learned_cache_is_pruned_and_cleared_on_sign_out() {
        let c = mem();
        let u = "2026-10-01T00:00:00Z";
        for (i, t) in [
            "2026-11-01T09:00:00Z",
            "2026-11-02T09:00:00Z",
            "2026-11-02T10:00:00Z",
        ]
        .iter()
        .enumerate()
        {
            learn(
                &c,
                &format!("fp{i}"),
                &learned_row("neutral", None, "ai"),
                t,
                u,
            )
            .unwrap();
        }
        prune_learned(&c, "2026-11-02T00:00:00Z", 1).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM context_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        forget_all(&c).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM context_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn another_servers_service_settings_are_dropped() {
        let c = mem();
        let prod = Some("https://tracker.aasnatech.com/autotrack");
        // Nothing stored: nothing to do.
        assert!(!forget_other_service(&c, prod).unwrap());
        // Same service: kept.
        kv_set(&c, "ai_url", "https://tracker.aasnatech.com/autotrack").unwrap();
        kv_set(&c, "ai_policy", "{}").unwrap();
        assert!(!forget_other_service(&c, prod).unwrap());
        assert!(kv_get(&c, "ai_policy").is_some());
        // A test server's service left over from an older version: dropped.
        for k in SERVICE_KEYS {
            kv_set(&c, k, "x").unwrap();
        }
        kv_set(&c, "ai_url", "http://localhost:8010").unwrap();
        kv_set(&c, "base_url", "https://tracker.aasnatech.com").unwrap();
        assert!(forget_other_service(&c, prod).unwrap());
        for k in SERVICE_KEYS {
            assert_eq!(kv_get(&c, k), None, "{k}");
        }
        assert!(kv_get(&c, "base_url").is_some(), "the sign-in stays");
        // Signed out (no server): any stored service is stale.
        kv_set(&c, "ai_url", "https://tracker.aasnatech.com/autotrack").unwrap();
        assert!(forget_other_service(&c, None).unwrap());
    }

    #[test]
    fn sign_out_forgets_the_server_and_its_autotrack() {
        let mut c = mem();
        for k in ACCOUNT_KEYS {
            kv_set(&c, k, "x").unwrap();
        }
        // Explicitly: a test server's service must not survive a switch to production.
        kv_set(&c, "ai_url", "http://localhost:8010").unwrap();
        kv_set(&c, "ai_policy", "{}").unwrap();
        kv_set(&c, "submitted:2026-11-24", "1").unwrap();
        kv_set(&c, "unrelated", "keep").unwrap();
        super::start(
            &mut c,
            &NewEntry::manual("p1", None, "", false),
            "2026-11-24T09:00:00Z",
        )
        .unwrap();
        assert_eq!(pending_count(&c), 1);

        forget_account(&c).unwrap();

        for k in ACCOUNT_KEYS {
            assert_eq!(kv_get(&c, k), None, "{k} survived sign-out");
        }
        assert_eq!(kv_get(&c, "ai_url"), None);
        assert_eq!(kv_get(&c, "ai_policy"), None);
        assert_eq!(kv_get(&c, "submitted:2026-11-24"), None);
        assert_eq!(kv_get(&c, "unrelated").as_deref(), Some("keep"));
        assert_eq!(pending_count(&c), 0);
    }
}

/// Upgrade check on a copy of a real database:
/// `TIMEPILOT_DB_COPY=/path/to/copy/tracker.db cargo test upgrade_copy -- --ignored --nocapture`
#[cfg(test)]
mod upgrade_check {
    #[test]
    #[ignore]
    fn upgrade_copy() {
        let path = std::env::var("TIMEPILOT_DB_COPY").expect("set TIMEPILOT_DB_COPY");
        let c = super::open(std::path::Path::new(&path)).expect("open + migrate");
        let v: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM entry", [], |r| r.get(0))
            .unwrap();
        let manual: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM entry WHERE source='manual'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let ok: String = c
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        println!("after upgrade: version {v}, {n} entries ({manual} manual), integrity {ok}");
        assert_eq!(v as usize, super::MIGRATIONS.len());
        assert_eq!(n, manual);
        assert_eq!(ok, "ok");
    }
}
