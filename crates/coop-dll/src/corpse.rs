//! Dead players are not enemy targets (`[GAMEPLAY] dead_players_untargetable`, default 1;
//! docs/RE-CORPSE-TARGET.md). NOT verified live.
//!
//! Bug (since upstream 1.0.9): one player dies in a fight, the other lives, and the enemies
//! keep on the corpse. In single player a death always reloads, so the game never had to
//! drop a dead player for good. Two checks the game already has do it here, each on the
//! machine whose enemies run their AI there (every machine runs some enemies' AI; NetAI
//! copies follow the owner's choice) [VAR, Ghidra on the 2026-11-11 dump, E:\RE\work_corpse]:
//! - Our own dead player: the perception record of the main player (`[[0x143D7A1E0]+0x88]`)
//!   copies the static byte `0x143D7A36F` into `+0xC3` (`0xBD3E40`), and both perception
//!   tests (`0x617280` new candidates, `0x6172F0` known ones) return "not perceived" while it
//!   is set: no sight, no hearing, so no new awareness and the old one decays (`0x612400`),
//!   as with a hidden living player. Only `0xBD3E40` reads the byte; the debug menu
//!   (`0xA52B80`) registers it. We set it while our player is dead and another player lives,
//!   and clear it otherwise.
//! - A dead friend's native remote: the AI's candidate filter `0x62BF30` keeps a chr only if
//!   `0xC09660(selfTeam, team(chr))` (M2 `0x143B0CAA0`) is hostile, else friendly. Team 0
//!   ("Disabled") is neither for every row (M2 column 0 all not-hostile, M1 column 0 the
//!   "no relation" functor), so the corpse leaves the search; lock-on and the idol's enemy
//!   finder use the same matrix. The copy's HP 0 already drops its awareness entries
//!   (`0x612320` -> `0xBD5D40`, HP <= 0). Team back when the friend lives again.
//!
//! Each machine decides from what it already knows (own HP, the friend's snapshot HP): no
//! protocol change, local setting. Both dead (a wipe): nothing changes against before.
//! Fail-safe: anything unknown (no link, no fresh snapshot, a load, feature off, shutdown,
//! a frame panic) restores the game's own state.
//!
//! Friendly fire (pvp.rs) writes team 28 on the same remote: while we hold it, pvp leaves
//! the team alone (`holds`); our restore writes the chr type's team 2 and pvp sets 28 again
//! on its next frame if it is still on.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::game::{Chr, module_base, read, write};
use crate::peers::{PeerId, PeerSet};
use crate::{log, native};

/// `[GAMEPLAY] dead_players_untargetable` (default on).
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// The main player's "not perceived" byte (`0xBD3E40` -> perception record `+0xC3`).
const HIDE_FLAG_RVA: usize = 0x3D7A36F;
const CHR_TEAM: usize = 0x74;
/// Team "Disabled": not hostile and not friendly to any team (M2 column 0).
pub const HIDDEN_TEAM: u32 = 0;
/// The chr type's team of a native remote player (`0x9F5170(chr, 1)` -> `0x9F64D0`).
const REMOTE_TEAM: u32 = 2;
/// pvp.rs "Coop NPC" (friendly fire); pvp writes it again itself.
const PVP_TEAM: u32 = 28;

// ===================================================================== pure, unit-tested

/// Our own dead player is hidden from the enemies on this machine.
pub fn hide_local(enabled: bool, connected: bool, own_dead: bool, other_alive: bool) -> bool {
    enabled && connected && own_dead && other_alive
}

/// A dead peer's native remote leaves the enemies' search here. `peer_dead` = its latest
/// visible snapshot says HP <= 0 (no snapshot = not dead: fail-safe); `other_alive` = we or
/// another peer live.
pub fn hide_replica(enabled: bool, connected: bool, peer_dead: bool, other_alive: bool) -> bool {
    enabled && connected && peer_dead && other_alive
}

