"""SQLite storage: project hints, AI usage log and day submissions.

No screenshots, window titles or tokens are ever stored here.
"""

from __future__ import annotations

import json
import sqlite3
import threading
from datetime import UTC, datetime, timedelta

# Applied in order, tracked in PRAGMA user_version. Never edit a released step.
MIGRATIONS = (
    """
    CREATE TABLE hint(
        org_id      TEXT NOT NULL,
        project_id  TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        keywords    TEXT NOT NULL DEFAULT '[]',
        updated_at  TEXT NOT NULL,
        updated_by  TEXT NOT NULL,
        PRIMARY KEY(org_id, project_id)
    );
    CREATE TABLE usage(
        id                 INTEGER PRIMARY KEY,
        ts                 TEXT NOT NULL,
        org_id             TEXT NOT NULL,
        member_id          TEXT NOT NULL,
        model              TEXT NOT NULL,
        input_tokens       INTEGER NOT NULL,
        output_tokens      INTEGER NOT NULL,
        cache_read_tokens  INTEGER NOT NULL,
        cache_write_tokens INTEGER NOT NULL,
        latency_ms         INTEGER NOT NULL,
        outcome            TEXT NOT NULL,
        has_image          INTEGER NOT NULL
    );
    CREATE INDEX usage_ts ON usage(ts);
    CREATE TABLE day_submission(
        org_id       TEXT NOT NULL,
        member_id    TEXT NOT NULL,
        day          TEXT NOT NULL,
        submitted_at TEXT NOT NULL,
        total_secs   INTEGER NOT NULL,
        entries      INTEGER NOT NULL,
        PRIMARY KEY(org_id, member_id, day)
    );
    """,
)

# USD per million tokens: (input, output, cache read, cache write). Used only for the
# admin cost estimate; unknown models show tokens without a cost.
PRICES = {
    "claude-haiku-5-5": (0.10, 0.50, 0.01, 0.125),
    "claude-sonnet-5-5": (2.00, 10.00, 0.10, 2.50),
    "claude-opus-5-5": (4.00, 20.00, 0.20, 5.00),
    # Gemini API / Vertex AI standard paid tier (Nov 2026); thinking tokens count as output.
    "gemini-3.5-flash-lite": (0.30, 2.50, 0.03, 0.0),
    "gemini-3.1-flash-lite": (0.25, 1.50, 0.025, 0.0),
    "gemini-2.5-flash-lite": (0.10, 0.40, 0.01, 0.0),
    "gemini-3.6-flash": (0.75, 3.75, 0.075, 0.0),
    "gemini-3.8-flash": (0.75, 3.75, 0.075, 0.0),
}


def _now() -> str:
    return datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


class Store:
    def __init__(self, path: str):
        self._db = sqlite3.connect(path, check_same_thread=False, isolation_level=None)
        self._lock = threading.Lock()
        with self._lock:
            self._db.execute("PRAGMA journal_mode=WAL")
            self._db.execute("PRAGMA synchronous=FULL")
            self._migrate()

    def _migrate(self) -> None:
        version = self._db.execute("PRAGMA user_version").fetchone()[0]
        for i, step in enumerate(MIGRATIONS[version:], start=version + 1):
            self._db.executescript(f"BEGIN; {step} PRAGMA user_version = {i}; COMMIT;")

    def close(self) -> None:
        with self._lock:
            self._db.close()

    # ---------- hints ----------

    def hints(self, org_id: str) -> dict[str, dict]:
        with self._lock:
            rows = self._db.execute(
                "SELECT project_id, description, keywords FROM hint WHERE org_id=?", (org_id,)
            ).fetchall()
        return {pid: {"description": desc, "keywords": json.loads(kw)} for pid, desc, kw in rows}

    def set_hint(self, org_id: str, project_id: str, description: str, keywords: list[str], by: str) -> None:
        with self._lock:
            self._db.execute(
                """INSERT INTO hint(org_id, project_id, description, keywords, updated_at, updated_by)
                   VALUES(?, ?, ?, ?, ?, ?)
                   ON CONFLICT(org_id, project_id) DO UPDATE SET
                     description=excluded.description, keywords=excluded.keywords,
                     updated_at=excluded.updated_at, updated_by=excluded.updated_by""",
                (org_id, project_id, description, json.dumps(keywords), _now(), by),
            )

    # ---------- usage ----------

    def log_usage(
        self,
        org_id: str,
        member_id: str,
        model: str,
        usage: dict,
        latency_ms: int,
        outcome: str,
        has_image: bool,
    ) -> None:
        with self._lock:
            self._db.execute(
                """INSERT INTO usage(ts, org_id, member_id, model, input_tokens, output_tokens,
                       cache_read_tokens, cache_write_tokens, latency_ms, outcome, has_image)
                   VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    _now(),
                    org_id,
                    member_id,
                    model,
                    usage.get("input_tokens", 0),
                    usage.get("output_tokens", 0),
                    usage.get("cache_read_tokens", 0),
                    usage.get("cache_write_tokens", 0),
                    latency_ms,
                    outcome,
                    int(has_image),
                ),
            )

    def usage_summary(self, org_id: str, days: int) -> dict:
        since = (datetime.now(UTC) - timedelta(days=days)).strftime("%Y-%m-%dT%H:%M:%SZ")
        with self._lock:
            rows = self._db.execute(
                """SELECT model, COUNT(*), SUM(input_tokens), SUM(output_tokens),
                          SUM(cache_read_tokens), SUM(cache_write_tokens), AVG(latency_ms),
                          COUNT(DISTINCT member_id), SUM(has_image)
                   FROM usage WHERE org_id=? AND ts>=? GROUP BY model ORDER BY model""",
                (org_id, since),
            ).fetchall()
        models = []
        total = 0.0
        for model, calls, inp, out, cr, cw, lat, members, images in rows:
            price = PRICES.get(model)
            cost = (
                None
                if price is None
                else (inp * price[0] + out * price[1] + cr * price[2] + cw * price[3]) / 1e6
            )
            total += cost or 0.0
            models.append(
                {
                    "model": model,
                    "calls": calls,
                    "with_screenshot": images or 0,
                    "input_tokens": inp,
                    "output_tokens": out,
                    "cache_read_tokens": cr,
                    "cache_write_tokens": cw,
                    "avg_latency_ms": round(lat or 0),
                    "members": members,
                    "estimated_cost_usd": None if cost is None else round(cost, 4),
                }
            )
        return {"days": days, "models": models, "estimated_cost_usd": round(total, 4)}

    # ---------- day submissions (W8) ----------

    def submit_day(self, org_id: str, member_id: str, day: str, total_secs: int, entries: int) -> None:
        with self._lock:
            self._db.execute(
                """INSERT INTO day_submission(org_id, member_id, day, submitted_at, total_secs, entries)
                   VALUES(?, ?, ?, ?, ?, ?)
                   ON CONFLICT(org_id, member_id, day) DO UPDATE SET
                     submitted_at=excluded.submitted_at, total_secs=excluded.total_secs,
                     entries=excluded.entries""",
                (org_id, member_id, day, _now(), total_secs, entries),
            )

    def submissions(self, org_id: str, day: str) -> list[dict]:
        with self._lock:
            rows = self._db.execute(
                """SELECT member_id, submitted_at, total_secs, entries FROM day_submission
                   WHERE org_id=? AND day=? ORDER BY member_id""",
                (org_id, day),
            ).fetchall()
        return [{"member_id": m, "submitted_at": s, "total_secs": t, "entries": e} for m, s, t, e in rows]
