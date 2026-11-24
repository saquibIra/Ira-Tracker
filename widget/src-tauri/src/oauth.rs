//! Browser sign-in: OAuth 2.0 authorization code flow with PKCE (RFC 7636) against the
//! Time Pilot server, returning to a loopback address on a random port (RFC 8252).
//! Nobody types a password or token into the app. The access token is short-lived and
//! the refresh token renews it; both live only in the OS keychain.
//!
//! Pure parts (PKCE, URLs, callback parsing, refresh timing) are unit-tested; the
//! loopback listener and HTTP calls are thin wrappers around them.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// solidtime's API scopes; the desktop app reads and writes the person's own time.
pub const SCOPES: &str = "create read update delete";
const CALLBACK_PATH: &str = "/callback";
/// How long the app waits for the browser to come back.
pub const WAIT: Duration = Duration::from_secs(300);
/// Renew the access token this long before it expires.
const REFRESH_MARGIN_SECS: i64 = 3600;

/// What the keychain keeps for a signed-in person.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds when the access token expires.
    pub expires_at: i64,
}

impl Session {
    pub fn needs_refresh(&self, now: i64) -> bool {
        self.refresh_token.is_some() && self.expires_at - now < REFRESH_MARGIN_SECS
    }
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A fresh verifier (64 characters from 48 random bytes) and its S256 challenge.
pub fn new_pkce() -> Pkce {
    let mut bytes = Vec::with_capacity(48);
    for _ in 0..3 {
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    let verifier = URL_SAFE_NO_PAD.encode(&bytes);
    let challenge = challenge_for(&verifier);
    Pkce {
        verifier,
        challenge,
    }
}

pub fn new_state() -> String {
    URL_SAFE_NO_PAD.encode(uuid::Uuid::new_v4().as_bytes())
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

pub fn authorize_url(
    base: &str,
    client_id: &str,
    redirect: &str,
    challenge: &str,
    state: &str,
) -> Result<String, String> {
    let mut url = Url::parse(&format!("{}/oauth/authorize", base.trim_end_matches('/')))
        .map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("scope", SCOPES)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.into())
}

#[derive(Debug, PartialEq)]
pub enum Callback {
    Code(String),
    /// The person declined, or the response does not belong to this sign-in.
    Failed(String),
    /// Another request to the listener (e.g. the browser asking for a favicon).
    Other,
}

/// Reads the first line of the browser's request to the loopback listener.
pub fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Callback::Other;
    };
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Callback::Other;
    };
    if url.path() != CALLBACK_PATH {
        return Callback::Other;
    }
    let get = |k: &str| {
        url.query_pairs()
            .find(|(name, _)| name == k)
            .map(|(_, v)| v.into_owned())
    };
    if get("state").as_deref() != Some(expected_state) {
        return Callback::Failed("The sign-in response did not match. Try again.".into());
    }
    if let Some(e) = get("error") {
        return Callback::Failed(match e.as_str() {
            "access_denied" => "Sign-in was cancelled in the browser.".into(),
            _ => get("error_description").unwrap_or(e),
        });
    }
    match get("code") {
        Some(c) if !c.is_empty() => Callback::Code(c),
        _ => Callback::Failed("The server did not return a sign-in code.".into()),
    }
}