/// The team to put back: the remote's own, never our hidden team or pvp's (pvp re-applies).
pub fn restore_team(orig: u32) -> u32 {
    match orig {
        HIDDEN_TEAM | PVP_TEAM => REMOTE_TEAM,
        t => t,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// Write `HIDDEN_TEAM`, remember `orig`.
    Hide { orig: u32 },
    /// Ours, but the team was changed under us: write `HIDDEN_TEAM` again.
    Rewrite,
    /// Write `team`, forget.
    Restore { team: u32 },
    /// The remote we changed is gone or replaced: forget without writing.
    Forget,
    /// Not held, but on `HIDDEN_TEAM` and not wanted hidden: write `team`.
    Unhide { team: u32 },
}

/// One peer's frame. `held` = (chr, original team) we hold; `cur` = (its native remote now,
/// its team now); `own_team` = the team the remote should have now when nobody hides it
/// (`REMOTE_TEAM`, or `PVP_TEAM` while pvp.rs puts friendly fire on it).
pub fn replica_step(held: Option<(usize, u32)>, cur: Option<(usize, u32)>, want: bool, own_team: u32) -> Step {
    match (held, cur) {
        (Some((h, _)), c) if c.map(|c| c.0) != Some(h) => Step::Forget,
        (Some((_, orig)), Some((_, team))) => {
            if !want {
                Step::Restore { team: restore_team(orig) }
            } else if team != HIDDEN_TEAM {
                Step::Rewrite
            } else {
                Step::Nothing
            }
        }
        (None, Some((_, team))) if want => Step::Hide { orig: if team == HIDDEN_TEAM { REMOTE_TEAM } else { team } },
        // Review m2: on team 0, not ours to hold and not to hide (a hub rebuilt after a panic,
        // a hold forgotten): the remote gets its own team back.
        (None, Some((_, HIDDEN_TEAM))) => Step::Unhide { team: own_team },
        _ => Step::Nothing,
    }
}

/// What the latest snapshot says about a peer: dead (visible, HP <= 0), alive, or unknown
/// (no snapshot, or hidden: loading, cutscene).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Alive,
    Dead,
    Unknown,
}

pub fn seen(snapshot: Option<(bool, i32)>) -> Seen {
    match snapshot {
        Some((false, hp)) if hp <= 0 => Seen::Dead,
        Some((false, _)) => Seen::Alive,
        _ => Seen::Unknown,
    }
}

/// Review m3: a friend's replica is hidden only after its snapshot said HP <= 0 for this long.
pub const DEAD_HOLD: Duration = Duration::from_millis(500);
/// Review m3: a hidden replica stays hidden this long without a fresh state before it is
/// given back (a snapshot gap must not flap the team).
pub const UNKNOWN_HOLD: Duration = Duration::from_secs(1);

/// Per-peer hysteresis state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hyst {
    dead_since: Option<Instant>,
    unknown_since: Option<Instant>,
}

/// Pure (review m3): does the replica count as dead this frame? `held` = we hide it now.
/// Alive restores at once (level-based); dead hides after `DEAD_HOLD`; unknown keeps a held
/// replica hidden for `UNKNOWN_HOLD`, never hides a new one.
pub fn replica_dead(h: &mut Hyst, s: Seen, held: bool, now: Instant) -> bool {
    match s {
        Seen::Alive => {
            *h = Hyst::default();
            false
        }
        Seen::Dead => {
            h.unknown_since = None;
            let since = *h.dead_since.get_or_insert(now);
            held || now.saturating_duration_since(since) >= DEAD_HOLD
        }
        Seen::Unknown => {
            let since = *h.unknown_since.get_or_insert(now);
            held && now.saturating_duration_since(since) < UNKNOWN_HOLD
        }
    }
}

/// Why a hold ends (log text).
pub fn restore_reason(enabled: bool, connected: bool, dead: bool, other_alive: bool) -> &'static str {
    if !enabled {
        "dead_players_untargetable = 0"
    } else if !connected {
        "no link"
    } else if !dead {
        "alive again or no fresh state"
    } else if !other_alive {
        "nobody else alive (wipe: the game's own behaviour)"
    } else {
        "kept"
    }
}

