//! Debounce for map-block changes without a load (`[DEBUG] block_change_grace_ms`).
//!
//! Walking along the border of two map blocks flips `game::current_block()` (ours) or the
//! friend's snapshot block back and forth within a second (live 2026-11-12, client
//! `ours Some(3), was Some(2)` four times in 15 s). A real load is handled elsewhere
//! (puppet.rs `invalidate_world` -> `nsession::on_world_reset`, `Native::forget`), so a block
//! difference alone may wait a little before the friend's native remote is deleted
//! (native.rs `wants_for`) or our native session engagement goes (nsession.rs `tick`).

use std::time::{Duration, Instant};

/// What one frame of the debounce saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The blocks agree and no grace runs.
    Same,
    /// The blocks differ since this frame: the grace starts.
    Started,
    /// The blocks still differ, this long so far (below the grace).
    Holding(Duration),
    /// The blocks differed this long, at least the grace: act now. The timer keeps running
    /// (the caller acts once and clears it, or stops calling).
    Expired(Duration),
    /// The blocks agree again after this long: the grace ends without action.
    Cancelled(Duration),
}

/// One frame: `differs` = the two blocks differ now; `since` = when they started to differ.
/// A zero grace expires in the first frame (the old immediate behaviour).
pub fn step(since: &mut Option<Instant>, differs: bool, now: Instant, grace: Duration) -> Step {
    match (differs, *since) {
        (false, None) => Step::Same,
        (false, Some(t)) => {
            *since = None;
            Step::Cancelled(now.saturating_duration_since(t))
        }
        (true, None) => {
            *since = Some(now);
            if grace.is_zero() { Step::Expired(Duration::ZERO) } else { Step::Started }
        }
        (true, Some(t)) => {
            let held = now.saturating_duration_since(t);
            if held >= grace { Step::Expired(held) } else { Step::Holding(held) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Duration = Duration::from_millis(2500);

    #[test]
    fn border_flicker_never_expires() {
        let t0 = Instant::now();
        let mut since = None;
        assert_eq!(step(&mut since, false, t0, G), Step::Same);
        assert_eq!(step(&mut since, true, t0, G), Step::Started);
        assert_eq!(step(&mut since, true, t0 + Duration::from_millis(900), G), Step::Holding(Duration::from_millis(900)));
        assert_eq!(step(&mut since, false, t0 + Duration::from_millis(1200), G), Step::Cancelled(Duration::from_millis(1200)));
        assert_eq!(since, None);
        // The next crossing starts a fresh grace.
        let t1 = t0 + Duration::from_millis(1500);
        assert_eq!(step(&mut since, true, t1, G), Step::Started);
        assert_eq!(step(&mut since, true, t1 + Duration::from_millis(2400), G), Step::Holding(Duration::from_millis(2400)));
        assert_eq!(step(&mut since, false, t1 + Duration::from_millis(2450), G), Step::Cancelled(Duration::from_millis(2450)));
    }

    #[test]
    fn a_lasting_change_expires_after_the_grace() {
        let t0 = Instant::now();
        let mut since = None;
        assert_eq!(step(&mut since, true, t0, G), Step::Started);
        assert_eq!(step(&mut since, true, t0 + G, G), Step::Expired(G));
        // Still running until the caller clears it.
        assert_eq!(step(&mut since, true, t0 + G + Duration::from_millis(5), G), Step::Expired(G + Duration::from_millis(5)));
    }

    #[test]
    fn zero_grace_is_the_old_immediate_rule() {
        let t0 = Instant::now();
        let mut since = None;
        assert_eq!(step(&mut since, true, t0, Duration::ZERO), Step::Expired(Duration::ZERO));
    }
}
