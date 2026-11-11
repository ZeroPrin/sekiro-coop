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

use coop_net::{LOADOUT_EXTRA_ACTIVE_PROSTHETIC, LOADOUT_SLOTS, Loadout, decode_loadout, encode_loadout};
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

// --- Active prosthetic (FORK.md "Active prosthetic", static RE 2026-11-12) -----
// ChrAsm (class vtable `0x2A1EDD0`; PGD+0x310 and the PlayerIns holder `chr+0x2140`):
// +0x08 arm style, +0x0C + 4*hand the ACTIVE slot index of that hand (getter `0xBF1480`,
// setter `0xBF1640`, both on ChrAsm+8), +0x24 the 25 gaitem handles. Hand 0 (left) =
// the prosthetic: its active ChrAsm slot is `2 * index` (`0x9CB210`, `0xE72220`).
// The game's switch (`0xB57E30` -> `0xA26150(chr, hand, (index + 1) % 3)`, skipping slots
// that hold bare hands 110000) runs only for the local player. Every PlayerIns update
// (`0xA37560`, chr types 0 and 1 alike: table `0x143B0A3B0` byte 1 = 0) copies the PGD's
// ChrAsm INTO the holder (`0x9CC9A0`) and reacts to a changed hand weapon itself
// (`0x7E3C60`, `0xA23AF0`, then `0x9F6780` / `0x9F6460`). So a replica shows its PGD's
// index; writing PGD+0x31C lets the game swap the tool.
const PGD_CHR_ASM: usize = 0x310;
const CHR_ASM_VTABLE_RVA: usize = 0x2A1EDD0;
const ASM_ARM_STYLE: usize = 0x08;
const ASM_ACTIVE_LEFT: usize = 0x0C;
/// PlayerIns -> its ChrAsm holder (PlayerIns vtable slot 0x2F0 `0xA28AC0` returns it).
const CHR_ASM_HOLDER: usize = 0x2140;
/// Prosthetic slots per hand (`(index + 1) % 3` in `0xB57E30`).
pub const PROSTHETIC_SLOTS: i32 = 3;
/// "Bare hands" weapon id: the game's switch skips a slot holding it.
const BARE_HANDS: i32 = 110_000;
/// A replica whose holder did not follow its PGD this long gets the old refresh (sync_chr +
/// parts reload).
const FOLLOW_CHECK_AFTER: Duration = Duration::from_millis(400);
/// The local ChrAsm holder may lag the PGD a frame; this many ticks in a row disagreeing
/// means the offsets are wrong.
const LOCAL_MISMATCH_LIMIT: u32 = 180;

/// The ChrAsm slot of active prosthetic index `active` (0..=2 -> 0 / 2 / 4).
pub fn prosthetic_slot(active: i32) -> Option<usize> {
    (0..PROSTHETIC_SLOTS).contains(&active).then(|| 2 * active as usize)
}

/// Why a received active prosthetic index is not put on the replica.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveSkip {
    /// -1: an older build, or the sender's own check failed.
    Unknown,
    /// Not 0..=2.
    OutOfRange,
    /// The sender's slot holds no tool (empty, bare hands, not a weapon id).
    NotATool,
    /// The replica's slot does not hold the sender's tool yet (loadout not applied).
    NotDressed,
}

/// The ChrAsm slot to make active on the replica, or why not. `sender` = the ids the
/// sender sent, `replica` = the replica PGD's ids now. Network data reaches the game only
/// through here.
pub fn check_active(active: i32, sender: &[i32; LOADOUT_SLOTS], replica: &[i32; LOADOUT_SLOTS]) -> Result<usize, ActiveSkip> {
    if active == -1 {
        return Err(ActiveSkip::Unknown);
    }
    let slot = prosthetic_slot(active).ok_or(ActiveSkip::OutOfRange)?;
    let id = sender[slot];
    if id == BARE_HANDS || !matches!(row_of(slot, id), Ok((SlotKind::Weapon, _))) {
        return Err(ActiveSkip::NotATool);
    }
    if replica[slot] != id {
        return Err(ActiveSkip::NotDressed);
    }
    Ok(slot)
}

/// The local ChrAsm values look like the layout found statically: the class vtable, an
/// arm style 0..=3 (ARM_STYLE_* in c0000_define.lua) and an index 0..=2.
pub fn asm_sane(vtable_ok: bool, arm_style: i32, active: i32) -> bool {
    vtable_ok && (0..=3).contains(&arm_style) && prosthetic_slot(active).is_some()
}

