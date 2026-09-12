use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use mdns_sd::{IntoTxtProperties, ServiceDaemon, ServiceEvent};
use txtodo_model::{DeviceId, Ulid};

use crate::discovery::{
    Announcement, AnnouncementError, DEBOUNCE_MS, DiscoveredPeer, Discovery, Ignored,
    MAX_BACKOFF_MS, MAX_LAN_PEERS, PeerEvent, PeerTable, SERVICE_TYPE, TXT_DEVICE, TXT_GROUP,
    TXT_PROTO, backoff_ms, parse_announcement,
};
use crate::frame::PROTOCOL_VERSION;
use crate::message::GroupId;

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn addr() -> Vec<SocketAddr> {
    vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242)]
}

fn txt(device: DeviceId, group: GroupId, proto: u16) -> mdns_sd::TxtProperties {
    let props: [(&str, String); 3] = [
        (TXT_DEVICE, device.to_string()),
        (TXT_GROUP, format!("{:032x}", group.0)),
        (TXT_PROTO, proto.to_string()),
    ];
    (&props[..]).into_txt_properties()
}

#[test]
fn a_well_formed_txt_record_round_trips_through_parse_announcement() {
    let d = device(7);
    let g = GroupId(99);
    let announcement = parse_announcement(&txt(d, g, PROTOCOL_VERSION)).unwrap();
    assert_eq!(announcement.device, d);
    assert_eq!(announcement.group, g);
    assert_eq!(announcement.proto, PROTOCOL_VERSION);
}

#[test]
fn each_missing_field_is_named_not_a_generic_error() {
    let props: [(&str, String); 0] = [];
    let empty: mdns_sd::TxtProperties = (&props[..]).into_txt_properties();
    assert_eq!(
        parse_announcement(&empty),
        Err(AnnouncementError::MissingField(TXT_DEVICE))
    );
}

#[test]
fn a_malformed_device_group_or_proto_field_is_a_typed_error() {
    let bad_device: [(&str, String); 3] = [
        (TXT_DEVICE, "not-a-ulid".to_string()),
        (TXT_GROUP, "0".repeat(32)),
        (TXT_PROTO, "1".to_string()),
    ];
    assert!(matches!(
        parse_announcement(&(&bad_device[..]).into_txt_properties()),
        Err(AnnouncementError::MalformedDevice(_))
    ));

    let bad_group: [(&str, String); 3] = [
        (TXT_DEVICE, device(1).to_string()),
        (TXT_GROUP, "not-hex".to_string()),
        (TXT_PROTO, "1".to_string()),
    ];
    assert!(matches!(
        parse_announcement(&(&bad_group[..]).into_txt_properties()),
        Err(AnnouncementError::MalformedGroup(_))
    ));

    let bad_proto: [(&str, String); 3] = [
        (TXT_DEVICE, device(1).to_string()),
        (TXT_GROUP, "0".repeat(32)),
        (TXT_PROTO, "not-a-number".to_string()),
    ];
    assert!(matches!(
        parse_announcement(&(&bad_proto[..]).into_txt_properties()),
        Err(AnnouncementError::MalformedProto(_))
    ));
}

#[test]
fn our_own_advertisement_is_ignored() {
    let me = device(1);
    let group = GroupId(1);
    let mut table = PeerTable::new(me, group);
    let event = table.observe(
        Announcement {
            device: me,
            group,
            proto: PROTOCOL_VERSION,
        },
        addr(),
        1_000,
    );
    assert_eq!(event, PeerEvent::Ignored(Ignored::OurOwnAdvertisement));
    assert!(table.is_empty());
}

#[test]
fn a_peer_in_another_group_is_never_connected_to() {
    let me = device(1);
    let mut table = PeerTable::new(me, GroupId(1));
    let foreign = GroupId(2);
    let event = table.observe(
        Announcement {
            device: device(2),
            group: foreign,
            proto: PROTOCOL_VERSION,
        },
        addr(),
        1_000,
    );
    assert_eq!(event, PeerEvent::Ignored(Ignored::ForeignGroup(foreign)));
    assert!(table.is_empty());
}

#[test]
fn a_peer_speaking_a_different_protocol_version_is_ignored() {
    let me = device(1);
    let group = GroupId(1);
    let mut table = PeerTable::new(me, group);
    let event = table.observe(
        Announcement {
            device: device(2),
            group,
            proto: PROTOCOL_VERSION + 1,
        },
        addr(),
        1_000,
    );
    assert_eq!(
        event,
        PeerEvent::Ignored(Ignored::UnsupportedProtocol(PROTOCOL_VERSION + 1))
    );
}

