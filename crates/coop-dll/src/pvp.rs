//! Friendly fire on the game's own hit packet (experimental, `[GAMEPLAY] friendly_fire`,
//! default 0; FORK.md "Friendly fire on the native hit packet"). NOT verified live.
//!
//! "Friendly fire", not duels: the friend stays an ally (no lock-on, no enemy HP bar, idols
//! usable, no deathblow / throw on them), but our careless hits on their copy hurt them.
//!
//! The game already knows "detected by the attacker, computed by the victim" [VAR, Ghidra on
//! the 2026-11-11 dump, E:\RE\notes-2026-11-11.md and E:\RE\work_pvp]:
//! - The damage router `0xB6A040(dmgModule, attacker, dmg)` picks an action from
//!   `table[victimClass * 7 + attackerClass]` (`0x2A7C590`, or `0x2A7C660` when `dmg+0xFC`
//!   is set; `0xB6F4C0`: 1 local player, 2 remote player). Vanilla `[1][2]` = 0 in both
//!   tables: a remote player's hit on us is reaction only (`0xB6AB40`: guard direction, hit
//!   SFX), no HP / posture. `[2][1]` = 4 (`0x2A7C590`): our hit on a remote player calls the
//!   damage module's slot 10, `0xB72CC0` for players: it serialises the hit (`0xB686A0`, a
//!   0xC0-byte record) and hands it to `[[remote+0x2038]]+0x20` ("send to this player",
//!   packet type 20), then `0xB68FF0(.., remote = 1)` plays the reaction on the remote only
//!   (`0xB6E6A0` writes HP / posture only when remote = 0).
//! - The victim's player update `0xA37560` calls `0xB72A80(dmgModule, rx)` (only for the
//!   local player, `rx` = PlayerIns+0x2088): `0x8D7B10(rx, &record)` returns the next type-20
//!   record of any session member and the PlayerIns whose session is that member (the
//!   attacker), `0xB680C0` rebuilds a full hit from it, and `0xB68FF0(dmgModule, attacker,
//!   hit, 0)` applies it to us by OUR current state (guard, deflect, i-frames, `0xBD4D40`).
//!   [VAR, Ghidra 2026-11-12, E:\RE\work_pvpff] PlayerIns+0x2088 is allocated in `0xA37560`
//!   only while `0x9F3430(player)` (`[[0x143D7A1E0]+0x88] == player`, the game's own online
//!   player) holds, and `0xB72A80` starts with `test rdx,rdx; jz <end>`: in our session rx is
//!   NULL [ARV, first live test: 22 / 24 hits received, 0 applied], so the vanilla call never
//!   reaches the call sites below. `drive_receive` therefore calls `0xB72A80` itself from the
//!   frame task (game thread) with a sentinel rx whenever hits are queued.
//!
//! What this module adds (only while `friendly_fire` is on and a link is up):
//! - Team 28 ("Coop NPC") on every friend's native remote only (`chr+0x74`); our own team is
//!   never touched, the original team comes back when the feature goes off. Team matrices
//!   (E:\RE\work_team\team_matrices.txt): M1 `0x143B0A680` [1][28] is the same functor as
//!   [1][29] (our hitboxes hit it; the router `[2][1]` = 4 still makes the type-20 packet),
//!   M2 `0x143B0CAA0` [1][28] / [28][1] are not hostile (no lock-on, so no enemy HP bar; the
//!   idol's enemy finder ignores it), enemies treat 28 like 2, and M1[28][1] = F (the copy's
//!   own hitboxes no longer hit us locally). Team 29 (Rival) made the friend a full enemy.
//!   The remote also gets the game's "cannot die" bit (`data+0x228 & 1`, read by `0xBD5DF0`)
//!   while its owner is alive: a local-only hit can then never kill the copy.
//! - Sender: the remote's session is native.rs's recv session (slot 4 = PlayerDebugSession
//!   `0x1061880` -> our slot 1); `capture` takes the type-20 record there and `tick` sends it
//!   to that remote's owner (bulk kind `BULK_PVP`, reliable, nonce + seq for duplicates).
//! - Receiver: call-site patches inside `0xB72A80` (threads suspended, every byte checked):
//!   both `call 0x8D7B10` -> `recv_hook` serves our queued records first (the record into the
//!   game's own buffer, returns the sender's native remote as the attacker), then the
//!   original (never for our sentinel rx); `call 0xB68FF0` -> `apply_hook` runs the game's
//!   application with OUR "cannot die" bit set for that one call (the HP setter `0xBD64E0`
//!   then keeps HP at 1), logs HP / posture before and after, and ends the round at 1 HP.
//!   Neither `0xB680C0` nor `0xB68FF0` (2 levels down) checks the team matrices [VAR].
//! - No deathblow / throw on a friend's copy: M1 decides those too (M1[1][28] passes), so the
//!   candidate checks of the two chr finders are wrapped in their vtables [VAR, RTTI]:
//!   `SprjChrThrowModule::RequestThrow_AllChr` finder (vtable slot `0x142A8EBD0` =
//!   `0xBDF930`) and `FdpChrCeremonyModule::_GetEnableExecuteCremonyChr` finder (slot
//!   `0x142A7BC20` = `0xB5DD30`) refuse every native remote. A received hit of type 5 is
//!   still never applied.
//! - Diagnostics only: `call 0xB6AB40` in `0xB6A040` -> `reaction_hook` counts the friend's
//!   copy hitting us (the laggy local path; 0 expected with team 28).
//! - Prosthetic tools, bullets, explosions need nothing extra [VAR, E:\RE\work_pvpff_bul]:
//!   `0x995600` -> `0xB06360(hit, victim, bullet)` (chr owner) -> `0xB68A50` -> victim damage
//!   module slot 9 (router) -> action 4 -> `0xB72CC0`, the same type-20 record (flags `+0x3B`
//!   bit 1 = bullet; the first live test already sent such records). Not covered: damage over
//!   time (SpEffects tick locally) and owner-less bullets (traps).
//!
//! Non-lethal duels: a hit that brings us to 1 HP (or a deathblow, hit type 5, which is not
//! applied at all) ends the duel: `pvp: duel over, <name> wins`, a DuelOver message to the
//! attacker, for 5 s both sides drop that peer's hits, and both refill HP / posture. Lethal
//! PvP is NOT supported: the death event `0x66CDA0` looks the killer's session member up and
//! crashes on a native remote (`0x66CE96`, the 1.0.11 crash). The death guard
//! (`death_attacker_hook`, `[DEBUG] pvp_death_guard`, on even without friendly_fire) makes it
//! see no killer when our player dies in the same frame as a friend's applied hit.
//! Boss fights (`friendly_fire_in_boss_fights = 0`): own teams back, hits dropped.
//!
//! Hooks run on the game thread and never panic into the game.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::game::{Chr, module_base, now_ms, read, read_ptr, write};
use crate::net::{Dest, Net};
use crate::peers::{MAX_PLAYERS, PeerId, PeerSet};
use crate::{hook, log, native};

/// `[GAMEPLAY] friendly_fire` (default off). The host's value is used by both (ScaleCfg 11th byte).
pub static FRIENDLY_FIRE: AtomicBool = AtomicBool::new(false);
/// `[DEBUG] pvp_dry_run` (default off): received hits are logged, never applied.
pub static DRY_RUN: AtomicBool = AtomicBool::new(false);
/// `[GAMEPLAY] friendly_fire_in_boss_fights` (default off): while a boss gauge is shown the
/// friends' copies get their own team back and player hits are dropped (lock-on, allied NPCs).
pub static IN_BOSS_FIGHTS: AtomicBool = AtomicBool::new(false);
/// `[DEBUG] pvp_death_guard` (default on, independent of friendly_fire): the death event's
/// "killed by a player" branch is skipped when the attacker is one of our native remotes.
pub static DEATH_GUARD: AtomicBool = AtomicBool::new(true);

/// Bulk kind of our messages (world 1/5/6/8, puppet 7, testsync 20-23, doctor 30, keyitems
/// 41-46, persist 50, audit 60, nsession 0x4E; 0xA6-0xA8 are taken by other fork branches).
pub const BULK_PVP: u8 = 0xA9;
/// The game's hit packet type and its size.
pub const HIT_PACKET_TYPE: u32 = 20;
pub const HIT_RECORD: usize = 0xC0;

const MAGIC: u8 = 0x50;
const VERSION: u8 = 1;
const KIND_HIT: u8 = 1;
const KIND_DUEL_OVER: u8 = 2;
const KIND_HELLO: u8 = 3;
const HEADER: usize = 11;

/// "Coop NPC": hit by us, not hostile for lock-on / idols (see the header).
const PVP_TEAM: u32 = 28;
const DUEL_OVER_FOR: Duration = Duration::from_secs(5);
/// A received hit that waited longer than this (a load, a hitch) is dropped.
const STALE_AFTER: Duration = Duration::from_millis(2000);
const MAX_QUEUE: usize = 32;
const STATS_EVERY: Duration = Duration::from_secs(10);
const INSTALL_RETRY: Duration = Duration::from_secs(2);
const INSTALL_TRIES: u32 = 10;
/// While on, a Hello goes to every peer this often; hits go only to a peer whose Hello came
/// within `HELLO_VALID` (an older build would leave our bulk kind in its 24-entry inbox).
const HELLO_EVERY: Duration = Duration::from_secs(10);
const HELLO_VALID_MS: u64 = 35_000;
/// The friend's type-1 frames older than this: its copy loses the "cannot die" bit.
const REPLICA_FRAME_FRESH_MS: u64 = 1000;
const DATA_MAX_HP: usize = 0x134;
const DATA_MAX_POSTURE: usize = 0x14C;

// --- Game code (RVAs, Sekiro 1.06) -------------------------------------------------

