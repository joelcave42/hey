//! This PC's network addresses, which phones use to find it: on the home
//! network, and through Tailscale when they're away.

use std::net::Ipv4Addr;

/// Tailscale gives each device an address in 100.64.0.0/10.
pub fn is_tailscale(ip: &Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

/// Every IPv4 address a phone might reach this PC on: local network ones
/// first (fastest at home), then Tailscale.
pub fn addresses() -> Vec<String> {
    let mut ips: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_link_local() => Some(ip),
            _ => None,
        })
        .collect();
    ips.sort_by_key(is_tailscale);
    ips.dedup();
    ips.iter().map(Ipv4Addr::to_string).collect()
}

/// This PC's Tailscale address, if Tailscale is running.
pub fn tailscale_ip() -> Option<String> {
    addresses()
        .into_iter()
        .find(|ip| ip.parse().is_ok_and(|ip| is_tailscale(&ip)))
}

#[cfg(test)]
mod tests {
    use super::is_tailscale;

    #[test]
    fn spots_tailscale_addresses() {
        assert!(is_tailscale(&"100.101.102.103".parse().unwrap()));
        assert!(is_tailscale(&"100.64.0.1".parse().unwrap()));
        assert!(!is_tailscale(&"100.128.0.1".parse().unwrap()));
        assert!(!is_tailscale(&"172.16.5.4".parse().unwrap()));
        assert!(!is_tailscale(&"198.51.100.7".parse().unwrap()));
    }
}
