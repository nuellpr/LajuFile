use std::time::Duration;

use lajufile_core::http::DEFAULT_USER_AGENT;
use url::Url;

pub(super) fn validate_http_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|error| format!("invalid URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("only http:// and https:// URLs are supported".to_string());
    }
    Ok(url)
}

fn is_public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let oct = v4.octets();
            let shared_cgnat = oct[0] == 100 && (oct[1] & 0xc0) == 64;
            let benchmark = oct[0] == 198 && (oct[1] & 0xfe) == 18;
            let reserved = oct[0] == 0 || oct[0] >= 240;
            !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_documentation()
                && !v4.is_unspecified()
                && !v4.is_multicast()
                && !shared_cgnat
                && !benchmark
                && !reserved
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_ip(std::net::IpAddr::V4(mapped));
            }
            let seg = v6.segments();
            let ula = (seg[0] & 0xfe00) == 0xfc00;
            let link_local = (seg[0] & 0xffc0) == 0xfe80;
            let site_local = (seg[0] & 0xffc0) == 0xfec0;
            !v6.is_loopback()
                && !v6.is_unspecified()
                && !v6.is_multicast()
                && !ula
                && !link_local
                && !site_local
        }
    }
}

pub(super) fn validate_public_page_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only http:// or https:// web page URLs are supported".to_string());
    }
    let Some(host) = url.host() else {
        return Err("Web page URL is missing a host".to_string());
    };
    match host {
        url::Host::Ipv4(v4) => {
            if !is_public_ip(std::net::IpAddr::V4(v4)) {
                return Err("Refusing to scan private or local IP addresses".to_string());
            }
        }
        url::Host::Ipv6(v6) => {
            if !is_public_ip(std::net::IpAddr::V6(v6)) {
                return Err("Refusing to scan private or local IP addresses".to_string());
            }
        }
        url::Host::Domain(domain) => {
            let lower = domain.trim_end_matches('.').to_ascii_lowercase();
            if lower == "localhost"
                || lower.ends_with(".localhost")
                || lower.ends_with(".local")
                || lower.ends_with(".internal")
            {
                return Err("Refusing to scan localhost or internal domains".to_string());
            }
        }
    }
    Ok(())
}

async fn resolve_public_ips(url: &Url) -> Result<Vec<std::net::SocketAddr>, String> {
    validate_public_page_url(url)?;
    let host = url
        .host_str()
        .ok_or_else(|| "Web page URL is missing a host".to_string())?;
    let port = url.port_or_known_default().unwrap_or(80);
    let resolved: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| format!("Resolve failed: {error}"))?
        .collect();
    if resolved.is_empty() {
        return Err("Could not resolve host".to_string());
    }
    for addr in &resolved {
        if !is_public_ip(addr.ip()) {
            return Err("Refusing to scan private or local network address".to_string());
        }
    }
    Ok(resolved)
}

/// Fetches up to 2 MB of HTML from one public web page and extracts unique link targets.
pub(super) async fn fetch_page_links(
    url: Url,
    proxy_url: Option<String>,
) -> Result<Vec<String>, String> {
    const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
    const MAX_REDIRECTS: usize = 5;

    let validated_proxy =
        lajufile_core::http::validate_proxy_url(proxy_url.as_deref().unwrap_or(""))?;
    let mut current_url = url;
    let mut response = {
        let mut hops = 0usize;
        loop {
            validate_public_page_url(&current_url)?;
            let mut builder = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none());
            if let Some(proxy) = &validated_proxy {
                // Best-effort local DNS SSRF check even when a proxy is configured:
                // block domains that resolve locally to private/loopback IPs, while allowing
                // remote-only DNS (e.g. corporate/Tor proxies) when local lookup fails.
                if let Ok(addrs) = tokio::net::lookup_host((
                    current_url.host_str().unwrap_or_default(),
                    current_url.port_or_known_default().unwrap_or(80),
                ))
                .await
                {
                    for addr in addrs {
                        if !is_public_ip(addr.ip()) {
                            return Err(
                                "Refusing to scan private or local network address".to_string()
                            );
                        }
                    }
                }
                builder =
                    builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| format!("Proxy: {e}"))?);
            } else {
                let addrs = resolve_public_ips(&current_url).await?;
                if let Some(url::Host::Domain(domain)) = current_url.host() {
                    builder = builder.resolve_to_addrs(domain, &addrs);
                }
            }
            let client = builder
                .build()
                .map_err(|error| format!("Client failed: {error}"))?;
            let resp = client
                .get(current_url.clone())
                .header(reqwest::header::USER_AGENT, DEFAULT_USER_AGENT)
                .send()
                .await
                .map_err(|error| format!("Fetch failed: {error}"))?;
            if resp.status().is_redirection() {
                if hops >= MAX_REDIRECTS {
                    return Err("Too many redirects".to_string());
                }
                let location = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| "Redirect missing Location header".to_string())?;
                current_url = current_url
                    .join(location)
                    .map_err(|error| format!("Invalid redirect target: {error}"))?;
                hops += 1;
                continue;
            }
            break resp
                .error_for_status()
                .map_err(|error| format!("Fetch failed: {error}"))?;
        }
    };

    if let Some(content_type) = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        let lower = content_type.to_ascii_lowercase();
        if !lower.contains("html") && !lower.contains("text/plain") {
            return Err(format!("Page is not HTML ({content_type})"));
        }
    }
    let final_url = response.url().clone();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Read failed: {error}"))?
    {
        let remaining = MAX_PAGE_BYTES.saturating_sub(bytes.len());
        if remaining == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if bytes.len() >= MAX_PAGE_BYTES {
            break;
        }
    }
    let html = String::from_utf8_lossy(&bytes);
    Ok(crate::ui::extract_links_from_html(
        final_url.as_str(),
        &html,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssrf_guard_blocks_loopback_private_and_metadata_urls() {
        for bad in [
            "http://127.0.0.1:53841/rules",
            "http://localhost:8080/",
            "http://sub.localhost/test",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.1/",
            "http://192.168.1.1/",
            "http://172.16.0.5/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "file:///C:/Windows/System32/drivers/etc/hosts",
        ] {
            let parsed = Url::parse(bad).unwrap();
            assert!(
                validate_public_page_url(&parsed).is_err(),
                "{bad} should be blocked"
            );
        }
        let ok = Url::parse("https://example.com/downloads").unwrap();
        assert!(validate_public_page_url(&ok).is_ok());
    }

    #[test]
    fn validate_http_url_rejects_non_http_schemes() {
        assert!(validate_http_url("https://example.com/a.zip").is_ok());
        assert!(validate_http_url("http://example.com/a.zip").is_ok());
        assert!(validate_http_url("file:///tmp/a.zip").is_err());
        assert!(validate_http_url("javascript:alert(1)").is_err());
    }
}