/// ChrAsm at `asm`: (arm style, active left index) when its vtable is the ChrAsm class.
fn asm_read(asm: usize) -> Option<(bool, i32, i32)> {
    let vt_ok = read_ptr(asm)? == module_base() + CHR_ASM_VTABLE_RVA;
    Some((vt_ok, read::<i32>(asm + ASM_ARM_STYLE)?, read::<i32>(asm + ASM_ACTIVE_LEFT)?))
}

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

/// Sender side of the active prosthetic: the runtime check of the static finding.
struct LocalActive {
    /// The check failed `LOCAL_MISMATCH_LIMIT` ticks in a row: -1 is sent from then on.
    disabled: bool,
    bad_ticks: u32,
    last: Option<i32>,
}

static LOCAL_ACTIVE: Mutex<LocalActive> = Mutex::new(LocalActive { disabled: false, bad_ticks: 0, last: None });

/// The local player's active prosthetic index (0..=2) to send, -1 when unknown or disabled.
fn local_active(pgd: usize) -> i32 {
    let Ok(mut st) = LOCAL_ACTIVE.lock() else { return -1 };
    if st.disabled {
        return -1;
    }
    let asm = asm_read(pgd + PGD_CHR_ASM);
    let holder = crate::game::player()
        .and_then(|c| read_ptr(c.0 + CHR_ASM_HOLDER))
        .filter(|&h| h >= 0x10000)
        .and_then(|h| read::<i32>(h + ASM_ACTIVE_LEFT));
    let verdict = match asm {
        Some((vt, arm, active)) if asm_sane(vt, arm, active) => match holder {
            // The holder is the PGD's copy (0xA37560) or its source (0xA26150).
            Some(h) if h != active => Err(format!("holder index {h} != PGD index {active}")),
            _ => Ok(active),
        },
        Some((vt, arm, active)) => Err(format!("vtable ok {vt}, arm style {arm}, index {active}")),
        None => Err("ChrAsm unreadable".into()),
    };
    match verdict {
        Ok(active) => {
            if st.bad_ticks > 0 && st.last.is_none() {
                log::line(&format!("loadout: local ChrAsm check passed after {} bad tick(s)", st.bad_ticks));
            }
            st.bad_ticks = 0;
            st.last = Some(active);
            active
        }
        Err(why) => {
            st.bad_ticks += 1;
            if st.bad_ticks >= LOCAL_MISMATCH_LIMIT {
                st.disabled = true;
                log::line(&format!(
                    "loadout: active prosthetic DISABLED: the local ChrAsm (PGD+0x310) failed its check {} ticks in a row ({why}); index not sent, peers keep their replica's slot",
                    st.bad_ticks
                ));
            }
            -1
        }
    }
}

/// Receiver side, per peer.
#[derive(Clone, Copy)]
struct Follow {
    /// The replica this state is about (a respawned replica starts over).
    chr: usize,
    /// After writing the index: when to check that the holder followed, and the index.
    check: Option<(Instant, i32)>,
    /// The last skip logged (index, reason), so a repeating one is logged once.
    logged: Option<(i32, ActiveSkip)>,
}

const FOLLOW_NONE: Follow = Follow { chr: 0, check: None, logged: None };
static FOLLOW: Mutex<[Follow; MAX_PLAYERS]> = Mutex::new([FOLLOW_NONE; MAX_PLAYERS]);
/// A replica's ChrAsm failed the layout check: this feature stays off on this machine.
static REPLICA_ASM_BAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Replica ChrAsm checks failed in a row (any replica); `REPLICA_ASM_MISS_LIMIT` turns it off.
static REPLICA_ASM_MISSES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// About 1 s of dress ticks.
const REPLICA_ASM_MISS_LIMIT: u32 = 60;

