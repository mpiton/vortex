//! Network policy for URLs supplied by plugins rather than users.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::domain::error::DomainError;

use super::nat64::{Nat64Prefix, discovered_prefixes};

pub(crate) fn validate_public_url(
    url: &reqwest::Url,
) -> Result<Option<Vec<SocketAddr>>, DomainError> {
    if url.scheme() != "https" {
        return Err(DomainError::NetworkError(
            "plugin URL must use HTTPS".into(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| DomainError::NetworkError("URL has no host".into()))?;
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(blocked());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_forbidden_ip(&ip) {
            Err(blocked())
        } else {
            Ok(None)
        };
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| DomainError::NetworkError("URL has no known port".into()))?;
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| DomainError::NetworkError("host resolution failed".into()))?
        .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|addr| is_forbidden_ip(&addr.ip())) {
        return Err(blocked());
    }
    Ok(Some(addresses))
}

pub(crate) fn restricted_download_client(
    url: &reqwest::Url,
    request_headers: &[(String, String)],
) -> Result<reqwest::Client, DomainError> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(DomainError::NetworkError(
            "plugin download URL must be credential-free HTTPS".into(),
        ));
    }
    let addresses = validate_public_url(url)?;
    let headers = validated_plugin_headers(request_headers)?;
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .user_agent("Vortex/0.1")
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(3600))
        .default_headers(headers);
    if let (Some(host), Some(addresses)) = (url.host_str(), addresses.as_deref()) {
        builder = builder.resolve_to_addrs(host, addresses);
    }
    builder
        .build()
        .map_err(|_| DomainError::NetworkError("restricted HTTP client creation failed".into()))
}

pub(crate) fn validated_plugin_headers(
    request_headers: &[(String, String)],
) -> Result<HeaderMap, DomainError> {
    let mut headers = HeaderMap::new();
    for (name, value) in request_headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| DomainError::NetworkError("plugin returned an invalid header".into()))?;
        if !matches!(
            name.as_str(),
            "accept"
                | "accept-language"
                | "authorization"
                | "cookie"
                | "origin"
                | "referer"
                | "user-agent"
        ) {
            return Err(DomainError::NetworkError(
                "plugin returned a disallowed download header".into(),
            ));
        }
        let value = HeaderValue::from_str(value)
            .map_err(|_| DomainError::NetworkError("plugin returned an invalid header".into()))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

fn blocked() -> DomainError {
    DomainError::NetworkError("plugin URL targets a non-public network".into())
}

pub(crate) fn is_forbidden_ip(ip: &IpAddr) -> bool {
    let normalized = match ip {
        IpAddr::V6(ip) if ip.to_ipv4_mapped().is_some() => IpAddr::V4(
            ip.to_ipv4_mapped()
                .unwrap_or(std::net::Ipv4Addr::UNSPECIFIED),
        ),
        other => *other,
    };
    match normalized {
        IpAddr::V4(ip) => {
            let [a, b, c, d] = ip.octets();
            a == 0
                || a == 10
                || a == 127
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 168)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || a >= 224
                || (a == 255 && b == 255 && c == 255 && d == 255)
        }
        IpAddr::V6(ip) => is_forbidden_ipv6(&ip, discovered_prefixes()),
    }
}

fn is_forbidden_ipv6(ip: &std::net::Ipv6Addr, nat64: &[Nat64Prefix]) -> bool {
    let segments = ip.segments();
    let first = segments[0];
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first & 0xffc0) == 0xfec0
        || matches!(segments, [0x0064, 0xff9b, 0x0001, ..])
        || matches!(segments, [0x0100, 0, 0, 0, ..])
        || is_orchid(&segments)
        || matches!(segments, [0x2001, 0x0002, 0, ..])
        || matches!(segments, [0x2001, 0x0db8, ..])
        || matches!(segments, [0x2002, ..])
        || matches!(segments, [0x3ff0..=0x3fff, ..])
        || matches!(segments, [0x5f00, ..])
        || nat64.iter().any(|prefix| {
            prefix
                .embedded_ipv4(*ip)
                .is_some_and(|ipv4| is_forbidden_ip(&IpAddr::V4(ipv4)))
        })
}

fn is_orchid(segments: &[u16; 8]) -> bool {
    segments[0] == 0x2001 && matches!(segments[1] & 0xfff0, 0x0010 | 0x0020 | 0x0030)
}

#[cfg(test)]
#[path = "safe_url_tests.rs"]
mod tests;
