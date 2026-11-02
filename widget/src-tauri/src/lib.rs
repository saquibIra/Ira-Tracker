mod api;
mod db;
mod idle;
mod input;

use api::{Api, ApiError, Catalog, Config};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tokio::sync::Notify;

const KEYRING_SERVICE: &str = "com.ira.tracker";
const KEYRING_USER: &str = "solidtime-token";
const TRAY_ID: &str = "main";
const SWITCHER: &str = "switcher";
const LOGIN: &str = "login";
/// Days of server history pulled into the local store (recent list, later the day review).
const PULL_DAYS: i64 = 7;
/// Background sync interval; user actions trigger an earlier sync.
const SYNC_EVERY: Duration = Duration::from_secs(60);
/// Catalog (projects/tasks) is refreshed when the switcher opens if older than this.
const CATALOG_MAX_AGE_SECS: i64 = 10 * 60;

struct AppState {
    db: Mutex<Connection>,
    http: reqwest::Client,
    cfg: Mutex<Option<Config>>,
    catalog: Mutex<Catalog>,
    kick: Notify,
    want_refresh: AtomicBool,
    offline: AtomicBool,
    last_error: Mutex<Option<String>>,
    away: Mutex<Option<idle::Away>>,
}

/// solidtime requires UTC without fractional seconds.
fn fmt_time(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn now() -> String {
    fmt_time(chrono::Utc::now())
}

// ---------- view model ----------

#[derive(Serialize, Clone)]
struct Item {
    project_id: String,
    task_id: Option<String>,
    project: String,
    task: Option<String>,
    client: Option<String>,
    color: String,
}

impl Catalog {
    fn item(&self, project_id: &str, task_id: Option<&str>) -> Option<Item> {
        let p = self.projects.iter().find(|p| p.id == project_id)?;
        let task = task_id.and_then(|t| self.tasks.iter().find(|x| x.id == t));
        Some(Item {
            project_id: p.id.clone(),
            task_id: task.map(|t| t.id.clone()),
            project: p.name.clone(),
            task: task.map(|t| t.name.clone()),
            client: p
                .client_id
                .as_ref()
                .and_then(|c| self.clients.iter().find(|x| &x.id == c))
                .map(|c| c.name.clone()),
            color: p.color.clone(),
        })
    }

    fn items(&self) -> Vec<Item> {
        let mut out = Vec::new();
        for p in self.projects.iter().filter(|p| !p.is_archived) {
            out.extend(self.item(&p.id, None));
            for t in self
                .tasks
                .iter()
                .filter(|t| t.project_id == p.id && !t.is_done)
            {
                out.extend(self.item(&p.id, Some(&t.id)));
            }
        }
        out
    }
}

#[derive(Serialize)]
struct Running {
    item: Option<Item>,
    description: String,
    start: String,
}

#[derive(Serialize)]
struct UiState {
    signed_in: bool,
    base_url: Option<String>,
    running: Option<Running>,
    recent: Vec<Item>,
    items: Vec<Item>,
    pending: i64,
    offline: bool,
    error: Option<String>,
}

fn running(st: &AppState) -> Option<Running> {
    let e = db::running(&st.db.lock().unwrap()).ok()??;
    let cat = st.catalog.lock().unwrap();
    Some(Running {
        item: e
            .project_id
            .as_deref()
            .and_then(|p| cat.item(p, e.task_id.as_deref())),
        description: e.description,
        start: e.start,
    })
}

fn recent(st: &AppState) -> Vec<Item> {
    let pairs = db::recent(&st.db.lock().unwrap(), 5).unwrap_or_default();
    let cat = st.catalog.lock().unwrap();
    pairs
        .iter()
        .filter_map(|(p, t)| cat.item(p, t.as_deref()))
        .collect()
}

fn ui_state(st: &AppState, with_items: bool) -> UiState {
    let cfg = st.cfg.lock().unwrap().clone();
    UiState {
        signed_in: cfg.is_some(),
        base_url: cfg.map(|c| c.base),
        running: running(st),
        recent: recent(st),
        items: if with_items {
            st.catalog.lock().unwrap().items()
        } else {
            vec![]
        },
        pending: db::pending_count(&st.db.lock().unwrap()),
        offline: st.offline.load(SeqCst),
        error: st.last_error.lock().unwrap().clone(),
    }
}

// ---------- tray ----------

fn short_name(name: &str) -> String {
    let s = name.rsplit(" – ").next().unwrap_or(name);
    if s.chars().count() > 18 {
        format!("{}…", s.chars().take(17).collect::<String>())
    } else {
        s.to_string()
    }
}

fn elapsed(start: &str) -> String {
    let mins = chrono::DateTime::parse_from_rfc3339(start)
        .map(|s| {
            (chrono::Utc::now() - s.with_timezone(&chrono::Utc))
                .num_minutes()
                .max(0)
        })
        .unwrap_or(0);
    format!("{}:{:02}", mins / 60, mins % 60)
}

fn label(item: &Item) -> String {
    match &item.task {
        Some(t) => format!("{} · {}", item.project, t),
        None => item.project.clone(),
    }
}

/// W2: hover text shows project short name and elapsed time while running.
fn update_tooltip(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let st = app.state::<AppState>();
    let pending = db::pending_count(&st.db.lock().unwrap());
    let sync_note = match (st.offline.load(SeqCst), pending) {
        (true, n) => format!("\nOffline · {n} waiting to sync"),
        (false, n) if n > 0 => format!("\n{n} waiting to sync"),
        _ => String::new(),
    };
    let text = match running(&st) {
        Some(r) => {
            let short = r
                .item
                .as_ref()
                .map(|i| short_name(&i.project))
                .unwrap_or_else(|| "No project".into());
            format!("{short} — {}{sync_note}", elapsed(&r.start))
        }
        None => format!("Ira Tracker — stopped{sync_note}"),
    };
    let _ = tray.set_tooltip(Some(text));
}

/// W2: the tray icon itself shows running vs stopped.
fn tray_icon(running: bool) -> tauri::image::Image<'static> {
    let bytes: &[u8] = if running {
        include_bytes!("../icons/tray-running.png")
    } else {
        include_bytes!("../icons/tray-stopped.png")
    };
    tauri::image::Image::from_bytes(bytes).expect("bundled tray icon")
}

