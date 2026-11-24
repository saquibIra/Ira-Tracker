//! W8 end-of-day review: the day's blocks (auto and manual), "Needs review" blocks to
//! fix, edits that teach auto-tracking, and Submit. A small reminder appears at the
//! configured review time (default 18:30) on days with tracked time.

use crate::{AppState, Item, ai, auto, changed, db, fmt_time, input, shots};
use chrono::{DateTime, Duration as Span, Local, NaiveDate, TimeZone, Utc};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub const REVIEW: &str = "review";
pub const NUDGE: &str = "nudge";
/// Days back the review can show (the local store keeps 7 days).
const MAX_OFFSET: i64 = 6;

#[derive(Serialize)]
pub struct EntryView {
    local_id: String,
    start: String,
    end: Option<String>,
    item: Option<Item>,
    description: String,
    auto: bool,
    needs_review: bool,
    running: bool,
    error: Option<String>,
}

#[derive(Serialize)]
pub struct DayView {
    date: String,
    label: String,
    offset: i64,
    entries: Vec<EntryView>,
    total_secs: i64,
    review_count: usize,
    submitted_at: Option<String>,
    pending: i64,
    items: Vec<Item>,
    captures: Vec<CaptureView>,
    /// Screen Recording state, so the review can explain missing screenshots.
    screen_permission: crate::context::Permission,
    keep_days: i64,
    capture_error: Option<String>,
}

/// A screenshot kept for the review, with what auto-tracking decided from it.
#[derive(Serialize)]
pub struct CaptureView {
    id: String,
    taken_at: String,
    app: String,
    title: String,
    /// project / review / neutral / pending
    kind: String,
    item: Option<Item>,
    note: String,
    confidence: Option<f64>,
    /// ai / keywords / cache / pending
    source: String,
    thumb: Option<String>,
}

/// Local calendar day `offset` days ago, as a UTC range.
pub fn day_range(today: NaiveDate, offset: i64) -> (NaiveDate, DateTime<Utc>, DateTime<Utc>) {
    let day = today - Span::days(offset.clamp(0, MAX_OFFSET));
    let local = |d: NaiveDate| {
        Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|| Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()))
    };
    (day, local(day), local(day.succ_opt().unwrap_or(day)))
}

