//! Wardrobe (FORK.md "Wardrobe"): mix-and-match outfit parts on the LOCAL player, changed
//! live from a small overlay panel (hotkey `wardrobe_key`, default F2).
//!
//! Outfits are EquipParamProtector rows in the protector slots 12..16 of the 25-slot
//! EquipGameData (`PGD+0x278`): 12 head, 13 torso, 14 arm, 15 legs, 16 the fifth protector
//! kind (`0x79E230(egd, k)` reads kinds 0..4 = slots 12..16; Shura's hair row 304000 goes
//! there [ARV]). Two choices: the skin (torso + arm + legs, and the default head) and the
//! head (with the head's own hair in slot 16). Legs always follow the skin (user decision
//! 2026-11-13 after live testing: mixed legs never matched anything).
//!
//! How the game itself changes the local player's armour (static RE, dump 2026-11-11, Ghidra
//! copy `E:\RE\ghidra_wardrobe`) [VAR]:
//! - EMEVD 2003[83] `ForcePlayerArmorEquip(type, id)` = case 0x53 of the bank-2003 dispatcher
//!   `0x904EF0`: egd = local PGD + 0x278; id < 0 -> `0x79D100(egd, 12 + type, 1)`; else
//!   `idx = 0x79D7E0(egd, &(id | 0x1000_0000))` and ONLY when found (no add): the inventory
//!   entry's handle (`0xC3C9B0` / `0xC3CB00`), `0x79F8A0(egd, 12 + type, &h, idx, 0, 1, 0)`,
//!   `0xC3CA40` (a `ret 0`). Type Body with id 101000 keeps the costume family:
//!   `(current / 100000) * 100000 + 1000`.
//! - The same sequence: New Game+ (`0xA8D130`, table `0x2A458A0` = 100000 141000 112000
//!   103000) and the temporary Mortal Blade of the reflection mode (`0xA917B0` adds with
//!   `0x79A710`, `0xA919C0` / `0xA91300` re-equip x31000 and remove with `0x79ECA0`).
//! - None of them reloads parts: the per-frame PlayerIns update `0xA37560` copies the PGD's
//!   ChrAsm into the holder (`0x9CC9A0`) but reacts only to weapon-slot changes (EGD+0x88
//!   flags, `0xA2ACF0`); the part loader `0xA361C0` runs from PlayerIns vtable slots 8, 21,
//!   27, 33, 35 and the cutscene loader. So after equipping we copy the holder as the equip
//!   menu does (`0x9CCEE0(holder, pgd)`, `0xD592F0`) and, one frame later, reload the parts
//!   through slot 21 (`ghost::reload_parts`; on replicas it ran live many times, test
//!   2026-11-12, never on the local player before) [ARV]. `wardrobe_reload = 0` skips it.
//!
//! Rows the player does not own (Sculptor's set, 153000, costumes before their Memory) are
//! added to the inventory like `0xA917B0` does and listed in `wardrobe_added`; "As in game"
//! re-equips the four/five original ids (`wardrobe_original`) and removes the added rows
//! that are not equipped and were not awarded by the game since (their item-lot flag).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, Chr, module_base, read, read_ptr};
use crate::{log, settings};

// ---------------------------------------------------------------------------------------
// Pure part: rows, choices, compatibility, ini text.
// ---------------------------------------------------------------------------------------

/// A protector item id carries these category bits in EquipGameData.
const CAT_PROTECTOR: i32 = 0x1000_0000;
/// ChrAsm / EquipGameData slot of the first protector (head); torso, arm, legs and the fifth
/// kind (hair, `loadout::SLOT_EMPTIABLE`) follow.
pub const SLOT_HEAD: usize = 12;
const _: () = assert!(SLOT_HEAD + 4 == crate::loadout::SLOT_EMPTIABLE);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skin {
    Wolf,
    Shura,
    Ashina,
    Tengu,
    Sculptor,
}

pub const SKINS: [Skin; 5] = [Skin::Wolf, Skin::Shura, Skin::Ashina, Skin::Tengu, Skin::Sculptor];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TorsoClass {
    /// x01000: the Mortal Blade on the back.
    MortalBlade,
    /// x31000: the sword at the waist, no Mortal Blade.
    Sword,
    /// 141000 (Wolf only): no sword at all (start of the game, New Game+).
    NoSword,
}

impl Skin {
    pub fn label(self) -> &'static str {
        match self {
            Skin::Wolf => "Wolf",
            Skin::Shura => "Shura",
            Skin::Ashina => "Ashina shinobi",
            Skin::Tengu => "Tengu",
            Skin::Sculptor => "Sculptor",
        }
    }

    pub fn ini(self) -> &'static str {
        match self {
            Skin::Wolf => "wolf",
            Skin::Shura => "shura",
            Skin::Ashina => "ashina",
            Skin::Tengu => "tengu",
            Skin::Sculptor => "sculptor",
        }
    }

    pub fn head(self) -> i32 {
        match self {
            Skin::Wolf => 100_000,
            Skin::Shura => 300_000,
            Skin::Ashina => 400_000,
            Skin::Tengu => 500_000,
            Skin::Sculptor => 200_000,
        }
    }

    /// The fifth protector slot: Shura's hair with Shura's head, else empty.
    pub fn hair(self) -> i32 {
        if self == Skin::Shura { 304_000 } else { -1 }
    }

    /// The prosthetic arm. The Sculptor's own arm (202000) has no left arm, so the Wolf's.
    pub fn arm(self) -> i32 {
        match self {
            Skin::Wolf | Skin::Sculptor => 102_000,
            Skin::Shura => 302_000,
            Skin::Ashina => 402_000,
            Skin::Tengu => 502_000,
        }
    }

    /// The legs row of this skin: legs are not a choice of their own, they follow the skin
    /// like torso and arm.
    pub fn legs(self) -> i32 {
        match self {
            Skin::Wolf => 103_000,
            Skin::Shura => 303_000,
            Skin::Ashina => 403_000,
            Skin::Tengu => 503_000,
            Skin::Sculptor => 203_000,
        }
    }

    /// The torso row of this skin for the sword / Mortal Blade state. Only the Wolf has a
    /// no-sword torso (141000); the others take their x31000 (no Mortal Blade). The
    /// Sculptor has one torso (201000, the cutscene model).
    pub fn torso(self, class: TorsoClass) -> i32 {
        let base = match self {
            Skin::Wolf => 100_000,
            Skin::Shura => 300_000,
            Skin::Ashina => 400_000,
            Skin::Tengu => 500_000,
            Skin::Sculptor => return 201_000,
        };
        match class {
            TorsoClass::MortalBlade => base + 1000,
            TorsoClass::Sword => base + 31_000,
            TorsoClass::NoSword if self == Skin::Wolf => 141_000,
            TorsoClass::NoSword => base + 31_000,
        }
    }
}

/// The skins the panel offers [VAR data, docs/RE-REMOTE-FX.md "Гардероб"]. Legs follow the
/// skin, so the leg / torso texture pairs (Tengu's torso `bd_m_9130` samples
/// `LG_M_9130_Leg_a` from Tengu's legs; Shura's legs `lg_m_9110` take their fire texture from
/// Shura's torso `bd_m_9110`) always come together now.
///
/// The Sculptor's skin and head are never offered (user decision 2026-11-12: the head barely
/// differs from the Wolf's, the torso / legs 201000 / 203000 are cutscene-only models that
/// render dark and nearly untextured, video 03:22). An ini value naming one falls back like
/// any refused choice; the rows stay known (`Skin::Sculptor`) so `wardrobe_added` / "As in
/// game" still remove 200000-203000.
pub fn skin_offered(skin: Skin) -> bool {
    skin != Skin::Sculptor
}

/// Every head except the Sculptor's goes with every skin, with no locks (user decision
/// 2026-11-13). Known cosmetic caveat, accepted by the user: Shura's head `fc_m_0240` (300000)
/// draws its fire overlay (`P_BD_M_9110_Fire*.mtd`) with `BD_M_9110_Fire_B_*`, which lives in
/// Shura's torso `bd_m_9110`: on any other skin the head lacks that texture (test4 video
/// 00:44, Wolf skin + Shura head: a translucent glowing bluish shell). The `fc_m_0200`
/// textures used by the Ashina / Tengu / Shura heads are always loaded: those render fine.
pub fn head_offered(head: Skin) -> bool {
    head != Skin::Sculptor
}

