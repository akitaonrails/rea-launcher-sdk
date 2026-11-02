//! "Latest call wins" tickets, for timers that must not undo a newer
//! request (the rumble test's stop).

use std::sync::atomic::{AtomicU64, Ordering};

/// Hands out increasing tickets; only the newest one is current.
#[derive(Debug, Default)]
pub(crate) struct Latest(AtomicU64);

#[cfg_attr(not(windows), allow(dead_code))]
impl Latest {
    pub(crate) const fn new() -> Latest {
        Latest(AtomicU64::new(0))
    }

    /// A new ticket, which makes every earlier one stale.
    pub(crate) fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub(crate) fn is_current(&self, ticket: u64) -> bool {
        self.0.load(Ordering::SeqCst) == ticket
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_latest_ticket_is_current() {
        let latest = Latest::new();
        let first = latest.next();
        assert!(latest.is_current(first));
        let second = latest.next();
        assert!(
            !latest.is_current(first),
            "the first timer must not stop the second rumble"
        );
        assert!(latest.is_current(second));
        latest.next(); // an explicit stop or set
        assert!(!latest.is_current(second));
    }
}