/// Call sites inside `0xB72A80` (type-20 receive) and their original targets.
const SITE_RECV_A: usize = 0xB72B1D;
const SITE_RECV_B: usize = 0xB72BC7;
const FN_RECV: usize = 0x8D7B10;
const SITE_APPLY: usize = 0xB72BB8;
const FN_APPLY: usize = 0xB68FF0;
/// Action 0 (reaction only) inside the damage router `0xB6A040`.
const SITE_REACTION: usize = 0xB6A12A;
const FN_REACTION: usize = 0xB6AB40;
/// Damage action tables `u32 [7][7]`.
const DMG_TABLES: [usize; 2] = [0x2A7C590, 0x2A7C660];
/// PlayerDebugSession slot 4: `mov rax,[rcx]; mov r9d,0x14; mov r8d,0xC0; jmp [rax+8]`.
const DEBUG_SEND_HIT_RVA: usize = 0x1061880;
const DEBUG_SEND_HIT_BYTES: [u8; 19] =
    [0x48, 0x8B, 0x01, 0x41, 0xB9, 0x14, 0x00, 0x00, 0x00, 0x41, 0xB8, 0xC0, 0x00, 0x00, 0x00, 0x48, 0xFF, 0x60, 0x08];
const DEBUG_SESSION_VT_RVA: usize = 0x2B86048;
/// Death event handler `0x66CDA0`: `call 0xA4A3F0(WorldChrMan, handle)` = the last attacker of
/// our dead player (damage list `[[0x143D56058]+0x38]`), whose session member it then looks up
/// (`0xA29DD0`, `0x8E40F0` -> -1 for a native remote, `0x7B2980(-1)` = NULL, then
/// `mov r8d,[rax+0x10]` at `0x66CE96` = the 1.0.11 crash). A null result takes its own
/// `jz 0x66CEE0` (returns 0).
const SITE_DEATH_ATTACKER: usize = 0x66CE5C;
const FN_CHR_BY_HANDLE: usize = 0xA4A3F0;
/// The type-20 receive loop itself: `test rdx,rdx; jz +0x176` (rx NULL = nothing to do).
const FN_RECV_LOOP: usize = 0xB72A80;
const RECV_LOOP_HEAD: [u8; 9] = [0x48, 0x85, 0xD2, 0x0F, 0x84, 0x76, 0x01, 0x00, 0x00];
/// PlayerIns+0x2088: the rx `0xA37560` hands to `0xB72A80` (diagnostic read only).
const PLAYER_RX: usize = 0x2088;
/// `0x9F3430(player)` = `[[0x143D7A1E0]+0x88] == player` (diagnostic read only).
const ONLINE_HOLDER_RVA: usize = 0x3D7A1E0;
const ONLINE_PLAYER: usize = 0x88;
/// Chr finder candidate checks (vtable slot RVA, function RVA, its first bytes, name).
const FINDERS: [(usize, usize, [u8; 8], &str); 2] = [
    (0x2A8EBD0, 0xBDF930, [0x4C, 0x8B, 0xDC, 0x57, 0x48, 0x83, 0xEC, 0x50], "throw finder (SprjChrThrowModule::RequestThrow_AllChr)"),
    (0x2A7BC20, 0xB5DD30, [0x40, 0x55, 0x56, 0x57, 0x41, 0x56, 0x41, 0x57], "deathblow finder (FdpChrCeremonyModule::_GetEnableExecuteCremonyChr)"),
];

// --- Layout ---------------------------------------------------------------------------
const CHR_TEAM: usize = 0x74;
const CHR_MODULES: usize = 0x1FF8;
const MOD_DATA: usize = 0x18;
const MOD_DAMAGE: usize = 0x98;
const DATA_HP: usize = 0x130;
const DATA_POSTURE: usize = 0x148;
/// Bit 0 = "cannot die" (`0xBD5DF0`), bit 1 = no damage (`0xBD4D40`), bit 4 = no posture damage.
const DATA_FLAGS: usize = 0x228;
const FLAG_NO_DEAD: u8 = 1;
/// Hit struct (`0xB680C0` output): result written by `0xB68FF0` (`0x84FEF0`), hit type
/// (5 = deathblow, `0xB6E6A0`), HP and posture damage (`0xB6E6A0`).
const HIT_RESULT: usize = 0x24;
const HIT_TYPE: usize = 0x28;
const HIT_HP: usize = 0x1E0;
const HIT_POSTURE: usize = 0x1E4;
const HIT_TYPE_DEATHBLOW: i32 = 5;
/// Record: BehaviorParam id (`0xB686A0` copies hit+0x4C; `0xB680C0` resolves it by table
/// 0xC + attacker kind), scaled damage shorts, flags (bit 1 = bullet: `+0x4C` then holds the
/// BulletParam id and the victim takes the AtkParam from that row).
const REC_ATK_ID: usize = 0x48;
const REC_DMG_FLAGS: usize = 0x3B;
const REC_FLAG_BULLET: u8 = 2;
const REC_DMG: usize = 0x30;

// ===================================================================== pure, unit-tested

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// One captured hit record. `held_ms` = capture -> send on the attacker's side.
    Hit { nonce: u32, seq: u32, held_ms: u16, record: Vec<u8> },
    /// The sender lost the duel (`reason` 1 = down to 1 HP, 2 = deathblow).
    DuelOver { nonce: u32, seq: u32, reason: u8 },
    /// "This build has friendly fire on": hits may be sent to the sender.
    Hello { nonce: u32, seq: u32 },
}

pub fn encode(m: &Msg) -> Vec<u8> {
    let (kind, nonce, seq) = match m {
        Msg::Hit { nonce, seq, .. } => (KIND_HIT, *nonce, *seq),
        Msg::DuelOver { nonce, seq, .. } => (KIND_DUEL_OVER, *nonce, *seq),
        Msg::Hello { nonce, seq } => (KIND_HELLO, *nonce, *seq),
    };
    let mut out = vec![MAGIC, VERSION, kind];
    out.extend_from_slice(&nonce.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    match m {
        Msg::Hit { held_ms, record, .. } => {
            out.extend_from_slice(&held_ms.to_le_bytes());
            out.extend_from_slice(record);
        }
        Msg::DuelOver { reason, .. } => out.push(*reason),
        Msg::Hello { .. } => {}
    }
    out
}

pub fn decode(b: &[u8]) -> Option<Msg> {
    if b.len() < HEADER || b[0] != MAGIC || b[1] != VERSION {
        return None;
    }
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let (nonce, seq) = (u32_at(3), u32_at(7));
    match b[2] {
        KIND_HIT if b.len() == HEADER + 2 + HIT_RECORD => Some(Msg::Hit {
            nonce,
            seq,
            held_ms: u16::from_le_bytes([b[HEADER], b[HEADER + 1]]),
            record: b[HEADER + 2..].to_vec(),
        }),
        KIND_DUEL_OVER if b.len() == HEADER + 1 => Some(Msg::DuelOver { nonce, seq, reason: b[HEADER] }),
        KIND_HELLO if b.len() == HEADER => Some(Msg::Hello { nonce, seq }),
        _ => None,
    }
}

/// Per-sender duplicate filter: the last `CAP` seqs of the sender's current nonce (a new
/// nonce = the sender's game restarted: start over).
#[derive(Default, Debug)]
pub struct Dedup {
    nonce: Option<u32>,
    recent: VecDeque<u32>,
}

impl Dedup {
    const CAP: usize = 256;

    /// True the first time (`nonce`, `seq`) is seen.
    pub fn accept(&mut self, nonce: u32, seq: u32) -> bool {
        if self.nonce != Some(nonce) {
            self.nonce = Some(nonce);
            self.recent.clear();
        }
        if self.recent.contains(&seq) {
            return false;
        }
        if self.recent.len() >= Self::CAP {
            self.recent.pop_front();
        }
        self.recent.push_back(seq);
        true
    }
}

/// The hit struct's HP damage word: an i32 in the game's own range, else (Ghidra typed the
/// field as float in `0xB6E6A0`) a whole f32. None = neither.
pub fn hp_damage(raw: u32) -> Option<i32> {
    let i = raw as i32;
    if (0..=1_000_000).contains(&i) {
        return Some(i);
    }
    let f = f32::from_bits(raw);
    (f.is_finite() && (0.0..=1_000_000.0).contains(&f)).then_some(f as i32)
}

/// The duel is lost: we were alive, the hit left us at 1 HP or less, and it either took HP
/// or its damage alone would have killed us (the "cannot die" clamp of the HP setter).
pub fn duel_lost(hp_before: i32, hp_after: i32, damage: Option<i32>) -> bool {
    hp_before > 0 && hp_after <= 1 && (hp_after < hp_before || damage.is_some_and(|d| d > 0 && d >= hp_before))
}

/// The vanilla cells the design relies on: `[1][2]` = 0 (reaction only) in both tables and
/// `[2][1]` = 4 (send type 20) in the first.
pub fn tables_ok(local_by_remote: [u32; 2], remote_by_local: u32) -> bool {
    local_by_remote == [0, 0] && remote_by_local == 4
}

/// Per-peer duel-over cooldowns (`now_ms` values; 0 = none).
#[derive(Clone, Copy, Debug)]
pub struct Cooldowns {
    until: [u64; MAX_PLAYERS],
}

impl Cooldowns {
    pub fn start(&mut self, p: PeerId, now: u64) {
        self.until[p.index()] = now + DUEL_OVER_FOR.as_millis() as u64;
    }
    pub fn active(&self, p: PeerId, now: u64) -> bool {
        now < self.until[p.index()]
    }
    pub fn reset(&mut self, p: PeerId) {
        self.until[p.index()] = 0;
    }
}

/// Each peer's last Hello (`now_ms`, 0 = none since it joined).
#[derive(Clone, Copy, Debug)]
pub struct HelloBook {
    at: [u64; MAX_PLAYERS],
}

impl HelloBook {
    /// Records a Hello; true when it is the first since the peer joined.
    pub fn seen(&mut self, p: PeerId, now: u64) -> bool {
        let first = self.at[p.index()] == 0;
        self.at[p.index()] = now.max(1);
        first
    }
    pub fn fresh(&self, p: PeerId, now: u64) -> bool {
        let at = self.at[p.index()];
        at != 0 && now.saturating_sub(at) <= HELLO_VALID_MS
    }
    pub fn reset(&mut self, p: PeerId) {
        self.at[p.index()] = 0;
    }
}

/// Peers of `before` that are gone from `now` or came back on a new link epoch.
pub fn left_peers(before: &[(PeerId, u64)], now: &[(PeerId, u64)]) -> Vec<PeerId> {
    before.iter().filter(|b| !now.contains(b)).map(|&(p, _)| p).collect()
}

/// The death guard's decision: the death event's attacker is one of our native remotes.
pub fn skip_death_attacker(chr: usize, is_native_remote: bool) -> bool {
    chr != 0 && is_native_remote
}

/// The record came from a bullet (prosthetic tool, explosion): flags `+0x3B` bit 1.
pub fn record_is_bullet(record: &[u8]) -> bool {
    record.get(REC_DMG_FLAGS).is_some_and(|f| f & REC_FLAG_BULLET != 0)
}

pub fn hit_kind(bullet: bool) -> &'static str {
    if bullet { "bullet" } else { "melee" }
}

