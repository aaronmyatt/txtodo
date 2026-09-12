//! `_txtodo._udp` mDNS discovery (plan M4 `sync-lan-transport`): advertises this device on the LAN
//! and finds others in the same sync group. `mdns-sd` runs its own thread and hands events back
//! over a channel — no async runtime of its own. Docs: <https://docs.rs/mdns-sd>.

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;

use mdns_sd::{Receiver, ServiceDaemon, ServiceEvent, ServiceInfo, TxtProperties};
use txtodo_model::{DeviceId, Ulid};

use crate::frame::PROTOCOL_VERSION;
use crate::message::GroupId;

/// RFC 6763 service type this build advertises and browses. One type for the whole protocol: the
/// wire is UDP (iroh QUIC), so there is no `_tcp` counterpart to keep in sync.
pub const SERVICE_TYPE: &str = "_txtodo._udp.local.";

/// TXT record keys. Never the group *key* — only its id — so a café-wifi eavesdropper learns that
/// two machines share a todo group, not what's in it (task notes: accepted for M4, revisit at M8).
pub const TXT_DEVICE: &str = "device";
/// TXT record key for the group id (never the group key itself — see [`TXT_DEVICE`]'s doc).
pub const TXT_GROUP: &str = "group";
/// TXT record key for the protocol version ([`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION)).
pub const TXT_PROTO: &str = "proto";

/// A peer table larger than this means something worth investigating on this LAN, not silent
/// unbounded growth.
pub const MAX_LAN_PEERS: usize = 100;

/// A handshake that keeps failing backs off, capped here rather than retried tightly forever.
pub const MAX_BACKOFF_MS: u64 = 60_000;

/// Re-announcements inside this window are the same sighting, not a new peer worth re-dialing.
pub const DEBOUNCE_MS: u64 = 2_000;

/// One device's advertised presence, parsed out of a resolved service's TXT record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Announcement {
    /// The advertising device.
    pub device: DeviceId,
    /// The sync group it claims to belong to.
    pub group: GroupId,
    /// The protocol version it speaks.
    pub proto: u16,
}

/// Why a resolved service's TXT record was not a usable [`Announcement`] — some other program
/// advertising under `_txtodo._udp` fails here rather than being treated as a peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnouncementError {
    /// The named TXT field was absent entirely.
    MissingField(&'static str),
    /// The `device` field was not a parseable ULID.
    MalformedDevice(String),
    /// The `group` field was not 32 hex digits.
    MalformedGroup(String),
    /// The `proto` field was not a `u16`.
    MalformedProto(String),
}

impl fmt::Display for AnnouncementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnnouncementError::MissingField(field) => write!(f, "TXT record missing {field}"),
            AnnouncementError::MalformedDevice(v) => write!(f, "TXT device {v:?} is not a ULID"),
            AnnouncementError::MalformedGroup(v) => {
                write!(f, "TXT group {v:?} is not 32 hex digits")
            }
            AnnouncementError::MalformedProto(v) => write!(f, "TXT proto {v:?} is not a u16"),
        }
    }
}

impl std::error::Error for AnnouncementError {}

/// Parses the TXT fields [`Discovery::start`] publishes. Never panics on foreign input: an
/// unparseable field is a typed error naming which one, consuming nothing.
pub fn parse_announcement(txt: &TxtProperties) -> Result<Announcement, AnnouncementError> {
    let device_str = txt
        .get_property_val_str(TXT_DEVICE)
        .ok_or(AnnouncementError::MissingField(TXT_DEVICE))?;
    let group_str = txt
        .get_property_val_str(TXT_GROUP)
        .ok_or(AnnouncementError::MissingField(TXT_GROUP))?;
    let proto_str = txt
        .get_property_val_str(TXT_PROTO)
        .ok_or(AnnouncementError::MissingField(TXT_PROTO))?;

    let device = Ulid::parse(device_str)
        .map(DeviceId::new)
        .ok_or_else(|| AnnouncementError::MalformedDevice(device_str.to_string()))?;
    let group = u128::from_str_radix(group_str, 16)
        .map(GroupId)
        .map_err(|_| AnnouncementError::MalformedGroup(group_str.to_string()))?;
    let proto = proto_str
        .parse::<u16>()
        .map_err(|_| AnnouncementError::MalformedProto(proto_str.to_string()))?;

    Ok(Announcement {
        device,
        group,
        proto,
    })
}

/// A peer discovered on the LAN, already filtered to our own group and protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredPeer {
    /// The peer's device id.
    pub device: DeviceId,
    /// Every address it advertised, in the order mDNS returned them.
    pub addresses: Vec<SocketAddr>,
}

/// Why an observed announcement produced no [`DiscoveredPeer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ignored {
    /// Our own advertisement, echoed back by the network.
    OurOwnAdvertisement,
    /// A device in a different sync group.
    ForeignGroup(GroupId),
    /// A device speaking a protocol version we don't.
    UnsupportedProtocol(u16),
    /// Seen again inside [`DEBOUNCE_MS`] of the last sighting.
    Debounced,
    /// [`MAX_LAN_PEERS`] is already full and this is not a peer we already track.
    TableFull,
}

/// One [`PeerTable::observe`] outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerEvent {
    /// A new sighting of a peer worth tracking.
    Found(DiscoveredPeer),
    /// Filtered out, and why.
    Ignored(Ignored),
}

/// The pure decision core behind discovery: self/foreign-group filtering, debounce, and the
/// `MAX_LAN_PEERS` bound. No sockets, no clock — `now_ms` is the caller's, so tests never sleep.
pub struct PeerTable {
    own_device: DeviceId,
    own_group: GroupId,
    max_peers: usize,
    debounce_ms: u64,
    last_seen_ms: BTreeMap<DeviceId, u64>,
}

