use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Spec section 72 (Crawler Security): block loopback, RFC1918 private
/// ranges, link-local (which covers the 169.254.169.254 cloud-metadata
/// case for free — it's a link-local address), and other
/// non-globally-routable ranges, unless the administrator has explicitly
/// opted in to private-network crawling.
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(v4: Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()
        // "This network" (0.0.0.0/8, minus the all-zero address already
        // covered by is_unspecified) and other reserved-but-not-yet-covered
        // low-value ranges.
        || v4.octets()[0] == 0
}

fn is_blocked_v6(v6: Ipv6Addr) -> bool {
    if let Some(mapped) = v6.to_ipv4_mapped() {
        return is_blocked_v4(mapped);
    }
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        || is_unique_local_v6(v6)
        || is_unicast_link_local_v6(v6)
}

/// fc00::/7 — IPv6 unique local addresses (the IPv6 analog of RFC1918).
fn is_unique_local_v6(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

/// fe80::/10 — IPv6 link-local addresses.
fn is_unicast_link_local_v6(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_loopback() {
        assert!(is_blocked_ip("127.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("::1".parse().unwrap()));
    }

    #[test]
    fn blocks_rfc1918_private_ranges() {
        assert!(is_blocked_ip("10.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("172.16.0.1".parse().unwrap()));
        assert!(is_blocked_ip("192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn blocks_cloud_metadata_address() {
        // AWS/GCP/Azure metadata service — a link-local address.
        assert!(is_blocked_ip("169.254.169.254".parse().unwrap()));
    }

    #[test]
    fn blocks_ipv6_unique_local_and_link_local() {
        assert!(is_blocked_ip("fc00::1".parse().unwrap()));
        assert!(is_blocked_ip("fd12:3456:789a::1".parse().unwrap()));
        assert!(is_blocked_ip("fe80::1".parse().unwrap()));
    }

    #[test]
    fn blocks_ipv4_mapped_private_address_in_ipv6() {
        // ::ffff:127.0.0.1 — a classic bypass attempt for naive
        // "only check if it's literally an Ipv4Addr" filters.
        assert!(is_blocked_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("::ffff:10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn allows_ordinary_public_addresses() {
        assert!(!is_blocked_ip("93.184.216.34".parse().unwrap())); // example.com-ish
        assert!(!is_blocked_ip("1.1.1.1".parse().unwrap()));
        assert!(!is_blocked_ip("2606:4700:4700::1111".parse().unwrap())); // cloudflare dns
    }

    #[test]
    fn blocks_multicast_and_broadcast() {
        assert!(is_blocked_ip("224.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("255.255.255.255".parse().unwrap()));
        assert!(is_blocked_ip("ff02::1".parse().unwrap()));
    }
}