// ===================================================================== game side

#[derive(Clone, Copy)]
struct Held {
    peer: PeerId,
    chr: usize,
    orig: u32,
}

static HELD: Mutex<Vec<Held>> = Mutex::new(Vec::new());
/// We wrote 1 into the hide byte (and it was 0 before).
static HIDE_SET: AtomicBool = AtomicBool::new(false);

/// pvp.rs: we hold this remote's team (pvp must not read or write it meanwhile).
pub fn holds(chr: usize) -> bool {
    held().iter().any(|h| h.chr == chr)
}

/// Review m1: a panic while the lock was held must not leave a remote at team 0 for good.
fn held() -> std::sync::MutexGuard<'static, Vec<Held>> {
    HELD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// puppet.rs (review m4): our warp without a loading screen (hands off, boss expulsion, stuck
/// watchdog, own-world reload, leave reload) skips `tick`: the hide byte goes back to 0 now,
/// so a revive during that warp never leaves our player unperceived. `tick` sets it again
/// afterwards if we are still dead.
pub fn on_warp(why: &str) {
    clear_hide(why);
}

fn clear_hide(why: &str) {
    if HIDE_SET.swap(false, Ordering::Relaxed) {
        let a = module_base() + HIDE_FLAG_RVA;
        write::<u8>(a, 0);
        log::line(&format!("corpse: our player is targetable again (hide flag {:#x} = 0: {why})", 0x1_4000_0000usize + HIDE_FLAG_RVA));
    }
}

fn restore_one(h: &Held, why: &str) {
    if native::live_remote(h.chr) != Some(h.peer) {
        log::line(&format!("corpse: peer {}'s native remote {:#x} is gone ({why}): nothing to restore", h.peer.slot(), h.chr));
        return;
    }
    let t = restore_team(h.orig);
    write::<u32>(h.chr + CHR_TEAM, t);
    log::line(&format!("corpse: peer {}'s native remote {:#x}: team {HIDDEN_TEAM} -> {t} restored ({why})", h.peer.slot(), h.chr));
}

/// Everything back (load, no link, shutdown, frame panic).
pub fn restore_all(why: &str) {
    clear_hide(why);
    let all: Vec<Held> = std::mem::take(&mut *held());
    for h in &all {
        restore_one(h, why);
    }
}

/// lib.rs: the mod is switched off.
pub fn shutdown() {
    restore_all("mod shutdown");
}

/// puppet.rs: a loading screen. The world (and every remote) is gone: forget the remotes,
/// clear our hide byte.
pub fn on_loading() {
    clear_hide("loading screen");
    let mut g = held();
    if !g.is_empty() {
        log::line(&format!("corpse: loading screen, {} hidden remote(s) forgotten (the world is rebuilt)", g.len()));
        g.clear();
    }
}

/// Owned by the puppet hub: a dropped hub (frame panic) restores everything.
#[derive(Default)]
pub struct Corpse {
    /// "The hide byte was already set by something else" was logged.
    foreign_logged: bool,
    /// Review m3: per-peer hysteresis of the replica's dead / unknown state.
    hyst: HashMap<PeerId, Hyst>,
}

impl Drop for Corpse {
    fn drop(&mut self) {
        let _ = std::panic::catch_unwind(|| restore_all("co-op hub reset"));
    }
}

