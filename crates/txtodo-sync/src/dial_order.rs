//! Which of a LAN peer's advertised addresses to dial, in what order (task
//! lan-dial-falls-to-relay, 2026-09-25). mDNS hands back everything a host has: IPv4, global and
//! temporary IPv6, and link-local IPv6 with no interface named. A link-local address with no scope
//! id cannot be dialed at all, and the one real sighting that fell back to the relay had nothing
//! else but those and global IPv6. IPv4 goes first: it is what a home LAN routes without extra
//! setup. Loopback is kept only when it is all there is (a same-host peer seen before its real
//! interface address, `LanEndpoint::connect`'s doc).
//! Ref: <https://www.rfc-editor.org/rfc/rfc4291#section-2.5.6> (link-local unicast, fe80::/10),
//! <https://doc.rust-lang.org/std/net/struct.SocketAddrV6.html#method.scope_id>

use std::net::SocketAddr;

/// `addrs` to dial, best first, each once: IPv4, then IPv6, then loopback only when nothing else
/// is left. Link-local IPv6 without a scope id is dropped. Empty when none can be dialed.
pub(crate) fn dial_order(addrs: &[SocketAddr]) -> Vec<SocketAddr> {
    let mut usable: Vec<SocketAddr> = Vec::with_capacity(addrs.len());
    for addr in addrs {
        if !unscoped_link_local(addr) && !usable.contains(addr) {
            usable.push(*addr);
        }
    }
    let rank = |a: &SocketAddr| match a {
        _ if a.ip().is_loopback() => 2,
        SocketAddr::V4(_) => 0,
        SocketAddr::V6(_) => 1,
    };
    // A stable sort keeps the advertised order within each rank.
    usable.sort_by_key(rank);
    if usable.iter().any(|a| !a.ip().is_loopback()) {
        usable.retain(|a| !a.ip().is_loopback());
    }
    debug_assert!(usable.len() <= addrs.len());
    usable
}

/// `fe80::/10` with no interface named: nothing to route it through.
fn unscoped_link_local(addr: &SocketAddr) -> bool {
    match addr {
        SocketAddr::V6(v6) => v6.ip().segments()[0] & 0xffc0 == 0xfe80 && v6.scope_id() == 0,
        SocketAddr::V4(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::dial_order;
    use std::net::SocketAddr;

    fn addrs(list: &[&str]) -> Vec<SocketAddr> {
        list.iter()
            .map(|a| a.parse().unwrap_or_else(|e| panic!("{a}: {e}")))
            .collect()
    }

    #[test]
    fn ipv4_goes_first_and_unscoped_link_local_is_dropped() {
        let got = dial_order(&addrs(&[
            "[fe80::4dd:1bfe:db50:8070]:60322",
            "[2001:f40:9a4:7329:185e:8d92:bd7a:5e65]:60322",
            "192.168.100.11:60322",
            "[fe80::1%4]:60322",
            "192.168.100.11:60322",
        ]));
        assert_eq!(
            got,
            addrs(&[
                "192.168.100.11:60322",
                "[2001:f40:9a4:7329:185e:8d92:bd7a:5e65]:60322",
                "[fe80::1%4]:60322",
            ])
        );
    }

    #[test]
    fn loopback_only_when_nothing_else_and_nothing_when_all_are_unscoped() {
        assert_eq!(
            dial_order(&addrs(&["127.0.0.1:9", "10.0.0.2:9"])),
            addrs(&["10.0.0.2:9"])
        );
        assert_eq!(
            dial_order(&addrs(&["127.0.0.1:9"])),
            addrs(&["127.0.0.1:9"])
        );
        assert!(dial_order(&addrs(&["[fe80::1]:9"])).is_empty());
    }
}