/// native.rs (game thread, the replica in the player chr set): put the active prosthetic
/// `l` carries on `peer`'s replica `chr` (PGD `pgd`). Writes the replica PGD's ChrAsm index
/// only when it differs; the game's PlayerIns update copies it into the holder and swaps
/// the tool itself. Checks `FOLLOW_CHECK_AFTER` later that the holder followed; if not,
/// falls back to the dress refresh (sync_chr + parts reload). Call every tick.
pub fn follow_active(chr: Chr, pgd: usize, peer: PeerId, l: &Loadout) {
    use std::sync::atomic::Ordering;
    if REPLICA_ASM_BAD.load(Ordering::Relaxed) || pgd < 0x10000 || local_pgd() == Some(pgd) {
        return;
    }
    let Ok(mut all) = FOLLOW.lock() else { return };
    let Some(f) = all.get_mut(peer.index()) else { return };
    if f.chr != chr.0 {
        *f = Follow { chr: chr.0, ..FOLLOW_NONE };
    }
    let who = format!("peer slot {}", peer.slot());
    let asm = pgd + PGD_CHR_ASM;
    // A due check first: did the game copy the index into the holder?
    if let Some((at, want)) = f.check
        && Instant::now() >= at
    {
        f.check = None;
        let held = read_ptr(chr.0 + CHR_ASM_HOLDER).filter(|&h| h >= 0x10000).and_then(|h| read::<i32>(h + ASM_ACTIVE_LEFT));
        if held == Some(want) {
            log::line(&format!("loadout: {who} active prosthetic {want}: holder of {:#x} followed (game swap)", chr.0));
        } else {
            let sync = sync_chr(chr, pgd);
            let reload = sync.as_ref().map_err(|e| e.clone()).and_then(|_| crate::ghost::reload_parts(chr));
            log::line(&format!(
                "loadout: {who} active prosthetic {want}: holder of {:#x} did not follow ({held:?}), fallback sync {:?}, parts reload {:?}",
                chr.0, sync, reload
            ));
        }
    }
    let Some((vt, arm, now)) = asm_read(asm) else { return };
    if !asm_sane(vt, arm, now) {
        // Not on the first miss: the first call comes from the dress in the creation frame,
        // on a PGD just filled from CharaInitParam. Only a lasting failure turns it off.
        let n = REPLICA_ASM_MISSES.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 || n >= REPLICA_ASM_MISS_LIMIT {
            log::line(&format!(
                "loadout: replica {:#x} ChrAsm (PGD {pgd:#x} +0x310) fails the check (vtable ok {vt}, arm style {arm}, index {now}), miss {n}{}",
                chr.0,
                if n >= REPLICA_ASM_MISS_LIMIT { ": active prosthetic DISABLED on this side" } else { "" }
            ));
        }
        if n >= REPLICA_ASM_MISS_LIMIT {
            REPLICA_ASM_BAD.store(true, Ordering::Relaxed);
        }
        return;
    }
    REPLICA_ASM_MISSES.store(0, Ordering::Relaxed);
    let want = l.active_prosthetic();
    let Some(replica) = ids_of(pgd) else { return };
    match check_active(want, &l.slots, &replica) {
        Ok(slot) => {
            f.logged = None;
            if now == want {
                return;
            }
            if !crate::game::write::<i32>(asm + ASM_ACTIVE_LEFT, want) {
                log::line(&format!("loadout: {who} active prosthetic {now} -> {want} not written (PGD {pgd:#x})"));
                return;
            }
            f.check = Some((Instant::now() + FOLLOW_CHECK_AFTER, want));
            log::line(&format!("loadout: {who} active prosthetic {now} -> {want} applied on {:#x} (ChrAsm slot {slot}, id {})", chr.0, l.slots[slot]));
        }
        Err(why) => {
            if f.logged != Some((want, why)) {
                f.logged = Some((want, why));
                // Unknown is the normal state for an older peer: said once, quietly.
                log::line(&format!("loadout: {who} active prosthetic {want} skipped ({why:?}), replica keeps {now}"));
            }
        }
    }
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
        if let Ok(mut f) = FOLLOW.lock() {
            f[i] = FOLLOW_NONE;
        }
    }

    if !tuning::loadout_sync() || peers.is_empty() {
        return;
    }
    let Some(pgd) = local_pgd() else { return };
    let Some(ids) = ids_of(pgd) else { return };
    // The combat art is slot 1; `art` and `extra[1..]` stay reserved (-1). A changed active
    // prosthetic makes `changed` true: sent at once, not on the refresh cadence.
    let mut ours = Loadout::new(ids);
    ours.extra[LOADOUT_EXTRA_ACTIVE_PROSTHETIC] = local_active(pgd);
    let Ok(mut sent) = SENT.lock() else { return };
    let now = Instant::now();
    let changed = sent.last != Some(ours);
    let due = sent.at.is_none_or(|t| now.duration_since(t) >= REFRESH_EVERY) && net.pending_bulk_chunks() == 0;
    if (changed || due) && net.send_bulk(BULK_LOADOUT, &encode_loadout(&ours)) {
        if sent.last.map(|l| l.slots) != Some(ids) {
            log::line(&format!("loadout: sent ids {ids:?}"));
        }
        let active = ours.active_prosthetic();
        if sent.last.map(|l| l.active_prosthetic()) != Some(active) {
            match prosthetic_slot(active) {
                Some(slot) => log::line(&format!("loadout: sent active prosthetic {active} (ChrAsm slot {slot}, id {})", ids[slot])),
                None => log::line(&format!("loadout: sent active prosthetic {active} (unknown)")),
            }
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

    #[test]
    fn prosthetic_slots_are_even() {
        assert_eq!(prosthetic_slot(0), Some(0));
        assert_eq!(prosthetic_slot(1), Some(2));
        assert_eq!(prosthetic_slot(2), Some(4));
        assert_eq!(prosthetic_slot(3), None);
        assert_eq!(prosthetic_slot(-1), None);
        assert_eq!(prosthetic_slot(i32::MIN), None);
    }

    #[test]
    fn check_active_validates_network_index() {
        // Dump: index 1 = slot 2 = 73000.
        assert_eq!(check_active(1, &DUMP, &DUMP), Ok(2));
        assert_eq!(check_active(0, &DUMP, &DUMP), Ok(0));
        assert_eq!(check_active(2, &DUMP, &DUMP), Ok(4));
        assert_eq!(check_active(-1, &DUMP, &DUMP), Err(ActiveSkip::Unknown));
        assert_eq!(check_active(3, &DUMP, &DUMP), Err(ActiveSkip::OutOfRange));
        assert_eq!(check_active(-2, &DUMP, &DUMP), Err(ActiveSkip::OutOfRange));
        assert_eq!(check_active(i32::MAX, &DUMP, &DUMP), Err(ActiveSkip::OutOfRange));
        let mut sender = DUMP;
        sender[2] = -1; // empty
        assert_eq!(check_active(1, &sender, &DUMP), Err(ActiveSkip::NotATool));
        sender[2] = 110_000; // bare hands: the game's switch skips it too
        assert_eq!(check_active(1, &sender, &DUMP), Err(ActiveSkip::NotATool));
        sender[2] = 0x1000_0000 | 100_000; // a protector id
        assert_eq!(check_active(1, &sender, &DUMP), Err(ActiveSkip::NotATool));
        // The sender's tool is not on the replica yet.
        let mut sender = DUMP;
        sender[4] = 71000;
        assert_eq!(check_active(2, &sender, &DUMP), Err(ActiveSkip::NotDressed));
        let mut replica = DUMP;
        replica[4] = 71000;
        assert_eq!(check_active(2, &sender, &replica), Ok(4));
    }

    #[test]
    fn asm_sanity() {
        // Dump: arm style 1, left index 1.
        assert!(asm_sane(true, 1, 1));
        assert!(asm_sane(true, 0, 0));
        assert!(asm_sane(true, 3, 2));
        assert!(!asm_sane(false, 1, 1));
        assert!(!asm_sane(true, 4, 1));
        assert!(!asm_sane(true, -1, 1));
        assert!(!asm_sane(true, 1, 3));
        assert!(!asm_sane(true, 1, -1));
    }

    #[test]
    fn active_prosthetic_round_trips_in_the_loadout_body() {
        let mut l = Loadout::new(DUMP);
        l.extra[LOADOUT_EXTRA_ACTIVE_PROSTHETIC] = 1;
        let d = decode_loadout(&encode_loadout(&l)).unwrap();
        assert_eq!(d, l);
        assert_eq!(check_active(d.active_prosthetic(), &d.slots, &DUMP), Ok(2));
        // An older build's body: -1, the replica keeps its slot.
        let old = decode_loadout(&encode_loadout(&Loadout::new(DUMP))).unwrap();
        assert_eq!(check_active(old.active_prosthetic(), &old.slots, &DUMP), Err(ActiveSkip::Unknown));
    }
}
