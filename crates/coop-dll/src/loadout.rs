//! Loadout sync (FORK.md "Loadout sync (experimental)", 2026-11-11, NOT verified live).
//!
//! The native remote of a peer (native.rs) was dressed with OUR equipment: `dress_like(chr,
//! player)` copied the observer's 25 ChrAsm slots, and nothing about equipment went over the
//! network (the snapshot carries pose / animation / HP only). The friend's replica wore our
//! prosthetics, and its PlayerGameData kept the CharaInitParam template's default combat art.
//!
//! Here: every connected in-world frame we read our own 25 ChrAsm ids (ghost.rs
//! `equip_ids`) and send them as `BULK_LOADOUT` (reliable, to every peer; the host relays)
//! when they changed, and again every `REFRESH_EVERY` (a late join, a lost link). Received
//! loadouts are kept per peer slot; native.rs dresses the peer's remote from them (`of`).
//! `[DEBUG] loadout_sync = 0` sends nothing and native.rs keeps dressing like us.
//!
//! The combat art is not sent yet (`Loadout::art` = -1): where the PlayerGameData keeps it
//! is not known (native.rs TODO(IDA)).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use coop_net::{LOADOUT_SLOTS, Loadout, decode_loadout, encode_loadout};

use crate::game::Chr;
use crate::net::Net;
use crate::peers::{MAX_PLAYERS, PeerId, PeerSet};
use crate::{ghost, log, tuning};

/// Bulk transfer kind (world.rs 1, 5, 6, 8; puppet.rs 7; testsync 20-23; doctor 30;
/// keyitems 41-46; persist 50; audit 60; nsession 0x4E).
pub const BULK_LOADOUT: u8 = 0xA8; // far from upstream's 41-50 run (46 / 47 are its likely next kinds)
/// Unchanged equipment is sent again this often (only while no bulk chunk is unacked, so a
/// stalled link does not pile them up).
const REFRESH_EVERY: Duration = Duration::from_secs(3);

const _: () = assert!(LOADOUT_SLOTS == ghost::EQUIP_SLOTS);

struct Sent {
    last: Option<Loadout>,
    at: Option<Instant>,
}

static SENT: Mutex<Sent> = Mutex::new(Sent { last: None, at: None });
/// The latest loadout of each peer, by `PeerId::index`.
static RECEIVED: Mutex<[Option<Loadout>; MAX_PLAYERS]> = Mutex::new([None; MAX_PLAYERS]);

/// native.rs: the latest loadout `peer` sent, if any.
pub fn of(peer: PeerId) -> Option<Loadout> {
    RECEIVED.lock().ok()?.get(peer.index()).copied().flatten()
}

/// Every connected in-world frame (puppet.rs, game thread, before `Native::tick`).
pub fn tick(net: &mut Net, peers: &PeerSet, player: Chr) {
    // Received: always drained, so they never crowd the bulk inbox.
    while let Some((from, data)) = net.take_bulk_from(BULK_LOADOUT) {
        let Some(l) = decode_loadout(&data) else {
            log::line(&format!("loadout: unreadable loadout from peer {} ({} bytes), ignored", from.slot(), data.len()));
            continue;
        };
        let Ok(mut got) = RECEIVED.lock() else { continue };
        let Some(e) = got.get_mut(from.index()) else { continue };
        if *e != Some(l) {
            log::line(&format!("loadout: peer {} wears {:?} (art {}, extra {:?})", from.slot(), l.slots, l.art, l.extra));
        }
        *e = Some(l);
    }
    // A peer that left: its slot may be someone else's next.
    if let Ok(mut got) = RECEIVED.lock() {
        for (i, e) in got.iter_mut().enumerate() {
            if e.is_some() && !peers.iter().any(|p| p.id.index() == i) {
                *e = None;
            }
        }
    }

    if !tuning::loadout_sync() || peers.is_empty() {
        return;
    }
    let Some(ids) = ghost::equip_ids(player) else { return };
    // TODO(IDA): the equipped combat art (and skills) into `art` / `extra` once their
    // storage is known (native.rs, where the receiver would write it).
    let ours = Loadout::new(ids);
    let Ok(mut sent) = SENT.lock() else { return };
    let now = Instant::now();
    let changed = sent.last != Some(ours);
    let due = sent.at.is_none_or(|t| now.duration_since(t) >= REFRESH_EVERY) && net.pending_bulk_chunks() == 0;
    if (changed || due) && net.send_bulk(BULK_LOADOUT, &encode_loadout(&ours)) {
        if changed {
            log::line(&format!("loadout: sent {ids:?}"));
        }
        sent.last = Some(ours);
        sent.at = Some(now);
    }
}