/// The throw / deathblow finders' decision: a candidate that is one of our native remotes is
/// refused (the friend is an ally: no deathblow, no throw).
pub fn finder_refuses(candidate: usize, is_native_remote: bool) -> bool {
    candidate != 0 && is_native_remote
}

/// What `drive_receive` reports once about the vanilla receive path: `rx` = PlayerIns+0x2088,
/// `online` = the game's own online player is ours (`0x9F3430`).
pub fn vanilla_receive_note(rx: Option<usize>, online: Option<bool>) -> &'static str {
    match (rx, online) {
        (Some(r), _) if r != 0 => "set: the game's own receive runs too (our queue is served by whichever comes first)",
        (Some(_), Some(true)) => "NULL although the game counts us as its online player (allocated on the next update)",
        (Some(_), _) => "NULL: the game's own receive never runs (expected in our session); the mod drives it",
        (None, _) => "unreadable",
    }
}

/// The copy may keep the "cannot die" bit: its owner's snapshot says more than 1 HP and its
/// type-1 frames are fresh (an owner who is dying or gone must be able to die here).
pub fn replica_undying(owner_hp: Option<i32>, frame_age_ms: Option<u64>) -> bool {
    owner_hp.is_some_and(|h| h > 1) && frame_age_ms.is_some_and(|a| a <= REPLICA_FRAME_FRESH_MS)
}

/// Every call site is `call rel32` to its expected target.
fn call_target(site_addr: usize, op: u8, rel: [u8; 4]) -> Option<usize> {
    (op == 0xE8).then(|| (site_addr as i64 + 5 + i32::from_le_bytes(rel) as i64) as usize)
}

// ===================================================================== hook-shared state

static INSTALLED: AtomicBool = AtomicBool::new(false);
/// A failed byte check: never again this run.
static BROKEN: AtomicBool = AtomicBool::new(false);
/// Setting on (host's), link up, hooks in: the hooks do their work.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static ORIG_RECV: AtomicUsize = AtomicUsize::new(0);
static ORIG_APPLY: AtomicUsize = AtomicUsize::new(0);
static ORIG_REACTION: AtomicUsize = AtomicUsize::new(0);
/// Per peer: until when its player hits are dropped (both directions).
static COOLDOWN: Mutex<Cooldowns> = Mutex::new(Cooldowns { until: [0; MAX_PLAYERS] });
/// A boss gauge is shown and `friendly_fire_in_boss_fights` is off: hits dropped.
static BOSS_PAUSE: AtomicBool = AtomicBool::new(false);
static N_DROP_BOSS: AtomicU64 = AtomicU64::new(0);
/// One near page for every stub (allocated once; slots 16 bytes apart).
static STUB_PAGE: AtomicUsize = AtomicUsize::new(0);
static DG_INSTALLED: AtomicBool = AtomicBool::new(false);
static DG_TRIED: AtomicBool = AtomicBool::new(false);
static ORIG_CHR_BY_HANDLE: AtomicUsize = AtomicUsize::new(0);

static N_CAPTURED: AtomicU64 = AtomicU64::new(0);
static N_SENT: AtomicU64 = AtomicU64::new(0);
static N_RECEIVED: AtomicU64 = AtomicU64::new(0);
static N_APPLIED: AtomicU64 = AtomicU64::new(0);
static N_DROP_COOLDOWN: AtomicU64 = AtomicU64::new(0);
static N_DROP_STALE: AtomicU64 = AtomicU64::new(0);
static N_DROP_NO_REMOTE: AtomicU64 = AtomicU64::new(0);
static N_DROP_OFF: AtomicU64 = AtomicU64::new(0);
static N_DROP_DUP: AtomicU64 = AtomicU64::new(0);
static N_DEATHBLOW: AtomicU64 = AtomicU64::new(0);
static N_DRY: AtomicU64 = AtomicU64::new(0);
/// The friend's copy hit us: vanilla `[1][2]` = 0, reaction only (the laggy path, not applied).
static N_REACTION_ONLY: AtomicU64 = AtomicU64::new(0);
static HIT_IDS: AtomicU32 = AtomicU32::new(0);
/// Each peer's last Hello.
static HELLOS: Mutex<HelloBook> = Mutex::new(HelloBook { at: [0; MAX_PLAYERS] });
/// The remotes we changed (static so `shutdown` can restore them).
static CHANGED: Mutex<Vec<Changed>> = Mutex::new(Vec::new());
static N_NO_HELLO: AtomicU64 = AtomicU64::new(0);
/// Our calls of `0xB72A80` (frames with queued hits) and finder candidates refused.
static N_DRIVES: AtomicU64 = AtomicU64::new(0);
static N_FINDER_REFUSED: AtomicU64 = AtomicU64::new(0);
static ORIG_FINDERS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
/// Its address is the rx `drive_receive` passes to `0xB72A80` (never dereferenced by the game:
/// both `0x8D7B10` calls are ours and return 0 for it once our queue is empty).
static RX_SENTINEL: u8 = 0;

fn sentinel_rx() -> usize {
    &RX_SENTINEL as *const u8 as usize
}

struct Captured {
    peer: PeerId,
    record: Vec<u8>,
    at: Instant,
}

struct Incoming {
    from: PeerId,
    seq: u32,
    held_ms: u16,
    record: Vec<u8>,
    at: Instant,
}

/// The record `recv_hook` handed to the game just now, for `apply_hook`.
#[derive(Clone, Copy)]
struct Serving {
    from: PeerId,
    seq: u32,
    held_ms: u16,
    queued_ms: u64,
    attacker: usize,
    /// BehaviorParam id of the record and whether it is a bullet (prosthetic tools, explosions).
    atk: i32,
    bullet: bool,
}

/// What the hooks saw, logged by `tick` (names, pings are only there).
enum Event {
    Applied { s: Serving, hp: (i32, i32), posture: (i32, i32), dmg_hp: Option<i32>, dmg_posture: i32, hit_type: i32, result: i32, lost: bool },
    Deathblow { s: Serving },
    Dry { from: PeerId, seq: u32, atk: i32 },
    Dropped { from: PeerId, seq: u32, why: &'static str },
    WeWon { from: PeerId, reason: u8 },
    Hello { from: PeerId },
    DeathGuard { peer: Option<PeerId> },
}

static OUTBOX: Mutex<VecDeque<Captured>> = Mutex::new(VecDeque::new());
static INBOX: Mutex<VecDeque<Incoming>> = Mutex::new(VecDeque::new());
static SERVING: Mutex<Option<Serving>> = Mutex::new(None);
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static DEDUP: Mutex<[Option<Dedup>; MAX_PLAYERS]> = Mutex::new([const { None }; MAX_PLAYERS]);

fn event(e: Event) {
    if let Ok(mut g) = EVENTS.lock()
        && g.len() < 256
    {
        g.push(e);
    }
}

fn cooling_down(p: PeerId) -> bool {
    COOLDOWN.lock().is_ok_and(|c| c.active(p, now_ms()))
}

fn start_cooldown(p: PeerId) {
    if let Ok(mut c) = COOLDOWN.lock() {
        c.start(p, now_ms());
    }
}

fn hello_fresh(p: PeerId) -> bool {
    HELLOS.lock().is_ok_and(|h| h.fresh(p, now_ms()))
}

/// Forget everything about a peer that left (or reconnected: new link epoch).
fn reset_peer(p: PeerId) {
    if let Ok(mut h) = HELLOS.lock() {
        h.reset(p);
    }
    if let Ok(mut c) = COOLDOWN.lock() {
        c.reset(p);
    }
    if let Ok(mut d) = DEDUP.lock() {
        d[p.index()] = None;
    }
    if let Ok(mut q) = INBOX.lock() {
        q.retain(|i| i.from != p);
    }
}

/// Our process' message nonce (a restart of our game = new nonce on the friend's filter).
fn nonce() -> u32 {
    static N: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        (t ^ (t >> 32) ^ std::process::id() as u64) as u32 | 1
    })
}

// ===================================================================== game-thread hooks