fn refresh_tray(app: &AppHandle) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let st = app.state::<AppState>();
    let s = ui_state(&st, false);

    let status = match &s.running {
        Some(r) => format!(
            "● {} ({})",
            r.item
                .as_ref()
                .map(label)
                .unwrap_or_else(|| "No project".into()),
            elapsed(&r.start)
        ),
        None => "Not tracking".into(),
    };
    let mut menu = MenuBuilder::new(app)
        .item(
            &MenuItemBuilder::with_id("status", status)
                .enabled(false)
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("stop", "Stop timer")
                .enabled(s.running.is_some())
                .build(app)?,
        )
        .separator();
    if s.signed_in {
        for (i, it) in s.recent.iter().enumerate() {
            menu = menu.text(format!("recent:{i}"), label(it));
        }
        if !s.recent.is_empty() {
            menu = menu.separator();
        }
        menu = menu.item(
            &MenuItemBuilder::with_id("switch", "Quick switch…")
                .accelerator("CmdOrCtrl+Shift+T")
                .build(app)?,
        );
    }
    if let Some(a) = st.away.lock().unwrap().as_ref() {
        menu = menu.text(
            "away",
            format!("Away {} min — Keep / Discard / Assign…", a.minutes),
        );
    }
    if let Some(err) = &s.error {
        menu = menu.item(
            &MenuItemBuilder::with_id("error", format!("⚠ {err}"))
                .enabled(false)
                .build(app)?,
        );
    }
    menu = menu.separator();
    menu = if s.signed_in {
        menu.text("signout", "Sign out")
    } else {
        menu.text("signin", "Sign in…")
    };
    tray.set_menu(Some(menu.text("quit", "Quit Ira Tracker").build()?))?;
    tray.set_icon(Some(tray_icon(s.running.is_some())))?;
    update_tooltip(app);
    Ok(())
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "stop" => {
            let _ = stop_timer(app.clone());
        }
        "switch" => open_window(app, SWITCHER),
        "signin" => open_window(app, LOGIN),
        "away" => idle::show_prompt(app),
        "signout" => {
            if let Err(e) = sign_out(app.clone()) {
                *app.state::<AppState>().last_error.lock().unwrap() = Some(e);
                let _ = refresh_tray(app);
            }
        }
        "quit" => app.exit(0),
        id => {
            if let Some(i) = id
                .strip_prefix("recent:")
                .and_then(|i| i.parse::<usize>().ok())
            {
                let st = app.state::<AppState>();
                if let Some(it) = recent(&st).get(i).cloned() {
                    let _ = start_timer(app.clone(), it.project_id, it.task_id, String::new());
                }
            }
        }
    }
}

