use std::time::Duration;

use reqwest::header::{
    CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, HeaderMap, HeaderName,
    LAST_MODIFIED, RANGE,
};
use url::Url;

use crate::http::{RequestOptions, apply_headers, ensure_identity_encoding, parse_content_range};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub final_url: Url,
    pub size: Option<u64>,
    pub ranged: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
}

#[derive(Debug)]
pub struct ProbeResponse {
    pub result: ProbeResult,
    /// Open `200 OK` response body when the server ignored `Range: bytes=0-0`,
    /// allowing callers to stream directly without issuing a second GET request.
    pub response: Option<reqwest::Response>,
}

#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub url: Url,
    pub options: RequestOptions,
    pub timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("HTTP {0}")]
    Status(u16),
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("probe timed out")]
    Timeout,
    #[error("invalid Content-Range: {0}")]
    ContentRange(String),
    #[error("unsupported URL scheme: {0}")]
    UnsupportedScheme(String),
    #[error("unsupported Content-Encoding: {0}")]
    UnsupportedEncoding(String),
}

pub async fn probe(
    client: &reqwest::Client,
    req: &ProbeRequest,
) -> Result<ProbeResult, ProbeError> {
    if !matches!(req.url.scheme(), "http" | "https") {
        return Err(ProbeError::UnsupportedScheme(req.url.scheme().to_string()));
    }

    let response = apply_headers(client.get(req.url.clone()), &req.options)
        .header(RANGE, "bytes=0-0")
        .timeout(req.timeout)
        .send()
        .await?;

    parse_probe_response(response, false).map(|parsed| parsed.result)
}

/// Probes metadata and retains the live HTTP response body when the server
/// ignores `Range: bytes=0-0` and responds with `200 OK`.
///
/// The timeout applies only while awaiting response headers so subsequent body
/// streaming is not cut short by the probe timeout.
pub async fn probe_with_response(
    client: &reqwest::Client,
    req: &ProbeRequest,
) -> Result<ProbeResponse, ProbeError> {
    if !matches!(req.url.scheme(), "http" | "https") {
        return Err(ProbeError::UnsupportedScheme(req.url.scheme().to_string()));
    }

    let request = apply_headers(client.get(req.url.clone()), &req.options)
        .header(RANGE, "bytes=0-0")
        .send();
    let response = tokio::time::timeout(req.timeout, request)
        .await
        .map_err(|_| ProbeError::Timeout)??;

    parse_probe_response(response, true)
}

fn parse_probe_response(
    response: reqwest::Response,
    keep_200_body: bool,
) -> Result<ProbeResponse, ProbeError> {
    let final_url = response.url().clone();
    let headers = response.headers();
    let status = response.status();

    let etag = header_string(headers, ETAG);
    let last_modified = header_string(headers, LAST_MODIFIED);
    let content_type = header_string(headers, CONTENT_TYPE);
    let content_disposition = header_string(headers, CONTENT_DISPOSITION);

    if let Err(encoding) = ensure_identity_encoding(headers) {
        return Err(ProbeError::UnsupportedEncoding(encoding));
    }

    match status.as_u16() {
        206 => {
            let raw = header_string(headers, CONTENT_RANGE)
                .ok_or_else(|| ProbeError::ContentRange("missing Content-Range".into()))?;
            let range =
                parse_content_range(&raw).ok_or_else(|| ProbeError::ContentRange(raw.clone()))?;
            if range.start != 0 {
                return Err(ProbeError::ContentRange(raw));
            }

            Ok(ProbeResponse {
                result: ProbeResult {
                    final_url,
                    size: range.total,
                    ranged: true,
                    etag,
                    last_modified,
                    content_type,
                    content_disposition,
                },
                response: None,
            })
        }
        200 => {
            let result = ProbeResult {
                final_url,
                size: header_string(headers, CONTENT_LENGTH)
                    .and_then(|value| value.trim().parse().ok()),
                ranged: false,
                etag,
                last_modified,
                content_type,
                content_disposition,
            };
            Ok(ProbeResponse {
                result,
                response: keep_200_body.then_some(response),
            })
        }
        416 => {
            let empty = header_string(headers, CONTENT_RANGE)
                .and_then(|raw| parse_content_range(&raw))
                .is_some_and(|range| range.total == Some(0));

            if empty {
                Ok(ProbeResponse {
                    result: ProbeResult {
                        final_url,
                        size: Some(0),
                        ranged: true,
                        etag,
                        last_modified,
                        content_type,
                        content_disposition,
                    },
                    response: None,
                })
            } else {
                Err(ProbeError::Status(416))
            }
        }
        status => Err(ProbeError::Status(status)),
    }
}

fn header_string(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