/// The sword state a worn torso says, None for a torso that does not tell (Sculptor 201000,
/// 641000, test rows, empty).
pub fn classify_torso(row: i32) -> Option<TorsoClass> {
    if !matches!(row / 100_000, 1 | 3 | 4 | 5) || row < 0 {
        return None;
    }
    match row % 100_000 {
        1000 => Some(TorsoClass::MortalBlade),
        31_000 => Some(TorsoClass::Sword),
        41_000 => Some(TorsoClass::NoSword),
        _ => None,
    }
}

/// An arm that has the prosthetic. 112000 (bare arm, before the prosthetic) and 122000
/// (arm missing, cutscenes) are story states: the wardrobe leaves those alone.
pub fn prosthetic_arm(row: i32) -> bool {
    matches!(row, 102_000 | 162_000 | 302_000 | 402_000 | 502_000 | 602_000)
}

/// The five protector rows (no category bits; -1 = empty).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outfit {
    pub head: i32,
    pub torso: i32,
    pub arm: i32,
    pub legs: i32,
    pub hair: i32,
}

impl Outfit {
    /// From the 25 EquipGameData ids (slots 12..16).
    pub fn from_ids(ids: &[i32; 25]) -> Self {
        let r = |id: i32| if id < 0 { -1 } else { id & 0x0FFF_FFFF };
        Self { head: r(ids[12]), torso: r(ids[13]), arm: r(ids[14]), legs: r(ids[15]), hair: r(ids[16]) }
    }

    pub fn rows(&self) -> [i32; 5] {
        [self.head, self.torso, self.arm, self.legs, self.hair]
    }

    pub fn from_rows(r: [i32; 5]) -> Self {
        Self { head: r[0], torso: r[1], arm: r[2], legs: r[3], hair: r[4] }
    }

    /// The full item id of a row as EquipGameData stores it.
    pub fn full_id(row: i32) -> i32 {
        if row < 0 { -1 } else { row | CAT_PROTECTOR }
    }
}

/// The two choices. `skin == None` = as in game (and a restore of the original outfit when
/// the wardrobe changed it); `head` None = as the skin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Choices {
    pub skin: Option<Skin>,
    pub head: Option<Skin>,
}

/// The outfit the choices ask for, None while the skin is "as in game". `class` = the sword
/// state the player is in, `current` = what is worn now (its arm decides whether the arm is
/// touched). Torso, arm and legs are always the skin's; the hair slot follows the head.
/// A skin / head choice that is not offered (set by hand in the ini, or kept from an older
/// build) falls back: skin -> as in game, head -> as the skin. An older build's
/// `wardrobe_legs` is not read at all: the skin's legs are put on.
pub fn resolve(c: &Choices, class: TorsoClass, current: &Outfit) -> Option<Outfit> {
    let skin = c.skin.filter(|&s| skin_offered(s))?;
    let head = c.head.filter(|&h| head_offered(h)).unwrap_or(skin);
    Some(Outfit {
        head: head.head(),
        torso: skin.torso(class),
        arm: if prosthetic_arm(current.arm) { skin.arm() } else { current.arm },
        legs: skin.legs(),
        hair: head.hair(),
    })
}

/// Panel rows.
pub const ROW_SKIN: usize = 0;
pub const ROW_HEAD: usize = 1;
pub const ROWS: usize = 2;

/// The choices after moving row `row` by `dir` (+1 / -1, wraps). Both lists start with their
/// "as in game" / "as skin" entry; the head list is the same for every skin, and a skin change
/// keeps the chosen head.
pub fn cycle(c: Choices, row: usize, dir: i32) -> Choices {
    fn step<T: Copy + PartialEq>(list: &[Option<T>], now: Option<T>, dir: i32) -> Option<T> {
        let i = list.iter().position(|&x| x == now).unwrap_or(0) as i32;
        list[(i + dir).rem_euclid(list.len() as i32) as usize]
    }
    let mut c = c;
    match row {
        ROW_SKIN => {
            let list: Vec<Option<Skin>> = std::iter::once(None).chain(SKINS.into_iter().filter(|&s| skin_offered(s)).map(Some)).collect();
            c.skin = step(&list, c.skin.filter(|&s| skin_offered(s)), dir);
        }
        ROW_HEAD => {
            let list: Vec<Option<Skin>> = std::iter::once(None).chain(SKINS.into_iter().filter(|&h| head_offered(h)).map(Some)).collect();
            c.head = step(&list, c.head.filter(|&h| head_offered(h)), dir);
        }
        _ => {}
    }
    c
}

pub fn skin_label(s: Option<Skin>) -> &'static str {
    s.map_or("As in game", Skin::label)
}

pub fn head_label(h: Option<Skin>) -> &'static str {
    h.map_or("As skin", Skin::label)
}

pub fn skin_ini(s: Option<Skin>) -> &'static str {
    s.map_or("game", Skin::ini)
}

pub fn head_ini(h: Option<Skin>) -> &'static str {
    h.map_or("skin", Skin::ini)
}

pub fn parse_skin(v: &str) -> Result<Option<Skin>, ()> {
    let v = v.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("game") {
        return Ok(None);
    }
    SKINS.into_iter().find(|s| s.ini().eq_ignore_ascii_case(v)).map(Some).ok_or(())
}

pub fn parse_head(v: &str) -> Result<Option<Skin>, ()> {
    let v = v.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("skin") {
        return Ok(None);
    }
    SKINS.into_iter().find(|s| s.ini().eq_ignore_ascii_case(v)).map(Some).ok_or(())
}

/// A protector row as it may appear in the ini lists (no category bits).
fn sane_row(r: i32) -> bool {
    (0..0x0FFF_FFFF).contains(&r)
}

/// `head,torso,arm,legs,hair` rows; empty = none.
pub fn outfit_ini(o: Option<Outfit>) -> String {
    o.map(|o| o.rows().iter().map(i32::to_string).collect::<Vec<_>>().join(",")).unwrap_or_default()
}

pub fn parse_outfit(v: &str) -> Result<Option<Outfit>, ()> {
    let v = v.trim();
    if v.is_empty() {
        return Ok(None);
    }
    let n: Vec<i32> = v.split(',').map(|p| p.trim().parse::<i32>()).collect::<Result<_, _>>().map_err(|_| ())?;
    let r: [i32; 5] = n.try_into().map_err(|_| ())?;
    // Head, torso, arm, legs must be rows; the hair slot may be empty.
    if !r[..4].iter().all(|&x| sane_row(x)) || !(r[4] == -1 || sane_row(r[4])) {
        return Err(());
    }
    Ok(Some(Outfit::from_rows(r)))
}

pub fn rows_ini(rows: &[i32]) -> String {
    rows.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

pub fn parse_rows(v: &str) -> Result<Vec<i32>, ()> {
    let v = v.trim();
    if v.is_empty() {
        return Ok(Vec::new());
    }
    let n: Vec<i32> = v.split(',').map(|p| p.trim().parse::<i32>()).collect::<Result<_, _>>().map_err(|_| ())?;
    if n.iter().all(|&x| sane_row(x)) { Ok(n) } else { Err(()) }
}

/// The event flag the game sets when it awards this row (ItemLotParam `getItemFlagId`):
/// such a row belongs to the player even if the wardrobe added it first.
pub fn game_lot_flag(row: i32) -> Option<u32> {
    Some(match row {
        102_000 => 50_003_070,
        131_000 => 50_003_080,
        101_000 => 50_003_090,
        300_000 => 50_003_400,
        301_000 => 50_003_401,
        331_000 => 50_003_402,
        302_000 => 50_003_403,
        303_000 => 50_003_404,
        304_000 => 50_003_405,
        400_000 => 50_003_410,
        401_000 => 50_003_411,
        431_000 => 50_003_412,
        402_000 => 50_003_413,
        403_000 => 50_003_414,
        500_000 => 50_003_420,
        501_000 => 50_003_421,
        531_000 => 50_003_422,
        502_000 => 50_003_423,
        503_000 => 50_003_424,
        _ => return None,
    })
}

/// The worn outfit differs from the one we put on: who did it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum External {
    /// Only torso / arm: a story event (Mortal Blade, the prosthetic, EMEVD 2003[83]):
    /// keep the choices, take the new sword / arm state, put the skin on again.
    Story,
    /// Head, legs or the hair slot too: the game's own Change Form menu or New Game+.
    /// The wardrobe steps back to "as in game".
    Yield,
}

pub fn external(applied: &Outfit, now: &Outfit) -> External {
    if applied.head == now.head && applied.legs == now.legs && applied.hair == now.hair { External::Story } else { External::Yield }
}

