//! Loadout sync (FORK.md "Loadout sync (experimental)", 2026-11-11, NOT verified live).
//!
//! The native remote of a peer (native.rs) was dressed with OUR equipment: `dress_like(chr,
//! player)` copied the observer's 25 ChrAsm slots, and nothing about equipment went over the
//! network (the snapshot carries pose / animation / HP only). The friend's replica wore our
//! prosthetics, and its PlayerGameData kept the CharaInitParam template's default combat art.
//!
//! Layout v1 sent the ChrAsm slots (`ChrIns+0x2140`, `+0x24 + 4*slot`). Those are gaitem
//! HANDLES (`0x8080xxxx`, `0x907Fxxxx`), and the ChrAsm getter `0x9CBE40` resolves them
//! through the LOCAL player's inventory: copied to another machine they mean nothing
//! (RE dump 2026-11-11, E:\RE\notes-2026-11-11.md "PGD локального игрока").
//!
//! Layout v2 (here) sends the real item ids: EquipGameData (`PGD+0x278`) `+0x374`, i32[25]
//! by ChrAsm slot (= `PGD+0x5EC`), category bits included (weapons 0x0, protectors
//! 0x1000_0000). Slot 1 (the right "weapon") is the equipped COMBAT ART (5200 Nightjar
//! Slash, 5100 Whirlwind Slash), slots 0/2/4 the prosthetics, 3/5 bare hands (110000),
//! 12..15 the protectors. The receiver replays on the replica's own PGD what the game does
//! when it fills a PGD from a CharaInitParam row (`0x7B7470`) or equips from the menu
//! (`0xD592F0`): find or add the item in that PGD's inventory, equip it into the slot
//! (`apply`), then copy the PGD's ChrAsm into the PlayerIns holders as the PlayerIns ctor
//! (`0xA217A0`) does (`sync_chr`). `[DEBUG] loadout_sync = 0` sends nothing and native.rs
//! keeps dressing like us.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use coop_net::{LOADOUT_SLOTS, Loadout, decode_loadout, encode_loadout};
use fromsoftware_shared::FromStatic;
use sekiro::param::{EQUIP_PARAM_PROTECTOR_ST, EQUIP_PARAM_WEAPON_ST, ParamDef};
use sekiro::sprj::SoloParamRepository;

use crate::game::{Chr, module_base, read, read_ptr};
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

// --- Game data (Sekiro 1.06, dump 2026-11-11) ---------------------------------
const GAME_DATA_MAN_RVA: usize = 0x3D5AAC0;
const GDM_LOCAL_PGD: usize = 0x08;
/// PlayerGameData -> EquipGameData (vtable 0x29C4730).
const PGD_EGD: usize = 0x278;
/// EquipGameData: the equipped item ids, i32 by ChrAsm slot (`0x7A52B0` writes them).
const EGD_IDS: usize = 0x374;
/// PlayerIns -> its PlayerGameData (vtable slot 0x1B8 `0xA2A0E0` returns it).
const CHR_PGD: usize = 0x2000;
/// Gaitem manager (handles `0x8080xxxx`; `0xC373B0` creates weapon gaitems in it).
const GAITEM_MAN_RVA: usize = 0x3D888A8;
/// `(gaitem_man, u32* out, const u32* full_id) -> u32*`: a new handle (one reference) for
/// any category (weapon `0xC373B0`, protector `0xC37110`, ...). Used by `0x79FDD0`,
/// `0xA917B0`.
const HANDLE_FROM_ID_RVA: usize = 0xC37740;
/// `(u32* handle)`: drops the reference (`0xC36BD0`); fill_a calls it after equipping.
const HANDLE_RELEASE_RVA: usize = 0xC396C0;
/// `(egd, const i32* full_id) -> i32`: inventory index of that item in THIS EGD, -1 if none
/// (hash at EGD+0x178).
const INV_FIND_RVA: usize = 0x79D7E0;
/// `(u64[2] out, egd, i32 inv_idx)` and `(u64[2]*, u32* out) -> u32*`: the inventory
/// entry's own handle (the equip menu `0xD592F0`, `0xA917B0`).
const INV_REF_RVA: usize = 0xC3C9B0;
const INV_REF_HANDLE_RVA: usize = 0xC3CB00;
/// `(egd, u32 category, const u32* handle, i32 count, u64 1, u8 new_mark, u8 no_auto_equip)
/// -> i32 inv_idx` (-1: not added). The inventory is EGD+0x168, per PGD.
const INV_ADD_RVA: usize = 0x79A710;
/// `(egd, u32 slot, const u32* handle, i32 inv_idx, i32 force, u8 write_id, u8 0)`: equips
/// into the PGD's ChrAsm (EGD+0x98) through the functor `0x7A46B0` -> `0x9CCBA0` and writes
/// `EGD+0x374[slot]` (`0x7A52B0`).
const EQUIP_RVA: usize = 0x79F8A0;
/// `(holder, pgd)`: copies the PGD's ChrAsm (PGD+0x310) into a PlayerIns holder, with handle
/// references; the PlayerIns ctor `0xA217A0` calls it for `+0x2140` and `+0x2198`.
const HOLDER_FROM_PGD_RVA: usize = 0x9CCEE0;
const CHR_HOLDERS: [usize; 2] = [0x2140, 0x2198];

