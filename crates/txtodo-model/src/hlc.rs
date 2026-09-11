//! Hybrid logical clock. Ref: Kulkarni et al., "Logical Physical Clocks and Consistent Snapshots"
//! <https://cse.buffalo.edu/tech-reports/2014-04.pdf>. Total order `(wall_ms, counter, device)`;
//! `tick` never goes backwards even when the wall clock does. The 5-minute skew guard is M4.

use crate::DeviceId;
use serde::{Deserialize, Serialize};

/// One HLC stamp. Derived `Ord` is field order, which is the HLC order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hlc {
    /// Physical component, Unix milliseconds.
    pub wall_ms: u64,
    /// Logical component; breaks ties within one millisecond.
    pub counter: u16,
    /// Tie-breaker across devices.
    pub device: DeviceId,
}

/// The only failure: more than `u16::MAX` ops in one millisecond on one device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HlcError {
    /// The wall time that overflowed.
    pub wall_ms: u64,
}

impl core::fmt::Display for HlcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "hlc counter overflow at wall_ms {}", self.wall_ms)
    }
}

impl std::error::Error for HlcError {}

impl Hlc {
    /// The zero stamp for a device; every `tick` is greater than this.
    pub const fn zero(device: DeviceId) -> Hlc {
        Hlc {
            wall_ms: 0,
            counter: 0,
            device,
        }
    }

    /// Advances to a stamp strictly greater than `self`, using `now_ms` when it is ahead. The
    /// receiving (merge) rule is M4; on one device only the send rule is needed.
    pub fn tick(&mut self, now_ms: u64) -> Result<Hlc, HlcError> {
        let before = *self;
        if now_ms > self.wall_ms {
            self.wall_ms = now_ms;
            self.counter = 0;
        } else {
            self.counter = self.counter.checked_add(1).ok_or(HlcError {
                wall_ms: self.wall_ms,
            })?;
        }
        debug_assert!(*self > before, "tick is strictly monotone");
        debug_assert_eq!(self.device, before.device, "tick never changes the device");
        Ok(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use txtodo_core::Ulid;

    fn dev(n: u128) -> DeviceId {
        DeviceId::new(Ulid::from_u128(n))
    }

    #[test]
    fn tick_follows_the_wall_forward_and_counts_when_it_stalls_or_regresses() {
        let mut hlc = Hlc::zero(dev(1));
        assert_eq!(
            hlc.tick(1_000).unwrap(),
            Hlc {
                wall_ms: 1_000,
                counter: 0,
                device: dev(1)
            }
        );
        assert_eq!(hlc.tick(1_000).unwrap().counter, 1);
        assert_eq!(
            hlc.tick(900).unwrap(),
            Hlc {
                wall_ms: 1_000,
                counter: 2,
                device: dev(1)
            }
        );
        assert_eq!(
            hlc.tick(1_001).unwrap(),
            Hlc {
                wall_ms: 1_001,
                counter: 0,
                device: dev(1)
            }
        );
    }

    #[test]
    fn counter_overflow_is_an_error_not_a_wrap() {
        let mut hlc = Hlc {
            wall_ms: 5,
            counter: u16::MAX,
            device: dev(1),
        };
        assert_eq!(hlc.tick(5), Err(HlcError { wall_ms: 5 }));
        assert_eq!(
            hlc.counter,
            u16::MAX,
            "a failed tick leaves the clock unchanged"
        );
    }

    proptest! {
        #[test]
        fn ticks_are_strictly_increasing(nows in proptest::collection::vec(0u64..10, 1..200)) {
            let mut hlc = Hlc::zero(dev(7));
            let mut prev = hlc;
            for now in nows {
                let next = hlc.tick(now).unwrap();
                prop_assert!(next > prev);
                prev = next;
            }
        }

        #[test]
        fn order_is_wall_then_counter_then_device(a: (u64, u16, u128), b: (u64, u16, u128)) {
            let x = Hlc { wall_ms: a.0, counter: a.1, device: dev(a.2) };
            let y = Hlc { wall_ms: b.0, counter: b.1, device: dev(b.2) };
            prop_assert_eq!(x.cmp(&y), (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
            let bytes = postcard::to_allocvec(&x).unwrap();
            prop_assert_eq!(postcard::from_bytes::<Hlc>(&bytes).unwrap(), x);
        }
    }
}
