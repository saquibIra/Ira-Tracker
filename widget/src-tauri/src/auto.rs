//! AI auto-tracking: the timer follows what the engineer is working on.
//!
//! Cheap signals first, screenshots last. Every 20 s while active we read the front
//! app and window title (local). The project comes from, in order:
//! 1. the learned cache (earlier AI answers for this window, the user's corrections),
//! 2. the admin's project keywords (repo names, ticket prefixes, customer names),
//! 3. only if both fail, and at most once per capture interval: a small grayscale
//!    screenshot sent to the Ira service, which asks Claude. The image is never stored.
//!
//! `Engine` turns those observations into timer switches with hysteresis (a new
//! project must hold for `min_switch_secs`) and backdates each switch to when the work
//! actually started. Unsure periods become "Needs review" entries (no project) that
//! the engineer fixes in the end-of-day review.

use crate::ai::{self, Classification, Hint, Policy};
use crate::context::{self, Active, Permission};
use crate::{AppState, capture, changed, db, fmt_time, idle, input, shots};
use chrono::{DateTime, Duration as Span, Local, NaiveTime, Utc};
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Bump when the consent text changes; users then confirm again.
// 2: screenshots kept on this computer for the review. 3: names the AI provider the
// server uses (Gemini or Claude) and shows the server's real policy.
pub const CONSENT_VERSION: &str = "4";
/// Context check interval while auto-tracking (the doc's ≤ 30 s polling budget).
const TICK_SECS: u64 = 20;
/// No input for this long: nothing changes on screen, skip the check.
const IDLE_SKIP_SECS: i64 = 60;
/// AI answers are reused for the same window for this long.
const AI_CACHE_HOURS: i64 = 4;
/// The user's own corrections are remembered for this long.
const USER_CACHE_DAYS: i64 = 30;
/// Policy and keyword hints are refreshed this often.
const REFRESH_SECS: i64 = 3600;
const LEARNED_MAX_ROWS: i64 = 2000;
const SEP: char = '\u{1f}';

// ---------- pure logic (unit-tested) ----------

/// Stable key for "this window": app + title, lower-cased, without unread counters
/// such as "(3) Slack".
pub fn fingerprint(app: &str, title: &str) -> String {
    let mut t = title.trim();
    if let Some(rest) = t.strip_prefix('(')
        && let Some((n, after)) = rest.split_once(')')
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit() || c == '+')
    {
        t = after.trim_start();
    }
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.chars().take(300).collect();
    format!("{}{SEP}{}", app.trim().to_lowercase(), t.to_lowercase())
}

/// Whether a window title identifies the work (a file in a folder, a chat channel, a
/// ticket) and is worth remembering. Generic titles ("Zoom Meeting", "New Tab",
/// "Downloads") show different work each time, so they are classified afresh.
pub fn specific(fp: &str) -> bool {
    let title = fp.split_once(SEP).map_or("", |(_, t)| t);
    let words = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .count();
    let separated = [" — ", " – ", " - ", " | ", " · "]
        .iter()
        .any(|s| title.contains(s));
    title.chars().count() >= 8 && (separated || words >= 4)
}

pub fn is_excluded(app: &str, excluded: &[String]) -> bool {
    let a = app.to_lowercase();
    excluded
        .iter()
        .any(|x| !x.trim().is_empty() && a.contains(&x.trim().to_lowercase()))
}

/// The one project whose keyword appears in `text` (at a word start), if exactly one does.
pub fn match_hints<'h>(text: &str, hints: &'h [Hint]) -> Option<&'h str> {
    let hay = text.to_lowercase();
    let mut found: Option<&str> = None;
    for h in hints {
        let hit = h.keywords.iter().any(|k| {
            let k = k.trim().to_lowercase();
            k.chars().count() >= 3
                && hay.match_indices(&k).any(|(i, _)| {
                    hay[..i]
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_alphanumeric())
                })
        });
        if hit {
            match found {
                Some(p) if p != h.project_id => return None, // ambiguous
                _ => found = Some(&h.project_id),
            }
        }
    }
    found
}

pub fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M").ok()
}

/// Start of the next local day (for "Pause until midnight"). Tracking has no work hours:
/// people work whenever they like.
pub fn next_midnight(now: DateTime<Local>) -> DateTime<Utc> {
    crate::review::day_range(now.date_naive(), 0).2
}

/// What the current window says about the work.
#[derive(Clone, Debug, PartialEq)]
pub enum Obs {
    Project {
        project_id: String,
        task_id: Option<String>,
        note: String,
    },
    /// Not clear which project: becomes a "Needs review" block.
    Review { note: String },
    /// Chat, email, lock screen: keep the current project.
    Neutral,
    /// Excluded app (password manager, personal messenger): never captured.
    Private,
    /// Not classified yet.
    Pending,
}