/// The torso of costume family `base` (100000, 300000, ...) for a sword state (the game's
/// own `(cur / 100000) * 100000 + 1000 / 31000` family rule; only the Wolf has x41000).
fn family_torso(base: i32, class: TorsoClass) -> i32 {
    match class {
        TorsoClass::MortalBlade => base + 1000,
        TorsoClass::Sword => base + 31_000,
        TorsoClass::NoSword if base == 100_000 => 141_000,
        TorsoClass::NoSword => base + 31_000,
    }
}

/// What "As in game" puts back: the remembered original, brought up to the story state the
/// character is in NOW. The story changes torso / arm while a skin is worn (Mortal Blade, the
/// prosthetic), also in sessions without this build, so a plain copy of the original could
/// take the Mortal Blade off the back or the prosthetic off the arm in the save.
/// - torso: the original's family with the sword state the worn torso tells (kept when the
///   worn torso tells nothing, e.g. the Sculptor's);
/// - arm: a bare / missing original arm becomes its family's prosthetic arm once the worn arm
///   has a prosthetic (the Wolf's 102000 when the family has none).
pub fn restore_target(orig: &Outfit, current: &Outfit) -> Outfit {
    let mut o = *orig;
    if let (Some(now), Some(_)) = (classify_torso(current.torso), classify_torso(orig.torso)) {
        o.torso = family_torso(orig.torso / 100_000 * 100_000, now);
    }
    if prosthetic_arm(current.arm) && !prosthetic_arm(orig.arm) {
        let fam = orig.arm / 100_000 * 100_000 + 2000;
        o.arm = if prosthetic_arm(fam) { fam } else { 102_000 };
    }
    o
}

/// The character the wardrobe's records (`wardrobe_original`, `wardrobe_added`) belong to:
/// the save slot (GameMan+0xAE4, 0..9) and its `total_sen_earned` then (ownworld.rs
/// fingerprint: only grows for one character). One ini serves every character of the save
/// file, so another character must never get this one's original outfit or lose items to
/// this one's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub slot: i32,
    pub sen: Option<u64>,
}

impl Owner {
    /// Same slot, and (when both are known) not fewer sen than when it was recorded.
    pub fn matches(&self, slot: Option<i32>, sen: Option<u64>) -> bool {
        slot == Some(self.slot) && match (self.sen, sen) {
            (Some(then), Some(now)) => now >= then,
            _ => true,
        }
    }
}

pub fn owner_ini(o: Option<Owner>) -> String {
    match o {
        None => String::new(),
        Some(Owner { slot, sen: None }) => slot.to_string(),
        Some(Owner { slot, sen: Some(s) }) => format!("{slot},{s}"),
    }
}

pub fn parse_owner(v: &str) -> Result<Option<Owner>, ()> {
    let v = v.trim();
    if v.is_empty() {
        return Ok(None);
    }
    let mut it = v.split(',').map(str::trim);
    let slot: i32 = it.next().ok_or(())?.parse().map_err(|_| ())?;
    let sen = match it.next() {
        Some(s) => Some(s.parse::<u64>().map_err(|_| ())?),
        None => None,
    };
    if it.next().is_some() || !(0..=9).contains(&slot) {
        return Err(());
    }
    Ok(Some(Owner { slot, sen }))
}

// ---------------------------------------------------------------------------------------
// Shared state (game task + overlay render thread).
// ---------------------------------------------------------------------------------------

/// Apply this long after the last change of a choice (cycling through options).
const DEBOUNCE: Duration = Duration::from_millis(200);
/// No two applies closer than this (each one reloads the parts).
const MIN_GAP: Duration = Duration::from_millis(400);
/// No part reload this soon after a load (new models / materials: docs IMPLEMENTATION-NOTES
/// gpuguard section; the driver crash fired most right after loads).
const AFTER_LOAD: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// The hotkey / pad button.
    Toggle,
    Close,
    /// -1 up, +1 down.
    Row(i32),
    /// -1 left, +1 right on the selected row.
    Cycle(i32),
    /// Enter / right trigger: no debounce wait.
    ApplyNow,
}

struct State {
    /// `wardrobe = 1`.
    enabled: bool,
    /// The game functions did not match the dump: the feature is off for this run.
    disabled: Option<String>,
    checked: bool,
    choices: Choices,
    /// What the ini said last time (read or written): a reload with the same text keeps
    /// panel edits not yet written.
    ini_seen: [String; 5],
    original: Option<Outfit>,
    added: Vec<i32>,
    /// The character `original` / `added` belong to (`wardrobe_owner`).
    owner: Option<Owner>,
    /// Logged once: the records belong to another character.
    foreign_logged: bool,
    /// Sword state, kept while a torso that does not tell it is worn.
    class: TorsoClass,
    /// What we last put on (or found matching the choices).
    applied: Option<Outfit>,
    open: bool,
    row: usize,
    edited_at: Option<Instant>,
    apply_now: bool,
    ini_dirty: bool,
    last_apply: Option<Instant>,
    /// Parts reload due on the next tick (the holder must hold the new handles first).
    reload_due: bool,
    status: String,
    /// The last "waiting" reason logged (logged once per reason).
    waiting_logged: Option<&'static str>,
    /// Rows refused (not in the param), logged once each.
    refused_logged: Vec<i32>,
    /// An outfit that was put on but did not fully take.
    failed: Option<Outfit>,
}

static STATE: Mutex<State> = Mutex::new(State {
    enabled: true,
    disabled: None,
    checked: false,
    choices: Choices { skin: None, head: None },
    ini_seen: [const { String::new() }; 5],
    original: None,
    added: Vec::new(),
    owner: None,
    foreign_logged: false,
    class: TorsoClass::Sword,
    applied: None,
    open: false,
    row: 0,
    edited_at: None,
    apply_now: false,
    ini_dirty: false,
    last_apply: None,
    reload_due: false,
    status: String::new(),
    waiting_logged: None,
    refused_logged: Vec::new(),
    failed: None,
});

/// `wardrobe_key` (virtual-key code; 113 = F2).
pub static KEY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(113);
/// `wardrobe_pad`: the right stick drives the open panel.
pub static PAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// `wardrobe_pad_button`: XInput mask that opens / closes the panel (0 = none).
pub static PAD_BUTTON: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
/// `wardrobe_reload`: reload the parts after a change (PlayerIns vtable slot 21). Off by
/// default: never run on the local player live; with 0 the wardrobe does exactly what the
/// game's own EMEVD ForcePlayerArmorEquip does (no reload either).
pub static RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The panel is open (read without the lock: `message_filter` runs on the window thread).
static OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A load is pending or under way (game thread: set by `tick` / `on_world_reset`, cleared by
/// the first in-world `tick` without a pending load). The panel does not open meanwhile.
static LOADING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn set_open(st: &mut State, open: bool) {
    st.open = open;
    OPEN.store(open && st.enabled, std::sync::atomic::Ordering::Relaxed);
}

/// A loading screen starts: the panel closes. Left open it kept holding the keyboard through
/// the load (`ui.rs` `message_filter`), and the F7 / F11 keys did not get through (live
/// 2026-11-13).
fn close_for_load(why: &str) {
    LOADING.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut st) = STATE.lock()
        && st.open
    {
        set_open(&mut st, false);
        log::line(&format!("wardrobe: panel closed ({why})"));
    }
}

/// The ini values (settings.rs reads, this parses). Returns warnings.
pub struct IniValues<'a> {
    pub enabled: bool,
    pub skin: &'a str,
    pub head: &'a str,
    pub original: &'a str,
    pub added: &'a str,
    pub owner: &'a str,
}

