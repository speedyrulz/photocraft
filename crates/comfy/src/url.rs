//! The address of a ComfyUI server, as a person types it.

use crate::{ComfyError, Result};

/// The address a local ComfyUI listens on by default.
pub const DEFAULT_URL: &str = "http://127.0.0.1:8188";

/// `raw` as a base URL with a scheme and no trailing slash: `192.168.1.5:8188` becomes
/// `http://192.168.1.5:8188`. A path is kept (a server behind a reverse proxy); credentials, a
/// query, a fragment and any scheme but http and https are refused.
pub fn normalize_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let bad = |why: &str| ComfyError::Param(format!("“{}” is not a ComfyUI address: {why}", trimmed.chars().take(80).collect::<String>()));
    if trimmed.is_empty() {
        return Err(ComfyError::Param("enter the address of a ComfyUI server, such as http://127.0.0.1:8188".into()));
    }
    if trimmed.len() > 2048 {
        return Err(ComfyError::Param("the ComfyUI address is too long".into()));
    }
    let url = if trimmed.contains("://") { trimmed.to_string() } else { format!("http://{trimmed}") };
    let (scheme, rest) = url.split_once("://").ok_or_else(|| bad("it has no host"))?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(bad("it must start with http:// or https://"));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) || rest.contains(['@', '?', '#', '\\']) {
        return Err(bad("use only a host, a port and an optional path"));
    }
    if host_of(rest).is_empty() {
        return Err(bad("it has no host"));
    }
    if let Some(port) = port_of(rest)
        && port.parse::<u16>().is_err()
    {
        return Err(bad("its port is not a number from 0 to 65535"));
    }
    Ok(url)
}

/// `rest`'s authority: what comes before the first `/`.
fn authority(rest: &str) -> &str {
    rest.split('/').next().unwrap_or(rest)
}

/// The host of `host[:port][/path]`, without an IPv6 address's brackets.
fn host_of(rest: &str) -> &str {
    let a = authority(rest);
    if let Some(v6) = a.strip_prefix('[') {
        return v6.split(']').next().unwrap_or("");
    }
    a.split(':').next().unwrap_or("")
}

/// The port of `host[:port][/path]`, when one is written.
fn port_of(rest: &str) -> Option<&str> {
    let a = authority(rest);
    match a.strip_prefix('[') {
        Some(v6) => v6.split_once(']')?.1.strip_prefix(':'),
        None => a.split_once(':').map(|(_, port)| port),
    }
}

/// The base URL's host and port: what a socket connects to.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn host_port(url: &str) -> Option<(String, u16)> {
    let (scheme, rest) = url.split_once("://")?;
    let port = match port_of(rest) {
        Some(p) => p.parse().ok()?,
        None if scheme.eq_ignore_ascii_case("https") => 443,
        None => 80,
    };
    Some((host_of(rest).to_string(), port))
}

/// Whether `raw` names this machine. Untrusted automation may only send a document to such a
/// server (see `authorize_engine_command`): anywhere else it could carry the image off.
pub fn is_loopback_url(raw: &str) -> bool {
    let Ok(url) = normalize_url(raw) else { return false };
    let Some((_, rest)) = url.split_once("://") else { return false };
    let host = host_of(rest);
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_completed_and_tidied() {
        assert_eq!(normalize_url("127.0.0.1:8188").unwrap(), "http://127.0.0.1:8188");
        assert_eq!(normalize_url(" http://gpu-box:8188/ ").unwrap(), "http://gpu-box:8188");
        assert_eq!(normalize_url("https://example.com/comfy/").unwrap(), "https://example.com/comfy");
        assert_eq!(normalize_url("http://[::1]:8188").unwrap(), "http://[::1]:8188");
    }

    #[test]
    fn unusable_addresses_are_errors_not_panics() {
        for bad in [
            "",
            "   ",
            "ftp://host",
            "file:///etc/passwd",
            "http://",
            "http://user:pw@host",
            "http://host:99999",
            "http://host:port",
            "http://ho st",
            "http://host?x=1",
            "://",
            "http://[",
            "http://[]",
            "é://\u{0}",
        ] {
            assert!(normalize_url(bad).is_err(), "{bad:?}");
        }
        assert!(normalize_url(&"a".repeat(5000)).is_err());
    }

    #[test]
    fn host_and_port_are_read_back() {
        assert_eq!(host_port("http://127.0.0.1:8188").unwrap(), ("127.0.0.1".to_string(), 8188));
        assert_eq!(host_port("http://[::1]:9000/x").unwrap(), ("::1".to_string(), 9000));
        assert_eq!(host_port("https://example.com/comfy").unwrap(), ("example.com".to_string(), 443));
        assert_eq!(host_port("http://example.com").unwrap(), ("example.com".to_string(), 80));
    }

    #[test]
    fn only_this_machine_counts_as_loopback() {
        for local in ["127.0.0.1:8188", "http://localhost:8188", "http://LOCALHOST", "http://127.8.8.8", "http://[::1]:8188"] {
            assert!(is_loopback_url(local), "{local}");
        }
        for remote in ["http://192.168.1.5:8188", "http://example.com", "http://127.0.0.1.example.com", "http://localhost.example.com", "garbage://x", ""] {
            assert!(!is_loopback_url(remote), "{remote}");
        }
    }
}