pub fn from_classification(c: &Classification, review_below: f64) -> Obs {
    if let Some(p) = &c.project_id
        && c.confidence >= review_below
    {
        return Obs::Project {
            project_id: p.clone(),
            task_id: c.task_id.clone(),
            note: c.activity.clone(),
        };
    }
    match c.category.as_str() {
        "communication" | "break" => Obs::Neutral,
        // Never put details of personal activity into a synced description.
        "non_work" => Obs::Review {
            note: "Not work-related?".into(),
        },
        _ if c.activity.is_empty() => Obs::Review {
            note: "Project unclear".into(),
        },
        _ => Obs::Review {
            note: c.activity.clone(),
        },
    }
}

/// Only confident project answers and neutral windows are worth remembering.
pub fn to_learned(o: &Obs, source: &str) -> Option<db::Learned> {
    match o {
        Obs::Project {
            project_id,
            task_id,
            note,
        } => Some(db::Learned {
            kind: "project".into(),
            project_id: Some(project_id.clone()),
            task_id: task_id.clone(),
            note: note.clone(),
            source: source.into(),
        }),
        Obs::Neutral => Some(db::Learned {
            kind: "neutral".into(),
            project_id: None,
            task_id: None,
            note: String::new(),
            source: source.into(),
        }),
        _ => None,
    }
}

pub fn from_learned(l: &db::Learned) -> Obs {
    match (l.kind.as_str(), &l.project_id) {
        ("project", Some(p)) => Obs::Project {
            project_id: p.clone(),
            task_id: l.task_id.clone(),
            note: l.note.clone(),
        },
        _ => Obs::Neutral,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub note: String,
}

impl Target {
    fn review(note: &str) -> Self {
        Target {
            project_id: None,
            task_id: None,
            note: note.into(),
        }
    }
}

/// The running timer, as the engine sees it.
pub struct Running {
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub start: DateTime<Utc>,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    /// Nothing was running: start now.
    Start { target: Target, at: DateTime<Utc> },
    /// Switch the running timer, backdated to `at` (never before its start).
    Switch { target: Target, at: DateTime<Utc> },
}

pub struct Params {
    pub min_switch: i64,
    pub unresolved_review: i64,
}

#[derive(Default)]
pub struct Engine {
    candidate: Option<(Target, DateTime<Utc>)>,
}

impl Engine {
    pub fn reset(&mut self) {
        self.candidate = None;
    }

    /// One observation of the current window (seen since `ctx_since`).
    pub fn observe(
        &mut self,
        now: DateTime<Utc>,
        obs: &Obs,
        ctx_since: DateTime<Utc>,
        running: Option<&Running>,
        p: &Params,
    ) -> Option<Action> {
        let age = (now - ctx_since).num_seconds();
        let target = match obs {
            Obs::Project {
                project_id,
                task_id,
                note,
            } => Some(Target {
                project_id: Some(project_id.clone()),
                task_id: task_id.clone(),
                note: note.clone(),
            }),
            Obs::Review { note } => Some(Target::review(note)),
            Obs::Pending if age >= p.unresolved_review => {
                Some(Target::review("Activity not recognized"))
            }
            Obs::Private if age >= p.unresolved_review => Some(Target::review("Private app")),
            _ => None,
        };

        let Some(cur) = running else {
            // Working but no timer: start one right away. If the project is not known
            // yet, start a review block that the first answer replaces (backdated).
            self.candidate = None;
            let t = target.unwrap_or_else(|| match obs {
                Obs::Private => Target::review("Private app"),
                Obs::Neutral => Target::review("Chat, email or break"),
                _ => Target::review("Activity not recognized yet"),
            });
            return Some(Action::Start { target: t, at: now });
        };
        let t = target?; // neutral / not yet known: keep the current timer

        let same =
            cur.project_id == t.project_id && (t.task_id.is_none() || t.task_id == cur.task_id);
        if same {
            self.candidate = None;
            return None;
        }
        let since = match &self.candidate {
            Some((c, s)) if c.project_id == t.project_id && c.task_id == t.task_id => {
                (*s).min(ctx_since)
            }
            _ => ctx_since,
        };
        self.candidate = Some((t.clone(), since));

        let unknown_now = cur.project_id.is_none();
        let resolves_unknown = unknown_now && t.project_id.is_some();
        if !resolves_unknown && (now - since).num_seconds() < p.min_switch {
            return None;
        }
        self.candidate = None;
        let mut at = since.max(cur.start);
        if unknown_now && (at - cur.start).num_seconds() < p.min_switch {
            at = cur.start; // a short unknown stretch belongs to the work that follows
        }
        Some(Action::Switch { target: t, at })
    }
}

// ---------- runtime ----------

/// Auto-tracking settings and live state, kept in `AppState`.
#[derive(Default)]
pub struct AutoState {
    pub enabled: bool,
    pub service: Option<String>,
    pub paused_until: Option<DateTime<Utc>>,
    pub pinned_until: Option<DateTime<Utc>>,
    pub last_source: Option<&'static str>,
    pub error: Option<String>,
    /// Why the last screenshot failed although permission was granted (shown in the review).
    pub capture_error: Option<String>,
}

impl AutoState {
    pub fn load(db: &rusqlite::Connection) -> Self {
        AutoState {
            enabled: db::kv_get(db, "auto_enabled").as_deref() == Some("1")
                && db::kv_get(db, "consent_version").as_deref() == Some(CONSENT_VERSION),
            // Always the signed-in server's own service (see ai::service_url).
            service: db::kv_get(db, "base_url").map(|b| ai::service_url(&b)),
            paused_until: db::kv_get(db, "paused_until")
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|t| t.with_timezone(&Utc))
                .filter(|t| *t > Utc::now()),
            ..Default::default()
        }
    }
}