/// settings.rs `apply`, every (re)load of the ini.
pub fn load_ini(v: IniValues<'_>, warn: &mut Vec<String>) {
    let Ok(mut st) = STATE.lock() else { return };
    st.enabled = v.enabled;
    if !v.enabled {
        set_open(&mut st, false);
    }
    let seen = [v.skin, v.head, v.original, v.added, v.owner].map(str::to_owned);
    if st.ini_seen == seen {
        return;
    }
    let first = st.ini_seen.iter().all(String::is_empty);
    st.ini_seen = seen;
    let owner = parse_owner(v.owner).unwrap_or_else(|_| {
        warn.push(format!("wardrobe_owner = {:?} is not 'save slot[,sen]', ignored", v.owner));
        None
    });
    let skin = parse_skin(v.skin).unwrap_or_else(|_| {
        warn.push(format!("wardrobe_skin = {:?} is not game/wolf/shura/ashina/tengu/sculptor, using game", v.skin));
        None
    });
    let head = parse_head(v.head).unwrap_or_else(|_| {
        warn.push(format!("wardrobe_head = {:?} is not skin/wolf/shura/ashina/tengu/sculptor, using skin", v.head));
        None
    });
    let original = parse_outfit(v.original).unwrap_or_else(|_| {
        warn.push(format!("wardrobe_original = {:?} is not five row ids, ignored", v.original));
        None
    });
    let added = parse_rows(v.added).unwrap_or_else(|_| {
        warn.push(format!("wardrobe_added = {:?} is not a list of row ids, ignored", v.added));
        Vec::new()
    });
    let choices = Choices { skin, head };
    if !first && choices != st.choices {
        log::line(&format!("wardrobe: choices from the ini: {}", describe(&choices)));
        st.edited_at = Some(Instant::now());
    }
    st.choices = choices;
    st.original = original;
    st.added = added;
    st.owner = owner;
    st.foreign_logged = false;
}

fn describe(c: &Choices) -> String {
    format!("skin {}, head {}", skin_ini(c.skin), head_ini(c.head))
}

/// Overlay: one input. Render thread; no game memory.
pub fn edit(e: Edit) {
    let Ok(mut st) = STATE.lock() else { return };
    if !st.enabled {
        return;
    }
    match e {
        Edit::Toggle => {
            if !st.open && LOADING.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            let open = !st.open;
            set_open(&mut st, open);
            log::line(if st.open { "wardrobe: panel opened" } else { "wardrobe: panel closed" });
        }
        Edit::Close => {
            if st.open {
                set_open(&mut st, false);
                log::line("wardrobe: panel closed");
            }
        }
        Edit::Row(d) if st.open => st.row = (st.row as i32 + d).rem_euclid(ROWS as i32) as usize,
        Edit::Cycle(d) if st.open => {
            let before = st.choices;
            st.choices = cycle(st.choices, st.row, d);
            if st.choices != before {
                log::line(&format!("wardrobe: choice changed: {}", describe(&st.choices)));
                st.edited_at = Some(Instant::now());
                st.ini_dirty = true;
            }
        }
        Edit::ApplyNow if st.open => st.apply_now = true,
        _ => {}
    }
}

pub fn panel_open() -> bool {
    OPEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// What the overlay draws.
pub struct View {
    pub choices: Choices,
    pub row: usize,
    pub status: String,
}

pub fn view() -> Option<View> {
    let st = STATE.lock().ok()?;
    (st.open && st.enabled).then(|| View { choices: st.choices, row: st.row, status: st.status.clone() })
}

// ---------------------------------------------------------------------------------------
// Game side.
// ---------------------------------------------------------------------------------------

const GAITEM_MAN_RVA: usize = 0x3D888A8;
/// PlayerGameData -> EquipGameData.
const PGD_EGD: usize = 0x278;
const PGD_CHR_ASM: usize = 0x310;
/// ChrAsm: the 25 gaitem handles.
const ASM_HANDLES: usize = 0x24;
/// PlayerIns -> its ChrAsm holder.
const CHR_ASM_HOLDER: usize = 0x2140;

const INV_FIND_RVA: usize = 0x79D7E0;
const INV_REF_RVA: usize = 0xC3C9B0;
const INV_REF_HANDLE_RVA: usize = 0xC3CB00;
const INV_REF_DROP_RVA: usize = 0xC3CA40;
const EQUIP_RVA: usize = 0x79F8A0;
const HANDLE_FROM_ID_RVA: usize = 0xC37740;
const INV_ADD_RVA: usize = 0x79A710;
const HANDLE_RELEASE_RVA: usize = 0xC396C0;
const INV_REMOVE_RVA: usize = 0x79ECA0;
const HOLDER_FROM_PGD_RVA: usize = 0x9CCEE0;

/// First bytes of every function called (dump 2026-11-11). A mismatch turns the wardrobe off.
const SITES: [(usize, &[u8]); 10] = [
    (INV_FIND_RVA, &[0x48, 0x81, 0xC1, 0x68, 0x01, 0x00, 0x00, 0xE9]),
    (INV_REF_RVA, &[0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83, 0xEC, 0x20]),
    (INV_REF_HANDLE_RVA, &[0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83, 0xEC, 0x30]),
    (INV_REF_DROP_RVA, &[0xC2, 0x00, 0x00]),
    (EQUIP_RVA, &[0x40, 0x55, 0x53, 0x56, 0x57, 0x41, 0x54, 0x41, 0x56, 0x41, 0x57, 0x48]),
    (HANDLE_FROM_ID_RVA, &[0x48, 0x89, 0x54, 0x24, 0x10, 0x53, 0x48, 0x83, 0xEC, 0x30]),
    (INV_ADD_RVA, &[0x40, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57]),
    (HANDLE_RELEASE_RVA, &[0x48, 0x83, 0xEC, 0x38, 0x48, 0xC7, 0x44, 0x24, 0x20, 0xFE, 0xFF, 0xFF]),
    (INV_REMOVE_RVA, &[0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x6C, 0x24, 0x10]),
    (HOLDER_FROM_PGD_RVA, &[0x48, 0x85, 0xD2, 0x74, 0x0C, 0x48, 0x81, 0xC2, 0x10, 0x03, 0x00, 0x00]),
];

fn check_sites() -> Result<(), String> {
    let base = module_base();
    for (rva, want) in SITES {
        let mut got = vec![0u8; want.len()];
        if !game::read_bytes(base + rva, &mut got) || got != want {
            return Err(format!("bytes at {rva:#x} are {got:02x?}, expected {want:02x?}"));
        }
    }
    Ok(())
}

/// Why the outfit must not be touched now (None = fine).
fn blocker(player: Chr, in_world_for: Duration) -> Option<&'static str> {
    if game::load_pending() {
        return Some("loading");
    }
    if in_world_for < AFTER_LOAD {
        return Some("just loaded");
    }
    if let Err(why) = game::safe_for_reward(in_world_for) {
        return Some(why);
    }
    if crate::cutscene::active() {
        return Some("cutscene");
    }
    if !game::talk_idle() {
        return Some("talk");
    }
    if crate::enemies::throw_active(player) {
        return Some("throw / deathblow");
    }
    None
}

struct Calls {
    gaitems: usize,
    find: extern "C" fn(usize, *const i32) -> i32,
    inv_ref: extern "C" fn(*mut [u64; 2], usize, i32) -> usize,
    ref_handle: extern "C" fn(*const [u64; 2], *mut u32) -> *mut u32,
    ref_drop: extern "C" fn(*mut [u64; 2]),
    equip: extern "C" fn(usize, u32, *const u32, i32, i32, u8, u8),
    from_id: extern "C" fn(usize, *mut u32, *const u32) -> *mut u32,
    add: extern "C" fn(usize, u32, *const u32, i32, u64, u8, u8) -> i32,
    release: extern "C" fn(*mut u32),
    /// `0x79ECA0(egd, index, u8 release_handle, u8 refresh)`: unequips the entry where worn
    /// (`0x79E750` / `0x79D100`), drops it (`0x7A8550`); the returned value is always 0. A u8,
    /// not a bool: only al is set, and a bool must be 0 or 1 to be valid in Rust.
    remove: extern "C" fn(usize, i32, u8, u8) -> u8,
    holder_from_pgd: extern "C" fn(usize, usize),
}

fn calls() -> Option<Calls> {
    let base = module_base();
    let gaitems = read_ptr(base + GAITEM_MAN_RVA).filter(|&g| g >= 0x10000)?;
    // SAFETY: the prologues were checked against the dump (`check_sites`); signatures from
    // their call sites (EMEVD 2003[83], `0xA917B0`, `0xA919C0`, `0xD592F0`).
    unsafe {
        Some(Calls {
            gaitems,
            find: std::mem::transmute(base + INV_FIND_RVA),
            inv_ref: std::mem::transmute(base + INV_REF_RVA),
            ref_handle: std::mem::transmute(base + INV_REF_HANDLE_RVA),
            ref_drop: std::mem::transmute(base + INV_REF_DROP_RVA),
            equip: std::mem::transmute(base + EQUIP_RVA),
            from_id: std::mem::transmute(base + HANDLE_FROM_ID_RVA),
            add: std::mem::transmute(base + INV_ADD_RVA),
            release: std::mem::transmute(base + HANDLE_RELEASE_RVA),
            remove: std::mem::transmute(base + INV_REMOVE_RVA),
            holder_from_pgd: std::mem::transmute(base + HOLDER_FROM_PGD_RVA),
        })
    }
}