fn parse(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Seconds of `[start, end)` inside `[from, to)`; a running entry counts up to `now`.
pub fn clipped_secs(
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
) -> i64 {
    let s = start.max(from);
    let e = end.unwrap_or(now).min(to);
    (e - s).num_seconds().max(0)
}

pub fn day_view(st: &AppState, offset: i64) -> Result<DayView, String> {
    let offset = offset.clamp(0, MAX_OFFSET);
    let now = Utc::now();
    let (day, from, to) = day_range(Local::now().date_naive(), offset);
    let entries = db::day_entries(&st.db.lock().unwrap(), &fmt_time(from), &fmt_time(to))
        .map_err(|e| e.to_string())?;
    let cat = st.catalog.lock().unwrap();
    let mut total = 0;
    let mut views = Vec::with_capacity(entries.len());
    for e in entries {
        let Some(start) = parse(&e.start) else {
            continue;
        };
        let end = e.end.as_deref().and_then(parse);
        total += clipped_secs(start, end, from, to, now);
        views.push(EntryView {
            item: e
                .project_id
                .as_deref()
                .and_then(|p| cat.item(p, e.task_id.as_deref())),
            needs_review: e.project_id.is_none(),
            running: e.end.is_none(),
            auto: e.source == "auto",
            local_id: e.local_id,
            start: e.start,
            end: e.end,
            description: e.description,
            error: e.error,
        });
    }
    let rows = db::captures_between(&st.db.lock().unwrap(), &fmt_time(from), &fmt_time(to))
        .map_err(|e| e.to_string())?;
    let captures = rows
        .into_iter()
        .map(|r| CaptureView {
            item: r
                .project_id
                .as_deref()
                .and_then(|p| cat.item(p, r.task_id.as_deref())),
            thumb: shots::data_url(&st.data_dir, &r.id, true),
            id: r.id,
            taken_at: r.taken_at,
            app: r.app,
            title: r.title,
            kind: r.kind,
            note: r.note,
            confidence: r.confidence,
            source: r.source,
        })
        .collect();
    let date = day.format("%Y-%m-%d").to_string();
    let db = st.db.lock().unwrap();
    Ok(DayView {
        screen_permission: crate::context::screen_permission(),
        keep_days: st.policy.lock().unwrap().keep_screenshots_days,
        capture_error: st.auto.lock().unwrap().capture_error.clone(),
        captures,
        review_count: views.iter().filter(|v| v.needs_review).count(),
        label: day.format("%A %-d %B").to_string(),
        submitted_at: db::kv_get(&db, &format!("submitted:{date}")),
        pending: db::pending_count(&db),
        items: cat.items(),
        date,
        offset,
        entries: views,
        total_secs: total,
    })
}

#[tauri::command]
pub fn get_day(st: State<AppState>, offset: i64) -> Result<DayView, String> {
    day_view(&st, offset)
}

/// Changes an entry's project/task and/or description. Reassigning an auto block
/// teaches auto-tracking what that window means.
#[tauri::command]
pub fn update_entry(
    app: AppHandle,
    local_id: String,
    project_id: Option<String>,
    task_id: Option<String>,
    description: Option<String>,
) -> Result<(), String> {
    let st = app.state::<AppState>();
    let entry = db::entry(&st.db.lock().unwrap(), &local_id)
        .map_err(|e| e.to_string())?
        .ok_or("This entry no longer exists")?;
    if let Some(p) = &project_id {
        let billable = {
            let cat = st.catalog.lock().unwrap();
            let project = cat
                .projects
                .iter()
                .find(|x| &x.id == p)
                .ok_or("Unknown project; refresh and try again")?;
            if task_id
                .as_ref()
                .is_some_and(|t| !cat.tasks.iter().any(|x| &x.id == t && &x.project_id == p))
            {
                return Err("Unknown task for this project".into());
            }
            project.is_billable
        };
        db::set_entry_project(
            &st.db.lock().unwrap(),
            &local_id,
            p,
            task_id.as_deref(),
            billable,
        )
        .map_err(|e| e.to_string())?;
        if let Some(fp) = entry.context.as_deref().filter(|_| entry.source == "auto") {
            auto::learn_user(&st, fp, p, task_id.as_deref());
        }
    }
    if let Some(d) = description {
        db::set_entry_description(&st.db.lock().unwrap(), &local_id, &input::description(&d))
            .map_err(|e| e.to_string())?;
    }
    changed(&app);
    Ok(())
}

/// solidtime-format UTC time from the review window; not in the future, not older than
/// the review range.
fn review_time(raw: &str) -> Result<String, String> {
    let t = DateTime::parse_from_rfc3339(raw.trim())
        .map_err(|_| "Invalid time".to_string())?
        .with_timezone(&Utc);
    let now = Utc::now();
    if t > now + Span::seconds(60) {
        return Err("Times cannot be in the future".into());
    }
    if t < now - Span::days(MAX_OFFSET + 2) {
        return Err("That is outside the review range".into());
    }
    Ok(fmt_time(t))
}

/// Changes when a block started/ended (the neighbouring block moves with the boundary).
#[tauri::command]
pub fn set_entry_times(
    app: AppHandle,
    local_id: String,
    start: String,
    end: Option<String>,
) -> Result<(), String> {
    let start = review_time(&start)?;
    let end = end.as_deref().map(review_time).transpose()?;
    let st = app.state::<AppState>();
    db::set_entry_times(
        &mut st.db.lock().unwrap(),
        &local_id,
        &start,
        end.as_deref(),
    )?;
    changed(&app);
    Ok(())
}

/// Splits a block at `at` (e.g. where a screenshot shows other work).
#[tauri::command]
pub fn split_entry(app: AppHandle, local_id: String, at: String) -> Result<String, String> {
    let at = review_time(&at)?;
    let st = app.state::<AppState>();
    let id = db::split_entry(&mut st.db.lock().unwrap(), &local_id, &at)?;
    changed(&app);
    Ok(id)
}

/// The full screenshot, as a data URL for the viewer.
#[tauri::command]
pub fn get_capture(st: State<AppState>, id: String) -> Result<String, String> {
    shots::data_url(&st.data_dir, &id, false).ok_or_else(|| "Screenshot no longer available".into())
}

#[tauri::command]
pub fn delete_capture(app: AppHandle, id: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    shots::delete(&st, &id)?;
    let _ = tauri::Emitter::emit(&app, "state-changed", ());
    Ok(())
}

#[tauri::command]
pub fn delete_entry(app: AppHandle, local_id: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    if !db::delete_entry(&st.db.lock().unwrap(), &local_id).map_err(|e| e.to_string())? {
        return Err("This entry no longer exists".into());
    }
    changed(&app);
    Ok(())
}

/// W8 Submit: only when nothing needs review. Recorded on the Ira service so leads can
/// see who submitted.
#[tauri::command]
pub async fn submit_day(app: AppHandle, offset: i64) -> Result<(), String> {
    let st = app.state::<AppState>();
    let view = day_view(&st, offset)?;
    if view.review_count > 0 {
        return Err(format!(
            "{} block(s) still need a project. Fix or delete them first.",
            view.review_count
        ));
    }
    let cfg = st.cfg.lock().unwrap().clone().ok_or("Sign in first")?;
    let base = st
        .auto
        .lock()
        .unwrap()
        .service
        .clone()
        .ok_or("Set the Ira service address in Auto-tracking settings first")?;
    ai::Service::new(&st.http, &base, &cfg.token, &cfg.org)
        .submit_day(&view.date, view.total_secs, view.entries.len() as i64)
        .await
        .map_err(|e| e.to_string())?;
    let _ = db::kv_set(
        &st.db.lock().unwrap(),
        &format!("submitted:{}", view.date),
        &crate::now(),
    );
    changed(&app);
    Ok(())
}

/// At the review time, once per day and only on days with tracked time (people work any
/// day): a small reminder that does not take focus and disappears after 8 s.
pub fn maybe_nudge(app: &AppHandle) {
    let st = app.state::<AppState>();
    if st.cfg.lock().unwrap().is_none() {
        return;
    }
    let now = Local::now();
    let (due, today) = {
        let p = st.policy.lock().unwrap();
        let at = auto::parse_hhmm(&p.review_at);
        (
            at.is_some_and(|a| now.time() >= a),
            now.format("%Y-%m-%d").to_string(),
        )
    };
    if !due {
        return;
    }
    {
        let db = st.db.lock().unwrap();
        let (_, from, to) = day_range(now.date_naive(), 0);
        let tracked =
            db::day_entries(&db, &fmt_time(from), &fmt_time(to)).is_ok_and(|e| !e.is_empty());
        if !tracked
            || db::kv_get(&db, "nudged").as_deref() == Some(today.as_str())
            || db::kv_get(&db, &format!("submitted:{today}")).is_some()
        {
            return;
        }
        let _ = db::kv_set(&db, "nudged", &today);
    }
    if app.get_webview_window(REVIEW).is_some() {
        return;
    }
    let (w, h) = (380.0, 120.0);
    let mut b = WebviewWindowBuilder::new(app, NUDGE, WebviewUrl::App("index.html".into()))
        .title("Ira Tracker – Review your day")
        .inner_size(w, h)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false);
    if let Ok(Some(m)) = app.primary_monitor() {
        let size = m.size().to_logical::<f64>(m.scale_factor());
        let pos = m.position().to_logical::<f64>(m.scale_factor());
        b = b.position(pos.x + size.width - w - 16.0, pos.y + 40.0);
    }
    if let Err(e) = b.build() {
        log::error!("review reminder: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + h * 3600, 0).unwrap()
    }

    #[test]
    fn durations_are_clipped_to_the_day() {
        let (from, to) = (t(0), t(24));
        assert_eq!(clipped_secs(t(-2), Some(t(1)), from, to, t(30)), 3600);
        assert_eq!(clipped_secs(t(23), Some(t(26)), from, to, t(30)), 3600);
        assert_eq!(
            clipped_secs(t(5), None, from, to, t(6)),
            3600,
            "running counts to now"
        );
        assert_eq!(clipped_secs(t(25), Some(t(26)), from, to, t(30)), 0);
    }

    #[test]
    fn day_range_is_one_local_day() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 19).unwrap();
        let (d, from, to) = day_range(today, 1);
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
        assert!((to - from).num_hours() >= 23 && (to - from).num_hours() <= 25);
        assert_eq!(day_range(today, 99).0, today - Span::days(MAX_OFFSET));
    }
}