fn tick() -> Duration {
    let secs = std::env::var("IRA_AUTO_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(TICK_SECS);
    Duration::from_secs(secs.max(2))
}

/// Testing override for the capture interval (`IRA_CAPTURE_SECS=30`).
fn capture_interval(p: &Policy) -> i64 {
    std::env::var("IRA_CAPTURE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(p.capture_interval_secs)
}

/// User-facing one-liner for the tray and settings window.
pub fn status(st: &AppState) -> String {
    let a = st.auto.lock().unwrap();
    let now = Utc::now();
    if !a.enabled {
        return "Off".into();
    }
    if let Some(p) = a.paused_until.filter(|p| *p > now) {
        return format!(
            "Paused until {}",
            p.with_timezone(&Local).format("%a %H:%M")
        );
    }
    if let Some(e) = &a.error {
        return format!("Problem: {e}");
    }
    let policy = st.policy.lock().unwrap();
    if !policy.enabled {
        return "Waiting for the server (AI not configured)".into();
    }
    if a.pinned_until.is_some_and(|p| p > now) {
        return "On (following your manual pick for now)".into();
    }
    if policy.keep_screenshots_days > 0 && context::screen_permission() == Permission::Denied {
        return "On, but no screenshots: allow Screen Recording for Ira Tracker".into();
    }
    "On".into()
}

pub async fn refresh(app: &AppHandle) {
    let st = app.state::<AppState>();
    let Some(cfg) = st.cfg.lock().unwrap().clone() else {
        return;
    };
    let Some(base) = st.auto.lock().unwrap().service.clone() else {
        return;
    };
    let svc = ai::Service::new(&st.http, &base, &cfg.token, &cfg.org);
    match svc.policy().await {
        Ok(p) => {
            if let Ok(s) = serde_json::to_string(&p) {
                let _ = db::kv_set(&st.db.lock().unwrap(), "ai_policy", &s);
            }
            *st.policy.lock().unwrap() = p;
            st.auto.lock().unwrap().error = None;
        }
        Err(e) => {
            log::warn!("policy refresh failed: {e}");
            st.auto.lock().unwrap().error = Some(e.to_string())
        }
    }
    let etag = db::kv_get(&st.db.lock().unwrap(), "ai_hints_etag");
    if let Ok(Some((hints, etag))) = svc.hints(etag.as_deref()).await {
        {
            let db = st.db.lock().unwrap();
            if let Ok(s) = serde_json::to_string(&hints) {
                let _ = db::kv_set(&db, "ai_hints", &s);
            }
            match etag {
                Some(e) => db::kv_set(&db, "ai_hints_etag", &e).ok(),
                None => db::kv_del(&db, "ai_hints_etag").ok(),
            };
        }
        *st.hints.lock().unwrap() = hints;
    }
    let before = fmt_time(Utc::now() - Span::days(USER_CACHE_DAYS));
    let _ = db::prune_learned(&st.db.lock().unwrap(), &before, LEARNED_MAX_ROWS);
    let keep = st.policy.lock().unwrap().keep_screenshots_days;
    shots::prune(&st, keep);
}

fn resolve(
    st: &AppState,
    fp: &str,
    active: &Active,
    excluded: bool,
    now: DateTime<Utc>,
) -> (Obs, &'static str) {
    if excluded {
        return (Obs::Private, "private");
    }
    let ai_since = fmt_time(now - Span::hours(AI_CACHE_HOURS));
    let user_since = fmt_time(now - Span::days(USER_CACHE_DAYS));
    if let Ok(Some(l)) = db::learned(&st.db.lock().unwrap(), fp, &ai_since, &user_since) {
        return (from_learned(&l), "cache");
    }
    let text = format!("{} {}", active.app, active.title);
    if let Some(p) = match_hints(&text, &st.hints.lock().unwrap()) {
        return (
            Obs::Project {
                project_id: p.to_string(),
                task_id: None,
                note: String::new(),
            },
            "keywords",
        );
    }
    (Obs::Pending, "pending")
}

/// One screenshot of the display the front window is on (None without permission).
async fn take_shot(st: &AppState, policy: &Policy, active: &Active) -> Option<capture::Shot> {
    if context::screen_permission() != Permission::Granted {
        return None;
    }
    let (px, q, near) = (policy.capture_max_px, policy.capture_quality, active.center);
    let result = tokio::task::spawn_blocking(move || capture::capture(px, q, near))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    let mut a = st.auto.lock().unwrap();
    match result {
        Ok(shot) => {
            a.capture_error = None;
            Some(shot)
        }
        Err(e) => {
            log::warn!("screenshot unavailable: {e}");
            a.capture_error = Some(e);
            None
        }
    }
}

/// Asks the Ira service; returns the observation and the AI's confidence.
async fn classify(
    app: &AppHandle,
    base: &str,
    active: &Active,
    fp: &str,
    policy: &Policy,
    image: Option<Vec<u8>>,
) -> Option<(Obs, f64)> {
    let st = app.state::<AppState>();
    let cfg = st.cfg.lock().unwrap().clone()?;
    let title: String = active.title.chars().take(1000).collect();
    let has_image = image.is_some();
    let svc = ai::Service::new(&st.http, base, &cfg.token, &cfg.org);
    // `image` is moved into the request and dropped as soon as it has been sent.
    match svc.classify(&active.app, &title, image).await {
        Ok(c) => {
            log::info!(
                "classified: project={} category={} confidence={:.2} image={}",
                c.project_id.as_deref().unwrap_or("-"),
                c.category,
                c.confidence,
                has_image
            );
            let obs = from_classification(&c, policy.review_below);
            if let Some(l) = to_learned(&obs, "ai").filter(|_| specific(fp)) {
                let now = Utc::now();
                let user_since = fmt_time(now - Span::days(USER_CACHE_DAYS));
                let _ = db::learn(&st.db.lock().unwrap(), fp, &l, &fmt_time(now), &user_since);
            }
            st.auto.lock().unwrap().error = None;
            Some((obs, c.confidence))
        }
        Err(e) => {
            log::warn!("classify failed: {e}");
            st.auto.lock().unwrap().error = Some(match e {
                // The service explains AI outages (e.g. "AI unavailable: credit balance too low").
                crate::api::ApiError::Offline(m) if m.starts_with("AI ") => m,
                crate::api::ApiError::Offline(_) => "Ira service unreachable".into(),
                other => other.to_string(),
            });
            None
        }
    }
}

fn apply(app: &AppHandle, action: Action, fp: &str) {
    let st = app.state::<AppState>();
    let (target, at) = match action {
        Action::Start { target, at } | Action::Switch { target, at } => (target, at),
    };
    // Only projects and tasks the engineer can actually book on.
    let (project, task, billable) = {
        let cat = st.catalog.lock().unwrap();
        match target
            .project_id
            .as_deref()
            .and_then(|p| cat.projects.iter().find(|x| x.id == p && !x.is_archived))
        {
            Some(p) => {
                let task = target
                    .task_id
                    .filter(|t| cat.tasks.iter().any(|x| &x.id == t && x.project_id == p.id));
                (Some(p.id.clone()), task, p.is_billable)
            }
            None => (None, None, false),
        }
    };
    let note = input::description(if project.is_none() && target.note.is_empty() {
        "Project unclear"
    } else {
        &target.note
    });
    let entry = db::NewEntry {
        project_id: project.as_deref(),
        task_id: task.as_deref(),
        description: &note,
        billable,
        auto: true,
        context: Some(fp),
    };
    log::info!(
        "auto switch: project={} at={}",
        project.as_deref().unwrap_or("needs-review"),
        fmt_time(at)
    );
    if let Err(e) = db::start(&mut st.db.lock().unwrap(), &entry, &fmt_time(at)) {
        log::error!("could not switch timer: {e}");
        return;
    }
    changed(app);
}

pub async fn monitor(app: AppHandle) {
    let mut engine = Engine::default();
    let mut ctx: Option<(String, DateTime<Utc>)> = None;
    let mut last_ai: Option<DateTime<Utc>> = None;
    let mut last_shot: Option<DateTime<Utc>> = None;
    let mut last_refresh: Option<DateTime<Utc>> = None;
    loop {
        let interval = tick();
        {
            let st = app.state::<AppState>();
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = st.auto_kick.notified() => { last_refresh = None; }
            }
        }
        let st = app.state::<AppState>();
        let now = Utc::now();
        let (enabled, base, paused, pinned) = {
            let a = st.auto.lock().unwrap();
            (a.enabled, a.service.clone(), a.paused_until, a.pinned_until)
        };
        let signed_in = st.cfg.lock().unwrap().is_some();
        let Some(base) = base.filter(|_| enabled && signed_in) else {
            engine.reset();
            ctx = None;
            continue;
        };
        if last_refresh.is_none_or(|t| (now - t).num_seconds() >= REFRESH_SECS) {
            refresh(&app).await;
            last_refresh = Some(now);
            let _ = crate::refresh_tray(&app);
        }
        let policy = st.policy.lock().unwrap().clone();
        if paused.is_some_and(|p| p > now) {
            engine.reset();
            ctx = None;
            continue;
        }
        if st.away.lock().unwrap().is_some()
            || idle::system_idle_secs().is_some_and(|i| i >= IDLE_SKIP_SECS)
        {
            continue; // the idle prompt (W6/W7) owns this period
        }
        let Ok(Some(active)) = tokio::task::spawn_blocking(context::active).await else {
            continue;
        };
        let excluded = is_excluded(&active.app, &policy.excluded_apps);
        let fp = if excluded {
            format!("{}{SEP}<private>", active.app.to_lowercase())
        } else {
            fingerprint(&active.app, &active.title)
        };
        let since = match &ctx {
            Some((f, s)) if *f == fp => *s,
            _ => now,
        };
        ctx = Some((fp.clone(), since));

        let (mut obs, mut source) = resolve(&st, &fp, &active, excluded, now);
        let stable = (now - since).num_seconds() >= interval.as_secs() as i64;
        let every = capture_interval(&policy);
        let keep = policy.keep_screenshots_days > 0;
        let need_ai = obs == Obs::Pending
            && policy.enabled
            && stable
            && last_ai.is_none_or(|t| (now - t).num_seconds() >= every);
        // One screenshot per interval for the day review (kept on this computer), and
        // whenever the AI needs to look. Excluded apps are never captured.
        let shot_due =
            keep && !excluded && last_shot.is_none_or(|t| (now - t).num_seconds() >= every);
        let shot = if need_ai || shot_due {
            take_shot(&st, &policy, &active).await
        } else {
            None
        };
        let mut confidence = None;
        if need_ai {
            last_ai = Some(now);
            let image = shot.as_ref().map(|s| s.jpeg.clone());
            if let Some((o, conf)) = classify(&app, &base, &active, &fp, &policy, image).await {
                obs = o;
                source = "ai";
                confidence = Some(conf);
            }
        }
        // Activity log for the review: one record per interval (and for every screenshot
        // the AI needed), with the window and the classification. If Screen Recording is
        // not allowed there is no picture, but the record still shows the decision.
        if keep && !excluded && (shot_due || shot.is_some()) {
            last_shot = Some(now);
            let row = capture_row(now, &active, &obs, source, confidence);
            let saved = match &shot {
                Some(s) => shots::save(&st, &row, s),
                None => shots::save_record(&st, &row),
            };
            match saved {
                Ok(()) => {
                    let _ = app.emit("state-changed", ());
                }
                Err(e) => log::error!("could not keep activity record: {e}"),
            }
        }
        if obs == Obs::Pending && !policy.enabled {
            // No AI on the server: keyword hints and the cache only; unknown windows
            // keep the current project instead of piling up review blocks.
            obs = Obs::Neutral;
        }
        st.auto.lock().unwrap().last_source = Some(source);

        let running = db::running(&st.db.lock().unwrap()).ok().flatten();
        let manual_recent = running.as_ref().is_some_and(|r| {
            r.source == "manual"
                && DateTime::parse_from_rfc3339(&r.start)
                    .is_ok_and(|s| (now - s.with_timezone(&Utc)).num_seconds() < policy.pin_secs)
        });
        if pinned.is_some_and(|p| p > now) || manual_recent {
            engine.reset();
            continue;
        }
        let cur = running.and_then(|r| {
            Some(Running {
                start: DateTime::parse_from_rfc3339(&r.start)
                    .ok()?
                    .with_timezone(&Utc),
                project_id: r.project_id,
                task_id: r.task_id,
            })
        });
        let params = Params {
            min_switch: policy.min_switch_secs,
            unresolved_review: policy.unresolved_review_secs,
        };
        if let Some(action) = engine.observe(Utc::now(), &obs, since, cur.as_ref(), &params) {
            apply(&app, action, &fp);
        }
    }
}

/// What the review shows next to a screenshot: the window and how it was classified.
fn capture_row(
    now: DateTime<Utc>,
    active: &Active,
    obs: &Obs,
    source: &str,
    confidence: Option<f64>,
) -> db::CaptureRow {
    let (kind, project_id, task_id, note) = match obs {
        Obs::Project {
            project_id,
            task_id,
            note,
        } => (
            "project",
            Some(project_id.clone()),
            task_id.clone(),
            note.clone(),
        ),
        Obs::Review { note } => ("review", None, None, note.clone()),
        Obs::Neutral => ("neutral", None, None, String::new()),
        Obs::Private => ("private", None, None, String::new()),
        Obs::Pending => ("pending", None, None, String::new()),
    };
    db::CaptureRow {
        id: uuid::Uuid::new_v4().to_string(),
        taken_at: fmt_time(now),
        app: active.app.chars().take(200).collect(),
        title: active.title.chars().take(500).collect(),
        kind: kind.into(),
        project_id,
        task_id,
        note,
        confidence,
        source: source.into(),
    }
}

// ---------- user actions ----------

/// A manual start/stop/switch: auto-switching steps back for `pin_secs`. It teaches
/// nothing: people often pick a project just before switching windows, so the window in
/// front at that moment says little about the pick.
pub fn manual_action(st: &AppState) {
    let pin = st.policy.lock().unwrap().pin_secs;
    st.auto.lock().unwrap().pinned_until = Some(Utc::now() + Span::seconds(pin));
}

/// A correction in the day review: this window meant `project`. Only remembered for
/// specific window titles (see `specific`).
pub fn learn_user(st: &AppState, fp: &str, project: &str, task: Option<&str>) {
    if !specific(fp) {
        return;
    }
    let now = Utc::now();
    let l = db::Learned {
        kind: "project".into(),
        project_id: Some(project.into()),
        task_id: task.map(String::from),
        note: String::new(),
        source: "user".into(),
    };
    let user_since = fmt_time(now - Span::days(USER_CACHE_DAYS));
    let _ = db::learn(&st.db.lock().unwrap(), fp, &l, &fmt_time(now), &user_since);
}

/// Private time: stops the timer and suspends auto-tracking. `minutes: None` pauses
/// until midnight.
pub fn pause(app: &AppHandle, minutes: Option<i64>) {
    let st = app.state::<AppState>();
    let until = match minutes {
        Some(m) => Utc::now() + Span::minutes(m.clamp(1, 24 * 60)),
        None => next_midnight(Local::now()),
    };
    let _ = db::stop(&mut st.db.lock().unwrap(), &crate::now());
    st.auto.lock().unwrap().paused_until = Some(until);
    let _ = db::kv_set(&st.db.lock().unwrap(), "paused_until", &fmt_time(until));
    changed(app);
}

pub fn resume(app: &AppHandle) {
    let st = app.state::<AppState>();
    {
        let mut a = st.auto.lock().unwrap();
        a.paused_until = None;
        a.pinned_until = None;
    }
    let _ = db::kv_del(&st.db.lock().unwrap(), "paused_until");
    st.auto_kick.notify_one();
    changed(app);
}

// ---------- commands (settings window) ----------

#[derive(Serialize)]
pub struct AutoView {
    enabled: bool,
    service_url: Option<String>,
    permission: Permission,
    status: String,
    paused: bool,
    server_ready: bool,
    capture_minutes: i64,
    excluded: Vec<String>,
    keep_days: i64,
    pin_minutes: i64,
    ai_provider: String,
}

pub fn view(st: &AppState) -> AutoView {
    let status = status(st);
    let a = st.auto.lock().unwrap();
    let p = st.policy.lock().unwrap();
    AutoView {
        enabled: a.enabled,
        service_url: a.service.clone(),
        permission: context::screen_permission(),
        status,
        paused: a.paused_until.is_some_and(|t| t > Utc::now()),
        server_ready: p.enabled,
        capture_minutes: (p.capture_interval_secs / 60).max(1),
        excluded: p.excluded_apps.clone(),
        keep_days: p.keep_screenshots_days,
        pin_minutes: (p.pin_secs / 60).max(1),
        ai_provider: p.ai_provider.clone(),
    }
}

#[tauri::command]
pub fn get_auto(st: tauri::State<AppState>) -> AutoView {
    view(&st)
}

/// The settings window asks the server for its current policy and AI provider, so the
/// consent screen describes what this server really does (user-invoked, so it may use
/// the network). Offline, the last known policy stays.
#[tauri::command]
pub async fn load_auto(app: AppHandle) -> AutoView {
    refresh(&app).await;
    view(&app.state::<AppState>())
}

/// Turns auto-tracking on (after the user accepted the consent screen) or off.
#[tauri::command]
pub async fn set_auto(app: AppHandle, enabled: bool) -> Result<AutoView, String> {
    let st = app.state::<AppState>();
    if !enabled {
        {
            let mut a = st.auto.lock().unwrap();
            a.enabled = false;
            a.error = None;
        }
        let _ = db::kv_set(&st.db.lock().unwrap(), "auto_enabled", "0");
        changed(&app);
        return Ok(view(&st));
    }
    let cfg = st
        .cfg
        .lock()
        .unwrap()
        .clone()
        .ok_or("Sign in to solidtime first")?;
    let base = ai::service_url(&cfg.base);
    // Check the service and our access before turning anything on.
    let policy = ai::Service::new(&st.http, &base, &cfg.token, &cfg.org)
        .policy()
        .await
        .map_err(|e| format!("Ira service: {e}"))?;
    {
        let db = st.db.lock().unwrap();
        for (k, v) in [
            ("ai_url", base.as_str()),
            ("auto_enabled", "1"),
            ("consent_version", CONSENT_VERSION),
        ] {
            db::kv_set(&db, k, v).map_err(|e| e.to_string())?;
        }
        if let Ok(s) = serde_json::to_string(&policy) {
            let _ = db::kv_set(&db, "ai_policy", &s);
        }
    }
    *st.policy.lock().unwrap() = policy;
    {
        let mut a = st.auto.lock().unwrap();
        a.enabled = true;
        a.service = Some(base);
        a.error = None;
    }
    if context::screen_permission() == Permission::Denied {
        context::request_screen_permission();
    }
    st.auto_kick.notify_one();
    changed(&app);
    Ok(view(&st))
}

#[tauri::command]
pub fn request_screen_permission() -> Permission {
    context::request_screen_permission()
}

/// Opens the Screen Recording pane of System Settings (macOS).
#[tauri::command]
pub fn open_screen_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

#[tauri::command]
pub fn pause_auto(app: AppHandle, minutes: Option<i64>) {
    pause(&app, minutes);
}

#[tauri::command]
pub fn resume_auto(app: AppHandle) {
    resume(&app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    fn proj(p: &str) -> Obs {
        Obs::Project {
            project_id: p.into(),
            task_id: None,
            note: format!("on {p}"),
        }
    }

    fn running(p: Option<&str>, start: i64) -> Running {
        Running {
            project_id: p.map(String::from),
            task_id: None,
            start: t(start),
        }
    }

    const P: Params = Params {
        min_switch: 120,
        unresolved_review: 900,
    };

    fn target_of(a: &Action) -> (Option<&str>, DateTime<Utc>) {
        match a {
            Action::Start { target, at } | Action::Switch { target, at } => {
                (target.project_id.as_deref(), *at)
            }
        }
    }

    #[test]
    fn starts_immediately_when_nothing_runs() {
        let mut e = Engine::default();
        let a = e.observe(t(0), &proj("A"), t(0), None, &P).unwrap();
        assert!(matches!(a, Action::Start { .. }));
        assert_eq!(target_of(&a), (Some("A"), t(0)));
        // not known yet: a review block that the first answer will replace
        let a = e.observe(t(0), &Obs::Pending, t(0), None, &P).unwrap();
        assert_eq!(target_of(&a).0, None);
    }

    #[test]
    fn switch_needs_the_new_project_to_hold_and_is_backdated() {
        let mut e = Engine::default();
        let cur = running(Some("A"), -3600);
        assert_eq!(e.observe(t(20), &proj("B"), t(0), Some(&cur), &P), None);
        assert_eq!(e.observe(t(100), &proj("B"), t(0), Some(&cur), &P), None);
        let a = e.observe(t(120), &proj("B"), t(0), Some(&cur), &P).unwrap();
        assert!(matches!(a, Action::Switch { .. }));
        assert_eq!(
            target_of(&a),
            (Some("B"), t(0)),
            "backdated to when B started"
        );
    }

    #[test]
    fn short_excursion_does_not_switch() {
        let mut e = Engine::default();
        let cur = running(Some("A"), -3600);
        assert_eq!(e.observe(t(20), &proj("B"), t(0), Some(&cur), &P), None);
        // back on A before 2 minutes: candidate dropped
        assert_eq!(e.observe(t(60), &proj("A"), t(60), Some(&cur), &P), None);
        assert_eq!(e.observe(t(200), &proj("B"), t(190), Some(&cur), &P), None);
    }

    #[test]
    fn chat_and_lock_screen_keep_the_current_project() {
        let mut e = Engine::default();
        let cur = running(Some("A"), -3600);
        for s in [20, 600, 3000] {
            assert_eq!(e.observe(t(s), &Obs::Neutral, t(0), Some(&cur), &P), None);
        }
    }

    #[test]
    fn unrecognised_or_private_for_long_becomes_review() {
        let mut e = Engine::default();
        let cur = running(Some("A"), -3600);
        assert_eq!(e.observe(t(600), &Obs::Pending, t(0), Some(&cur), &P), None);
        let a = e
            .observe(t(900), &Obs::Pending, t(0), Some(&cur), &P)
            .unwrap();
        assert_eq!(target_of(&a), (None, t(0)));

        let mut e = Engine::default();
        let a = e
            .observe(t(900), &Obs::Private, t(0), Some(&cur), &P)
            .unwrap();
        match a {
            Action::Switch { target, .. } => assert_eq!(target.note, "Private app"),
            _ => panic!(),
        }
    }

    #[test]
    fn unsure_answer_switches_to_review_after_hysteresis() {
        let mut e = Engine::default();
        let cur = running(Some("A"), -3600);
        let r = Obs::Review {
            note: "Video call".into(),
        };
        assert_eq!(e.observe(t(60), &r, t(0), Some(&cur), &P), None);
        assert_eq!(
            target_of(&e.observe(t(120), &r, t(0), Some(&cur), &P).unwrap()),
            (None, t(0))
        );
    }

    #[test]
    fn known_project_replaces_review_block_at_once() {
        let mut e = Engine::default();
        let cur = running(None, 0);
        // the window was there before the review block started: replace it entirely
        let a = e.observe(t(40), &proj("A"), t(-5), Some(&cur), &P).unwrap();
        assert_eq!(target_of(&a), (Some("A"), t(0)));
        // a long unknown stretch is kept for review
        let a = e
            .observe(t(700), &proj("B"), t(600), Some(&cur), &P)
            .unwrap();
        assert_eq!(target_of(&a), (Some("B"), t(600)));
        // a short one is absorbed into the work that follows
        let a = e.observe(t(80), &proj("B"), t(60), Some(&cur), &P).unwrap();
        assert_eq!(target_of(&a), (Some("B"), t(0)));
    }

    #[test]
    fn same_project_without_task_keeps_the_task() {
        let mut e = Engine::default();
        let cur = Running {
            project_id: Some("A".into()),
            task_id: Some("T1".into()),
            start: t(-60),
        };
        assert_eq!(e.observe(t(500), &proj("A"), t(0), Some(&cur), &P), None);
    }

    #[test]
    fn switch_is_never_before_the_running_entry() {
        let mut e = Engine::default();
        let cur = running(Some("A"), 100);
        let a = e.observe(t(300), &proj("B"), t(0), Some(&cur), &P).unwrap();
        assert_eq!(target_of(&a), (Some("B"), t(100)));
    }

    #[test]
    fn fingerprint_ignores_unread_counters_and_case() {
        assert_eq!(
            fingerprint("Slack", "(3) #acme - Ira"),
            fingerprint("slack", "#acme - Ira")
        );
        assert_eq!(fingerprint("Code", "  a   b "), format!("code{SEP}a b"));
        assert_ne!(fingerprint("Code", "(draft) x"), fingerprint("Code", "x"));
    }

    fn hint(p: &str, k: &[&str]) -> Hint {
        Hint {
            project_id: p.into(),
            keywords: k.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn keywords_match_at_word_start_and_must_be_unique() {
        let hints = [
            hint("bill", &["ira-billing", "BILL-"]),
            hint("acme", &["acme"]),
        ];
        assert_eq!(
            match_hints("Code — ira-billing — invoice.rs", &hints),
            Some("bill")
        );
        assert_eq!(
            match_hints("Jira BILL-123 fix rounding", &hints),
            Some("bill")
        );
        assert_eq!(match_hints("Chrome — Acme Portal", &hints), Some("acme"));
        assert_eq!(match_hints("pacman", &hints), None, "not inside a word");
        assert_eq!(match_hints("acme vs BILL-1", &hints), None, "ambiguous");
        assert_eq!(
            match_hints("x", &[hint("p", &["ab"])]),
            None,
            "too short to trust"
        );
    }

    #[test]
    fn only_specific_titles_are_remembered() {
        for t in [
            "index.ts — mmcg-platform",
            "auto.rs — ira_tracker",
            "#random - Ira Technologies - Slack",
            "[MOB-142] Push notifications crash on Android",
        ] {
            assert!(specific(&fingerprint("App", t)), "{t}");
        }
        for t in ["Zoom Meeting", "New Tab", "Downloads", "", "Inbox"] {
            assert!(!specific(&fingerprint("App", t)), "{t}");
        }
        assert!(!specific(&format!("whatsapp{SEP}<private>")));
    }

    #[test]
    fn excluded_apps_match_case_insensitively() {
        let ex = vec!["1Password".to_string(), "WhatsApp".to_string()];
        assert!(is_excluded("1Password 8", &ex));
        assert!(is_excluded("whatsapp", &ex));
        assert!(!is_excluded("Code", &ex));
        assert!(!is_excluded("Code", &[" ".to_string()]));
    }

    #[test]
    fn classification_maps_to_observations() {
        let c = |p: Option<&str>, cat: &str, conf: f64| Classification {
            project_id: p.map(String::from),
            task_id: None,
            category: cat.into(),
            confidence: conf,
            activity: "Reading logs".into(),
        };
        assert!(matches!(
            from_classification(&c(Some("A"), "work", 0.8), 0.6),
            Obs::Project { .. }
        ));
        assert_eq!(
            from_classification(&c(Some("A"), "work", 0.5), 0.6),
            Obs::Review {
                note: "Reading logs".into()
            }
        );
        assert_eq!(
            from_classification(&c(None, "communication", 0.9), 0.6),
            Obs::Neutral
        );
        assert_eq!(
            from_classification(&c(None, "non_work", 0.9), 0.6),
            Obs::Review {
                note: "Not work-related?".into()
            },
            "no personal details in descriptions"
        );
    }

    #[test]
    fn only_confident_answers_are_learned() {
        assert!(to_learned(&proj("A"), "ai").is_some());
        assert!(to_learned(&Obs::Neutral, "ai").is_some());
        assert!(to_learned(&Obs::Review { note: "x".into() }, "ai").is_none());
        assert!(to_learned(&Obs::Pending, "ai").is_none());
        let l = to_learned(&proj("A"), "ai").unwrap();
        assert_eq!(from_learned(&l), proj("A"));
    }

    #[test]
    fn pause_until_midnight_ends_at_the_next_local_day() {
        // Any day and hour, including weekends and late evenings: no work hours.
        for (d, h) in [(20, 18), (21, 10), (22, 23), (16, 0)] {
            let now = Local.with_ymd_and_hms(2026, 11, d, h, 30, 0).unwrap();
            let next = next_midnight(now).with_timezone(&Local);
            assert_eq!(next.date_naive(), now.date_naive().succ_opt().unwrap());
            assert_eq!(next.time(), NaiveTime::MIN);
        }
    }
}
