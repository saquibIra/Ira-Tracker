//! Thin client for the solidtime REST API (personal API token auth).

use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::RemoteEntry;

#[derive(Clone, Debug)]
pub struct Config {
    pub base: String,
    pub token: String,
    pub org: String,
    pub member: String,
}

#[derive(Debug)]
pub enum ApiError {
    /// Network down, timeout or 5xx: retry later.
    Offline(String),
    /// Token invalid or revoked.
    Auth,
    /// The server refused this request (4xx): retrying unchanged will not help.
    Rejected(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ApiError::Offline(m) => write!(f, "Server unreachable: {m}"),
            ApiError::Auth => write!(f, "Token rejected; please sign in again"),
            ApiError::Rejected(m) => write!(f, "Server rejected the request: {m}"),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub clients: Vec<ClientRec>,
    pub projects: Vec<ProjectRec>,
    pub tasks: Vec<TaskRec>,
    #[serde(default)]
    pub fetched_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClientRec {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectRec {
    pub id: String,
    pub name: String,
    pub color: String,
    pub client_id: Option<String>,
    pub is_billable: bool,
    #[serde(default)]
    pub is_archived: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TaskRec {
    pub id: String,
    pub name: String,
    pub project_id: String,
    #[serde(default)]
    pub is_done: bool,
}

pub struct Api<'a> {
    http: &'a Client,
    base: String,
    token: &'a str,
}

impl<'a> Api<'a> {
    pub fn new(http: &'a Client, base: &str, token: &'a str) -> Self {
        Api {
            http,
            base: format!("{}/api/v1", base.trim_end_matches('/')),
            token,
        }
    }

    pub fn from_config(http: &'a Client, cfg: &'a Config) -> Self {
        Self::new(http, &cfg.base, &cfg.token)
    }

    async fn send(&self, req: RequestBuilder) -> Result<Value, ApiError> {
        let res = req
            .bearer_auth(self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ApiError::Offline(e.to_string()))?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        match status {
            s if s.is_success() => Ok(serde_json::from_str(&text).unwrap_or(Value::Null)),
            StatusCode::UNAUTHORIZED => Err(ApiError::Auth),
            s if s.is_server_error() => Err(ApiError::Offline(format!("HTTP {s}"))),
            s => {
                let msg = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
                    .unwrap_or(text);
                Err(ApiError::Rejected(format!("HTTP {}: {msg}", s.as_u16())))
            }
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// Returns (organization_id, member_id) of the user's first membership.
    pub async fn whoami(&self) -> Result<(String, String), ApiError> {
        let v = self
            .send(self.http.get(self.url("/users/me/memberships")))
            .await?;
        let m = v["data"]
            .get(0)
            .ok_or(ApiError::Rejected("No organization membership".into()))?;
        Ok((
            m["organization"]["id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            m["id"].as_str().unwrap_or_default().to_string(),
        ))
    }

    async fn list<T: for<'de> Deserialize<'de>>(
        &self,
        org: &str,
        resource: &str,
        query: &[(&str, &str)],
    ) -> Result<Vec<T>, ApiError> {
        let v = self
            .send(
                self.http
                    .get(self.url(&format!("/organizations/{org}/{resource}")))
                    .query(query),
            )
            .await?;
        serde_json::from_value(v["data"].clone()).map_err(|e| ApiError::Rejected(e.to_string()))
    }

    pub async fn catalog(&self, org: &str) -> Result<Catalog, ApiError> {
        Ok(Catalog {
            clients: self.list(org, "clients", &[]).await?,
            projects: self.list(org, "projects", &[]).await?,
            tasks: self.list(org, "tasks", &[("done", "false")]).await?,
            fetched_at: chrono::Utc::now().timestamp(),
        })
    }

    pub async fn entries_since(
        &self,
        cfg: &Config,
        start: &str,
    ) -> Result<Vec<RemoteEntry>, ApiError> {
        self.list(
            &cfg.org,
            "time-entries",
            &[
                ("member_id", &cfg.member),
                ("start", start),
                ("limit", "500"),
            ],
        )
        .await
    }

    pub async fn create_entry(&self, org: &str, body: &Value) -> Result<String, ApiError> {
        let v = self
            .send(
                self.http
                    .post(self.url(&format!("/organizations/{org}/time-entries")))
                    .json(body),
            )
            .await?;
        v["data"]["id"]
            .as_str()
            .map(String::from)
            .ok_or(ApiError::Rejected("No id in response".into()))
    }

    pub async fn update_entry(&self, org: &str, id: &str, body: &Value) -> Result<(), ApiError> {
        self.send(
            self.http
                .put(self.url(&format!("/organizations/{org}/time-entries/{id}")))
                .json(body),
        )
        .await
        .map(|_| ())
    }

    pub async fn delete_entry(&self, org: &str, id: &str) -> Result<(), ApiError> {
        match self
            .send(
                self.http
                    .delete(self.url(&format!("/organizations/{org}/time-entries/{id}"))),
            )
            .await
        {
            Err(ApiError::Rejected(m)) if m.starts_with("HTTP 404") => Ok(()),
            r => r.map(|_| ()),
        }
    }
}