/// Category bits of a full item id (`id >> 28`).
const CAT_WEAPON: u32 = 0;
const CAT_PROTECTOR: u32 = 1;

/// What a ChrAsm slot holds, as the game fills it (`0x7B7470`: weapon kinds 0..5 through
/// `0x9CB5D0` = slot k, protector kinds through `0x9CB3E0` = slot 12 + k, accessories through
/// `0x9CB4F0` = 17 + k with another add path `0x79BF90`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// EquipParamWeapon: prosthetics, the combat art (slot 1), bare hands.
    Weapon,
    /// EquipParamProtector, id with category bits 0x1000_0000.
    Protector,
    /// Not replayed (accessories, unknown slots).
    Other,
}

pub fn slot_kind(slot: usize) -> SlotKind {
    match slot {
        0..=11 => SlotKind::Weapon,
        12..=16 => SlotKind::Protector,
        _ => SlotKind::Other,
    }
}

/// One slot to put on the replica.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub slot: usize,
    pub from: i32,
    pub to: i32,
}

/// Why a slot that differs is left as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// Empty (-1): unequipping is not replayed.
    Empty,
    /// A slot kind we do not replay.
    Unsupported,
    /// Category bits do not match the slot kind.
    WrongCategory,
    /// No such row in the slot kind's param.
    NoRow,
}

/// The param row a slot's id must exist as: (kind, row id), or why not.
pub fn row_of(slot: usize, id: i32) -> Result<(SlotKind, u32), Skip> {
    if id < 0 {
        return Err(Skip::Empty);
    }
    let id = id as u32;
    let (cat, row) = (id >> 28, id & 0x0FFF_FFFF);
    match slot_kind(slot) {
        SlotKind::Weapon if cat == CAT_WEAPON => Ok((SlotKind::Weapon, row)),
        SlotKind::Protector if cat == CAT_PROTECTOR => Ok((SlotKind::Protector, row)),
        SlotKind::Other => Err(Skip::Unsupported),
        _ => Err(Skip::WrongCategory),
    }
}

/// The slots of `wanted` that differ from `current` and are safe to put on (their row exists:
/// `exists(kind, row)`), and the differing slots that are skipped. Network data never reaches
/// a game call unless it passes here.
pub fn plan(
    current: &[i32; LOADOUT_SLOTS],
    wanted: &[i32; LOADOUT_SLOTS],
    exists: impl Fn(SlotKind, u32) -> bool,
) -> (Vec<Change>, Vec<(usize, i32, Skip)>) {
    let mut put = Vec::new();
    let mut skip = Vec::new();
    for slot in 0..LOADOUT_SLOTS {
        let (from, to) = (current[slot], wanted[slot]);
        if from == to {
            continue;
        }
        match row_of(slot, to) {
            Ok((kind, row)) if exists(kind, row) => put.push(Change { slot, from, to }),
            Ok(_) => skip.push((slot, to, Skip::NoRow)),
            Err(why) => skip.push((slot, to, why)),
        }
    }
    (put, skip)
}