/// native.rs `recv_side_send`: a type-20 record our hit on peer index `i`'s native remote
/// wanted to send to that player. Game thread (inside `0xB72CC0`).
pub fn capture(i: usize, data: usize, size: u32) {
    if data < 0x10000 || size as usize != HIT_RECORD {
        return;
    }
    let Some(peer) = PeerId::new(i as u8) else { return };
    if !ACTIVE.load(Ordering::Relaxed) {
        N_DROP_OFF.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if BOSS_PAUSE.load(Ordering::Relaxed) {
        N_DROP_BOSS.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if cooling_down(peer) {
        N_DROP_COOLDOWN.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let record = unsafe { std::slice::from_raw_parts(data as *const u8, HIT_RECORD) }.to_vec();
    N_CAPTURED.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut q) = OUTBOX.lock() {
        if q.len() >= MAX_QUEUE {
            q.pop_front();
        }
        q.push_back(Captured { peer, record, at: Instant::now() });
    }
}

/// The next queued record to serve now, with its attacker chr; drops what must not apply.
fn serve(buf: *mut u8) -> Option<usize> {
    if buf.is_null() {
        return None;
    }
    loop {
        let inc = INBOX.lock().ok()?.pop_front()?;
        if BOSS_PAUSE.load(Ordering::Relaxed) {
            N_DROP_BOSS.fetch_add(1, Ordering::Relaxed);
            event(Event::Dropped { from: inc.from, seq: inc.seq, why: "boss fight (friendly_fire_in_boss_fights = 0)" });
            continue;
        }
        if cooling_down(inc.from) {
            N_DROP_COOLDOWN.fetch_add(1, Ordering::Relaxed);
            event(Event::Dropped { from: inc.from, seq: inc.seq, why: "duel over cooldown" });
            continue;
        }
        let queued = inc.at.elapsed();
        if queued > STALE_AFTER {
            N_DROP_STALE.fetch_add(1, Ordering::Relaxed);
            event(Event::Dropped { from: inc.from, seq: inc.seq, why: "stale (waited > 2 s here)" });
            continue;
        }
        // Re-checked against the player chr set now: the remote may have been deleted since
        // native.rs published it.
        let Some(attacker) = native::chr_of_peer(inc.from).filter(|&c| native::live_remote(c) == Some(inc.from)) else {
            N_DROP_NO_REMOTE.fetch_add(1, Ordering::Relaxed);
            event(Event::Dropped { from: inc.from, seq: inc.seq, why: "no native remote of the sender" });
            continue;
        };
        let atk = i32::from_le_bytes(inc.record[REC_ATK_ID..REC_ATK_ID + 4].try_into().unwrap_or([0xFF; 4]));
        if DRY_RUN.load(Ordering::Relaxed) {
            N_DRY.fetch_add(1, Ordering::Relaxed);
            event(Event::Dry { from: inc.from, seq: inc.seq, atk });
            continue;
        }
        unsafe { std::ptr::copy_nonoverlapping(inc.record.as_ptr(), buf, HIT_RECORD) };
        if let Ok(mut s) = SERVING.lock() {
            *s = Some(Serving { from: inc.from, seq: inc.seq, held_ms: inc.held_ms, queued_ms: queued.as_millis() as u64, attacker, atk, bullet: record_is_bullet(&inc.record) });
        }
        return Some(attacker);
    }
}

/// `0x8D7B10(rx, record*) -> PlayerIns*` at both call sites in `0xB72A80`. Our sentinel rx
/// (`drive_receive`) never reaches the original.
extern "C" fn recv_hook(rx: usize, buf: *mut u8) -> usize {
    if ACTIVE.load(Ordering::Relaxed)
        && let Ok(Some(attacker)) = std::panic::catch_unwind(|| serve(buf))
    {
        return attacker;
    }
    if rx == sentinel_rx() {
        return 0;
    }
    let f: extern "C" fn(usize, *mut u8) -> usize = unsafe { std::mem::transmute(ORIG_RECV.load(Ordering::Relaxed)) };
    f(rx, buf)
}

/// Chr finder candidate check `(finder, candidate chr, ..) -> bool` (`0xBDF930` takes two
/// arguments, `0xB5DD30` four; the extra registers are passed through untouched).
fn finder_call(i: usize, this: usize, chr: usize, a3: usize, a4: usize) -> u64 {
    let refuse = std::panic::catch_unwind(|| finder_refuses(chr, native::peer_of_chr(chr).is_some())).unwrap_or(false);
    if refuse {
        N_FINDER_REFUSED.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let f: extern "C" fn(usize, usize, usize, usize) -> u64 = unsafe { std::mem::transmute(ORIG_FINDERS[i].load(Ordering::Relaxed)) };
    f(this, chr, a3, a4)
}

extern "C" fn throw_finder_hook(this: usize, chr: usize, a3: usize, a4: usize) -> u64 {
    finder_call(0, this, chr, a3, a4)
}

extern "C" fn ceremony_finder_hook(this: usize, chr: usize, a3: usize, a4: usize) -> u64 {
    finder_call(1, this, chr, a3, a4)
}

fn local_data() -> Option<usize> {
    let p = crate::game::player()?;
    read_ptr(read_ptr(p.0 + CHR_MODULES)? + MOD_DATA)
}

struct Before {
    data: usize,
    hp: i32,
    posture: i32,
    flags: u8,
}

/// `0xB68FF0(dmgModule, attacker, hit, remote)` at `0xB72BB8`: ours with the "cannot die"
/// bit for that one call; anything else straight through.
extern "C" fn apply_hook(module: usize, attacker: usize, hit: usize, remote: u8) {
    let f: extern "C" fn(usize, usize, usize, u8) = unsafe { std::mem::transmute(ORIG_APPLY.load(Ordering::Relaxed)) };
    let ours = SERVING.lock().ok().and_then(|mut g| g.take()).filter(|s| s.attacker == attacker && attacker != 0);
    let Some(s) = ours else {
        f(module, attacker, hit, remote);
        return;
    };
    let before = std::panic::catch_unwind(|| -> Option<Before> {
        let hit_type = read::<i32>(hit + HIT_TYPE)?;
        if hit_type == HIT_TYPE_DEATHBLOW {
            return None;
        }
        let data = local_data()?;
        let b = Before { data, hp: read::<i32>(data + DATA_HP)?, posture: read::<i32>(data + DATA_POSTURE)?, flags: read::<u8>(data + DATA_FLAGS)? };
        (b.hp > 0 && write(data + DATA_FLAGS, b.flags | FLAG_NO_DEAD)).then_some(b)
    })
    .ok()
    .flatten();
    let Some(b) = before else {
        // A deathblow (or unreadable state, or already dead): not applied, the duel is over.
        let deathblow = read::<i32>(hit + HIT_TYPE) == Some(HIT_TYPE_DEATHBLOW);
        if deathblow {
            N_DEATHBLOW.fetch_add(1, Ordering::Relaxed);
            start_cooldown(s.from);
            event(Event::Deathblow { s });
        } else {
            event(Event::Dropped { from: s.from, seq: s.seq, why: "our HP / data module unreadable or HP 0" });
        }
        return;
    };
    f(module, attacker, hit, remote);
    let _ = std::panic::catch_unwind(|| {
        // Only our bit back; anything else the game changed in the byte stays.
        if let Some(now) = read::<u8>(b.data + DATA_FLAGS) {
            write(b.data + DATA_FLAGS, (now & !FLAG_NO_DEAD) | (b.flags & FLAG_NO_DEAD));
        }
        let hp = read::<i32>(b.data + DATA_HP).unwrap_or(-1);
        let posture = read::<i32>(b.data + DATA_POSTURE).unwrap_or(-1);
        let dmg_hp = read::<u32>(hit + HIT_HP).and_then(hp_damage);
        let lost = duel_lost(b.hp, hp, dmg_hp);
        if lost {
            start_cooldown(s.from);
        }
        N_APPLIED.fetch_add(1, Ordering::Relaxed);
        event(Event::Applied {
            s,
            hp: (b.hp, hp),
            posture: (b.posture, posture),
            dmg_hp,
            dmg_posture: read::<i32>(hit + HIT_POSTURE).unwrap_or(-1),
            hit_type: read::<i32>(hit + HIT_TYPE).unwrap_or(-1),
            result: read::<i32>(hit + HIT_RESULT).unwrap_or(-1),
            lost,
        });
    });
}

fn is_local_damage_module(module: usize) -> bool {
    crate::game::player().and_then(|p| read_ptr(p.0 + CHR_MODULES)).and_then(|m| read_ptr(m + MOD_DAMAGE)) == Some(module)
}

/// `0xB6AB40(dmgModule, attacker, hit)` (action 0) in the router: counts only.
extern "C" fn reaction_hook(module: usize, attacker: usize, hit: usize) {
    if ACTIVE.load(Ordering::Relaxed) && attacker != 0 {
        let _ = std::panic::catch_unwind(|| {
            if native::peer_of_chr(attacker).is_some() && is_local_damage_module(module) {
                N_REACTION_ONLY.fetch_add(1, Ordering::Relaxed);
            }
        });
    }
    let f: extern "C" fn(usize, usize, usize) = unsafe { std::mem::transmute(ORIG_REACTION.load(Ordering::Relaxed)) };
    f(module, attacker, hit)
}

/// `0xA4A3F0(WorldChrMan, handle) -> ChrIns*` at `0x66CE5C` (death event handler `0x66CDA0`):
/// a native remote as the killer would crash the session-member lookup after it; null makes
/// the handler leave through its own "no attacker" path.
extern "C" fn death_attacker_hook(wcm: usize, handle: u32) -> usize {
    let f: extern "C" fn(usize, u32) -> usize = unsafe { std::mem::transmute(ORIG_CHR_BY_HANDLE.load(Ordering::Relaxed)) };
    let chr = f(wcm, handle);
    let peer = std::panic::catch_unwind(|| native::peer_of_chr(chr)).ok().flatten();
    if skip_death_attacker(chr, peer.is_some()) {
        event(Event::DeathGuard { peer });
        return 0;
    }
    chr
}

// ===================================================================== install

/// The shared stub page (allocated once, near the module).
fn stub_page() -> Option<usize> {
    let have = STUB_PAGE.load(Ordering::Acquire);
    if have != 0 {
        return Some(have);
    }
    let page = hook::alloc_near_pub(module_base() + SITE_RECV_A)?;
    STUB_PAGE.store(page, Ordering::Release);
    Some(page)
}

/// Writes `mov rax, hook_fn; jmp rax` into stub slot `i` and returns the rel32 from `site_addr`.
fn make_stub(page: usize, i: usize, site_addr: usize, hook_fn: usize) -> Option<[u8; 4]> {
    let stub = page + i * 16;
    let mut code = [0u8; 12];
    code[0] = 0x48;
    code[1] = 0xB8;
    code[2..10].copy_from_slice(&(hook_fn as u64).to_le_bytes());
    code[10] = 0xFF;
    code[11] = 0xE0;
    unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), stub as *mut u8, code.len()) };
    i32::try_from(stub as i64 - (site_addr + 5) as i64).ok().map(i32::to_le_bytes)
}

/// Stub slot of the death guard (0..3 are the PvP sites).
const DG_STUB: usize = 4;