/// Windows are created on demand and destroyed on close to keep idle memory low.
fn open_window(app: &AppHandle, label: &str) {
    if label == SWITCHER {
        let st = app.state::<AppState>();
        let age = chrono::Utc::now().timestamp() - st.catalog.lock().unwrap().fetched_at;
        if age > CATALOG_MAX_AGE_SECS {
            st.want_refresh.store(true, SeqCst);
            st.kick.notify_one();
        }
        if st.cfg.lock().unwrap().is_none() {
            return open_window(app, LOGIN);
        }
    }
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let (title, w, h, decorated) = if label == LOGIN {
        ("Ira Tracker – Sign in", 440.0, 420.0, true)
    } else {
        ("Ira Tracker", 560.0, 460.0, false)
    };
    match WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(w, h)
        .resizable(false)
        .decorations(decorated)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .focused(true)
        .build()
    {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("window {label}: {e}"),
    }
}

/// Local state changed: update tray and windows, and schedule a sync.
fn changed(app: &AppHandle) {
    let _ = refresh_tray(app);
    let _ = app.emit("state-changed", ());
    app.state::<AppState>().kick.notify_one();
}

// ---------- commands ----------

#[tauri::command]
fn get_state(st: State<AppState>) -> UiState {
    ui_state(&st, true)
}

