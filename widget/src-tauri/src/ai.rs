//! Client for the Autotrack service (server/autotrack): auto-tracking policy, project
//! keyword hints, screen classification and day submission.
//!
//! Auth is the engineer's solidtime token plus the organization id; the service checks
//! both with solidtime. The Anthropic key lives only on the server.

use crate::api::ApiError;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Auto-tracking policy, set centrally on the server (GET /v1/config). Defaults match
/// the server's, so a widget that has never reached the server behaves the same.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub enabled: bool,
    pub capture_interval_secs: i64,
    pub min_switch_secs: i64,
    pub pin_secs: i64,
    pub review_below: f64,
    pub unresolved_review_secs: i64,
    pub capture_max_px: u32,
    pub capture_quality: u8,
    /// When the end-of-day review reminder appears (local time, on days with tracked time).
    pub review_at: String,
    pub excluded_apps: Vec<String>,
    /// Screenshots kept on the engineer's computer for the day review; 0 = none.
    pub keep_screenshots_days: i64,
    /// Which AI the server asks ("gemini", "anthropic"); named on the consent screen.
    pub ai_provider: String,
}

/// The Autotrack service of a solidtime server: `<server>/autotrack` (Caddy routes it there), or the
/// local development service next to a local solidtime. Derived, never configured, so
/// a widget cannot keep talking to another server's service.
pub fn service_url(server: &str) -> String {
    let s = server.trim().trim_end_matches('/');
    let host = s
        .split_once("://")
        .map_or(s, |(_, rest)| rest)
        .split(['/', ':'])
        .next()
        .unwrap_or_default();
    if host == "localhost" || host == "127.0.0.1" {
        "http://localhost:8010".into()
    } else {
        format!("{s}/autotrack")
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            enabled: false,
            capture_interval_secs: 300,
            min_switch_secs: 120,
            pin_secs: 1800,
            review_below: 0.6,
            unresolved_review_secs: 900,
            capture_max_px: 1568,
            capture_quality: 60,
            review_at: "18:30".into(),
            keep_screenshots_days: 3,
            ai_provider: String::new(),
            excluded_apps: [
                "1Password",
                "Bitwarden",
                "Dashlane",
                "KeePass",
                "Keychain Access",
                "LastPass",
                "Passwords",
                "Messages",
                "Signal",
                "Telegram",
                "WhatsApp",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hint {
    pub project_id: String,
    pub keywords: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Classification {
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub category: String,
    pub confidence: f64,
    pub activity: String,
}

pub struct Service<'a> {
    http: &'a Client,
    base: String,
    token: &'a str,
    org: &'a str,
}

impl<'a> Service<'a> {
    pub fn new(http: &'a Client, base: &str, token: &'a str, org: &'a str) -> Self {
        Service {
            http,
            base: base.trim_end_matches('/').to_string(),
            token,
            org,
        }
    }

    async fn send(&self, req: RequestBuilder) -> Result<reqwest::Response, ApiError> {
        let res = req
            .bearer_auth(self.token)
            .header("X-TimePilot-Org", self.org)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ApiError::Offline(e.to_string()))?;
        let status = res.status();
        if status.is_success() || status == StatusCode::NOT_MODIFIED {
            return Ok(res);
        }
        let text = res.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        Err(match status {
            StatusCode::UNAUTHORIZED => ApiError::Auth,
            StatusCode::TOO_MANY_REQUESTS => ApiError::Offline(detail),
            s if s.is_server_error() => ApiError::Offline(detail),
            _ => ApiError::Rejected(detail),
        })
    }

    async fn json<T: for<'de> Deserialize<'de>>(res: reqwest::Response) -> Result<T, ApiError> {
        res.json::<T>()
            .await
            .map_err(|e| ApiError::Rejected(format!("unexpected response: {e}")))
    }

    pub async fn policy(&self) -> Result<Policy, ApiError> {
        let res = self
            .send(self.http.get(format!("{}/v1/config", self.base)))
            .await?;
        Self::json(res).await
    }

    /// Keyword hints. `Ok(None)` means unchanged since `etag`.
    pub async fn hints(
        &self,
        etag: Option<&str>,
    ) -> Result<Option<(Vec<Hint>, Option<String>)>, ApiError> {
        let mut req = self.http.get(format!("{}/v1/hints", self.base));
        if let Some(e) = etag {
            req = req.header("If-None-Match", e);
        }
        let res = self.send(req).await?;
        if res.status() == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        let etag = res
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        #[derive(Deserialize)]
        struct Body {
            hints: Vec<Hint>,
        }
        let body: Body = Self::json(res).await?;
        Ok(Some((body.hints, etag)))
    }

    /// Sends the window context and (optionally) the screenshot. The image is moved in
    /// and dropped when the request finishes.
    pub async fn classify(
        &self,
        app: &str,
        title: &str,
        image: Option<Vec<u8>>,
    ) -> Result<Classification, ApiError> {
        let mut form = reqwest::multipart::Form::new()
            .text("app", app.to_string())
            .text("title", title.to_string());
        if let Some(jpg) = image {
            let part = reqwest::multipart::Part::bytes(jpg)
                .file_name("screen.jpg")
                .mime_str("image/jpeg")
                .map_err(|e| ApiError::Rejected(e.to_string()))?;
            form = form.part("image", part);
        }
        let res = self
            .send(
                self.http
                    .post(format!("{}/v1/classify", self.base))
                    .multipart(form),
            )
            .await?;
        Self::json(res).await
    }

    pub async fn submit_day(
        &self,
        day: &str,
        total_secs: i64,
        entries: i64,
    ) -> Result<(), ApiError> {
        self.send(
            self.http
                .post(format!("{}/v1/days/{day}/submit", self.base))
                .json(&serde_json::json!({"total_secs": total_secs, "entries": entries})),
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_parses_partial_server_answers() {
        let p: Policy =
            serde_json::from_str(r#"{"enabled": true, "capture_interval_secs": 600}"#).unwrap();
        assert!(p.enabled);
        assert_eq!(p.capture_interval_secs, 600);
        assert_eq!(p.min_switch_secs, 120, "missing fields keep defaults");
    }

    #[test]
    fn service_is_derived_from_the_server() {
        assert_eq!(
            service_url("https://time.example.com"),
            "https://time.example.com/autotrack"
        );
        assert_eq!(
            service_url(" https://time.example.com/ "),
            "https://time.example.com/autotrack"
        );
        assert_eq!(
            service_url("http://localhost:8000"),
            "http://localhost:8010"
        );
        assert_eq!(
            service_url("http://127.0.0.1:8000/"),
            "http://localhost:8010"
        );
        // A host that merely starts with "localhost" is not local.
        assert_eq!(
            service_url("https://localhost.example.com"),
            "https://localhost.example.com/autotrack"
        );
    }

    #[test]
    fn classification_parses_server_answer() {
        let c: Classification = serde_json::from_str(
            r#"{"project_id":"p","task_id":null,"category":"work","confidence":0.8,"activity":"x","model":"m"}"#,
        )
        .unwrap();
        assert_eq!(c.project_id.as_deref(), Some("p"));
    }

    /// Against a running Autotrack service + solidtime:
    /// `TIMEPILOT_E2E_URL=http://localhost:8010 TIMEPILOT_E2E_TOKEN=… TIMEPILOT_E2E_ORG=… cargo test e2e_ -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn e2e_service_round_trip() {
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
        let (url, token, org) = (
            env("TIMEPILOT_E2E_URL"),
            env("TIMEPILOT_E2E_TOKEN"),
            env("TIMEPILOT_E2E_ORG"),
        );
        let http = Client::new();
        let svc = Service::new(&http, &url, &token, &org);
        tauri::async_runtime::block_on(async {
            let p = svc.policy().await.expect("policy");
            println!(
                "policy: enabled={} interval={}s",
                p.enabled, p.capture_interval_secs
            );
            let (hints, etag) = svc
                .hints(None)
                .await
                .expect("hints")
                .expect("first fetch has a body");
            println!("hints: {} projects, etag={etag:?}", hints.len());
            assert!(
                svc.hints(etag.as_deref())
                    .await
                    .expect("hints again")
                    .is_none(),
                "304 on same etag"
            );
            let jpg = crate::capture::encode_jpeg(&[128u8; 64 * 48], 64, 48, 60).unwrap();
            match svc
                .classify("Code", "orbit-billing — invoice.rs", Some(jpg))
                .await
            {
                Ok(c) => println!("classify: {c:?}"),
                Err(e) => println!("classify: {e} (expected without ANTHROPIC_API_KEY)"),
            }
            let bad = Service::new(&http, &url, "wrong-token", &org);
            assert!(matches!(bad.policy().await, Err(ApiError::Auth)));
        });
    }
}