/// The death guard (`[DEBUG] pvp_death_guard`): one call-site patch, independent of
/// friendly_fire, tried once when the first native remote exists.
fn install_death_guard() {
    let base = module_base();
    let site = base + SITE_DEATH_ATTACKER;
    let op = read::<u8>(site).unwrap_or(0);
    let rel = read::<[u8; 4]>(site + 1).unwrap_or([0; 4]);
    if call_target(site, op, rel) != Some(base + FN_CHR_BY_HANDLE) {
        log::line(&format!("pvp: death guard NOT installed: {site:#x} has {op:02x} {rel:02x?}, not call {:#x}", base + FN_CHR_BY_HANDLE));
        return;
    }
    let Some(page) = stub_page() else {
        log::line("pvp: death guard NOT installed: no memory near the module");
        return;
    };
    ORIG_CHR_BY_HANDLE.store(base + FN_CHR_BY_HANDLE, Ordering::Relaxed);
    let Some(new) = make_stub(page, DG_STUB, site, death_attacker_hook as *const () as usize) else {
        log::line("pvp: death guard NOT installed: stub out of rel32 range");
        return;
    };
    match hook::write_frozen(&[(site + 1, &rel[..], &new[..])]) {
        Ok(()) => {
            DG_INSTALLED.store(true, Ordering::Relaxed);
            log::line(&format!("pvp: death guard installed at {site:#x} (death event 0x66CDA0: a native remote as the killer = no killer)"));
        }
        Err(e) => log::line(&format!("pvp: death guard NOT installed: frozen write failed: {e:?}")),
    }
}

/// Every check first (call sites, damage tables, PlayerDebugSession slot 4), then one
/// frozen write of the four rel32s. Err(true) = busy, try again later.
fn install() -> Result<(), (bool, String)> {
    let base = module_base();
    let sites: [(usize, usize, usize, &str); 4] = [
        (SITE_RECV_A, FN_RECV, recv_hook as *const () as usize, "pvp type-20 recv 1 (0xB72B1D)"),
        (SITE_RECV_B, FN_RECV, recv_hook as *const () as usize, "pvp type-20 recv 2 (0xB72BC7)"),
        (SITE_APPLY, FN_APPLY, apply_hook as *const () as usize, "pvp type-20 apply (0xB72BB8)"),
        (SITE_REACTION, FN_REACTION, reaction_hook as *const () as usize, "pvp router action 0 (0xB6A12A)"),
    ];
    let mut rels = Vec::with_capacity(sites.len());
    for &(site, target, _, name) in &sites {
        let op = read::<u8>(base + site).unwrap_or(0);
        let rel = read::<[u8; 4]>(base + site + 1).unwrap_or([0; 4]);
        if call_target(base + site, op, rel) != Some(base + target) {
            return Err((false, format!("{name}: bytes {op:02x} {rel:02x?} are not call {:#x}", base + target)));
        }
        rels.push(rel);
    }
    let cell = |t: usize, v: usize, a: usize| read::<u32>(base + t + (v * 7 + a) * 4).unwrap_or(u32::MAX);
    let l12 = [cell(DMG_TABLES[0], 1, 2), cell(DMG_TABLES[1], 1, 2)];
    let r21 = cell(DMG_TABLES[0], 2, 1);
    if !tables_ok(l12, r21) {
        return Err((false, format!("damage tables not vanilla: [1][2] = {l12:?} (want [0, 0]), [2][1] = {r21} (want 4)")));
    }
    let found: Vec<u8> = (0..DEBUG_SEND_HIT_BYTES.len()).filter_map(|i| read::<u8>(base + DEBUG_SEND_HIT_RVA + i)).collect();
    if found != DEBUG_SEND_HIT_BYTES {
        return Err((false, format!("PlayerDebugSession slot 4 (0x1061880) has {found:02x?}")));
    }
    if read_ptr(base + DEBUG_SESSION_VT_RVA + 8 * 4) != Some(base + DEBUG_SEND_HIT_RVA) {
        return Err((false, "PlayerDebugSession vtable slot 4 is not 0x1061880".into()));
    }
    // drive_receive calls 0xB72A80 itself: its first bytes must be the rx NULL check.
    let head: Vec<u8> = (0..RECV_LOOP_HEAD.len()).filter_map(|i| read::<u8>(base + FN_RECV_LOOP + i)).collect();
    if head != RECV_LOOP_HEAD {
        return Err((false, format!("type-20 receive loop 0xB72A80 starts with {head:02x?}")));
    }
    // The finders: each vtable slot holds its function, whose first bytes are as expected.
    let mut finder_old: Vec<[u8; 8]> = Vec::with_capacity(FINDERS.len());
    for &(slot, func, prologue, name) in &FINDERS {
        let found = read_ptr(base + slot);
        if found != Some(base + func) {
            return Err((false, format!("{name}: vtable slot {:#x} holds {found:#x?}, not {:#x}", base + slot, base + func)));
        }
        let code: Vec<u8> = (0..prologue.len()).filter_map(|i| read::<u8>(base + func + i)).collect();
        if code != prologue {
            return Err((false, format!("{name}: {:#x} starts with {code:02x?}", base + func)));
        }
        finder_old.push(((base + func) as u64).to_le_bytes());
    }
    let Some(page) = stub_page() else {
        return Err((false, "no memory near the module for the stubs".into()));
    };
    ORIG_RECV.store(base + FN_RECV, Ordering::Relaxed);
    ORIG_APPLY.store(base + FN_APPLY, Ordering::Relaxed);
    ORIG_REACTION.store(base + FN_REACTION, Ordering::Relaxed);
    for (i, &(_, func, ..)) in FINDERS.iter().enumerate() {
        ORIG_FINDERS[i].store(base + func, Ordering::Relaxed);
    }
    let mut news: Vec<[u8; 4]> = Vec::with_capacity(sites.len());
    for (i, &(site, _, hook_fn, name)) in sites.iter().enumerate() {
        let Some(rel) = make_stub(page, i, base + site, hook_fn) else {
            return Err((false, format!("{name}: stub out of rel32 range")));
        };
        news.push(rel);
    }
    // Vtable slots take the absolute address of our wrapper (no stub needed).
    let finder_new: [[u8; 8]; 2] =
        [(throw_finder_hook as *const () as u64).to_le_bytes(), (ceremony_finder_hook as *const () as u64).to_le_bytes()];
    let mut items: Vec<(usize, &[u8], &[u8])> =
        sites.iter().zip(rels.iter().zip(&news)).map(|(&(site, ..), (old, new))| (base + site + 1, &old[..], &new[..])).collect();
    for (i, &(slot, ..)) in FINDERS.iter().enumerate() {
        items.push((base + slot, &finder_old[i][..], &finder_new[i][..]));
    }
    match hook::write_frozen(&items) {
        Ok(()) => {
            log::line(&format!(
                "pvp: hooks installed (type-20 recv x2 + apply in 0xB72A80, driven by the mod; action 0 counter in 0xB6A040; throw + deathblow finders refuse friends' copies (vtable 0x142A8EBD0, 0x142A7BC20); tables [1][2] = {l12:?}, [2][1] = {r21}; stubs {page:#x})"
            ));
            Ok(())
        }
        Err(hook::freeze::Error::Bytes(i)) => Err((
            false,
            format!("{} changed under us (nothing written)", sites.get(i).map_or_else(|| FINDERS[i - sites.len()].3, |s| s.3)),
        )),
        Err(e) => Err((true, format!("frozen write failed: {e:?} (nothing written)"))),
    }
}

/// lib.rs, when the mod disables itself: every hook passes straight through.
pub fn shutdown() {
    ACTIVE.store(false, Ordering::Relaxed);
    BOSS_PAUSE.store(false, Ordering::Relaxed);
    restore_all("mod shutdown");
    if let Ok(mut q) = INBOX.lock() {
        q.clear();
    }
    if let Ok(mut q) = OUTBOX.lock() {
        q.clear();
    }
    if let Ok(mut s) = SERVING.lock() {
        *s = None;
    }
}

/// net.rs: a bulk of kind `BULK_PVP` from `from` (any thread).
pub fn on_bulk(from: PeerId, data: &[u8]) {
    let _ = std::panic::catch_unwind(|| {
        let Some(m) = decode(data) else {
            log::line(&format!("pvp: unreadable message from peer {} ({} bytes)", from.slot(), data.len()));
            return;
        };
        let (nonce, seq) = match &m {
            Msg::Hit { nonce, seq, .. } | Msg::DuelOver { nonce, seq, .. } | Msg::Hello { nonce, seq } => (*nonce, *seq),
        };
        let fresh = DEDUP.lock().map(|mut d| d[from.index()].get_or_insert_with(Dedup::default).accept(nonce, seq)).unwrap_or(true);
        if !fresh {
            N_DROP_DUP.fetch_add(1, Ordering::Relaxed);
            return;
        }
        match m {
            Msg::Hit { seq, held_ms, record, .. } => {
                N_RECEIVED.fetch_add(1, Ordering::Relaxed);
                if !ACTIVE.load(Ordering::Relaxed) {
                    N_DROP_OFF.fetch_add(1, Ordering::Relaxed);
                    event(Event::Dropped { from, seq, why: "friendly fire is off here" });
                    return;
                }
                if let Ok(mut q) = INBOX.lock() {
                    if q.len() >= MAX_QUEUE {
                        q.pop_front();
                    }
                    q.push_back(Incoming { from, seq, held_ms, record, at: Instant::now() });
                }
            }
            Msg::Hello { .. } => {
                if HELLOS.lock().is_ok_and(|mut h| h.seen(from, now_ms())) {
                    event(Event::Hello { from });
                }
            }
            Msg::DuelOver { reason, .. } => {
                start_cooldown(from);
                if let Ok(mut q) = INBOX.lock() {
                    q.retain(|i| i.from != from);
                }
                event(Event::WeWon { from, reason });
            }
        }
    });
}

// ===================================================================== per frame

/// One frame's view (puppet.rs, after the native remotes were refreshed).
pub struct Frame<'a> {
    pub net: &'a mut Net,
    pub player: Chr,
    pub peers: &'a PeerSet,
    /// The host's `friendly_fire` as a client got it (ScaleCfg), None = not (yet) known.
    pub host_ff: Option<bool>,
}

/// One remote we changed: its original team and "cannot die" bit.
struct Changed {
    chr: usize,
    peer: PeerId,
    team: u32,
    no_dead: u8,
}