/// Whether `row` exists in the param of `kind` (game thread; false while params are not
/// loaded).
fn row_exists(kind: SlotKind, row: u32) -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return false };
    let name = match kind {
        SlotKind::Weapon => EQUIP_PARAM_WEAPON_ST::NAME,
        SlotKind::Protector => EQUIP_PARAM_PROTECTOR_ST::NAME,
        SlotKind::Other => return false,
    };
    repo.params().find(|p| p.struct_name() == name).is_some_and(|p| p.data.find_index(row).is_some())
}

/// The 25 equipped item ids of a PlayerGameData.
pub fn ids_of(pgd: usize) -> Option<[i32; LOADOUT_SLOTS]> {
    let mut ids = [0i32; LOADOUT_SLOTS];
    for (slot, id) in ids.iter_mut().enumerate() {
        *id = read::<i32>(pgd + PGD_EGD + EGD_IDS + 4 * slot)?;
    }
    Some(ids)
}

fn local_pgd() -> Option<usize> {
    read_ptr(module_base() + GAME_DATA_MAN_RVA).and_then(|g| read_ptr(g + GDM_LOCAL_PGD))
}

/// Puts `wanted` on the remote PlayerGameData `pgd` (never the local one), slot by slot
/// where it differs and passes `plan`. Game thread only. Returns the number of slots equipped.
pub fn apply(pgd: usize, wanted: &[i32; LOADOUT_SLOTS], who: &str) -> Result<usize, String> {
    let base = module_base();
    if pgd < 0x10000 || local_pgd() == Some(pgd) {
        return Err(format!("not a remote PGD ({pgd:#x})"));
    }
    let gaitems = read_ptr(base + GAITEM_MAN_RVA).filter(|&g| g >= 0x10000).ok_or("gaitem manager null")?;
    let current = ids_of(pgd).ok_or("replica PGD ids unreadable")?;
    let (put, skipped) = plan(&current, wanted, row_exists);
    for (slot, id, why) in &skipped {
        log::line(&format!("loadout: {who} slot {slot} id {id} skipped ({why:?})"));
    }
    let egd = pgd + PGD_EGD;
    let from_id: extern "C" fn(usize, *mut u32, *const u32) -> *mut u32 = unsafe { std::mem::transmute(base + HANDLE_FROM_ID_RVA) };
    let release: extern "C" fn(*mut u32) = unsafe { std::mem::transmute(base + HANDLE_RELEASE_RVA) };
    let find: extern "C" fn(usize, *const i32) -> i32 = unsafe { std::mem::transmute(base + INV_FIND_RVA) };
    let inv_ref: extern "C" fn(*mut [u64; 2], usize, i32) -> usize = unsafe { std::mem::transmute(base + INV_REF_RVA) };
    let ref_handle: extern "C" fn(*const [u64; 2], *mut u32) -> *mut u32 = unsafe { std::mem::transmute(base + INV_REF_HANDLE_RVA) };
    let add: extern "C" fn(usize, u32, *const u32, i32, u64, u8, u8) -> i32 = unsafe { std::mem::transmute(base + INV_ADD_RVA) };
    let equip: extern "C" fn(usize, u32, *const u32, i32, i32, u8, u8) = unsafe { std::mem::transmute(base + EQUIP_RVA) };
    let mut done = 0;
    for c in put {
        let full = c.to;
        // Already in this PGD's inventory: equip its own entry (the menu's way).
        let found = find(egd, &full);
        let (idx, how) = if found >= 0 {
            let mut r = [0u64; 2];
            let mut h = 0u32;
            inv_ref(&mut r, egd, found);
            ref_handle(&r, &mut h);
            if h == 0 {
                log::line(&format!("loadout: {who} slot {} {} -> {full} not applied (inventory entry {found} has no handle)", c.slot, c.from));
                continue;
            }
            equip(egd, c.slot as u32, &h, found, 0, 1, 0);
            (found, "in inventory")
        } else {
            // New to it: a fresh gaitem, added to this PGD's inventory, equipped, released
            // (fill_a `0x7B7470`; no auto-equip, no "new item" mark).
            let mut h = 0u32;
            from_id(gaitems, &mut h, &(full as u32));
            if h == 0 {
                log::line(&format!("loadout: {who} slot {} {} -> {full} not applied (no gaitem handle)", c.slot, c.from));
                continue;
            }
            let idx = add(egd, full as u32 & 0xF000_0000, &h, 1, 1, 0, 1);
            if idx >= 0 {
                equip(egd, c.slot as u32, &h, idx, 0, 1, 0);
            }
            release(&mut h);
            if idx < 0 {
                log::line(&format!("loadout: {who} slot {} {} -> {full} not applied (inventory add returned {idx})", c.slot, c.from));
                continue;
            }
            (idx, "added")
        };
        let now = read::<i32>(egd + EGD_IDS + 4 * c.slot);
        log::line(&format!("loadout: {who} slot {} {} -> {full} applied (ret inventory {idx}, {how}; slot now {now:?})", c.slot, c.from));
        done += 1;
    }
    Ok(done)
}

