use std::time::Duration;

use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, CONTENT_ENCODING, COOKIE, HeaderMap, REFERER, USER_AGENT,
};

pub const DEFAULT_USER_AGENT: &str = concat!("LajuFile/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestOptions {
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub cookie: Option<String>,
}

pub fn validate_proxy_url(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = url::Url::parse(trimmed).map_err(|e| format!("Invalid proxy URL: {e}"))?;
    if !matches!(
        parsed.scheme(),
        "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
    ) {
        return Err(format!(
            "Unsupported proxy scheme '{}' (use http, https, or socks5)",
            parsed.scheme()
        ));
    }
    if parsed.host_str().is_none() {
        return Err("Proxy URL is missing a host".to_string());
    }
    Ok(Some(parsed.to_string()))
}

fn base_client_builder(
    connect_timeout: Duration,
    read_timeout: Duration,
) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .tcp_nodelay(true)
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Duration::from_secs(30))
        .connect_timeout(connect_timeout)
        .read_timeout(read_timeout)
}

pub fn build_client(
    connect_timeout: Duration,
    read_timeout: Duration,
) -> Result<reqwest::Client, reqwest::Error> {
    base_client_builder(connect_timeout, read_timeout).build()
}

pub fn build_client_with_proxy(
    connect_timeout: Duration,
    read_timeout: Duration,
    proxy_url: Option<&str>,
) -> Result<reqwest::Client, String> {
    let mut builder = base_client_builder(connect_timeout, read_timeout);
    if let Some(proxy) = validate_proxy_url(proxy_url.unwrap_or(""))? {
        builder = builder.proxy(reqwest::Proxy::all(&proxy).map_err(|e| format!("Proxy: {e}"))?);
    }
    builder
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))
}

pub fn apply_headers(
    mut builder: reqwest::RequestBuilder,
    opts: &RequestOptions,
) -> reqwest::RequestBuilder {
    builder = builder
        .header(
            USER_AGENT,
            opts.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT),
        )
        .header(ACCEPT, "*/*")
        .header(ACCEPT_ENCODING, "identity");

    if let Some(referrer) = &opts.referrer {
        builder = builder.header(REFERER, referrer);
    }
    if let Some(cookie) = &opts.cookie {
        builder = builder.header(COOKIE, cookie);
    }
    builder
}

pub fn ensure_identity_encoding(headers: &HeaderMap) -> Result<(), String> {
    for value in headers.get_all(CONTENT_ENCODING).iter() {
        let value = value.to_str().map_err(|_| "<invalid>".to_string())?;
        for token in value.split(',') {
            let token = token.trim();
            if token.is_empty() {
                return Err("<empty>".to_string());
            }
            if !token.eq_ignore_ascii_case("identity") {
                return Err(token.to_string());
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: Option<u64>,
}

pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let total = match total {
        "*" => None,
        value => Some(value.parse().ok()?),
    };

    if range == "*" {
        return Some(ContentRange {
            start: 0,
            end: 0,
            total,
        });
    }

    let (start, end) = range.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    if start > end {
        return None;
    }

    Some(ContentRange { start, end, total })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(options: &RequestOptions) -> reqwest::Request {
        let client = build_client(Duration::from_secs(5), Duration::from_secs(30)).unwrap();
        apply_headers(client.get("https://example.com/file.iso"), options)
            .build()
            .unwrap()
    }

    #[test]
    fn apply_headers_sets_identity_and_defaults() {
        let request = request(&RequestOptions::default());
        let headers = request.headers();

        assert_eq!(
            headers.get(USER_AGENT).unwrap().to_str().unwrap(),
            DEFAULT_USER_AGENT
        );
        assert_eq!(headers.get(ACCEPT).unwrap().to_str().unwrap(), "*/*");
        assert_eq!(
            headers.get(ACCEPT_ENCODING).unwrap().to_str().unwrap(),
            "identity"
        );
    }

    #[test]
    fn apply_headers_uses_optional_values() {
        let options = RequestOptions {
            user_agent: Some("TestAgent/1".into()),
            referrer: Some("https://ref.example/".into()),
            cookie: Some("session=abc".into()),
        };
        let request = request(&options);
        let headers = request.headers();

        assert_eq!(headers.get(USER_AGENT).unwrap(), "TestAgent/1");
        assert_eq!(headers.get(REFERER).unwrap(), "https://ref.example/");
        assert_eq!(headers.get(COOKIE).unwrap(), "session=abc");
    }

    #[test]
    fn parses_content_range() {
        assert_eq!(
            parse_content_range("bytes 0-0/1000"),
            Some(ContentRange {
                start: 0,
                end: 0,
                total: Some(1000)
            })
        );
        assert_eq!(
            parse_content_range("bytes 0-99/*"),
            Some(ContentRange {
                start: 0,
                end: 99,
                total: None
            })
        );
        assert_eq!(
            parse_content_range("bytes */0"),
            Some(ContentRange {
                start: 0,
                end: 0,
                total: Some(0)
            })
        );
    }

    #[test]
    fn rejects_invalid_content_range() {
        assert_eq!(parse_content_range("bytes 5-4/100"), None);
        assert_eq!(parse_content_range("nonsense"), None);
        assert_eq!(parse_content_range("bytes 0-0/abc"), None);
    }

    #[test]
    fn identity_encoding_accepts_absent_and_identity() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(ensure_identity_encoding(&headers), Ok(()));

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_ENCODING,
            reqwest::header::HeaderValue::from_static("Identity"),
        );
        assert_eq!(ensure_identity_encoding(&headers), Ok(()));
    }

    #[test]
    fn identity_encoding_rejects_any_other_token() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_ENCODING,
            reqwest::header::HeaderValue::from_static("identity, gzip"),
        );
        assert_eq!(ensure_identity_encoding(&headers), Err("gzip".to_string()));

        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(
            reqwest::header::CONTENT_ENCODING,
            reqwest::header::HeaderValue::from_static("identity"),
        );
        headers.append(
            reqwest::header::CONTENT_ENCODING,
            reqwest::header::HeaderValue::from_static("gzip"),
        );
        assert_eq!(ensure_identity_encoding(&headers), Err("gzip".to_string()));
    }

    #[test]
    fn default_user_agent_matches_package_version() {
        assert_eq!(
            DEFAULT_USER_AGENT,
            concat!("LajuFile/", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn validate_proxy_url_checks_schemes_and_build_client() {
        assert!(validate_proxy_url("").unwrap().is_none());
        assert!(validate_proxy_url("http://127.0.0.1:7890").is_ok());
        assert!(validate_proxy_url("https://proxy.example:8443").is_ok());
        assert!(validate_proxy_url("socks5://127.0.0.1:1080").is_ok());
        assert!(validate_proxy_url("ftp://127.0.0.1:21").is_err());
        assert!(validate_proxy_url("not a url").is_err());
        assert!(
            build_client_with_proxy(
                Duration::from_secs(5),
                Duration::from_secs(10),
                Some("socks5://127.0.0.1:1080")
            )
            .is_ok()
        );
    }
}