/// Puts `row` into `slot` of the local EGD the way EMEVD 2003[83] does; a row not in the
/// inventory is added first (`0xA917B0`'s way). -1 (the hair slot only) empties the slot as
/// `0x79D100` does for slots 17..21.
fn equip_row(c: &Calls, egd: usize, slot: usize, row: i32) -> Result<(&'static str, bool), String> {
    if row < 0 {
        let h = 0u32;
        (c.equip)(egd, slot as u32, &h, -1, 0, 1, 0);
        return Ok(("emptied", false));
    }
    let full = Outfit::full_id(row);
    let idx = (c.find)(egd, &full);
    if idx >= 0 {
        let mut r = [0u64; 2];
        let mut h = 0u32;
        (c.inv_ref)(&mut r, egd, idx);
        (c.ref_handle)(&r, &mut h);
        if h == 0 {
            (c.ref_drop)(&mut r);
            return Err(format!("inventory entry {idx} has no handle"));
        }
        (c.equip)(egd, slot as u32, &h, idx, 0, 1, 0);
        (c.ref_drop)(&mut r);
        return Ok(("from inventory", false));
    }
    let mut h = 0u32;
    (c.from_id)(c.gaitems, &mut h, &(full as u32));
    if h == 0 {
        return Err("no gaitem handle".into());
    }
    // count 1, no "new" mark, no auto-equip.
    let idx = (c.add)(egd, CAT_PROTECTOR as u32, &h, 1, 1, 0, 1);
    if idx >= 0 {
        (c.equip)(egd, slot as u32, &h, idx, 0, 1, 0);
    }
    (c.release)(&mut h);
    if idx < 0 {
        return Err(format!("inventory add returned {idx}"));
    }
    Ok(("added to inventory", true))
}

fn local_outfit(pgd: usize) -> Option<Outfit> {
    crate::loadout::ids_of(pgd).map(|ids| Outfit::from_ids(&ids))
}

/// Puts `want` on. Returns the rows added to the inventory.
fn put_on(player: Chr, pgd: usize, current: &Outfit, want: &Outfit) -> Result<Vec<i32>, String> {
    let c = calls().ok_or("gaitem manager null")?;
    let egd = pgd + PGD_EGD;
    let mut added = Vec::new();
    for (k, (&from, &to)) in current.rows().iter().zip(want.rows().iter()).enumerate() {
        if from == to {
            continue;
        }
        let slot = SLOT_HEAD + k;
        match equip_row(&c, egd, slot, to) {
            Ok((how, new)) => {
                if new {
                    added.push(to);
                }
                log::line(&format!("wardrobe: slot {slot} {from} -> {to} ({how})"));
            }
            Err(e) => log::line(&format!("wardrobe: slot {slot} {from} -> {to} NOT applied: {e}")),
        }
    }
    // The equip menu's holder copy for the local player (`0xD592F0`).
    if let Some(h) = read_ptr(player.0 + CHR_ASM_HOLDER).filter(|&h| h >= 0x10000) {
        (c.holder_from_pgd)(h, pgd);
    }
    Ok(added)
}

/// Removes the rows the wardrobe added that are neither worn nor (since) the player's own.
fn clean_added(pgd: usize, added: &mut Vec<i32>, keep: &[i32]) {
    let Some(c) = calls() else { return };
    let egd = pgd + PGD_EGD;
    let worn = local_outfit(pgd).map(|o| o.rows()).unwrap_or([-1; 5]);
    added.retain(|&row| {
        if worn.contains(&row) || keep.contains(&row) {
            return true;
        }
        if let Some(f) = game_lot_flag(row)
            && game::event_flag(f) != Some(false)
        {
            log::line(&format!("wardrobe: {row} kept in the inventory (awarded by the game, flag {f})"));
            return false;
        }
        let full = Outfit::full_id(row);
        let idx = (c.find)(egd, &full);
        if idx >= 0 {
            // 0x79ECA0's result is that of 0x7A8550, which always returns 0 [VAR]: the logged
            // "ok false" of test4 meant nothing. The entry is checked by looking it up again.
            (c.remove)(egd, idx, 1, 0);
            let left = (c.find)(egd, &full);
            if left < 0 {
                log::line(&format!("wardrobe: {row} removed from the inventory (entry {idx}, gone)"));
            } else {
                // Still there: stays tracked, so a later clean (or the next run's
                // `wardrobe_added`) tries again instead of leaving it behind for good.
                log::line(&format!("wardrobe: {row} NOT removed from the inventory (entry {idx}, still at {left}): kept tracked"));
            }
            return left >= 0;
        }
        false
    });
}

fn ini_values(st: &State) -> [String; 5] {
    [
        skin_ini(st.choices.skin).to_owned(),
        head_ini(st.choices.head).to_owned(),
        outfit_ini(st.original),
        rows_ini(&st.added),
        owner_ini(st.owner),
    ]
}

/// An older build's `wardrobe_legs` line is neither read nor written: it is simply ignored.
const INI_KEYS: [&str; 5] = ["wardrobe_skin", "wardrobe_head", "wardrobe_original", "wardrobe_added", "wardrobe_owner"];

fn remember_ini(st: &mut State) {
    // No records, no owner.
    if st.original.is_none() && st.added.is_empty() {
        st.owner = None;
    }
    st.ini_seen = ini_values(st);
    let upd: Vec<(&str, String)> = INI_KEYS.into_iter().zip(st.ini_seen.iter().cloned()).collect();
    if let Err(e) = settings::write_keys(&upd) {
        log::line(&format!("wardrobe: ini not written: {e}"));
    }
    st.ini_dirty = false;
}