/// The PlayerIns `chr` (whose PGD is `pgd`) takes the PGD's ChrAsm into its holders, as its
/// ctor did. Returns how many holders were synced.
pub fn sync_chr(chr: Chr, pgd: usize) -> Result<usize, String> {
    if read_ptr(chr.0 + CHR_PGD) != Some(pgd) {
        return Err("chr's PGD is not the replica's".into());
    }
    let copy: extern "C" fn(usize, usize) = unsafe { std::mem::transmute(module_base() + HOLDER_FROM_PGD_RVA) };
    let mut n = 0;
    for off in CHR_HOLDERS {
        if let Some(h) = read_ptr(chr.0 + off).filter(|&h| h >= 0x10000) {
            copy(h, pgd);
            n += 1;
        }
    }
    if n == 0 {
        return Err("no ChrAsm holder".into());
    }
    Ok(n)
}

struct Sent {
    last: Option<Loadout>,
    at: Option<Instant>,
}

static SENT: Mutex<Sent> = Mutex::new(Sent { last: None, at: None });
/// The latest loadout of each peer, by `PeerId::index`.
static RECEIVED: Mutex<[Option<Loadout>; MAX_PLAYERS]> = Mutex::new([None; MAX_PLAYERS]);
/// A peer whose loadout bodies we cannot read was logged (an older build sends v1 every 3 s).
static UNREADABLE_LOGGED: Mutex<[bool; MAX_PLAYERS]> = Mutex::new([false; MAX_PLAYERS]);

/// native.rs: the latest loadout `peer` sent, if any.
pub fn of(peer: PeerId) -> Option<Loadout> {
    RECEIVED.lock().ok()?.get(peer.index()).copied().flatten()
}

