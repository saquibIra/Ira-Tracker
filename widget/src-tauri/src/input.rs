//! Validation of values coming from the UI before they reach the database or network.

/// W5 descriptions are one line. solidtime accepts up to 5000 characters; we keep
/// them short so a long paste can never block the sync queue.
pub const MAX_DESCRIPTION: usize = 500;

/// Single line, trimmed, at most `MAX_DESCRIPTION` characters.
pub fn description(raw: &str) -> String {
    let one_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.chars().take(MAX_DESCRIPTION).collect()
}

/// The server URL must use HTTPS so the API token is never sent in clear text.
/// Plain HTTP is allowed only for a server on this machine (local development).
pub fn server_url(raw: &str) -> Result<String, String> {
    let url =
        reqwest::Url::parse(raw.trim()).map_err(|_| "Enter a valid server URL".to_string())?;
    let local = matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("[::1]")
    );
    match url.scheme() {
        "https" => {}
        "http" if local => {}
        _ => return Err("Server URL must start with https://".into()),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Server URL must not contain credentials".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("Server URL must not contain ? or #".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_is_single_trimmed_line() {
        assert_eq!(
            description("  fix login\n  bug\t ABC-12 "),
            "fix login bug ABC-12"
        );
        assert_eq!(description(""), "");
    }

    #[test]
    fn description_is_capped() {
        let long = "é".repeat(MAX_DESCRIPTION + 50);
        assert_eq!(description(&long).chars().count(), MAX_DESCRIPTION);
    }

    #[test]
    fn https_urls_are_accepted() {
        assert_eq!(
            server_url(" https://time.ira.example/ ").unwrap(),
            "https://time.ira.example"
        );
        assert_eq!(
            server_url("https://ira.example/solidtime").unwrap(),
            "https://ira.example/solidtime"
        );
    }

    #[test]
    fn plain_http_only_for_this_machine() {
        assert_eq!(
            server_url("http://localhost:8000").unwrap(),
            "http://localhost:8000"
        );
        assert!(server_url("http://127.0.0.1:8000").is_ok());
        assert!(server_url("http://10.0.0.5:8000").is_err());
        assert!(server_url("http://time.ira.example").is_err());
    }

    #[test]
    fn unsafe_or_invalid_urls_are_rejected() {
        assert!(server_url("ftp://ira.example").is_err());
        assert!(server_url("https://user:pw@ira.example").is_err());
        assert!(server_url("https://ira.example/?x=1").is_err());
        assert!(server_url("not a url").is_err());
        assert!(server_url("javascript:alert(1)").is_err());
    }
}