impl Corpse {
    /// Every in-world frame, after the native remotes were refreshed and before pvp.
    pub fn tick(&mut self, player: Chr, peers: &PeerSet, connected: bool) {
        let enabled = ENABLED.load(Ordering::Relaxed);
        let own_dead = player.hp().is_some_and(|h| h <= 0);
        let own_alive = player.hp().is_some_and(|h| h > 0);
        let now = Instant::now();

        // Our own player.
        let others_alive = peers.iter().any(|p| p.alive());
        let want = hide_local(enabled, connected, own_dead, others_alive);
        if want && !HIDE_SET.load(Ordering::Relaxed) {
            let a = module_base() + HIDE_FLAG_RVA;
            match read::<u8>(a) {
                Some(0) => {
                    if write::<u8>(a, 1) {
                        HIDE_SET.store(true, Ordering::Relaxed);
                        log::line(&format!(
                            "corpse: our player is dead and another player lives: enemies on this PC no longer perceive it (hide flag {:#x} = 1)",
                            0x1_4000_0000usize + HIDE_FLAG_RVA
                        ));
                    }
                }
                Some(v) if !self.foreign_logged => {
                    self.foreign_logged = true;
                    log::line(&format!("corpse: hide flag already {v} (set by something else): left alone"));
                }
                _ => {}
            }
        } else if !want && HIDE_SET.load(Ordering::Relaxed) {
            clear_hide(restore_reason(enabled, connected, own_dead, others_alive));
        }

        // Every peer's native remote.
        let mut held = held();
        self.hyst.retain(|id, _| peers.iter().any(|p| p.id == *id));
        for p in peers.iter() {
            let pos = held.iter().position(|h| h.peer == p.id);
            // Review m3: dead for 0.5 s before hiding, 1 s without a fresh state before
            // giving a hidden one back; alive gives it back at once.
            let s = seen(p.snapshot.map(|s| (s.hidden, s.hp)));
            let dead = replica_dead(self.hyst.entry(p.id).or_default(), s, pos.is_some(), now);
            let other_alive = own_alive || peers.iter().any(|q| q.id != p.id && q.alive());
            let want = hide_replica(enabled, connected, dead, other_alive);
            let cur = p.native.and_then(|c| read::<u32>(c.0 + CHR_TEAM).map(|t| (c.0, t)));
            let own_team = if crate::pvp::wants_coop_npc_team(p.id) { PVP_TEAM } else { REMOTE_TEAM };
            match replica_step(pos.map(|i| (held[i].chr, held[i].orig)), cur, want, own_team) {
                Step::Nothing => {}
                Step::Hide { orig } => {
                    let Some((chr, _)) = cur else { continue };
                    if native::live_remote(chr) != Some(p.id) {
                        continue;
                    }
                    write::<u32>(chr + CHR_TEAM, HIDDEN_TEAM);
                    held.push(Held { peer: p.id, chr, orig });
                    log::line(&format!(
                        "corpse: peer {}'s native remote {chr:#x} is dead (snapshot HP {} for {} ms+): team {orig} -> {HIDDEN_TEAM}, enemies here no longer pick it",
                        p.id.slot(),
                        p.hp().map_or_else(|| "?".to_string(), |h| h.to_string()),
                        DEAD_HOLD.as_millis()
                    ));
                }
                Step::Unhide { team } => {
                    if let Some((chr, _)) = cur
                        && native::live_remote(chr) == Some(p.id)
                    {
                        write::<u32>(chr + CHR_TEAM, team);
                        log::line(&format!(
                            "corpse: peer {}'s native remote {chr:#x} was on team {HIDDEN_TEAM} without our hold: team {team} restored ({})",
                            p.id.slot(),
                            if team == PVP_TEAM { "friendly fire on it" } else { "its own team" }
                        ));
                    }
                }
                Step::Rewrite => {
                    if let Some((chr, team)) = cur
                        && native::live_remote(chr) == Some(p.id)
                    {
                        write::<u32>(chr + CHR_TEAM, HIDDEN_TEAM);
                        log::line(&format!("corpse: peer {}'s native remote {chr:#x}: team {team} written under us, {HIDDEN_TEAM} again", p.id.slot()));
                    }
                }
                Step::Restore { .. } => {
                    if let Some(i) = pos {
                        let h = held.remove(i);
                        restore_one(&h, restore_reason(enabled, connected, dead, other_alive));
                    }
                }
                Step::Forget => {
                    if let Some(i) = pos {
                        let h = held.remove(i);
                        log::line(&format!(
                            "corpse: peer {}'s native remote {:#x} was replaced or removed: nothing to restore",
                            h.peer.slot(),
                            h.chr
                        ));
                    }
                }
            }
        }
        // Peers that left: their remotes are deleted with them.
        held.retain(|h| {
            let here = peers.iter().any(|p| p.id == h.peer);
            if !here {
                log::line(&format!("corpse: peer {} left: its remote {:#x} is forgotten", h.peer.slot(), h.chr));
            }
            here
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hidden_only_dead_with_a_living_other() {
        assert!(hide_local(true, true, true, true));
        assert!(!hide_local(false, true, true, true), "setting off");
        assert!(!hide_local(true, false, true, true), "no link");
        assert!(!hide_local(true, true, false, true), "alive / revived / respawned");
        assert!(!hide_local(true, true, true, false), "both dead: as before");
    }

    #[test]
    fn replica_hidden_only_dead_with_a_living_other() {
        assert!(hide_replica(true, true, true, true));
        assert!(!hide_replica(true, true, false, true));
        assert!(!hide_replica(true, true, true, false));
        assert!(!hide_replica(false, true, true, true));
        assert!(!hide_replica(true, false, true, true));
    }

    #[test]
    fn restore_never_leaves_hidden_or_pvp_team() {
        assert_eq!(restore_team(2), 2);
        assert_eq!(restore_team(28), 2, "pvp writes 28 again itself");
        assert_eq!(restore_team(0), 2);
        assert_eq!(restore_team(7), 7);
    }

    const A: usize = 0x1000;
    const B: usize = 0x2000;

    /// Friend dies -> hidden; resurrects (Sekiro's own) -> restored.
    #[test]
    fn dead_then_revived() {
        assert_eq!(replica_step(None, Some((A, 2)), false, 2), Step::Nothing);
        assert_eq!(replica_step(None, Some((A, 2)), true, 2), Step::Hide { orig: 2 });
        assert_eq!(replica_step(Some((A, 2)), Some((A, 0)), true, 2), Step::Nothing);
        assert_eq!(replica_step(Some((A, 2)), Some((A, 0)), false, 2), Step::Restore { team: 2 });
    }

    /// Friend respawns at the idol: the remote is deleted / recreated: never write the old one.
    #[test]
    fn respawn_or_recreated_replica_is_forgotten() {
        assert_eq!(replica_step(Some((A, 2)), None, true, 2), Step::Forget);
        assert_eq!(replica_step(Some((A, 2)), None, false, 2), Step::Forget);
        assert_eq!(replica_step(Some((A, 2)), Some((B, 2)), false, 2), Step::Forget);
        assert_eq!(replica_step(Some((A, 2)), Some((B, 2)), true, 2), Step::Forget);
        // Next frame the new one is judged on its own.
        assert_eq!(replica_step(None, Some((B, 2)), true, 2), Step::Hide { orig: 2 });
        assert_eq!(replica_step(None, Some((B, 2)), false, 2), Step::Nothing);
    }

    /// Friendly fire on: the dead copy had pvp's 28; it comes back as 2 (pvp sets 28 again).
    #[test]
    fn friendly_fire_on_and_off() {
        assert_eq!(replica_step(None, Some((A, 28)), true, 2), Step::Hide { orig: 28 });
        assert_eq!(replica_step(Some((A, 28)), Some((A, 0)), false, 2), Step::Restore { team: 2 });
        // Something wrote a team while we hold it: hidden again, original kept.
        assert_eq!(replica_step(Some((A, 2)), Some((A, 28)), true, 2), Step::Rewrite);
        assert_eq!(replica_step(Some((A, 2)), Some((A, 2)), true, 2), Step::Rewrite);
    }

    /// A hub rebuilt after a panic finds team 0 on a remote: its own team is 2.
    #[test]
    fn found_hidden_after_a_reset() {
        assert_eq!(replica_step(None, Some((A, 0)), true, 2), Step::Hide { orig: 2 });
    }

    #[test]
    fn no_remote_nothing_to_do() {
        assert_eq!(replica_step(None, None, true, 2), Step::Nothing);
        assert_eq!(replica_step(None, None, false, 2), Step::Nothing);
    }

    /// Review m2: a remote left on team 0 without our hold goes back to its own team (2), or to
    /// pvp's 28 while friendly fire is on it; a held one or a wanted one is not touched here.
    #[test]
    fn stray_hidden_team_is_restored() {
        assert_eq!(replica_step(None, Some((A, 0)), false, 2), Step::Unhide { team: 2 });
        assert_eq!(replica_step(None, Some((A, 0)), false, 28), Step::Unhide { team: 28 });
        assert_eq!(replica_step(None, Some((A, 2)), false, 28), Step::Nothing, "pvp sets 28 itself");
        assert_eq!(replica_step(None, Some((A, 0)), true, 2), Step::Hide { orig: 2 });
        assert_eq!(replica_step(Some((A, 2)), Some((A, 0)), true, 2), Step::Nothing);
    }

    #[test]
    fn snapshot_states() {
        assert_eq!(seen(Some((false, 0))), Seen::Dead);
        assert_eq!(seen(Some((false, -5))), Seen::Dead);
        assert_eq!(seen(Some((false, 1))), Seen::Alive);
        assert_eq!(seen(Some((true, 0))), Seen::Unknown, "hidden (loading)");
        assert_eq!(seen(None), Seen::Unknown);
    }

    /// Review m3: HP 0 must last 0.5 s before hiding; a held replica survives a 1 s gap in the
    /// snapshots; alive restores at once.
    #[test]
    fn hysteresis() {
        let t = Instant::now();
        let ms = Duration::from_millis;
        let mut h = Hyst::default();
        assert!(!replica_dead(&mut h, Seen::Dead, false, t));
        assert!(!replica_dead(&mut h, Seen::Dead, false, t + ms(499)));
        assert!(replica_dead(&mut h, Seen::Dead, false, t + ms(500)));
        // Held now: a snapshot gap keeps it for 1 s, then gives it back.
        assert!(replica_dead(&mut h, Seen::Unknown, true, t + ms(600)));
        assert!(replica_dead(&mut h, Seen::Unknown, true, t + ms(1599)));
        assert!(!replica_dead(&mut h, Seen::Unknown, true, t + ms(1600)));
        // A fresh dead snapshot after the gap: still dead (dead_since kept).
        assert!(replica_dead(&mut h, Seen::Dead, false, t + ms(1700)));
        // Alive: back at once, and a new death waits 0.5 s again.
        assert!(!replica_dead(&mut h, Seen::Alive, true, t + ms(1800)));
        assert!(!replica_dead(&mut h, Seen::Dead, false, t + ms(1900)));
        assert!(replica_dead(&mut h, Seen::Dead, false, t + ms(2400)));
        // Unknown never hides a replica we do not hold; a one-frame HP 0 flicker neither.
        let mut h = Hyst::default();
        assert!(!replica_dead(&mut h, Seen::Unknown, false, t));
        assert!(!replica_dead(&mut h, Seen::Dead, false, t + ms(10)));
        assert!(!replica_dead(&mut h, Seen::Alive, false, t + ms(30)));
        assert!(!replica_dead(&mut h, Seen::Dead, false, t + ms(600)));
    }

    #[test]
    fn reasons() {
        assert_eq!(restore_reason(false, true, true, true), "dead_players_untargetable = 0");
        assert_eq!(restore_reason(true, false, true, true), "no link");
        assert_eq!(restore_reason(true, true, false, true), "alive again or no fresh state");
        assert!(restore_reason(true, true, true, false).starts_with("nobody else alive"));
    }
}