#[test]
fn a_new_peer_in_our_group_is_found() {
    let me = device(1);
    let group = GroupId(1);
    let peer = device(2);
    let mut table = PeerTable::new(me, group);
    let event = table.observe(
        Announcement {
            device: peer,
            group,
            proto: PROTOCOL_VERSION,
        },
        addr(),
        1_000,
    );
    assert_eq!(
        event,
        PeerEvent::Found(DiscoveredPeer {
            device: peer,
            addresses: addr(),
        })
    );
    assert_eq!(table.len(), 1);
}

#[test]
fn a_reannouncement_inside_the_debounce_window_is_debounced() {
    let me = device(1);
    let group = GroupId(1);
    let peer = device(2);
    let mut table = PeerTable::with_limits(me, group, MAX_LAN_PEERS, 2_000);
    let announcement = Announcement {
        device: peer,
        group,
        proto: PROTOCOL_VERSION,
    };
    assert!(matches!(
        table.observe(announcement, addr(), 1_000),
        PeerEvent::Found(_)
    ));
    assert_eq!(
        table.observe(announcement, addr(), 1_999),
        PeerEvent::Ignored(Ignored::Debounced)
    );
    assert!(matches!(
        table.observe(announcement, addr(), 3_001),
        PeerEvent::Found(_)
    ));
}

#[test]
fn the_peer_past_max_lan_peers_is_dropped_with_a_named_reason_not_pushed() {
    let me = device(0);
    let group = GroupId(1);
    let mut table = PeerTable::with_limits(me, group, 2, DEBOUNCE_MS);
    for n in 1..=2 {
        let event = table.observe(
            Announcement {
                device: device(n),
                group,
                proto: PROTOCOL_VERSION,
            },
            addr(),
            1_000,
        );
        assert!(matches!(event, PeerEvent::Found(_)));
    }
    let event = table.observe(
        Announcement {
            device: device(3),
            group,
            proto: PROTOCOL_VERSION,
        },
        addr(),
        1_000,
    );
    assert_eq!(event, PeerEvent::Ignored(Ignored::TableFull));
    assert_eq!(table.len(), 2);
}

#[test]
fn a_removed_peer_can_be_found_again_without_waiting_out_the_debounce() {
    let me = device(0);
    let group = GroupId(1);
    let peer = device(1);
    let mut table = PeerTable::new(me, group);
    let announcement = Announcement {
        device: peer,
        group,
        proto: PROTOCOL_VERSION,
    };
    assert!(matches!(
        table.observe(announcement, addr(), 1_000),
        PeerEvent::Found(_)
    ));
    table.remove(peer);
    assert!(table.is_empty());
    assert!(matches!(
        table.observe(announcement, addr(), 1_001),
        PeerEvent::Found(_)
    ));
}

#[test]
fn backoff_grows_exponentially_and_never_exceeds_the_cap() {
    assert_eq!(backoff_ms(0), 250);
    assert_eq!(backoff_ms(1), 500);
    assert_eq!(backoff_ms(2), 1_000);
    assert_eq!(backoff_ms(30), MAX_BACKOFF_MS);
    assert_eq!(backoff_ms(u32::MAX), MAX_BACKOFF_MS);
}

/// Real `mdns-sd` daemons on loopback: one advertises under [`SERVICE_TYPE`] with our TXT shape,
/// the other browses and resolves it, driven off the callback with a bounded timeout rather than a
/// sleep. This is the one test in this file that touches the network for real.
#[test]
fn two_real_daemons_discover_each_other_on_the_lan() {
    let advertiser_device = device(1000);
    let group = GroupId(42);
    let advertiser = Discovery::start(advertiser_device, group, "discovery-test.local.", 4242)
        .expect("advertiser starts");

    let browser = ServiceDaemon::new().expect("browser daemon");
    let events = browser.browse(SERVICE_TYPE).expect("browse");

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut found = None;
    while std::time::Instant::now() < deadline && found.is_none() {
        if let Ok(ServiceEvent::ServiceResolved(resolved)) =
            events.recv_timeout(Duration::from_millis(200))
            && let Ok(announcement) = parse_announcement(resolved.get_properties())
            && announcement.device == advertiser_device
        {
            found = Some(announcement);
        }
    }

    advertiser.shutdown();
    let _ = browser.shutdown();

    let announcement = found.expect("the advertiser is resolved within the timeout");
    assert_eq!(announcement.group, group);
    assert_eq!(announcement.proto, PROTOCOL_VERSION);
}
