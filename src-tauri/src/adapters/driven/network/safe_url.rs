//! Network policy for URLs supplied by plugins rather than users.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use crate::domain::error::DomainError;

pub(crate) fn validate_public_url(
    url: &reqwest::Url,
) -> Result<Option<Vec<SocketAddr>>, DomainError> {
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
) -> Result<reqwest::Client, DomainError> {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(DomainError::NetworkError(
            "plugin download URL must be credential-free HTTPS".into(),
        ));
    }
    let addresses = validate_public_url(url)?;
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .user_agent("Vortex/0.1")
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(3600));
    if let (Some(host), Some(addresses)) = (url.host_str(), addresses.as_deref()) {
        builder = builder.resolve_to_addrs(host, addresses);
    }
    builder
        .build()
        .map_err(|_| DomainError::NetworkError("restricted HTTP client creation failed".into()))
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
        IpAddr::V6(ip) => {
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
                || matches!(segments, [0x2001, 0x0002, 0, ..])
                || matches!(segments, [0x2001, 0x0db8, ..])
                || matches!(segments, [0x2002, ..])
                || matches!(segments, [0x3ff0..=0x3fff, ..])
                || matches!(segments, [0x5f00, ..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_loopback_private_link_local_and_mapped_addresses() {
        for raw in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "::ffff:127.0.0.1",
            "fec0::1",
            "64:ff9b::c0a8:1",
            "64:ff9b:1::c0a8:1",
            "2001:10::1",
            "2001:20::1",
        ] {
            assert!(is_forbidden_ip(&raw.parse().unwrap()), "{raw}");
        }
    }

    #[test]
    fn rejects_private_ipv4_embedded_in_discovered_operator_nat64_prefix() {
        let prefix = Nat64Prefix::new("2606:4700:64::".parse().unwrap(), 96).unwrap();
        let target = "2606:4700:64::c0a8:1".parse().unwrap();

        assert!(is_forbidden_ipv6(&target, &[prefix]));
    }

    #[test]
    fn accepts_globally_routable_ipv6_address() {
        assert!(!is_forbidden_ip(&"2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn restricted_client_requires_https_and_public_destination() {
        let http = reqwest::Url::parse("http://1.1.1.1/file").unwrap();
        let local = reqwest::Url::parse("https://127.0.0.1/file").unwrap();
        assert!(restricted_download_client(&http).is_err());
        assert!(restricted_download_client(&local).is_err());
    }
}