#[tauri::command]
fn start_timer(
    app: AppHandle,
    project_id: String,
    task_id: Option<String>,
    description: String,
) -> Result<(), String> {
    let st = app.state::<AppState>();
    let billable = {
        let cat = st.catalog.lock().unwrap();
        if cat.item(&project_id, task_id.as_deref()).is_none()
            || task_id
                .as_ref()
                .is_some_and(|t| !cat.tasks.iter().any(|x| &x.id == t))
        {
            return Err("Unknown project or task; refresh and try again".into());
        }
        cat.projects
            .iter()
            .find(|p| p.id == project_id)
            .is_some_and(|p| p.is_billable)
    };
    db::start(
        &mut st.db.lock().unwrap(),
        &project_id,
        task_id.as_deref(),
        &input::description(&description),
        billable,
        &now(),
    )
    .map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
fn stop_timer(app: AppHandle) -> Result<(), String> {
    db::stop(&mut app.state::<AppState>().db.lock().unwrap(), &now()).map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
fn set_description(app: AppHandle, description: String) -> Result<(), String> {
    db::set_running_description(
        &app.state::<AppState>().db.lock().unwrap(),
        &input::description(&description),
    )
    .map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

/// Organizations available to this token; the sign-in window shows a picker when
/// there is more than one.
#[tauri::command]
async fn list_organizations(
    app: AppHandle,
    base_url: String,
    token: String,
) -> Result<Vec<api::Membership>, String> {
    let base = input::server_url(&base_url)?;
    let st = app.state::<AppState>();
    Api::new(&st.http, &base, token.trim())
        .memberships()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn sign_in(
    app: AppHandle,
    base_url: String,
    token: String,
    organization_id: Option<String>,
) -> Result<(), String> {
    let base = input::server_url(&base_url)?;
    let token = token.trim().to_string();
    let st = app.state::<AppState>();
    let all = Api::new(&st.http, &base, &token)
        .memberships()
        .await
        .map_err(|e| e.to_string())?;
    let chosen = api::choose_membership(&all, organization_id.as_deref())?;
    let (org, member) = (chosen.org_id.clone(), chosen.member_id.clone());
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .and_then(|k| k.set_password(&token))
        .map_err(|e| format!("Could not save token to keychain: {e}"))?;
    {
        let db = st.db.lock().unwrap();
        for (k, v) in [
            ("base_url", &base),
            ("org_id", &org),
            ("member_id", &member),
        ] {
            db::kv_set(&db, k, v).map_err(|e| e.to_string())?;
        }
    }
    let cfg = Config {
        base,
        token,
        org,
        member,
    };
    if pull(&app, &Api::from_config(&st.http, &cfg), &cfg)
        .await
        .is_err()
    {
        st.want_refresh.store(true, SeqCst);
    }
    *st.cfg.lock().unwrap() = Some(cfg);
    *st.last_error.lock().unwrap() = None;
    changed(&app);
    if let Some(w) = app.get_webview_window(LOGIN) {
        let _ = w.close();
    }
    Ok(())
}

#[tauri::command]
fn sign_out(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    {
        let db = st.db.lock().unwrap();
        let pending = db::pending_count(&db);
        if pending > 0 {
            return Err(format!(
                "{pending} entries not synced yet; connect to the server first"
            ));
        }
        db.execute_batch("DELETE FROM entry;")
            .map_err(|e| e.to_string())?;
        for k in ["base_url", "org_id", "member_id", "catalog"] {
            let _ = db::kv_del(&db, k);
        }
    }
    if let Ok(k) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = k.delete_credential();
    }
    *st.cfg.lock().unwrap() = None;
    *st.catalog.lock().unwrap() = Catalog::default();
    *st.last_error.lock().unwrap() = None;
    changed(&app);
    Ok(())
}

// ---------- sync ----------

async fn pull(app: &AppHandle, api: &Api<'_>, cfg: &Config) -> Result<(), ApiError> {
    let st = app.state::<AppState>();
    let catalog = api.catalog(&cfg.org).await?;
    let since = fmt_time(chrono::Utc::now() - chrono::Duration::days(PULL_DAYS));
    let entries = api.entries_since(cfg, &since).await?;
    {
        let mut db = st.db.lock().unwrap();
        if let Ok(s) = serde_json::to_string(&catalog) {
            let _ = db::kv_set(&db, "catalog", &s);
        }
        db::merge_remote(&mut db, &entries).map_err(|e| ApiError::Rejected(e.to_string()))?;
        let _ = db::prune(&db, &since);
    }
    *st.catalog.lock().unwrap() = catalog;
    Ok(())
}

async fn push(app: &AppHandle, api: &Api<'_>, cfg: &Config) -> Result<(), ApiError> {
    let st = app.state::<AppState>();
    let dirty = db::dirty(&st.db.lock().unwrap()).unwrap_or_default();
    for e in dirty {
        // A create may have reached the server before a crash; adopt it instead of
        // creating a duplicate.
        let mut e = e;
        if e.remote_id.is_none() && e.create_attempted {
            e.remote_id = find_created(api, cfg, &e).await?;
        }
        let res = if e.deleted {
            match &e.remote_id {
                Some(id) => api.delete_entry(&cfg.org, id).await.map(|_| None),
                None => Ok(None),
            }
        } else {
            let body = json!({
                "member_id": cfg.member,
                "project_id": e.project_id,
                "task_id": e.task_id,
                "start": e.start,
                "end": e.end,
                "billable": e.billable,
                "description": e.description,
            });
            match &e.remote_id {
                None => {
                    db::mark_create_attempted(&st.db.lock().unwrap(), &e.local_id)
                        .map_err(|err| ApiError::Rejected(err.to_string()))?;
                    api.create_entry(&cfg.org, &body).await.map(Some)
                }
                Some(id) => api
                    .update_entry(&cfg.org, id, &body)
                    .await
                    .map(|_| Some(id.clone())),
            }
        };
        let db = st.db.lock().unwrap();
        let _ = match res {
            Ok(None) => db::remove(&db, &e.local_id, e.rev),
            Ok(Some(remote)) => db::mark_synced(&db, &e.local_id, &remote, e.rev),
            Err(ApiError::Rejected(msg)) => {
                *st.last_error.lock().unwrap() = Some(msg.clone());
                db::mark_rejected(&db, &e.local_id, e.rev, &msg)
            }
            Err(other) => return Err(other),
        };
    }
    Ok(())
}

async fn find_created(
    api: &Api<'_>,
    cfg: &Config,
    e: &db::Entry,
) -> Result<Option<String>, ApiError> {
    let Ok(start) = chrono::DateTime::parse_from_rfc3339(&e.start) else {
        return Ok(None);
    };
    let next = fmt_time(start.with_timezone(&chrono::Utc) + chrono::Duration::seconds(1));
    let found = api.entries_starting_at(cfg, &e.start, &next).await?;
    Ok(found
        .into_iter()
        .find(|r| r.project_id == e.project_id && r.task_id == e.task_id)
        .map(|r| r.id))
}

async fn sync_once(app: &AppHandle) {
    let st = app.state::<AppState>();
    let Some(cfg) = st.cfg.lock().unwrap().clone() else {
        return;
    };
    let api = Api::from_config(&st.http, &cfg);
    let mut result = push(app, &api, &cfg).await;
    if result.is_ok() && st.want_refresh.swap(false, SeqCst) {
        result = pull(app, &api, &cfg).await;
        if result.is_err() {
            st.want_refresh.store(true, SeqCst);
        }
    }
    match result {
        Ok(()) => {
            st.offline.store(false, SeqCst);
        }
        Err(ApiError::Offline(m)) => {
            eprintln!("sync: {m}");
            st.offline.store(true, SeqCst);
        }
        Err(e) => {
            st.offline.store(false, SeqCst);
            *st.last_error.lock().unwrap() = Some(e.to_string());
        }
    }
    let _ = refresh_tray(app);
    let _ = app.emit("state-changed", ());
}

/// Network budget: at most one batched sync per minute, and nothing when there is
/// nothing to send. A local change schedules the next allowed slot.
async fn sync_loop(app: AppHandle) {
    let mut last: Option<std::time::Instant> = None;
    loop {
        let st = app.state::<AppState>();
        tokio::select! {
            _ = st.kick.notified() => {}
            _ = tokio::time::sleep(SYNC_EVERY) => {}
        }
        if let Some(t) = last {
            tokio::time::sleep(SYNC_EVERY.saturating_sub(t.elapsed())).await;
        }
        let busy = st.want_refresh.load(SeqCst) || db::pending_count(&st.db.lock().unwrap()) > 0;
        if busy {
            sync_once(&app).await;
            last = Some(std::time::Instant::now());
        }
    }
}

async fn ticker(app: AppHandle) {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        update_tooltip(&app);
    }
}

// ---------- app ----------

fn load_config(db: &Connection) -> Option<Config> {
    Some(Config {
        base: db::kv_get(db, "base_url")?,
        org: db::kv_get(db, "org_id")?,
        member: db::kv_get(db, "member_id")?,
        token: keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .ok()?
            .get_password()
            .ok()?,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mods = if cfg!(target_os = "macos") {
        Modifiers::SUPER
    } else {
        Modifiers::CONTROL
    };
    let hotkey = Shortcut::new(Some(mods | Modifiers::SHIFT), Code::KeyT);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            open_window(app, SWITCHER)
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, ev| {
                    if ev.state() == ShortcutState::Pressed {
                        open_window(app, SWITCHER);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_state,
            start_timer,
            stop_timer,
            set_description,
            list_organizations,
            sign_in,
            sign_out,
            idle::get_away,
            idle::resolve_away
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("tracker.db"))?;
            let catalog = db::kv_get(&conn, "catalog")
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let cfg = load_config(&conn);
            let signed_in = cfg.is_some();
            app.manage(AppState {
                db: Mutex::new(conn),
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(20))
                    .build()?,
                cfg: Mutex::new(cfg),
                catalog: Mutex::new(catalog),
                kick: Notify::new(),
                want_refresh: AtomicBool::new(true),
                offline: AtomicBool::new(false),
                last_error: Mutex::new(None),
                away: Mutex::new(None),
            });

            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tray_icon(false))
                .show_menu_on_left_click(true)
                .on_menu_event(|app, ev| handle_menu(app, ev.id().as_ref()))
                .build(app)?;
            refresh_tray(app.handle())?;

            if let Err(e) = app.global_shortcut().register(hotkey) {
                eprintln!("hotkey not registered: {e}");
            }
            #[cfg(not(debug_assertions))]
            {
                use tauri_plugin_autostart::ManagerExt;
                let _ = app.autolaunch().enable();
            }

            if signed_in {
                app.state::<AppState>().kick.notify_one();
            } else {
                open_window(app.handle(), LOGIN);
            }
            tauri::async_runtime::spawn(sync_loop(app.handle().clone()));
            tauri::async_runtime::spawn(ticker(app.handle().clone()));
            tauri::async_runtime::spawn(idle::monitor(app.handle().clone()));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Ira Tracker")
        .run(|_, event| {
            // Closing the last window must not quit a tray app.
            if let RunEvent::ExitRequested {
                api, code: None, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