/// Puts one remote's team and bit back, only while it is still that peer's live remote.
fn restore(c: &Changed, why: &str) {
    if native::live_remote(c.chr) != Some(c.peer) {
        log::line(&format!("pvp: peer {}'s native remote {:#x} is gone ({why}): nothing to restore", c.peer.slot(), c.chr));
        return;
    }
    write(c.chr + CHR_TEAM, c.team);
    if let Some(d) = data_of(c.chr)
        && let Some(fl) = read::<u8>(d + DATA_FLAGS)
    {
        write(d + DATA_FLAGS, (fl & !FLAG_NO_DEAD) | (c.no_dead & FLAG_NO_DEAD));
    }
    log::line(&format!("pvp: peer {}'s native remote {:#x}: team {} restored ({why})", c.peer.slot(), c.chr, c.team));
}

/// Every changed remote back (feature off, link gone, mod shutdown, hub reset).
fn restore_all(why: &str) {
    let all: Vec<Changed> = CHANGED.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    for c in &all {
        restore(c, why);
    }
}

fn data_of(chr: usize) -> Option<usize> {
    read_ptr(read_ptr(chr + CHR_MODULES)? + MOD_DATA)
}

/// Our player back to full HP and posture after a duel (`posture::set_hp` = the game's
/// setter `0xBD64E0`, `add_posture` = `0xBD4DE0`, as livesync's refill).
fn refill(player: Chr) {
    let Some(d) = data_of(player.0) else { return };
    let max_hp = read::<i32>(d + DATA_MAX_HP).unwrap_or(0);
    let max_posture = read::<i32>(d + DATA_MAX_POSTURE).unwrap_or(0);
    if max_hp > 0 {
        crate::posture::set_hp(player, max_hp);
    }
    if max_posture > 0 {
        crate::posture::add_posture(d, max_posture, false);
    }
    log::line(&format!("pvp: duel over: our HP {max_hp} and posture {max_posture} refilled"));
}

#[derive(Default)]
pub struct Pvp {
    install_at: Option<Instant>,
    install_tries: u32,
    stats_at: Option<Instant>,
    last_reason: String,
    seq: u32,
    hello_at: Option<Instant>,
    /// Connected peers and their link epochs last frame (leave / rejoin detection).
    links: Vec<(PeerId, u64)>,
    /// Remotes already reported as "held at 1 HP while the owner is dead".
    held_logged: Vec<usize>,
    /// PlayerIns+0x2088 was logged once (diagnostic).
    rx_noted: bool,
}

impl Drop for Pvp {
    /// The hub is dropped on a frame panic: nothing of ours stays on a living remote.
    fn drop(&mut self) {
        let _ = std::panic::catch_unwind(|| {
            ACTIVE.store(false, Ordering::Relaxed);
            BOSS_PAUSE.store(false, Ordering::Relaxed);
            restore_all("co-op hub reset");
        });
    }
}

impl Pvp {
    /// puppet.rs, every frame without a Net: everything off and restored.
    pub fn off(&mut self, why: &str) {
        let was = ACTIVE.swap(false, Ordering::Relaxed);
        BOSS_PAUSE.store(false, Ordering::Relaxed);
        if was {
            log::line(&format!("pvp: player-vs-player damage off ({why})"));
            if let Ok(mut q) = INBOX.lock() {
                q.clear();
            }
        }
        if CHANGED.lock().is_ok_and(|g| !g.is_empty()) {
            restore_all(why);
        }
        for (p, _) in std::mem::take(&mut self.links) {
            reset_peer(p);
        }
        self.hello_at = None;
    }

    pub fn tick(&mut self, f: Frame) {
        // Leave / rejoin: forget the peer's Hello, cooldown, duplicate filter and queue.
        let links: Vec<(PeerId, u64)> = f.peers.iter().map(|p| (p.id, p.epoch)).collect();
        for p in left_peers(&self.links, &links) {
            reset_peer(p);
            log::line(&format!("pvp: peer {} left or reconnected: its Hello, cooldown and duplicate filter are reset", p.slot()));
        }
        self.links = links;
        if DEATH_GUARD.load(Ordering::Relaxed) && !DG_TRIED.load(Ordering::Relaxed) && f.peers.iter().any(|p| p.native.is_some()) {
            DG_TRIED.store(true, Ordering::Relaxed);
            install_death_guard();
        }

        let connected = f.net.connected();
        let is_host = f.net.is_host();
        let own = FRIENDLY_FIRE.load(Ordering::Relaxed);
        let setting = if is_host { own } else { f.host_ff.unwrap_or(false) };
        let want = connected && setting && !BROKEN.load(Ordering::Relaxed);
        if want && !INSTALLED.load(Ordering::Relaxed) && self.install_at.is_none_or(|t| t.elapsed() >= INSTALL_RETRY) {
            self.install_at = Some(Instant::now());
            self.install_tries += 1;
            match install() {
                Ok(()) => INSTALLED.store(true, Ordering::Relaxed),
                Err((retry, why)) => {
                    let give_up = !retry || self.install_tries >= INSTALL_TRIES;
                    if give_up {
                        BROKEN.store(true, Ordering::Relaxed);
                    }
                    log::line(&format!("pvp: hooks NOT installed: {why}{}", if give_up { "; PvP off for this run" } else { "; retrying" }));
                }
            }
        }
        let active = want && INSTALLED.load(Ordering::Relaxed);
        let reason = match (connected, is_host, own, f.host_ff, BROKEN.load(Ordering::Relaxed)) {
            (_, _, _, _, true) => "hooks unavailable".to_string(),
            (false, ..) => "no link".to_string(),
            (true, true, o, ..) => format!("friendly_fire = {} here (host)", o as u8),
            (true, false, o, h, _) => format!("host's friendly_fire = {}, ours {}", h.map_or("? (older host build)".into(), |h| (h as u8).to_string()), o as u8),
        };
        if ACTIVE.swap(active, Ordering::Relaxed) != active || (active && reason != self.last_reason) {
            log::line(&format!(
                "pvp: player-vs-player damage {} ({reason}){}",
                if active { "ON" } else { "off" },
                if active && DRY_RUN.load(Ordering::Relaxed) { "; DRY RUN: received hits are only logged" } else { "" }
            ));
            if !active && let Ok(mut q) = INBOX.lock() {
                q.clear();
            }
        }
        self.last_reason = reason;
        // Boss fights: own teams back and hits dropped unless friendly_fire_in_boss_fights.
        let boss = active && !IN_BOSS_FIGHTS.load(Ordering::Relaxed) && crate::game::boss_state().is_some();
        if BOSS_PAUSE.swap(boss, Ordering::Relaxed) != boss {
            log::line(&format!(
                "pvp: boss fight {}: {}",
                if boss { "started" } else { "over" },
                if boss { "friends' copies get their own team back, player hits are dropped (friendly_fire_in_boss_fights = 0)" } else { "friendly fire again" }
            ));
        }
        self.teams(f.peers, active && !boss);
        if active {
            self.check_send_path(f.peers);
        }
        if active && self.hello_at.is_none_or(|t| t.elapsed() >= HELLO_EVERY) {
            self.hello_at = Some(Instant::now());
            for p in f.peers.iter() {
                self.seq = self.seq.wrapping_add(1);
                f.net.send_bulk_to(Dest::Peer(p.id), BULK_PVP, &encode(&Msg::Hello { nonce: nonce(), seq: self.seq }));
            }
        }
        if !active {
            self.hello_at = None;
        }
        if active {
            self.drive_receive(f.player);
        }
        self.flush_out(&mut *f.net, f.peers, f.player, active);
        self.drain_events(&mut *f.net, f.peers, f.player);
        self.stats(active);
    }

    /// Team 29 + "cannot die while the owner lives" on the native remote of every peer with
    /// a fresh Hello while `on`; the originals back for every other remote.
    fn teams(&mut self, peers: &PeerSet, on: bool) {
        let remotes: Vec<(PeerId, usize, Option<i32>)> =
            peers.iter().filter_map(|p| Some((p.id, p.native?.0, p.snapshot.map(|s| s.hp)))).collect();
        let Ok(mut changed) = CHANGED.lock() else { return };
        // Gone remotes: forget (never write to a deleted chr).
        changed.retain(|c| {
            let here = remotes.iter().any(|&(p, chr, _)| chr == c.chr && p == c.peer);
            if !here {
                log::line(&format!("pvp: peer {}'s native remote {:#x} is gone (nothing to restore)", c.peer.slot(), c.chr));
            }
            here
        });
        self.held_logged.retain(|chr| remotes.iter().any(|&(_, c, _)| c == *chr));
        for &(peer, chr, hp) in &remotes {
            let want = on && hello_fresh(peer);
            let pos = changed.iter().position(|c| c.chr == chr);
            if !want {
                if let Some(i) = pos {
                    let c = changed.remove(i);
                    restore(&c, if on { "no Hello from that peer" } else { "friendly fire off or paused" });
                }
                continue;
            }
            let Some(team) = read::<u32>(chr + CHR_TEAM) else { continue };
            let data = data_of(chr);
            let flags = data.and_then(|d| read::<u8>(d + DATA_FLAGS));
            let orig_bit = match pos {
                Some(i) => changed[i].no_dead,
                None => {
                    let Some(fl) = flags else { continue };
                    // Already ours (our hub was rebuilt after a frame panic): the remote's own
                    // values were the chr type's team 2 and no bit.
                    let (t, b) = if team == PVP_TEAM { (2, 0) } else { (team, fl & FLAG_NO_DEAD) };
                    changed.push(Changed { chr, peer, team: t, no_dead: b });
                    log::line(&format!(
                        "pvp: peer {}'s native remote {chr:#x}: team {t} -> {PVP_TEAM} (Coop NPC: our hits reach it, no lock-on / enemy bar), our player's team unchanged",
                        peer.slot()
                    ));
                    b
                }
            };
            if team != PVP_TEAM {
                write(chr + CHR_TEAM, PVP_TEAM);
            }
            if let (Some(d), Some(fl)) = (data, flags) {
                let undying = replica_undying(hp, native::frame_age_ms(peer));
                let bit = if undying { fl | FLAG_NO_DEAD } else { (fl & !FLAG_NO_DEAD) | orig_bit };
                if bit != fl {
                    write(d + DATA_FLAGS, bit);
                }
                let copy_hp = read::<i32>(d + DATA_HP);
                if hp.is_some_and(|h| h <= 0) && copy_hp == Some(1) && !self.held_logged.contains(&chr) {
                    self.held_logged.push(chr);
                    log::line(&format!("pvp: replica held at 1 HP while its owner is dead (peer {}, remote {chr:#x}): undying bit cleared", peer.slot()));
                }
            }
        }
    }