/// Every connected in-world frame (puppet.rs, game thread, before `Native::tick`).
pub fn tick(net: &mut Net, peers: &PeerSet) {
    // Received: always drained, so they never crowd the bulk inbox.
    while let Some((from, data)) = net.take_bulk_from(BULK_LOADOUT) {
        let Some(l) = decode_loadout(&data) else {
            let mut logged = UNREADABLE_LOGGED.lock();
            if let Ok(w) = logged.as_mut()
                && let Some(w) = w.get_mut(from.index())
                && !*w
            {
                *w = true;
                log::line(&format!(
                    "loadout: unreadable loadout from peer {} ({} bytes, layout {:?}; an older build?), ignored",
                    from.slot(),
                    data.len(),
                    data.get(2)
                ));
            }
            continue;
        };
        let Ok(mut got) = RECEIVED.lock() else { continue };
        let Some(e) = got.get_mut(from.index()) else { continue };
        if *e != Some(l) {
            log::line(&format!("loadout: peer {} ids {:?}", from.slot(), l.slots));
        }
        *e = Some(l);
    }
    // A peer that left: its slot may be someone else's next.
    for i in 0..MAX_PLAYERS {
        if peers.iter().any(|p| p.id.index() == i) {
            continue;
        }
        if let Ok(mut got) = RECEIVED.lock() {
            got[i] = None;
        }
        if let Ok(mut w) = UNREADABLE_LOGGED.lock() {
            w[i] = false;
        }
    }

    if !tuning::loadout_sync() || peers.is_empty() {
        return;
    }
    let Some(ids) = local_pgd().and_then(ids_of) else { return };
    // The combat art is slot 1; `art` / `extra` stay reserved (-1).
    let ours = Loadout::new(ids);
    let Ok(mut sent) = SENT.lock() else { return };
    let now = Instant::now();
    let changed = sent.last != Some(ours);
    let due = sent.at.is_none_or(|t| now.duration_since(t) >= REFRESH_EVERY) && net.pending_bulk_chunks() == 0;
    if (changed || due) && net.send_bulk(BULK_LOADOUT, &encode_loadout(&ours)) {
        if changed {
            log::line(&format!("loadout: sent ids {ids:?}"));
        }
        sent.last = Some(ours);
        sent.at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The local player's ids in the dump (E:\RE\notes-2026-11-11.md).
    const DUMP: [i32; LOADOUT_SLOTS] = [
        70100, 5200, 73000, 110000, 72000, 110000, -1, -1, -1, -1, -1, -1, 268_535_456, 268_566_456, 268_537_456, 268_538_456, -1, -1, -1,
        -1, -1, -1, -1, -1, -1,
    ];

    #[test]
    fn slot_kinds_follow_fill_a() {
        assert_eq!(slot_kind(0), SlotKind::Weapon);
        assert_eq!(slot_kind(1), SlotKind::Weapon);
        assert_eq!(slot_kind(11), SlotKind::Weapon);
        assert_eq!(slot_kind(12), SlotKind::Protector);
        assert_eq!(slot_kind(16), SlotKind::Protector);
        assert_eq!(slot_kind(17), SlotKind::Other);
        assert_eq!(slot_kind(24), SlotKind::Other);
    }

    #[test]
    fn row_of_checks_category() {
        assert_eq!(row_of(1, 5200), Ok((SlotKind::Weapon, 5200)));
        assert_eq!(row_of(12, 268_535_456), Ok((SlotKind::Protector, 100_000)));
        assert_eq!(row_of(1, 268_535_456), Err(Skip::WrongCategory));
        assert_eq!(row_of(12, 100_000), Err(Skip::WrongCategory));
        assert_eq!(row_of(0, 0x4000_0001), Err(Skip::WrongCategory));
        assert_eq!(row_of(0, -1), Err(Skip::Empty));
        assert_eq!(row_of(0, i32::MIN), Err(Skip::Empty));
        assert_eq!(row_of(18, 0x2000_0001), Err(Skip::Unsupported));
    }

    #[test]
    fn plan_puts_only_differing_existing_rows() {
        let weapons = [5100u32, 5200, 70100, 72000, 73000, 110000];
        let exists = |k: SlotKind, r: u32| match k {
            SlotKind::Weapon => weapons.contains(&r),
            SlotKind::Protector => r == 100_000 || r == 131_000,
            SlotKind::Other => false,
        };
        // Same: nothing to do, nothing skipped.
        assert_eq!(plan(&DUMP, &DUMP, exists), (vec![], vec![]));
        let mut want = DUMP;
        want[1] = 5100; // another combat art
        want[13] = 0x1000_0000 | 100_000;
        want[2] = 999_999; // no such weapon row
        want[4] = -1; // unequip: not replayed
        want[20] = 0x2000_0005; // accessory: not replayed
        want[3] = 0x1000_0000 | 100_000; // protector id in a weapon slot
        let (put, skip) = plan(&DUMP, &want, exists);
        assert_eq!(
            put,
            vec![Change { slot: 1, from: 5200, to: 5100 }, Change { slot: 13, from: 268_566_456, to: 0x1000_0000 | 100_000 }]
        );
        assert_eq!(
            skip,
            vec![
                (2, 999_999, Skip::NoRow),
                (3, 0x1000_0000 | 100_000, Skip::WrongCategory),
                (4, -1, Skip::Empty),
                (20, 0x2000_0005, Skip::Unsupported)
            ]
        );
        // Without params (not loaded): nothing reaches the game.
        let (put, _) = plan(&DUMP, &want, |_, _| false);
        assert!(put.is_empty());
    }
}