/// Every in-world frame (puppet.rs, game thread, not during our warp).
pub fn tick(player: Chr, in_world_for: Duration) {
    if game::load_pending() {
        close_for_load("loading");
    } else {
        LOADING.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    let mut reload = None;
    tick_locked(player, in_world_for, &mut reload);
    // The parts reload runs without our lock: the render thread (panel) must never wait on
    // a model build.
    if let Some(same) = reload {
        if game::load_pending() {
            log::line("wardrobe: parts reload dropped (a load started; it rebuilds the parts)");
        } else {
            let r = crate::ghost::reload_parts(player);
            log::line(&format!("wardrobe: parts reload {r:?} (holder follows the PGD: {same})"));
        }
    }
}

fn tick_locked(player: Chr, in_world_for: Duration, reload: &mut Option<bool>) {
    let Ok(mut st) = STATE.lock() else { return };
    if !st.enabled || st.disabled.is_some() {
        return;
    }
    let now = Instant::now();
    let settled = st.edited_at.is_none_or(|t| now.duration_since(t) >= DEBOUNCE) || st.apply_now;
    if st.ini_dirty && settled {
        remember_ini(&mut st);
    }
    // Nothing chosen and nothing to restore: no game memory is read.
    if st.choices.skin.is_none() && st.original.is_none() && !st.reload_due {
        st.applied = None;
        st.status = "As in game".into();
        return;
    }
    if !st.checked {
        st.checked = true;
        if let Err(e) = check_sites() {
            log::line(&format!("wardrobe: DISABLED: {e}"));
            st.status = "Disabled (game code differs, see log)".into();
            st.disabled = Some(e);
            return;
        }
    }
    let Some(pgd) = crate::loadout::local_pgd() else { return };
    let Some(current) = local_outfit(pgd) else { return };

    // The parts reload one frame after the equip: the holder holds the new handles by now.
    if st.reload_due {
        st.reload_due = false;
        let same = (12..=16).all(|s| {
            let h = read_ptr(player.0 + CHR_ASM_HOLDER).and_then(|h| read::<u32>(h + ASM_HANDLES + 4 * s));
            h.is_some() && h == read::<u32>(pgd + PGD_CHR_ASM + ASM_HANDLES + 4 * s)
        });
        if RELOAD.load(std::sync::atomic::Ordering::Relaxed) {
            *reload = Some(same);
        } else {
            log::line(&format!("wardrobe: parts reload off (wardrobe_reload = 0; holder follows the PGD: {same})"));
        }
    }

    // The records belong to one character of the save file (`wardrobe_owner`).
    if st.original.is_some() || !st.added.is_empty() {
        let (slot, sen) = crate::ownworld::fingerprint();
        match st.owner {
            None => {
                if let Some(slot) = slot {
                    st.owner = Some(Owner { slot, sen });
                    log::line(&format!("wardrobe: records taken as save slot {slot}'s (sen {sen:?})"));
                    remember_ini(&mut st);
                } else {
                    st.status = "Waiting: save slot".into();
                    return;
                }
            }
            Some(o) if !o.matches(slot, sen) => {
                if !st.foreign_logged {
                    st.foreign_logged = true;
                    log::line(&format!(
                        "wardrobe: original {:?} / added {:?} belong to save slot {} (sen {:?}), this is slot {slot:?} (sen {sen:?}): \
                         nothing changed here; load that character and pick 'As in game', or clear wardrobe_original / wardrobe_added / wardrobe_owner",
                        st.original.map(|o| o.rows()),
                        st.added,
                        o.slot,
                        o.sen
                    ));
                }
                st.status = format!("Waiting: outfit of save slot {} (see log)", o.slot);
                return;
            }
            Some(_) => {}
        }
    }

    // Somebody else changed the outfit we put on. Handled only when the outfit may be
    // touched (a Change Form menu / talk / cutscene may still be changing it over several
    // frames, and no inventory entry is removed under an open menu).
    if let Some(a) = st.applied
        && a != current
    {
        if let Some(why) = blocker(player, in_world_for) {
            st.status = format!("Waiting: {why}");
            return;
        }
        match external(&a, &current) {
            External::Story => {
                log::line(&format!("wardrobe: the game changed torso / arm {:?} -> {:?}: putting the skin on again", a.rows(), current.rows()));
                st.edited_at = Some(now);
            }
            External::Yield => {
                log::line(&format!(
                    "wardrobe: the game changed the outfit {:?} -> {:?} (Change Form / New Game+?): back to 'as in game'",
                    a.rows(),
                    current.rows()
                ));
                st.choices.skin = None;
                st.original = None;
                let mut added = std::mem::take(&mut st.added);
                clean_added(pgd, &mut added, &[]);
                st.added = added;
                remember_ini(&mut st);
            }
        }
        st.applied = None;
    }
    if let Some(c) = classify_torso(current.torso)
        && st.applied.is_none_or(|a| a.torso != current.torso)
    {
        st.class = c;
    }

    let resolved = resolve(&st.choices, st.class, &current);
    // A skin the rules refuse (wardrobe_skin = sculptor, never offered) acts as "as in game".
    let refused = st.choices.skin.is_some() && resolved.is_none();
    let want = match resolved {
        Some(w) => w,
        None => match st.original {
            // "As in game" after a change: restore (in the story state of now).
            Some(o) => restore_target(&o, &current),
            None => {
                st.status = if refused { "As in game (that skin is not offered)".into() } else { "As in game".into() };
                return;
            }
        },
    };
    let restoring = resolved.is_none();
    if current == want {
        st.waiting_logged = None;
        st.failed = None;
        if restoring {
            // The clean-up removes inventory entries: only when the outfit may be touched.
            if let Some(why) = blocker(player, in_world_for) {
                st.status = format!("Waiting: {why}");
                return;
            }
            // Restored: forget the original, take back what we added.
            st.original = None;
            st.applied = None;
            let mut added = std::mem::take(&mut st.added);
            clean_added(pgd, &mut added, &want.rows());
            st.added = added;
            remember_ini(&mut st);
            log::line(&format!("wardrobe: original outfit {:?} restored", want.rows()));
            st.status = "As in game".into();
            return;
        }
        st.applied = Some(want);
        st.status = "Applied".into();
        return;
    }
    if st.failed == Some(want) {
        // Tried and a slot did not take: not again until the choices or the game change.
        return;
    }
    if !settled || st.last_apply.is_some_and(|t| now.duration_since(t) < MIN_GAP) {
        st.status = "...".into();
        return;
    }
    if let Some(why) = blocker(player, in_world_for) {
        if st.waiting_logged != Some(why) {
            st.waiting_logged = Some(why);
            log::line(&format!("wardrobe: change skipped for now ({why}), wanted {:?}", want.rows()));
        }
        st.status = format!("Waiting: {why}");
        return;
    }
    st.waiting_logged = None;
    // Every row must exist in EquipParamProtector (the hair slot may be emptied).
    let missing: Vec<i32> = want
        .rows()
        .into_iter()
        .filter(|&r| r >= 0 && !crate::loadout::row_exists(crate::loadout::SlotKind::Protector, r as u32))
        .collect();
    if !missing.is_empty() {
        if !st.refused_logged.contains(&missing[0]) {
            st.refused_logged.extend(&missing);
            log::line(&format!("wardrobe: change skipped: rows {missing:?} are not in EquipParamProtector (params not loaded?)"));
        }
        st.status = "Waiting: game data".into();
        return;
    }
    if st.original.is_none() && !restoring {
        let (slot, sen) = crate::ownworld::fingerprint();
        let Some(slot) = slot else {
            st.status = "Waiting: save slot".into();
            return;
        };
        if st.owner.is_none_or(|o| !o.matches(Some(slot), sen)) {
            st.owner = Some(Owner { slot, sen });
        }
        st.original = Some(current);
        log::line(&format!("wardrobe: original outfit remembered {:?} (save slot {slot}, sen {sen:?})", current.rows()));
        // In the ini before anything is changed.
        remember_ini(&mut st);
    }
    st.apply_now = false;
    st.last_apply = Some(now);
    match put_on(player, pgd, &current, &want) {
        Ok(added) => {
            for r in added {
                if !st.added.contains(&r) {
                    st.added.push(r);
                }
            }
            let after = local_outfit(pgd);
            log::line(&format!("wardrobe: applied {} -> ids {:?} (worn now {:?})", describe(&st.choices), want.rows(), after.map(|o| o.rows())));
            st.applied = after;
            st.reload_due = true;
            if after == Some(want) {
                st.status = "Applied".into();
            } else {
                st.failed = Some(want);
                st.status = "Partly applied (see log)".into();
                log::line(&format!("wardrobe: worn {:?} is not the wanted {:?}: not retried until a choice changes", after.map(|o| o.rows()), want.rows()));
            }
        }
        Err(e) => {
            log::line(&format!("wardrobe: change skipped ({e})"));
            st.status = format!("Waiting: {e}");
        }
    }
    remember_ini(&mut st);
}

/// A load screen / world reset: the next tick compares again (the save keeps the outfit);
/// the panel closes.
pub fn on_world_reset() {
    close_for_load("world reset");
    if let Ok(mut st) = STATE.lock() {
        st.reload_due = false;
    }
}

// ---------------------------------------------------------------------------------------
// Overlay (render thread).
// ---------------------------------------------------------------------------------------

use hudhook::imgui::{Condition, Key, Ui};

const YELLOW: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

/// Render-thread input state.
#[derive(Default)]
pub struct Panel {
    nav: crate::menu::PadNav,
    pad_was_down: bool,
}

impl Panel {
    pub fn draw(&mut self, ui: &Ui, key: Key, key_name: &str) {
        if ui.is_key_pressed_no_repeat(key) {
            edit(Edit::Toggle);
        }
        let mask = PAD_BUTTON.load(std::sync::atomic::Ordering::Relaxed);
        if mask != 0 || self.pad_was_down {
            let buttons = if mask != 0 { crate::spectate_input::pad_buttons_down() } else { None };
            let (down, pressed) = crate::spectate_input::pad_edge(self.pad_was_down, buttons, mask);
            self.pad_was_down = down;
            if pressed {
                edit(Edit::Toggle);
            }
        }
        if view().is_none() {
            self.nav = crate::menu::PadNav::default();
            return;
        }
        if ui.is_key_pressed(Key::UpArrow) {
            edit(Edit::Row(-1));
        }
        if ui.is_key_pressed(Key::DownArrow) {
            edit(Edit::Row(1));
        }
        if ui.is_key_pressed(Key::LeftArrow) {
            edit(Edit::Cycle(-1));
        }
        if ui.is_key_pressed(Key::RightArrow) {
            edit(Edit::Cycle(1));
        }
        if ui.is_key_pressed_no_repeat(Key::Enter) || ui.is_key_pressed_no_repeat(Key::KeypadEnter) {
            edit(Edit::ApplyNow);
        }
        if ui.is_key_pressed_no_repeat(Key::Escape) {
            edit(Edit::Close);
        }
        let pad = PAD.load(std::sync::atomic::Ordering::Relaxed);
        if pad {
            static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            let now = T0.get_or_init(Instant::now).elapsed().as_millis() as u64;
            for a in self.nav.step(crate::spectate_input::pad_state(), now) {
                match a {
                    crate::menu::NavAction::Move(m) => edit(Edit::Row(m)),
                    crate::menu::NavAction::Adjust(n) => edit(Edit::Cycle(n.signum())),
                    crate::menu::NavAction::Apply => edit(Edit::ApplyNow),
                    // R3 is the game's lock-on: not used here.
                    crate::menu::NavAction::Toggle => {}
                }
            }
        }
        let Some(v) = view() else { return };
        let [_, dh] = ui.io().display_size;
        ui.window("Wardrobe")
            .position([12.0, dh * 0.4], Condition::Always)
            .position_pivot([0.0, 0.5])
            .always_auto_resize(true)
            .resizable(false)
            .movable(false)
            .collapsible(false)
            .focus_on_appearing(false)
            .save_settings(false)
            .bg_alpha(0.75)
            .build(|| {
                ui.text(format!("Wardrobe  [{key_name}] close"));
                ui.separator();
                let rows = [("Skin", skin_label(v.choices.skin)), ("Head", head_label(v.choices.head))];
                for (i, (name, value)) in rows.iter().enumerate() {
                    let line = format!("{name}:  < {value} >");
                    if i == v.row {
                        ui.text_colored(YELLOW, format!("> {line}"));
                    } else {
                        ui.text(format!("  {line}"));
                    }
                }
                ui.text_disabled("Torso, arm and legs follow the skin");
                ui.separator();
                ui.text_disabled("Up/Down row, Left/Right change, Enter now");
                if pad {
                    ui.text_disabled("Pad: right stick, RT = now");
                }
                if !v.status.is_empty() {
                    ui.text(&v.status);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WOLF_SWORD: Outfit = Outfit { head: 100_000, torso: 131_000, arm: 102_000, legs: 103_000, hair: -1 };

    #[test]
    fn torso_variants_follow_the_sword_state() {
        assert_eq!(classify_torso(101_000), Some(TorsoClass::MortalBlade));
        assert_eq!(classify_torso(131_000), Some(TorsoClass::Sword));
        assert_eq!(classify_torso(141_000), Some(TorsoClass::NoSword));
        assert_eq!(classify_torso(331_000), Some(TorsoClass::Sword));
        assert_eq!(classify_torso(401_000), Some(TorsoClass::MortalBlade));
        // The Sculptor's single torso, the unused 641000 and empty tell nothing.
        assert_eq!(classify_torso(201_000), None);
        assert_eq!(classify_torso(641_000), None);
        assert_eq!(classify_torso(-1), None);
        assert_eq!(Skin::Wolf.torso(TorsoClass::NoSword), 141_000);
        assert_eq!(Skin::Shura.torso(TorsoClass::NoSword), 331_000);
        assert_eq!(Skin::Tengu.torso(TorsoClass::MortalBlade), 501_000);
        assert_eq!(Skin::Ashina.torso(TorsoClass::Sword), 431_000);
        for c in [TorsoClass::MortalBlade, TorsoClass::Sword, TorsoClass::NoSword] {
            assert_eq!(Skin::Sculptor.torso(c), 201_000);
            // A skin keeps the class it was given (round trip through the worn torso).
            for s in [Skin::Wolf, Skin::Shura, Skin::Ashina, Skin::Tengu] {
                let back = classify_torso(s.torso(c)).unwrap();
                if s == Skin::Wolf || c != TorsoClass::NoSword {
                    assert_eq!(back, c, "{s:?} {c:?}");
                }
            }
        }
    }

    #[test]
    fn legs_follow_the_skin() {
        let rows: Vec<i32> = SKINS.into_iter().map(Skin::legs).collect();
        assert_eq!(rows, vec![103_000, 303_000, 403_000, 503_000, 203_000]);
        // Whatever the head, the outfit's legs (and torso / arm) are the skin's own.
        for s in SKINS.into_iter().filter(|&s| skin_offered(s)) {
            for head in std::iter::once(None).chain(SKINS.into_iter().map(Some)) {
                let o = resolve(&Choices { skin: Some(s), head }, TorsoClass::Sword, &WOLF_SWORD).unwrap();
                assert_eq!((o.torso, o.arm, o.legs), (s.torso(TorsoClass::Sword), s.arm(), s.legs()), "{s:?} {head:?}");
            }
        }
    }

    #[test]
    fn heads_and_skins_offered() {
        // The Sculptor is never offered, neither as skin nor as head; all other heads are.
        assert!(!skin_offered(Skin::Sculptor));
        assert!(!head_offered(Skin::Sculptor));
        for s in [Skin::Wolf, Skin::Shura, Skin::Ashina, Skin::Tengu] {
            assert!(skin_offered(s) && head_offered(s), "{s:?}");
        }
    }

    #[test]
    fn resolve_builds_the_outfit() {
        // Shura's head on the Ashina skin: no lock; its hair comes along, legs are Ashina's.
        let c = Choices { skin: Some(Skin::Ashina), head: Some(Skin::Shura) };
        let o = resolve(&c, TorsoClass::MortalBlade, &WOLF_SWORD).unwrap();
        assert_eq!(o, Outfit { head: 300_000, torso: 401_000, arm: 402_000, legs: 403_000, hair: 304_000 });
        // Every offered head with every offered skin: the head's row and the head's hair.
        for s in SKINS.into_iter().filter(|&s| skin_offered(s)) {
            for h in SKINS.into_iter().filter(|&h| head_offered(h)) {
                let o = resolve(&Choices { skin: Some(s), head: Some(h) }, TorsoClass::Sword, &WOLF_SWORD).unwrap();
                assert_eq!((o.head, o.hair), (h.head(), h.hair()), "{s:?} {h:?}");
            }
        }
        // The Shura skin with another head: no Shura hair.
        let c = Choices { skin: Some(Skin::Shura), head: Some(Skin::Wolf) };
        let o = resolve(&c, TorsoClass::Sword, &WOLF_SWORD).unwrap();
        assert_eq!(o, Outfit { head: 100_000, torso: 331_000, arm: 302_000, legs: 303_000, hair: -1 });
        // "As skin": the skin's head and hair.
        let c = Choices { skin: Some(Skin::Shura), head: None };
        assert_eq!(resolve(&c, TorsoClass::Sword, &WOLF_SWORD).unwrap().hair, 304_000);
        // A Sculptor skin from the ini acts as "as in game" (restore).
        let c = Choices { skin: Some(Skin::Sculptor), ..Default::default() };
        assert_eq!(resolve(&c, TorsoClass::Sword, &WOLF_SWORD), None);
        // A Sculptor head from the ini falls back to the skin's own.
        let c = Choices { skin: Some(Skin::Wolf), head: Some(Skin::Sculptor) };
        let o = resolve(&c, TorsoClass::Sword, &WOLF_SWORD).unwrap();
        assert_eq!((o.head, o.legs), (100_000, 103_000));
        // The rows stay known for the restore of an older build's choice.
        assert_eq!((Skin::Sculptor.head(), Skin::Sculptor.torso(TorsoClass::Sword), Skin::Sculptor.legs()), (200_000, 201_000, 203_000));
        // Before the prosthetic (bare arm 112000) the arm is left alone.
        let early = Outfit { torso: 141_000, arm: 112_000, ..WOLF_SWORD };
        let c = Choices { skin: Some(Skin::Tengu), ..Default::default() };
        let o = resolve(&c, TorsoClass::NoSword, &early).unwrap();
        assert_eq!(o, Outfit { head: 500_000, torso: 531_000, arm: 112_000, legs: 503_000, hair: -1 });
        // As in game: nothing.
        assert_eq!(resolve(&Choices::default(), TorsoClass::Sword, &WOLF_SWORD), None);
    }

    #[test]
    fn worn_legs_of_an_older_build_are_replaced() {
        // An older build put the tabi (153000) / Wolf legs under the Ashina / Shura skins: the
        // new rule wants the skin's legs, so the next tick changes only that slot.
        for (skin, old_legs, want) in [(Skin::Ashina, 153_000, 403_000), (Skin::Shura, 503_000, 303_000), (Skin::Tengu, 303_000, 503_000)] {
            let c = Choices { skin: Some(skin), head: None };
            let want_o = resolve(&c, TorsoClass::Sword, &WOLF_SWORD).unwrap();
            let worn = Outfit { legs: old_legs, ..want_o };
            assert_eq!(want_o.legs, want);
            let changed: Vec<usize> = (0..5).filter(|&k| worn.rows()[k] != want_o.rows()[k]).collect();
            assert_eq!(changed, vec![3], "{skin:?}");
        }
    }

    #[test]
    fn cycling_skins_and_heads() {
        // Skins: as in game, Wolf, Shura, Ashina, Tengu (no Sculptor), wrapping both ways.
        let mut c = Choices::default();
        let mut seen = Vec::new();
        for _ in 0..5 {
            c = cycle(c, ROW_SKIN, 1);
            seen.push(c.skin);
        }
        assert_eq!(seen, vec![Some(Skin::Wolf), Some(Skin::Shura), Some(Skin::Ashina), Some(Skin::Tengu), None]);
        assert_eq!(cycle(Choices::default(), ROW_SKIN, -1).skin, Some(Skin::Tengu));
        // A Sculptor skin from the ini steps like "as in game".
        let c = Choices { skin: Some(Skin::Sculptor), ..Default::default() };
        assert_eq!(cycle(c, ROW_SKIN, 1).skin, Some(Skin::Wolf));
        // Heads: as skin, Wolf, Shura, Ashina, Tengu, on every skin and while "as in game".
        for skin in std::iter::once(None).chain(SKINS.into_iter().filter(|&s| skin_offered(s)).map(Some)) {
            let mut h = Choices { skin, head: None };
            let mut seen = Vec::new();
            for _ in 0..5 {
                h = cycle(h, ROW_HEAD, 1);
                seen.push(h.head);
            }
            assert_eq!(seen, vec![Some(Skin::Wolf), Some(Skin::Shura), Some(Skin::Ashina), Some(Skin::Tengu), None], "{skin:?}");
            assert_eq!(cycle(Choices { skin, head: None }, ROW_HEAD, -1).head, Some(Skin::Tengu));
        }
        // A Sculptor head from the ini steps like "as skin".
        let h = Choices { skin: Some(Skin::Wolf), head: Some(Skin::Sculptor) };
        assert_eq!(cycle(h, ROW_HEAD, 1).head, Some(Skin::Wolf));
        // A skin change keeps the chosen head (Shura's too), also through "as in game".
        let mut c = Choices { skin: Some(Skin::Shura), head: Some(Skin::Shura) };
        for _ in 0..5 {
            c = cycle(c, ROW_SKIN, 1);
            assert_eq!(c.head, Some(Skin::Shura));
        }
        // A head change keeps the skin; an unknown row changes nothing.
        let c = Choices { skin: Some(Skin::Tengu), head: None };
        assert_eq!(cycle(c, ROW_HEAD, 1).skin, Some(Skin::Tengu));
        assert_eq!(cycle(c, ROWS, 1), c);
    }

    #[test]
    fn ini_round_trip() {
        for s in std::iter::once(None).chain(SKINS.into_iter().map(Some)) {
            assert_eq!(parse_skin(skin_ini(s)), Ok(s));
            assert_eq!(parse_head(head_ini(s)), Ok(s));
        }
        assert_eq!(parse_skin(""), Ok(None));
        assert_eq!(parse_skin(" Tengu "), Ok(Some(Skin::Tengu)));
        assert_eq!(parse_skin("fundoshi"), Err(()));
        // The ini keys the wardrobe owns: no legs any more.
        assert!(!INI_KEYS.contains(&"wardrobe_legs"));
        let o = Outfit { hair: 304_000, ..WOLF_SWORD };
        assert_eq!(outfit_ini(Some(o)), "100000,131000,102000,103000,304000");
        assert_eq!(parse_outfit(&outfit_ini(Some(o))), Ok(Some(o)));
        assert_eq!(parse_outfit(&outfit_ini(Some(WOLF_SWORD))), Ok(Some(WOLF_SWORD)));
        assert_eq!(parse_outfit(""), Ok(None));
        assert_eq!(parse_outfit("1,2,3"), Err(()));
        assert_eq!(parse_outfit("-1,131000,102000,103000,-1"), Err(()));
        assert_eq!(parse_outfit("a,b,c,d,e"), Err(()));
        assert_eq!(parse_rows(&rows_ini(&[153_000, 200_000])), Ok(vec![153_000, 200_000]));
        assert_eq!(parse_rows(""), Ok(vec![]));
        assert_eq!(parse_rows("1,-5"), Err(()));
    }

    #[test]
    fn ids_and_external_changes() {
        let mut ids = [-1i32; 25];
        ids[12] = 0x1000_0000 | 100_000;
        ids[13] = 268_566_456; // 131000 in the dump
        ids[14] = 0x1000_0000 | 102_000;
        ids[15] = 0x1000_0000 | 103_000;
        assert_eq!(Outfit::from_ids(&ids), WOLF_SWORD);
        assert_eq!(Outfit::full_id(304_000), 0x1000_0000 | 304_000);
        assert_eq!(Outfit::full_id(-1), -1);
        // The Mortal Blade cutscene swaps the torso: a story change.
        let story = Outfit { torso: 101_000, ..WOLF_SWORD };
        assert_eq!(external(&WOLF_SWORD, &story), External::Story);
        let arm = Outfit { arm: 112_000, ..WOLF_SWORD };
        assert_eq!(external(&WOLF_SWORD, &arm), External::Story);
        // The game's own Change Form / New Game+ touch the head or legs too.
        let other = Outfit { head: 400_000, torso: 431_000, ..WOLF_SWORD };
        assert_eq!(external(&WOLF_SWORD, &other), External::Yield);
        let hair = Outfit { hair: 304_000, ..WOLF_SWORD };
        assert_eq!(external(&WOLF_SWORD, &hair), External::Yield);
    }

    #[test]
    fn restore_follows_the_story_state_of_now() {
        // Original before the prosthetic and without the Mortal Blade; meanwhile (skin Ashina)
        // the story gave both: the restore keeps them.
        let early = Outfit { torso: 141_000, arm: 112_000, ..WOLF_SWORD };
        let now = Outfit { head: 400_000, torso: 401_000, arm: 402_000, legs: 403_000, hair: -1 };
        assert_eq!(restore_target(&early, &now), Outfit { torso: 101_000, arm: 102_000, ..early });
        // Nothing changed in the story: the original as it was.
        let now = Outfit { torso: 431_000, ..now };
        assert_eq!(restore_target(&WOLF_SWORD, &now), WOLF_SWORD);
        // A Tengu original keeps its family.
        let tengu = Outfit { head: 500_000, torso: 531_000, arm: 502_000, legs: 503_000, hair: -1 };
        assert_eq!(restore_target(&tengu, &Outfit { torso: 101_000, ..WOLF_SWORD }).torso, 501_000);
        // No sword state for non-Wolf families: x31000.
        assert_eq!(restore_target(&tengu, &Outfit { torso: 141_000, ..WOLF_SWORD }).torso, 531_000);
        // The Sculptor's torso tells nothing: torso kept. A missing arm stays the original's.
        let sculpt = Outfit { head: 200_000, torso: 201_000, arm: 122_000, legs: 203_000, hair: -1 };
        assert_eq!(restore_target(&WOLF_SWORD, &sculpt), WOLF_SWORD);
        // Stable: restoring the target again changes nothing (the tick compares to it).
        let t = restore_target(&early, &Outfit { torso: 401_000, arm: 402_000, ..WOLF_SWORD });
        assert_eq!(restore_target(&early, &t), t);
    }

    #[test]
    fn owner_of_the_records() {
        let o = Owner { slot: 2, sen: Some(1000) };
        assert!(o.matches(Some(2), Some(1000)));
        assert!(o.matches(Some(2), Some(5000)));
        assert!(o.matches(Some(2), None));
        // Another slot, the title screen, a new character in the same slot.
        assert!(!o.matches(Some(3), Some(5000)));
        assert!(!o.matches(None, Some(1000)));
        assert!(!o.matches(Some(2), Some(10)));
        for x in [None, Some(o), Some(Owner { slot: 0, sen: None })] {
            assert_eq!(parse_owner(&owner_ini(x)), Ok(x));
        }
        assert_eq!(parse_owner(" 4 , 77 "), Ok(Some(Owner { slot: 4, sen: Some(77) })));
        for bad in ["10", "-1", "a", "1,2,3", "1,-2", "1,"] {
            assert_eq!(parse_owner(bad), Err(()), "{bad}");
        }
    }

    #[test]
    fn award_flags_of_costume_rows() {
        assert_eq!(game_lot_flag(304_000), Some(50_003_405));
        assert_eq!(game_lot_flag(531_000), Some(50_003_422));
        // Never awarded: the Sculptor's set and the tabi.
        for r in [200_000, 201_000, 203_000, 153_000] {
            assert_eq!(game_lot_flag(r), None);
        }
    }
}
