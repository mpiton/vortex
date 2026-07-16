use std::net::{Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::sync::OnceLock;

const PREFIX_LENGTHS: [u8; 6] = [32, 40, 48, 56, 64, 96];
const WELL_KNOWN_IPV4: [Ipv4Addr; 2] =
    [Ipv4Addr::new(192, 0, 0, 170), Ipv4Addr::new(192, 0, 0, 171)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Nat64Prefix {
    network: u128,
    length: u8,
}

impl Nat64Prefix {
    pub(super) fn new(address: Ipv6Addr, length: u8) -> Option<Self> {
        if !PREFIX_LENGTHS.contains(&length) {
            return None;
        }
        Some(Self {
            network: u128::from(address) & prefix_mask(length),
            length,
        })
    }

    pub(super) fn embedded_ipv4(self, address: Ipv6Addr) -> Option<Ipv4Addr> {
        if u128::from(address) & prefix_mask(self.length) != self.network {
            return None;
        }
        let bytes = address.octets();
        if self.length < 96 && bytes[8] != 0 {
            return None;
        }
        let octets = match self.length {
            32 => [bytes[4], bytes[5], bytes[6], bytes[7]],
            40 => [bytes[5], bytes[6], bytes[7], bytes[9]],
            48 => [bytes[6], bytes[7], bytes[9], bytes[10]],
            56 => [bytes[7], bytes[9], bytes[10], bytes[11]],
            64 => [bytes[9], bytes[10], bytes[11], bytes[12]],
            96 => [bytes[12], bytes[13], bytes[14], bytes[15]],
            _ => return None,
        };
        Some(Ipv4Addr::from(octets))
    }
}

pub(super) fn discovered_prefixes() -> &'static [Nat64Prefix] {
    static PREFIXES: OnceLock<Vec<Nat64Prefix>> = OnceLock::new();
    PREFIXES.get_or_init(|| {
        let known = Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0);
        let mut prefixes = Nat64Prefix::new(known, 96).into_iter().collect();
        if let Ok(addresses) = ("ipv4only.arpa", 80).to_socket_addrs() {
            for address in addresses.filter_map(|address| match address.ip() {
                std::net::IpAddr::V6(ip) => Some(ip),
                std::net::IpAddr::V4(_) => None,
            }) {
                discover_from_address(address, &mut prefixes);
            }
        }
        prefixes
    })
}

fn discover_from_address(address: Ipv6Addr, prefixes: &mut Vec<Nat64Prefix>) {
    for length in PREFIX_LENGTHS {
        let Some(prefix) = Nat64Prefix::new(address, length) else {
            continue;
        };
        if prefix
            .embedded_ipv4(address)
            .is_some_and(|ipv4| WELL_KNOWN_IPV4.contains(&ipv4))
            && !prefixes.contains(&prefix)
        {
            prefixes.push(prefix);
        }
    }
}

fn prefix_mask(length: u8) -> u128 {
    u128::MAX << (128 - length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_all_rfc_6052_layouts() {
        for (address, length) in [
            ("2001:db8:c000:221::", 32),
            ("2001:db8:1c0:2:21::", 40),
            ("2001:db8:122:c000:2:2100::", 48),
            ("2001:db8:122:3c0:0:221::", 56),
            ("2001:db8:122:344:c0:2:2100::", 64),
            ("2001:db8:122:344::c000:221", 96),
        ] {
            let address = address.parse().unwrap();
            let prefix = Nat64Prefix::new(address, length).unwrap();
            assert_eq!(
                prefix.embedded_ipv4(address),
                Some(Ipv4Addr::new(192, 0, 2, 33))
            );
        }
    }

    #[test]
    fn accepts_96_prefixes_with_nonzero_fifth_segment() {
        let address: Ipv6Addr = "2606:4700:64:1:ab00:cd00:c0a8:1".parse().unwrap();
        let prefix = Nat64Prefix::new(address, 96).expect("valid /96 prefix");

        assert_eq!(
            prefix.embedded_ipv4(address),
            Some(Ipv4Addr::new(192, 168, 0, 1))
        );
    }
}