    /// Frame task (game thread), hits queued: the game's own receive loop
    /// `0xB72A80(our dmgModule, sentinel rx)` applies them (`recv_hook` serves the queue, then 0
    /// for the sentinel; `apply_hook` logs each). Vanilla only calls it with PlayerIns+0x2088,
    /// which stays NULL in our session.
    fn drive_receive(&mut self, player: Chr) {
        if INBOX.lock().map_or(true, |q| q.is_empty()) {
            return;
        }
        if !self.rx_noted {
            self.rx_noted = true;
            let rx = read_ptr(player.0 + PLAYER_RX);
            let online = read_ptr(module_base() + ONLINE_HOLDER_RVA).filter(|&h| h != 0).and_then(|h| read_ptr(h + ONLINE_PLAYER)).map(|p| p == player.0);
            log::line(&format!(
                "pvp: receive path: PlayerIns+0x2088 = {} ({}); game's online player is ours: {}",
                rx.map_or("?".into(), |v| format!("{v:#x}")),
                vanilla_receive_note(rx, online),
                online.map_or("?".into(), |o| o.to_string())
            ));
        }
        let Some(module) = read_ptr(player.0 + CHR_MODULES).and_then(|m| read_ptr(m + MOD_DAMAGE)).filter(|&m| m != 0) else {
            return; // a load: the queue goes stale and is dropped with a reason on a later frame
        };
        N_DRIVES.fetch_add(1, Ordering::Relaxed);
        let f: extern "C" fn(usize, usize) = unsafe { std::mem::transmute(module_base() + FN_RECV_LOOP) };
        f(module, sentinel_rx());
    }

    /// Once per remote recv vtable: its slot 4 must be PlayerDebugSession's `0x1061880`
    /// (else our slot 1 never sees the hit records).
    fn check_send_path(&mut self, peers: &PeerSet) {
        static CHECKED: AtomicBool = AtomicBool::new(false);
        if CHECKED.load(Ordering::Relaxed) || !peers.iter().any(|p| p.native.is_some()) {
            return;
        }
        let Some(slot4) = native::recv_vt_hit_slot() else { return };
        CHECKED.store(true, Ordering::Relaxed);
        let want = module_base() + DEBUG_SEND_HIT_RVA;
        log::line(&format!(
            "pvp: remote session slot 4 = {slot4:#x} ({}): our hits on the friend's copy are {}",
            if slot4 == want { "PlayerDebugSession 0x1061880" } else { "UNEXPECTED" },
            if slot4 == want { "captured in native.rs slot 1" } else { "NOT captured (no hits will be sent)" }
        ));
    }

