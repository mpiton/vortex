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