/// Waits for the browser to come back to `listener`, answering with a small page.
/// Returns the authorization code. Blocking: run it on a blocking thread.
pub fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cancel.load(Ordering::SeqCst) {
            return Err("Sign-in cancelled.".into());
        }
        match listener.accept() {
            Ok((stream, _)) => match answer(stream, state) {
                Callback::Code(code) => return Ok(code),
                Callback::Failed(msg) => return Err(msg),
                Callback::Other => {}
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Sign-in timed out. Try again.".into())
}

fn answer(mut stream: TcpStream, state: &str) -> Callback {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 8192];
    let mut len = 0;
    while len < buf.len() {
        match stream.read(&mut buf[len..]) {
            Ok(0) => break,
            Ok(n) => {
                len += n;
                if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let request = String::from_utf8_lossy(&buf[..len]);
    let result = parse_callback(request.lines().next().unwrap_or_default(), state);
    let (status, body) = match &result {
        Callback::Code(_) => (
            "200 OK",
            page(
                true,
                "You're signed in",
                "You can close this tab and go back to Time Pilot.",
            ),
        ),
        Callback::Failed(msg) => ("200 OK", page(false, "Sign-in didn't finish", msg)),
        Callback::Other => ("404 Not Found", String::new()),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    result
}

/// The page the browser shows after returning to the app (static text only).
fn page(ok: bool, title: &str, text: &str) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let accent = if ok { "#0D9488" } else { "#B4540A" };
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Time Pilot</title>
<style>:root{{color-scheme:light dark}}body{{margin:0;min-height:100vh;display:grid;place-items:center;font:16px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Inter,sans-serif;background:#0b1a2b;color:#e6edf3}}main{{max-width:26rem;padding:2rem;text-align:center}}h1{{font-size:1.5rem;margin:1.25rem 0 .5rem;color:{accent}}}p{{margin:0;color:#b6c2cf}}</style></head>
<body><main><svg width="56" height="56" viewBox="0 0 48 48" aria-hidden="true"><rect width="48" height="48" rx="12" fill="#0D9488"/><circle cx="24" cy="24" r="14.5" fill="none" stroke="#fff" stroke-opacity=".35" stroke-width="3"/><path d="M24 24V14M24 24l9-5.2" stroke="#fff" stroke-width="3.2" stroke-linecap="round"/><circle cx="24" cy="24" r="3" fill="#fff"/></svg>
<h1>{}</h1><p>{}</p></main></body></html>"##,
        esc(title),
        esc(text)
    )
}

/// Opens the default browser (never a window inside the app, so password managers and
/// existing browser sessions work).
pub fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let result = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", windows)))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    result
        .map(|_| ())
        .map_err(|e| format!("Could not open the browser: {e}"))
}

#[derive(Deserialize)]
struct ClientInfo {
    desktop_client_id: String,
}

/// The desktop app's OAuth client id, published by the server's Autotrack service.
pub async fn desktop_client_id(http: &Client, service: &str) -> Result<String, String> {
    let url = format!("{}/v1/client", service.trim_end_matches('/'));
    let res = http
        .get(&url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| {
            "Can't reach the Time Pilot server. Check the internet connection.".to_string()
        })?;
    if !res.status().is_success() {
        return Err(format!(
            "The Time Pilot server is not ready for desktop sign-in (HTTP {}).",
            res.status().as_u16()
        ));
    }
    let info: ClientInfo = res.json().await.map_err(|e| e.to_string())?;
    if info.desktop_client_id.trim().is_empty() {
        return Err("The Time Pilot server is not ready for desktop sign-in.".into());
    }
    Ok(info.desktop_client_id)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Debug, PartialEq)]
pub enum RefreshError {
    /// Network or server trouble: try again later.
    Offline(String),
    /// The refresh token is no longer valid: the person must sign in again.
    Expired,
}

async fn token_request(
    http: &Client,
    base: &str,
    form: &[(&str, &str)],
) -> Result<TokenResponse, (Option<StatusCode>, String)> {
    let res = http
        .post(format!("{}/oauth/token", base.trim_end_matches('/')))
        .header("Accept", "application/json")
        .form(form)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| (None, e.to_string()))?;
    let status = res.status();
    if !status.is_success() {
        return Err((Some(status), format!("HTTP {}", status.as_u16())));
    }
    res.json().await.map_err(|e| (Some(status), e.to_string()))
}

fn session_from(
    client_id: &str,
    t: TokenResponse,
    now: i64,
    old_refresh: Option<String>,
) -> Session {
    Session {
        client_id: client_id.to_string(),
        access_token: t.access_token,
        refresh_token: t.refresh_token.or(old_refresh),
        expires_at: now + t.expires_in.unwrap_or(3600).max(60),
    }
}

pub async fn exchange_code(
    http: &Client,
    base: &str,
    client_id: &str,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> Result<Session, String> {
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("redirect_uri", redirect),
        ("code_verifier", verifier),
        ("code", code),
    ];
    let t = token_request(http, base, &form)
        .await
        .map_err(|(_, e)| format!("Sign-in could not be completed ({e}). Try again."))?;
    Ok(session_from(
        client_id,
        t,
        chrono::Utc::now().timestamp(),
        None,
    ))
}

pub async fn refresh(http: &Client, base: &str, s: &Session) -> Result<Session, RefreshError> {
    let Some(rt) = s.refresh_token.clone() else {
        return Err(RefreshError::Expired);
    };
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", s.client_id.as_str()),
        ("refresh_token", rt.as_str()),
        ("scope", SCOPES),
    ];
    match token_request(http, base, &form).await {
        Ok(t) => Ok(session_from(
            &s.client_id,
            t,
            chrono::Utc::now().timestamp(),
            Some(rt),
        )),
        Err((Some(st), _)) if st == StatusCode::BAD_REQUEST || st == StatusCode::UNAUTHORIZED => {
            Err(RefreshError::Expired)
        }
        Err((_, e)) => Err(RefreshError::Offline(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let p = new_pkce();
        assert_eq!(p.verifier.len(), 64);
        assert!(
            p.verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_eq!(p.challenge, challenge_for(&p.verifier));
        assert_ne!(new_pkce().verifier, p.verifier);
    }

    #[test]
    fn authorize_url_carries_pkce_and_state() {
        let u = authorize_url(
            "https://t.example.com/",
            "cid",
            &redirect_uri(5123),
            "chal",
            "st8",
        )
        .unwrap();
        let url = Url::parse(&u).unwrap();
        assert_eq!(url.path(), "/oauth/authorize");
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5123/callback");
        assert_eq!(q["code_challenge"], "chal");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["state"], "st8");
        assert_eq!(q["scope"], SCOPES);
    }

    #[test]
    fn callback_is_checked() {
        assert_eq!(
            parse_callback("GET /callback?code=abc&state=s1 HTTP/1.1", "s1"),
            Callback::Code("abc".into())
        );
        assert!(matches!(
            parse_callback("GET /callback?code=abc&state=other HTTP/1.1", "s1"),
            Callback::Failed(_)
        ));
        assert!(matches!(
            parse_callback("GET /callback?error=access_denied&state=s1 HTTP/1.1", "s1"),
            Callback::Failed(m) if m.contains("cancelled")
        ));
        assert_eq!(
            parse_callback("GET /favicon.ico HTTP/1.1", "s1"),
            Callback::Other
        );
        assert_eq!(
            parse_callback("POST /callback HTTP/1.1", "s1"),
            Callback::Other
        );
        assert_eq!(parse_callback("", "s1"), Callback::Other);
    }

    #[test]
    fn refresh_starts_an_hour_before_expiry() {
        let s = Session {
            client_id: "c".into(),
            access_token: "a".into(),
            refresh_token: Some("r".into()),
            expires_at: 10_000,
        };
        assert!(!s.needs_refresh(10_000 - 3601));
        assert!(s.needs_refresh(10_000 - 3599));
        assert!(
            !Session {
                refresh_token: None,
                ..s
            }
            .needs_refresh(20_000)
        );
    }

    #[test]
    fn loopback_listener_returns_the_code() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n")
                .unwrap();
            let mut s2 = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s2.write_all(b"GET /callback?code=c0de&state=xyz HTTP/1.1\r\nHost: x\r\n\r\n")
                .unwrap();
            let mut reply = String::new();
            let _ = s2.read_to_string(&mut reply);
            reply
        });
        let cancel = AtomicBool::new(false);
        let code = wait_for_code(&listener, "xyz", &cancel, Duration::from_secs(5)).unwrap();
        assert_eq!(code, "c0de");
        let reply = browser.join().unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK"));
        assert!(reply.contains("signed in"));
    }

    #[test]
    fn cancel_stops_waiting() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cancel = AtomicBool::new(true);
        let err = wait_for_code(&listener, "s", &cancel, Duration::from_secs(5)).unwrap_err();
        assert!(err.contains("cancelled"));
    }
}