    fn flush_out(&mut self, net: &mut Net, peers: &PeerSet, player: Chr, active: bool) {
        let out: Vec<Captured> = OUTBOX.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
        for c in out {
            if !active {
                N_DROP_OFF.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if !hello_fresh(c.peer) {
                N_NO_HELLO.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            self.seq = self.seq.wrapping_add(1);
            let seq = self.seq;
            let id = HIT_IDS.fetch_add(1, Ordering::Relaxed) + 1;
            let held_ms = c.at.elapsed().as_millis().min(u16::MAX as u128) as u16;
            let atk = i32::from_le_bytes(c.record[REC_ATK_ID..REC_ATK_ID + 4].try_into().unwrap_or([0xFF; 4]));
            let dmg: Vec<i16> = (0..5).map(|k| i16::from_le_bytes([c.record[REC_DMG + 2 * k], c.record[REC_DMG + 2 * k + 1]])).collect();
            let bullet = record_is_bullet(&c.record);
            let msg = encode(&Msg::Hit { nonce: nonce(), seq, held_ms, record: c.record });
            let ok = net.send_bulk_to(Dest::Peer(c.peer), BULK_PVP, &msg);
            if ok {
                N_SENT.fetch_add(1, Ordering::Relaxed);
            }
            let p = peers.by_id(c.peer);
            let ours = player.position();
            let theirs = p.and_then(|p| p.native).and_then(|r| r.position());
            let dist = match (ours, theirs) {
                (Some(a), Some(b)) => format!("{:.2} m", crate::game::distance(a, b)),
                _ => "? m".into(),
            };
            log::line(&format!(
                "pvp: hit #{seq} (local {id}) sent to {} (peer {}, ok {ok}): behavior {atk} ({}), record dmg {dmg:?}, held {held_ms} ms; us {ours:?}, replica {theirs:?} ({dist}); last type 1 of the victim {} ms ago; ping {} ms",
                p.map_or("?", |p| p.name.as_str()),
                c.peer.slot(),
                hit_kind(bullet),
                native::frame_age_ms(c.peer).map_or("?".into(), |v| v.to_string()),
                p.and_then(|p| p.rtt_ms).map_or("?".into(), |v| v.to_string()),
            ));
        }
    }

    fn drain_events(&mut self, net: &mut Net, peers: &PeerSet, player: Chr) {
        let evs: Vec<Event> = EVENTS.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
        let name = |id: PeerId| peers.by_id(id).map_or_else(|| format!("peer {}", id.slot()), |p| p.name.clone());
        let rtt = |id: PeerId| peers.by_id(id).and_then(|p| p.rtt_ms);
        for e in evs {
            match e {
                Event::Applied { s, hp, posture, dmg_hp, dmg_posture, hit_type, result, lost } => {
                    let est = rtt(s.from).map_or("?".into(), |r| (s.held_ms as u32 + r / 2).to_string());
                    log::line(&format!(
                        "pvp: hit #{} from {} received (behavior {} ({}), +~{est} ms sender->here incl. {} ms held there, {} ms queued here) -> applied hp {}->{} posture {}->{} (hit hp {} posture {dmg_posture}, type {hit_type}, result {result}){}",
                        s.seq,
                        name(s.from),
                        s.atk,
                        hit_kind(s.bullet),
                        s.held_ms,
                        s.queued_ms,
                        hp.0,
                        hp.1,
                        posture.0,
                        posture.1,
                        dmg_hp.map_or("?".into(), |v| v.to_string()),
                        if lost { ", CLAMPED at 1 HP (duel lost)" } else { "" }
                    ));
                    if lost {
                        self.duel_lost(net, s.from, 1, &name(s.from));
                        refill(player);
                    }
                }
                Event::Deathblow { s } => {
                    log::line(&format!("pvp: hit #{} from {} is a deathblow: not applied (lethal PvP is not supported)", s.seq, name(s.from)));
                    self.duel_lost(net, s.from, 2, &name(s.from));
                    refill(player);
                }
                Event::Dry { from, seq, atk } => {
                    log::line(&format!("pvp: hit #{seq} from {} received, behavior {atk}: DRY RUN, not applied", name(from)))
                }
                Event::Dropped { from, seq, why } => log::line(&format!("pvp: hit #{seq} from {} dropped: {why}", name(from))),
                Event::Hello { from } => log::line(&format!("pvp: {} has friendly fire on: our hits on their copy are sent to them", name(from))),
                Event::WeWon { from, reason } => {
                    log::line(&format!(
                        "pvp: duel over, we win ({} {}); player hits ignored for {} s",
                        name(from),
                        if reason == 2 { "took a deathblow" } else { "is down to 1 HP" },
                        DUEL_OVER_FOR.as_secs()
                    ));
                    refill(player);
                }
                Event::DeathGuard { peer } => log::line(&format!(
                    "pvp: death event with a player attacker skipped (replica {})",
                    peer.map_or_else(|| "?".into(), |p| format!("of {}", name(p)))
                )),
            }
        }
    }

    fn duel_lost(&mut self, net: &mut Net, winner: PeerId, reason: u8, winner_name: &str) {
        self.seq = self.seq.wrapping_add(1);
        let ok = net.send_bulk_to(Dest::Peer(winner), BULK_PVP, &encode(&Msg::DuelOver { nonce: nonce(), seq: self.seq, reason }));
        if let Ok(mut q) = INBOX.lock() {
            q.retain(|i| i.from != winner);
        }
        log::line(&format!(
            "pvp: duel over, {winner_name} wins; player hits ignored for {} s (told the winner: {ok})",
            DUEL_OVER_FOR.as_secs()
        ));
    }

    fn stats(&mut self, active: bool) {
        if self.stats_at.is_some_and(|t| t.elapsed() < STATS_EVERY) {
            return;
        }
        self.stats_at = Some(Instant::now());
        let take = |a: &AtomicU64| a.swap(0, Ordering::Relaxed);
        let v = [
            take(&N_CAPTURED),
            take(&N_SENT),
            take(&N_RECEIVED),
            take(&N_APPLIED),
            take(&N_REACTION_ONLY),
            take(&N_DROP_COOLDOWN),
            take(&N_DROP_STALE),
            take(&N_DROP_NO_REMOTE),
            take(&N_DROP_OFF),
            take(&N_DROP_DUP),
            take(&N_DEATHBLOW),
            take(&N_DRY),
            take(&N_NO_HELLO),
            take(&N_DROP_BOSS),
            take(&N_DRIVES),
            take(&N_FINDER_REFUSED),
        ];
        if !active && v.iter().all(|&n| n == 0) {
            return;
        }
        log::line(&format!(
            "pvp: 10 s: our hits captured {} sent {}; friend's received {} applied {} (receive loop driven {}x); local-vs-replica hit dropped (laggy path, reaction only) {}; throw / deathblow on a friend refused {}; dropped: cooldown {} stale {} no remote {} off {} duplicate {} deathblow {} dry run {} friend without friendly fire {} boss fight {}",
            v[0], v[1], v[2], v[3], v[14], v[4], v[15], v[5], v[6], v[7], v[8], v[9], v[10], v[11], v[12], v[13]
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Vec<u8> {
        (0..HIT_RECORD).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn hit_message_round_trip() {
        let m = Msg::Hit { nonce: 0xDEAD_BEEF, seq: 42, held_ms: 17, record: record() };
        let b = encode(&m);
        assert_eq!(b.len(), HEADER + 2 + HIT_RECORD);
        assert_eq!(decode(&b), Some(m));
    }

    #[test]
    fn duel_over_round_trip() {
        let m = Msg::DuelOver { nonce: 7, seq: u32::MAX, reason: 2 };
        assert_eq!(decode(&encode(&m)), Some(m));
    }

    #[test]
    fn hello_round_trip() {
        let m = Msg::Hello { nonce: 3, seq: 9 };
        let b = encode(&m);
        assert_eq!(b.len(), HEADER);
        assert_eq!(decode(&b), Some(m));
        let mut long = b;
        long.push(0);
        assert_eq!(decode(&long), None);
    }

    #[test]
    fn bad_messages_are_refused() {
        let good = encode(&Msg::Hit { nonce: 1, seq: 1, held_ms: 0, record: record() });
        assert_eq!(decode(&good[..good.len() - 1]), None, "short record");
        let mut long = good.clone();
        long.push(0);
        assert_eq!(decode(&long), None, "long record");
        let mut magic = good.clone();
        magic[0] = 0;
        assert_eq!(decode(&magic), None);
        let mut ver = good.clone();
        ver[1] = 2;
        assert_eq!(decode(&ver), None, "another version");
        let mut kind = good;
        kind[2] = 9;
        assert_eq!(decode(&kind), None);
        assert_eq!(decode(&[]), None);
    }

    #[test]
    fn dedup_drops_repeats_and_restarts_on_a_new_nonce() {
        let mut d = Dedup::default();
        assert!(d.accept(5, 1));
        assert!(d.accept(5, 2));
        assert!(!d.accept(5, 1), "duplicate");
        assert!(!d.accept(5, 2));
        // The friend restarted the game: seq 1 again, new nonce.
        assert!(d.accept(9, 1));
        assert!(!d.accept(9, 1));
        // Out of order within the window is fine.
        assert!(d.accept(9, 3));
        assert!(d.accept(9, 2));
        // The window forgets the oldest.
        for s in 10..10 + Dedup::CAP as u32 {
            assert!(d.accept(9, s));
        }
        assert!(d.accept(9, 1), "fell out of the window");
    }

    #[test]
    fn clamp_rules() {
        // Normal hit: no.
        assert!(!duel_lost(500, 380, Some(120)));
        // Clamped by the "cannot die" bit: the setter left 1 HP.
        assert!(duel_lost(100, 1, Some(300)));
        assert!(duel_lost(100, 1, None), "HP fell to 1");
        // Exactly to 1 by a normal hit counts as the end too.
        assert!(duel_lost(120, 1, Some(119)));
        // Already at 1: a deflected / blocked hit (no damage) does not end it again ...
        assert!(!duel_lost(1, 1, Some(0)));
        assert!(!duel_lost(1, 1, None));
        // ... a hit that would have killed does.
        assert!(duel_lost(1, 1, Some(40)));
        // Dead before: never.
        assert!(!duel_lost(0, 0, Some(40)));
        // Bit failed and we died anyway: still the end (logged as clamped).
        assert!(duel_lost(50, 0, Some(80)));
    }

    #[test]
    fn hp_damage_reads_int_or_float() {
        assert_eq!(hp_damage(120), Some(120));
        assert_eq!(hp_damage(0), Some(0));
        assert_eq!(hp_damage(120.0f32.to_bits()), Some(120));
        assert_eq!(hp_damage((-5i32) as u32), None);
        assert_eq!(hp_damage(f32::NAN.to_bits()), None);
    }

    #[test]
    fn tables_must_be_vanilla() {
        assert!(tables_ok([0, 0], 4));
        assert!(!tables_ok([1, 0], 4), "1.0.11-style [1][2] = 1 (local application of the copy's hits)");
        assert!(!tables_ok([0, 2], 4));
        assert!(!tables_ok([0, 0], 1), "[2][1] no longer sends type 20");
    }

    #[test]
    fn call_target_decodes_rel32() {
        // 0xB72B1D: e8 ee 4f d6 ff -> 0x8D7B10 (base 0x140000000, dump 2026-11-11).
        let base = 0x1_4000_0000usize;
        assert_eq!(call_target(base + 0xB72B1D, 0xE8, [0xEE, 0x4F, 0xD6, 0xFF]), Some(base + 0x8D7B10));
        assert_eq!(call_target(base + 0xB72BB8, 0xE8, [0x33, 0x64, 0xFF, 0xFF]), Some(base + 0xB68FF0));
        assert_eq!(call_target(base + 0xB6A12A, 0xE8, [0x11, 0x0A, 0x00, 0x00]), Some(base + 0xB6AB40));
        assert_eq!(call_target(base + 0xB72BC7, 0xE9, [0x44, 0x4F, 0xD6, 0xFF]), None, "jmp, not call");
    }

    fn peer(i: u8) -> PeerId {
        PeerId::new(i).unwrap()
    }

    #[test]
    fn cooldown_is_per_peer() {
        let mut c = Cooldowns { until: [0; MAX_PLAYERS] };
        c.start(peer(1), 1000);
        assert!(c.active(peer(1), 1000));
        assert!(c.active(peer(1), 5999));
        assert!(!c.active(peer(1), 6000), "5 s");
        assert!(!c.active(peer(2), 1000), "another peer keeps fighting");
        c.reset(peer(1));
        assert!(!c.active(peer(1), 1001));
    }

    #[test]
    fn hello_expires_and_resets_when_the_peer_leaves() {
        let mut h = HelloBook { at: [0; MAX_PLAYERS] };
        assert!(!h.fresh(peer(1), 10));
        assert!(h.seen(peer(1), 10), "first Hello");
        assert!(!h.seen(peer(1), 20), "not the first any more");
        assert!(h.fresh(peer(1), 20 + HELLO_VALID_MS));
        assert!(!h.fresh(peer(1), 21 + HELLO_VALID_MS), "expired");
        assert!(!h.fresh(peer(2), 20));
        // Leave / rejoin detection, then the reset.
        let before = [(peer(1), 7), (peer(2), 3)];
        assert_eq!(left_peers(&before, &[(peer(2), 3)]), vec![peer(1)], "left");
        assert_eq!(left_peers(&before, &[(peer(1), 8), (peer(2), 3)]), vec![peer(1)], "new link epoch");
        assert!(left_peers(&before, &before).is_empty());
        h.seen(peer(1), 30);
        h.reset(peer(1));
        assert!(!h.fresh(peer(1), 31));
        assert!(h.seen(peer(1), 40), "first again after the reset");
        // A Hello at now_ms 0 still counts.
        let mut z = HelloBook { at: [0; MAX_PLAYERS] };
        z.seen(peer(3), 0);
        assert!(z.fresh(peer(3), 0));
    }

    #[test]
    fn death_guard_skips_only_native_remotes() {
        assert!(skip_death_attacker(0x7ff0_1234_0000, true));
        assert!(!skip_death_attacker(0x7ff0_1234_0000, false), "an enemy killer stays");
        assert!(!skip_death_attacker(0, false), "no attacker: unchanged");
        assert!(!skip_death_attacker(0, true));
    }

    #[test]
    fn bullet_flag_is_record_byte_0x3b_bit_1() {
        let mut r = vec![0u8; HIT_RECORD];
        assert!(!record_is_bullet(&r));
        r[0x3B] = 0x02;
        assert!(record_is_bullet(&r));
        r[0x3B] = 0xFD;
        assert!(!record_is_bullet(&r), "other bits only");
        assert!(!record_is_bullet(&[]), "short record");
        assert_eq!(hit_kind(true), "bullet");
        assert_eq!(hit_kind(false), "melee");
    }

    #[test]
    fn finders_refuse_only_native_remotes() {
        assert!(finder_refuses(0x7ff0_1234_0000, true), "a friend's copy: no throw / deathblow");
        assert!(!finder_refuses(0x7ff0_1234_0000, false), "an enemy stays a candidate");
        assert!(!finder_refuses(0, true));
        assert!(!finder_refuses(0, false));
    }

    #[test]
    fn receive_note_tells_why_the_mod_drives() {
        assert!(vanilla_receive_note(Some(0), Some(false)).starts_with("NULL: the game's own receive never runs"));
        assert!(vanilla_receive_note(Some(0), None).starts_with("NULL: the game's own receive never runs"));
        assert!(vanilla_receive_note(Some(0), Some(true)).starts_with("NULL although"));
        assert!(vanilla_receive_note(Some(0x1234), Some(true)).starts_with("set"));
        assert!(vanilla_receive_note(Some(0x1234), None).starts_with("set"));
        assert_eq!(vanilla_receive_note(None, Some(true)), "unreadable");
    }

    #[test]
    fn team_and_patch_constants() {
        // 28 = Coop NPC (hit by us, not hostile); 29 (Rival) made the friend a full enemy.
        assert_eq!(PVP_TEAM, 28);
        // 0xB72A80: test rdx,rdx; jz rel32 -> 0xB72BFF (the function's end).
        assert_eq!(&RECV_LOOP_HEAD[..5], &[0x48, 0x85, 0xD2, 0x0F, 0x84]);
        let rel = i32::from_le_bytes(RECV_LOOP_HEAD[5..9].try_into().unwrap());
        assert_eq!(FN_RECV_LOOP as i64 + 3 + 6 + rel as i64, 0xB72BFF);
        // Two distinct finder slots, each holding a distinct function.
        assert_ne!(FINDERS[0].0, FINDERS[1].0);
        assert_ne!(FINDERS[0].1, FINDERS[1].1);
    }

    #[test]
    fn replica_undying_only_while_its_owner_lives_and_streams() {
        assert!(replica_undying(Some(500), Some(80)));
        assert!(!replica_undying(Some(1), Some(80)), "owner at 1 HP");
        assert!(!replica_undying(Some(0), Some(80)), "owner dead");
        assert!(!replica_undying(None, Some(80)), "no snapshot");
        assert!(!replica_undying(Some(500), Some(REPLICA_FRAME_FRESH_MS + 1)), "stale frames");
        assert!(!replica_undying(Some(500), None));
    }

    #[test]
    fn bulk_kind_is_free() {
        for k in [1u8, 5, 6, 7, 8, 20, 21, 22, 23, 30, 41, 42, 43, 44, 45, 46, 50, 60, crate::nsession::BULK_KIND, 0xA6, 0xA7, 0xA8] {
            assert_ne!(BULK_PVP, k);
        }
    }
}