impl PeerTable {
    /// [`MAX_LAN_PEERS`] and [`DEBOUNCE_MS`] as the bounds.
    pub fn new(own_device: DeviceId, own_group: GroupId) -> PeerTable {
        PeerTable::with_limits(own_device, own_group, MAX_LAN_PEERS, DEBOUNCE_MS)
    }

    /// [`PeerTable::new`], with caller-chosen bounds (tests use small ones so `TableFull` doesn't
    /// need 100 fixtures to exercise).
    pub fn with_limits(
        own_device: DeviceId,
        own_group: GroupId,
        max_peers: usize,
        debounce_ms: u64,
    ) -> PeerTable {
        PeerTable {
            own_device,
            own_group,
            max_peers,
            debounce_ms,
            last_seen_ms: BTreeMap::new(),
        }
    }

    /// How many distinct peers are currently tracked (survived every filter at least once).
    pub fn len(&self) -> usize {
        self.last_seen_ms.len()
    }

    /// `len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.last_seen_ms.is_empty()
    }

    /// Applies self/foreign-group/protocol filtering, then debounce, then the table bound, in that
    /// order — a foreign-group peer is refused before it could ever consume a table slot.
    pub fn observe(
        &mut self,
        announcement: Announcement,
        addresses: Vec<SocketAddr>,
        now_ms: u64,
    ) -> PeerEvent {
        if announcement.device == self.own_device {
            return PeerEvent::Ignored(Ignored::OurOwnAdvertisement);
        }
        if announcement.group != self.own_group {
            return PeerEvent::Ignored(Ignored::ForeignGroup(announcement.group));
        }
        if announcement.proto != PROTOCOL_VERSION {
            return PeerEvent::Ignored(Ignored::UnsupportedProtocol(announcement.proto));
        }
        match self.last_seen_ms.get(&announcement.device) {
            Some(&last) if now_ms.saturating_sub(last) < self.debounce_ms => {
                PeerEvent::Ignored(Ignored::Debounced)
            }
            Some(_) => {
                self.last_seen_ms.insert(announcement.device, now_ms);
                PeerEvent::Found(DiscoveredPeer {
                    device: announcement.device,
                    addresses,
                })
            }
            None if self.last_seen_ms.len() >= self.max_peers => {
                PeerEvent::Ignored(Ignored::TableFull)
            }
            None => {
                self.last_seen_ms.insert(announcement.device, now_ms);
                PeerEvent::Found(DiscoveredPeer {
                    device: announcement.device,
                    addresses,
                })
            }
        }
    }

    /// Drops a peer whose advertisement was removed (mDNS goodbye or TTL expiry).
    pub fn remove(&mut self, device: DeviceId) {
        self.last_seen_ms.remove(&device);
    }
}

/// Bounded exponential backoff for a peer whose handshake keeps failing: `250ms * 2^attempt`,
/// capped at [`MAX_BACKOFF_MS`] rather than growing (or retrying) without limit.
pub fn backoff_ms(attempt: u32) -> u64 {
    const BASE_MS: u64 = 250;
    BASE_MS
        .saturating_mul(1u64 << attempt.min(20))
        .min(MAX_BACKOFF_MS)
}

/// Failure talking to the local mDNS daemon — never a peer's fault, always this host's.
#[derive(Debug)]
pub struct DiscoveryError(mdns_sd::Error);

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mDNS discovery: {}", self.0)
    }
}

impl std::error::Error for DiscoveryError {}

/// Owns the mDNS daemon: advertises this device under [`SERVICE_TYPE`] and can browse for others.
/// The daemon runs its own thread; [`browse`](Discovery::browse) hands back its event channel
/// as-is; turning those events into [`PeerEvent`]s through a [`PeerTable`] is the caller's job, so
/// the caller can inject `now_ms` for tests rather than a real clock living in here.
pub struct Discovery {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Discovery {
    /// Starts the local daemon and registers this device's advertisement. `host_name` is a label
    /// only (mDNS requires one) — `ServiceInfo::enable_addr_auto` fills in every real interface
    /// address rather than us guessing which one a LAN peer can actually reach.
    pub fn start(
        device: DeviceId,
        group: GroupId,
        host_name: &str,
        port: u16,
    ) -> Result<Discovery, DiscoveryError> {
        let daemon = ServiceDaemon::new().map_err(DiscoveryError)?;
        let props: [(&str, String); 3] = [
            (TXT_DEVICE, device.to_string()),
            (TXT_GROUP, format!("{:032x}", group.0)),
            (TXT_PROTO, PROTOCOL_VERSION.to_string()),
        ];
        let instance_name = device.to_string();
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            host_name,
            "",
            port,
            &props[..],
        )
        .map_err(DiscoveryError)?
        .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon.register(info).map_err(DiscoveryError)?;
        Ok(Discovery { daemon, fullname })
    }

    /// This device's own advertised fullname — a browse result naming it is our own echo, filtered
    /// by [`PeerTable`] on device id rather than this string, but useful for logging.
    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    /// Starts browsing for other `_txtodo._udp` advertisers; each event is a raw [`ServiceEvent`],
    /// unfiltered — feed [`ServiceEvent::ServiceResolved`] payloads through [`parse_announcement`]
    /// and [`PeerTable::observe`].
    pub fn browse(&self) -> Result<Receiver<ServiceEvent>, DiscoveryError> {
        self.daemon.browse(SERVICE_TYPE).map_err(DiscoveryError)
    }

    /// Stops advertising and browsing; best-effort, since a daemon already gone has nothing to
    /// clean up.
    pub fn shutdown(self) {
        let _ = self.daemon.shutdown();
    }
}
