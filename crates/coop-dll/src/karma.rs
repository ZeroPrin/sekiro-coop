//! Karma (test13): the co-op death penalty, made the way vanilla makes the Bell Demon.
//! docs/IMPLEMENTATION-NOTES.md "test13: Karma", art and texts in docs/KARMA-ASSETS.md.
//!
//! - Your own true death while a friend is linked (`DEATH_COMMITTED` 0 -> 1; a Resurrection
//!   is not a commit, the friend's deaths are not ours) gives one **Karma** goods item, with
//!   the normal "item received" popup, once you are back in the world. At most `MAX_KARMA`.
//! - The inventory count IS the state: it lives in the co-op save like any item. Nothing
//!   else is stored (the old `coop_state.json` stacks are not read any more).
//! - While you hold k > 0 Karma, exactly one of six SpEffect rows (`SP_ROWS[k - 1]`) is kept
//!   on the main player, re-asserted once a second as vanilla's Bell Demon script keeps
//!   SpEffect 8600 while flag 9830 is on (common.emevd). The row carries the stat penalty
//!   (max HP / max posture / outgoing damage rates, `rates`) and the HUD status icon; the
//!   game computes max HP and posture from it, nothing writes HP or posture per frame.
//! - Resting at an idol yourself removes all Karma (the game's own RemoveItemFromPlayer
//!   routine) and the effect. Rests the mod causes (the friend's mirrored rest, the join rest,
//!   a resync) are not yours: world.rs reports each one through `note_mod_rest` before it
//!   writes the idol mailbox, and the next `REST_NOW` at that idol is consumed as mirrored.
//!   Works solo too (the old rule only ran while connected), and an own rest within the old
//!   2 s echo window of the friend's still counts.
//! - Setting `death_debuffs` (the host's value wins while linked): 0 = no new Karma and no
//!   penalty; Karma already held stays in the inventory, inert, until your next own rest.
//!
//! Rows (rewritten in memory once the params exist, at the title before any save loads, and
//! re-checked every 5 s; the param files themselves are never replaced and no lookup is hooked):
//! - Goods `GOODS_ID` = 930150, a debug "skill voucher" (goodsType 7, no name in any FMG, in no
//!   item lot or shop, no code constant; `actionUnlockParamId` 0). Rebuilt from Prayer Bead
//!   4000 (key item with a visible count): maxNum 6, not usable / droppable / storable /
//!   sellable, icon `ITEM_ICON`. Names come from items.rs's GoodsName/Info/Caption detours.
//! - SpEffect `SP_ROWS` = 99005, 99008, 99050, 99400, 99500, 99700: test rows referenced by no
//!   param column, EMEVD, HKS literal, MSB value or code constant (E:\RE\work_karma\rows).
//!   Each is built from the per-byte mode of all rows (every field at its default), the
//!   effect-target bits of vanilla 3361 (a player status effect with an icon, as the old v2
//!   did), then our values. saveCategory -1 (never saved), stateInfo 0 (no remote mirror,
//!   remotefx.rs), every session-request bit (+0x169) cleared: v1 of the old phase 2 copied
//!   row 4 "[System] SOS sign" with requestSOS = 1, and the PlayerIns SpEffect update `0xA30B50`
//!   asks for an SOS sign (`0xA26540`) every frame such an effect is on — the likely cause of
//!   "Inspect on doors does nothing" [ARV, static].
//!
//! Game calls (RVAs 1.06, Ghidra on the 2026-11-11 dump, E:\RE\work_karma):
//! - `0x9E9C00(chr, id, source, 0, 0, &{1,1,1,1}, 1.0, 0, 0, 0)` adds an effect (as before).
//! - `u8 0xBFEFB0(list, id)` tests; `u8 0xBFB6D0(list, id)` unlinks and frees the first node
//!   with the id (EMEVD ClearSpEffect `0x924800` uses it). `0xC00340` is NOT a removal: it
//!   sets node `+0x26/+0x27` (keep one more update).
//! - `0x67B230(_, u32 category, id, n)`: EMEVD RemoveItemFromPlayer (called from the EMEVD
//!   dispatcher `0x904EF0`): finds the entry by `category << 28 | id`, unequips it, consumes
//!   one through `0x79AFA0` -> `0x79AB70(egd, idx, -1, ...)`. It stops after the first unit of a
//!   stack, so we call it once per unit and re-read the count.
//! - `0xA2BE40(player, 100)`: the idol's refill of the player (world.rs uses it too).

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use fromsoftware_shared::FromStatic;
use sekiro::param::{EQUIP_PARAM_GOODS_ST, ParamDef, SP_EFFECT_PARAM_ST};
use sekiro::sprj::SoloParamRepository;

use crate::game::{self, module_base, read, read_bytes, read_ptr};
use crate::{items, log, tuning};

// --- Design numbers ---------------------------------------------------------------------------

/// The Karma goods row (a debug skill voucher in vanilla).
pub const GOODS_ID: u32 = 930_150;
/// Karma held at most (also the row's maxNum).
pub const MAX_KARMA: u32 = 6;
/// SpEffect row for k Karma = `SP_ROWS[k - 1]`.
pub const SP_ROWS: [u32; MAX_KARMA as usize] = [99_005, 99_008, 99_050, 99_400, 99_500, 99_700];
/// Item icon cell repainted by icons.rs (MENU_ItemIcon_00172; vanilla art: a red gourd).
pub const ITEM_ICON: u16 = 172;
/// HUD status icon cell repainted by icons.rs (MENU_StatusIcon_02150; vanilla art: poison
/// resistance down, used by no SpEffect row).
pub const STATUS_ICON: i32 = 2150;
/// Per Karma: max HP and max posture lost, percent.
pub const HP_PCT_PER_KARMA: u32 = 10;
/// Per Karma: damage dealt lost, percent.
pub const DAMAGE_PCT_PER_KARMA: u32 = 2;

/// Factors for `k` Karma: (max HP and max posture, damage dealt). THE place where stacks are
/// combined: additive, 1 - 0.10 k and 1 - 0.02 k (6 Karma: 0.40 and 0.88). For a
/// multiplicative rule return `(0.9f32.powi(k), 0.98f32.powi(k))` here instead.
pub fn rates(k: u32) -> (f32, f32) {
    let k = k.min(MAX_KARMA);
    let keep = |pct: u32| (100u32.saturating_sub(pct * k)) as f32 / 100.0;
    (keep(HP_PCT_PER_KARMA), keep(DAMAGE_PCT_PER_KARMA))
}

// --- Game addresses -----------------------------------------------------------------------------

const DEATH_COMMITTED: usize = 0x3EA0D74;
/// The idol entity id during the frame a rest is processed, -1 otherwise (world.rs, items.rs).
const REST_NOW: usize = 0x3AFEB9C;
const CHR_SP_LIST: usize = 0x11D0;
const APPLY_RVA: usize = 0x9E9C00;
const APPLY_PROLOGUE: [u8; 11] = [0x48, 0x8B, 0xC4, 0x48, 0x89, 0x58, 0x08, 0x48, 0x89, 0x68, 0x10];
const HAS_RVA: usize = 0xBFEFB0;
const HAS_PROLOGUE: [u8; 9] = [0x48, 0x8B, 0x49, 0x08, 0x48, 0x85, 0xC9, 0x74, 0x15];
const REMOVE_RVA: usize = 0xBFB6D0;
const REMOVE_PROLOGUE: [u8; 10] = [0x48, 0x83, 0xEC, 0x28, 0x8B, 0xC2, 0x48, 0x8B, 0x51, 0x08];
const REMOVE_ITEM_RVA: usize = 0x67B230;
// push rbp; push rsi; push rdi; push r14; push r15; sub rsp, 0C0h
const REMOVE_ITEM_PROLOGUE: [u8; 15] = [0x40, 0x55, 0x56, 0x57, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xEC, 0xC0, 0x00, 0x00, 0x00];
const PLAYER_REST_RVA: usize = 0xA2BE40;
const PLAYER_REST_PROLOGUE: [u8; 15] = [0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x6C, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18];
const GOODS: u32 = 0x4000_0000;
/// SoloParamRepository holders (items.rs `solo_param_ready`): 3 EquipParamGoods, 15 SpEffectParam.
const GOODS_HOLDER: usize = 3;
const SP_EFFECT_HOLDER: usize = 15;
/// Prayer Bead: the goods template (key item, visible count).
const GOODS_TEMPLATE: u32 = 4000;
/// Sort position right after the Prayer Bead (2700) in the key items.
const SORT_ID: i32 = 2701;
/// Vanilla Yaksha's Sugar effect: a player status effect with an icon (effect-target donor).
const TARGET_DONOR: u32 = 3361;
const SP_ROW_SIZE: usize = 0x440;
const GOODS_ROW_SIZE: usize = 0xA0;

const TICK: Duration = Duration::from_secs(1);
const ROWS_EVERY: Duration = Duration::from_secs(5);
/// A rest the mod caused at idol X: the next `REST_NOW` = X within this long is that rest.
const MOD_REST_WINDOW: Duration = Duration::from_secs(3);
/// After the clean-up of an own rest: wait this long, then top the HP up to the new max once.
const TOPUP_AFTER: Duration = Duration::from_millis(800);
const VK_F1: i32 = 0x70;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(vk: i32) -> i16;
}

// --- Pure part (unit tested) -------------------------------------------------------------------

/// One of our six SpEffect rows (filtered out of SpEffect-list packets, native.rs).
pub fn is_karma_sp(id: i32) -> bool {
    id > 0 && SP_ROWS.contains(&(id as u32))
}

/// The SpEffect row for `k` Karma (None = no effect).
pub fn row_for(k: u32) -> Option<u32> {
    (1..=MAX_KARMA).contains(&k).then(|| SP_ROWS[k as usize - 1])
}

/// The level whose row should be on the player: the count held (capped), none while the
/// mechanic is off or an own rest is being cleaned up.
pub fn effective_level(held: u32, enabled: bool, clearing: bool) -> u32 {
    if enabled && !clearing { held.min(MAX_KARMA) } else { 0 }
}

/// May one more Karma be granted? `held` in the inventory, `pending` not given yet.
pub fn room_for_one(held: u32, pending: u32) -> bool {
    held + pending < MAX_KARMA
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rest {
    Own,
    /// A rest the mod caused (mirrored / join / resync).
    Mod,
}

/// `REST_NOW` read this frame vs last frame: a rest starts when it changes to an idol id.
/// `token` = (idol, age) of the last rest the mod caused and not yet matched.
pub fn classify_rest(now: i32, last: i32, token: Option<(i32, Duration)>) -> Option<Rest> {
    if now == -1 || now == last {
        return None;
    }
    match token {
        Some((idol, age)) if idol == now && age <= MOD_REST_WINDOW => Some(Rest::Mod),
        _ => Some(Rest::Own),
    }
}

/// SP_EFFECT_PARAM_ST offsets (vendor generated.rs; E:\RE\work_karma\rows\offsets.tsv).
mod f {
    pub const ICON_ID: usize = 0x00;
    pub const EFFECT_ENDURANCE: usize = 0x08;
    pub const MAX_HP_RATE: usize = 0x10;
    pub const MAX_MP_RATE: usize = 0x14;
    /// = max posture (`0xBFD500` -> `0xC04FB0` reads +0x18).
    pub const MAX_STAMINA_RATE: usize = 0x18;
    /// physics / magic / fire / thunder AttackPowerRate: kinds 0, 13, 14, 15 of `0xC04280`.
    pub const ATTACK_POWER_RATES: [usize; 4] = [0x48, 0x4C, 0x50, 0x54];
    /// darkAttackPowerRate: kind 16 of `0xC04280`.
    pub const DARK_ATTACK_POWER_RATE: usize = 0x1EC;
    pub const SP_CATEGORY: usize = 0x146;
    pub const CATEGORY_PRIORITY: usize = 0x148;
    pub const SAVE_CATEGORY: usize = 0x149;
    pub const STATE_INFO: usize = 0x15E;
    /// effectTargetSelf/Friend/Enemy/Player/AI/Live/Ghost/WhiteGhost.
    pub const BITS_167: usize = 0x167;
    /// bit 0 targetBlackGhost, 1 targetAttacker, 2 dispIconNonactive, 6 bGameClearBonus (NG+ factor).
    pub const BITS_168: usize = 0x168;
    /// requestSOS / BlackSOS / ForceJoinBlackSOS / KickSession / LeaveSession / NpcInveda.
    pub const BITS_169: usize = 0x169;
    /// bit 1 bCurrHpIndependeMaxHp.
    pub const BITS_16A: usize = 0x16A;
    /// bit 0 effectTargetPcOnlyTarget.
    pub const BITS_16C: usize = 0x16C;
    /// vowType0..15.
    pub const BITS_16D: usize = 0x16D;
    pub const BITS_16E: usize = 0x16E;
    /// bits 0..2 effectTargetOpposeTarget/FriendlyTarget/SelfTarget.
    pub const BITS_174: usize = 0x174;
    /// bit 3 effectTargetPcDeceased.
    pub const BITS_275: usize = 0x275;
}

fn wr_f32(b: &mut [u8], o: usize, v: f32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// The most common value of every byte over all rows: each field's default.
pub fn mode_row<'a>(rows: impl Iterator<Item = &'a [u8]>, row_size: usize) -> Vec<u8> {
    let mut counts = vec![[0u32; 256]; row_size];
    for r in rows {
        for (c, &b) in counts.iter_mut().zip(r) {
            c[b as usize] += 1;
        }
    }
    // Ties go to the lower byte value (max_by_key keeps the last max, so reverse).
    counts.iter().map(|c| (0..256usize).rev().max_by_key(|&v| c[v]).unwrap() as u8).collect()
}

/// Our SpEffect row for `k` Karma: the default row, the effect-target bits of `donor`, then
/// icon, infinite duration, the rates of `rates(k)`, never saved, no category, no state, no
/// session request, no NG+ factor.
pub fn make_sp_row(template: &[u8], donor: &[u8], k: u32) -> Vec<u8> {
    let mut r = template.to_vec();
    for o in [f::BITS_167, f::BITS_16D, f::BITS_16E] {
        r[o] = donor[o];
    }
    let take = |r: &mut Vec<u8>, o: usize, mask: u8| r[o] = (r[o] & !mask) | (donor[o] & mask);
    take(&mut r, f::BITS_168, 0x07);
    take(&mut r, f::BITS_16C, 0x01);
    take(&mut r, f::BITS_174, 0x07);
    take(&mut r, f::BITS_275, 0x08);
    let (hp, dmg) = rates(k);
    r[f::ICON_ID..f::ICON_ID + 4].copy_from_slice(&STATUS_ICON.to_le_bytes());
    wr_f32(&mut r, f::EFFECT_ENDURANCE, -1.0);
    wr_f32(&mut r, f::MAX_HP_RATE, hp);
    wr_f32(&mut r, f::MAX_STAMINA_RATE, hp);
    wr_f32(&mut r, f::MAX_MP_RATE, 1.0);
    for o in f::ATTACK_POWER_RATES {
        wr_f32(&mut r, o, dmg);
    }
    wr_f32(&mut r, f::DARK_ATTACK_POWER_RATE, dmg);
    r[f::SP_CATEGORY..f::SP_CATEGORY + 2].copy_from_slice(&0u16.to_le_bytes());
    r[f::CATEGORY_PRIORITY] = 0;
    r[f::SAVE_CATEGORY] = -1i8 as u8;
    r[f::STATE_INFO..f::STATE_INFO + 2].copy_from_slice(&0u16.to_le_bytes());
    r[f::BITS_169] = 0;
    r[f::BITS_168] &= !0x40;
    r[f::BITS_16A] &= !0x02;
    r
}

/// The Karma goods row from the Prayer Bead's.
pub fn make_goods_row(template: &EQUIP_PARAM_GOODS_ST) -> EQUIP_PARAM_GOODS_ST {
    let mut r = template.clone();
    r.set_ref_id_default(-1);
    r.set_ref_id_1(-1);
    r.set_ref_category(0);
    r.set_action_unlock_param_id(0);
    r.set_icon_id(ITEM_ICON);
    r.set_sort_id(SORT_ID);
    r.set_max_num(MAX_KARMA as i16);
    r.set_max_repository_num(0);
    r.set_goods_type(1);
    r.set_item_ui_display_type(1);
    r.set_basic_price(0);
    r.set_sell_value(-1);
    r.set_sale_value(-1);
    r.set_consume_hero_point(0);
    r.set_consume_hp_rate(0.0);
    r.set_is_consume(false);
    r.set_is_only_one(false);
    r.set_is_discard(false);
    r.set_is_deposit(false);
    r.set_is_drop(false);
    r.set_is_full_supple_item(false);
    r.set_is_enhance(false);
    r.set_is_bonfire_warp_item(false);
    r.set_auto_replenish_type(0);
    // 0x79AB70 syncs the count of every goods row with the same non-zero id.
    r.set_sync_num_vary_id(0);
    r.set_is_notice_log(true);
    r.set_is_notice_dialog(false);
    r.set_vagrant_item_lot_id(-1);
    r.set_vagrant_bonus_ene_drop_item_lot_id(-1);
    r.set_vagrant_item_ene_drop_item_lot_id(-1);
    r.set_first_get_event_flag_id(-1);
    r.set_use_get_item(0);
    r
}

/// (name, short info, long caption) in English and Russian (docs/KARMA-ASSETS.md "Texts").
pub fn text_strings(russian: bool) -> [String; 3] {
    let (hp, dmg, max) = (HP_PCT_PER_KARMA, DAMAGE_PCT_PER_KARMA, MAX_KARMA);
    if russian {
        [
            "Карма".into(),
            "Бремя смертей в совместной игре. Снижает здоровье, концентрацию и урон. Отдых у идола очищает её.".into(),
            format!(
                "Карма копится, когда вы гибнете,\nсражаясь бок о бок с соратником.\n\nКаждая единица снижает максимальное\nздоровье и концентрацию на {hp}%,\nа наносимый урон на {dmg}% (не больше {max}).\n\nОтдохните у идола Скульптора сами,\nчтобы очистить карму."
            ),
        ]
    } else {
        [
            "Karma".into(),
            "The weight of deaths in co-op. Lowers vitality, posture and damage. Rest at an idol to cleanse it.".into(),
            format!(
                "Karma gathers each time you truly die\nwhile fighting beside a companion.\n\nEach Karma lowers your max vitality and\nposture by {hp}% and the damage you deal\nby {dmg}% (up to {max}).\n\nRest at a Sculptor's Idol yourself\nto cleanse it."
            ),
        ]
    }
}

fn contains_cyrillic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

// --- Texts for items.rs's detours (any thread) --------------------------------------------------

/// 0 = not known yet (English), 1 = English, 2 = Russian (the game's language, from the
/// vanilla name of the template).
static LANG: AtomicU8 = AtomicU8::new(0);
/// The goods row is ours (written); only then does `text` answer for `GOODS_ID`.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Karma held at the last look, for the overlay / logs.
pub static HELD: AtomicU32 = AtomicU32::new(0);

fn wide_texts(russian: bool) -> &'static [Vec<u16>; 3] {
    static EN: OnceLock<[Vec<u16>; 3]> = OnceLock::new();
    static RU: OnceLock<[Vec<u16>; 3]> = OnceLock::new();
    let cell = if russian { &RU } else { &EN };
    cell.get_or_init(|| text_strings(russian).map(|s| s.encode_utf16().chain([0]).collect()))
}

/// GoodsName (0) / GoodsInfo (1) / GoodsCaption (2) of Karma; None for any other id.
pub fn text(id: u32, which: usize) -> Option<*const u16> {
    if id != GOODS_ID || which > 2 || !ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    Some(wide_texts(LANG.load(Ordering::Relaxed) == 2)[which].as_ptr())
}

// --- Mirrored-rest tokens (world.rs) -----------------------------------------------------------

static MOD_REST: Mutex<Option<(i32, Instant)>> = Mutex::new(None);

/// world.rs, right before it writes the idol mailbox for a rest that is not our own
/// (the friend's mirrored rest, the join rest, a resync).
pub fn note_mod_rest(idol: i32) {
    if let Ok(mut t) = MOD_REST.lock() {
        *t = Some((idol, Instant::now()));
    }
}

// --- Runtime -----------------------------------------------------------------------------------

#[derive(Default)]
struct State {
    /// Rows: None = not tried, Some(true) = written, Some(false) = refused (logged).
    rows: Option<bool>,
    rows_checked: Option<Instant>,
    sp_rows: Vec<(u32, Vec<u8>)>,
    goods_row: Option<EQUIP_PARAM_GOODS_ST>,
    calls_ok: Option<bool>,
    last_committed: Option<u32>,
    last_rest: i32,
    /// Karma earned and not given yet (given once back in the world, safe).
    pending: u32,
    /// An own rest: the items go once no menu is open; the effect goes at once.
    clearing: bool,
    /// Karma held when the own rest was seen (for the HP top-up).
    cleared_from: u32,
    topup_at: Option<Instant>,
    next_tick: Option<Instant>,
    /// The row currently kept on the player (0 = none) and the last one logged.
    on: u32,
    logged: Option<(u32, u32, bool)>,
    f1_down: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    let mut g = STATE.lock().ok()?;
    Some(f(g.get_or_insert_with(|| State { last_rest: -1, ..Default::default() })))
}

fn bytes_of<T>(r: &T, n: usize) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts((r as *const T).cast::<u8>(), n) }.to_vec()
}

fn prologue_ok(rva: usize, want: &[u8]) -> bool {
    let mut b = vec![0u8; want.len()];
    read_bytes(module_base() + rva, &mut b) && b == want
}

/// Builds the rows from the loaded params (once). Err = Karma stays off this run.
fn build_rows(st: &mut State) -> Result<(), String> {
    if std::mem::size_of::<SP_EFFECT_PARAM_ST>() != SP_ROW_SIZE || std::mem::size_of::<EQUIP_PARAM_GOODS_ST>() != GOODS_ROW_SIZE {
        return Err("param row sizes do not match the vendor layout".into());
    }
    let repo = unsafe { SoloParamRepository::instance() }.map_err(|_| "no param repository".to_string())?;
    let file = |name: &str| repo.params().find(|p| p.struct_name() == name).map(|p| &*p.data);
    let sp = file(SP_EFFECT_PARAM_ST::NAME).ok_or("no SpEffectParam")?;
    let goods = file(EQUIP_PARAM_GOODS_ST::NAME).ok_or("no EquipParamGoods")?;
    let all: Vec<(u32, Vec<u8>)> = unsafe { sp.rows::<SP_EFFECT_PARAM_ST>() }.map(|(id, r)| (id, bytes_of(r, SP_ROW_SIZE))).collect();
    let template = mode_row(all.iter().map(|(_, b)| b.as_slice()), SP_ROW_SIZE);
    let donor = all.iter().find(|(id, _)| *id == TARGET_DONOR).map_or_else(|| template.clone(), |(_, b)| b.clone());
    let mut rows = Vec::new();
    for (i, &id) in SP_ROWS.iter().enumerate() {
        let Some((_, vanilla)) = all.iter().find(|(r, _)| *r == id) else {
            return Err(format!("SpEffect row {id} does not exist"));
        };
        let want = make_sp_row(&template, &donor, i as u32 + 1);
        let changed = vanilla.iter().zip(&want).filter(|(a, b)| a != b).count();
        log::line(&format!("karma: SpEffect row {id} = level {} ({changed} bytes differ from vanilla)", i + 1));
        rows.push((id, want));
    }
    let used = items::referenced_ids(repo).ok_or("item lots / shops unreadable")?;
    if used.contains(&GOODS_ID) {
        return Err(format!("goods row {GOODS_ID} is given by an item lot or a shop"));
    }
    let row = unsafe { goods.get_row_by_id::<EQUIP_PARAM_GOODS_ST>(GOODS_ID) }.ok_or(format!("goods row {GOODS_ID} does not exist"))?;
    let tpl = unsafe { goods.get_row_by_id::<EQUIP_PARAM_GOODS_ST>(GOODS_TEMPLATE) }.ok_or(format!("goods template {GOODS_TEMPLATE} does not exist"))?;
    log::line(&format!(
        "karma: goods row {GOODS_ID} vanilla type {} max {} icon {} unlock {}; template {GOODS_TEMPLATE} type {} ui {}",
        row.goods_type(),
        row.max_num(),
        row.icon_id(),
        row.action_unlock_param_id(),
        tpl.goods_type(),
        tpl.item_ui_display_type()
    ));
    st.goods_row = Some(make_goods_row(tpl));
    st.sp_rows = rows;
    Ok(())
}

/// Writes our rows where they differ (idempotent).
fn write_rows(st: &State) -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    let mut wrote = Vec::new();
    for p in repo.params_mut() {
        let name = p.struct_name().to_owned();
        let file = &mut *p.data;
        if name == SP_EFFECT_PARAM_ST::NAME {
            for (id, want) in &st.sp_rows {
                let Some(row) = (unsafe { file.get_row_by_id_mut::<SP_EFFECT_PARAM_ST>(*id) }) else { return false };
                let ptr = (row as *mut SP_EFFECT_PARAM_ST).cast::<u8>();
                if unsafe { std::slice::from_raw_parts(ptr, SP_ROW_SIZE) } != want.as_slice() {
                    unsafe { std::ptr::copy_nonoverlapping(want.as_ptr(), ptr, SP_ROW_SIZE) };
                    wrote.push(*id);
                }
            }
        } else if name == EQUIP_PARAM_GOODS_ST::NAME
            && let Some(want) = &st.goods_row
        {
            let Some(row) = (unsafe { file.get_row_by_id_mut::<EQUIP_PARAM_GOODS_ST>(GOODS_ID) }) else { return false };
            if bytes_of(row, GOODS_ROW_SIZE) != bytes_of(want, GOODS_ROW_SIZE) {
                *row = want.clone();
                wrote.push(GOODS_ID);
            }
        }
    }
    if !wrote.is_empty() {
        log::line(&format!("karma: rows written {wrote:?}"));
    }
    true
}

/// Every frame from the title on (lib.rs, after items.rs): the rows as soon as the params
/// exist (before any save is loaded, so the inventory never meets the vanilla maxNum 1),
/// and the game's language once the item names resolve.
pub fn prepare() {
    with(|st| {
        let now = Instant::now();
        if st.rows == Some(false) || st.rows_checked.is_some_and(|t| now.duration_since(t) < ROWS_EVERY) {
            return;
        }
        if !items::solo_param_ready(GOODS_HOLDER) || !items::solo_param_ready(SP_EFFECT_HOLDER) {
            return;
        }
        st.rows_checked = Some(now);
        if st.rows.is_none() {
            match build_rows(st) {
                Ok(()) => st.rows = Some(true),
                Err(e) => {
                    log::line(&format!("karma: OFF, rows refused: {e}"));
                    st.rows = Some(false);
                    return;
                }
            }
        }
        if write_rows(st) {
            ACTIVE.store(true, Ordering::Release);
        }
        if LANG.load(Ordering::Relaxed) == 0
            && let Some(name) = items::goods_name(GOODS_TEMPLATE)
        {
            let ru = contains_cyrillic(&name);
            LANG.store(if ru { 2 } else { 1 }, Ordering::Relaxed);
            log::line(&format!("karma: texts in {} (item {GOODS_TEMPLATE} is '{name}')", if ru { "Russian" } else { "English" }));
        }
    });
}

fn calls_ok(st: &mut State) -> bool {
    *st.calls_ok.get_or_insert_with(|| {
        let checks = [
            (APPLY_RVA, &APPLY_PROLOGUE[..]),
            (HAS_RVA, &HAS_PROLOGUE[..]),
            (REMOVE_RVA, &REMOVE_PROLOGUE[..]),
            (REMOVE_ITEM_RVA, &REMOVE_ITEM_PROLOGUE[..]),
            (PLAYER_REST_RVA, &PLAYER_REST_PROLOGUE[..]),
        ];
        let bad: Vec<String> = checks.iter().filter(|(rva, want)| !prologue_ok(*rva, want)).map(|(rva, _)| format!("{rva:#x}")).collect();
        if !bad.is_empty() {
            log::line(&format!("karma: OFF, unexpected bytes at {bad:?} (game version?)"));
        }
        bad.is_empty()
    })
}

/// Karma in the inventory (None = unreadable).
fn held() -> Option<u32> {
    let inv = crate::audit::read_inventory()?;
    Some(inv.iter().find(|(id, _)| *id == GOODS | GOODS_ID).map_or(0, |(_, n)| *n))
}

type HasFn = extern "C" fn(usize, i32) -> u8;
type RemoveFn = extern "C" fn(usize, i32) -> u8;
type ApplyFn = extern "C" fn(usize, i32, usize, u8, u8, *const [f32; 4], f32, u8, i32, u8);

/// 16-byte aligned like `{1,1,1,1}` at 0x328AE90 (the game loads it with movaps).
#[repr(C, align(16))]
struct Vec4([f32; 4]);
static ONE4: Vec4 = Vec4([1.0; 4]);

/// Keeps exactly the row of level `k` on the player (0 = none). Logs changes.
fn keep_effect(st: &mut State, player: usize, k: u32) {
    let base = module_base();
    let Some(list) = read_ptr(player + CHR_SP_LIST).filter(|&l| l >= 0x10000) else { return };
    let has: HasFn = unsafe { std::mem::transmute(base + HAS_RVA) };
    let remove: RemoveFn = unsafe { std::mem::transmute(base + REMOVE_RVA) };
    let want = row_for(k);
    for id in SP_ROWS {
        if Some(id) != want {
            for _ in 0..4 {
                if has(list, id as i32) == 0 || remove(list, id as i32) == 0 {
                    break;
                }
            }
        }
    }
    let mut present = false;
    if let Some(id) = want {
        present = has(list, id as i32) != 0;
        let alive = game::Chr(player).hp().is_some_and(|hp| hp > 0);
        if !present && alive {
            let apply: ApplyFn = unsafe { std::mem::transmute(base + APPLY_RVA) };
            apply(player, id as i32, player, 0, 0, &ONE4.0, 1.0, 0, 0, 0);
            present = has(list, id as i32) != 0;
        }
    }
    st.on = if present { k } else { 0 };
    let line = (k, want.unwrap_or(0), present);
    if st.logged != Some(line) {
        st.logged = Some(line);
        match want {
            Some(id) => log::line(&format!("karma: level {k}: SpEffect {id} on the player: {present}")),
            None => log::line("karma: level 0: no Karma effect on the player"),
        }
    }
}

/// Removes every Karma from the inventory through the game's RemoveItemFromPlayer, one unit
/// per call. Returns what is left (None = unreadable).
fn remove_all() -> Option<u32> {
    let remove: extern "C" fn(usize, u32, u32, i32) -> u8 = unsafe { std::mem::transmute(module_base() + REMOVE_ITEM_RVA) };
    let start = held()?;
    let mut left = start;
    for _ in 0..(MAX_KARMA + 4) {
        if left == 0 {
            break;
        }
        remove(0, GOODS, GOODS_ID, 1);
        let now = held()?;
        if now >= left {
            log::line(&format!("karma: RemoveItemFromPlayer left the count at {now}, giving up"));
            return Some(now);
        }
        left = now;
    }
    log::line(&format!("karma: removed {start} -> {left} (own rest)"));
    Some(left)
}

/// Every in-world frame (puppet.rs). `enabled`: the setting (the host's while linked);
/// `linked`: a friend is connected; `hands_off`: our warp tears the world down.
pub fn on_frame(enabled: bool, linked: bool, hands_off: bool, in_world_for: Duration) {
    with(|st| tick(st, enabled, linked, hands_off, in_world_for));
}

fn tick(st: &mut State, enabled: bool, linked: bool, hands_off: bool, in_world_for: Duration) {
    if st.rows != Some(true) || !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let base = module_base();
    let now = Instant::now();

    // Own true death while linked -> one more Karma, given once back in the world.
    let committed = read::<u32>(base + DEATH_COMMITTED).unwrap_or(0);
    if let Some(last) = st.last_committed
        && committed == 1
        && last != 1
    {
        let have = HELD.load(Ordering::Relaxed);
        if !enabled || !linked {
            log::line(&format!("karma: died, no Karma ({})", if enabled { "no friend linked" } else { "death_debuffs = 0" }));
        } else if room_for_one(have, st.pending) {
            st.pending += 1;
            log::line(&format!("karma: died in co-op: +1 Karma after the reload (held {have}, pending {})", st.pending));
        } else {
            log::line(&format!("karma: died in co-op, already at {MAX_KARMA} Karma (held {have}, pending {})", st.pending));
        }
    }
    st.last_committed = Some(committed);

    // Dev trigger (debug_hotkeys = 1): F1 gives one Karma, solo too.
    let f1 = tuning::debug_hotkeys() && unsafe { GetAsyncKeyState(VK_F1) } as u16 & 0x8000 != 0;
    if f1 && !st.f1_down {
        let have = HELD.load(Ordering::Relaxed);
        if room_for_one(have, st.pending) {
            st.pending += 1;
            log::line(&format!("karma: F1 (debug_hotkeys): +1 Karma (held {have})"));
        } else {
            log::line(&format!("karma: F1 (debug_hotkeys): already at {MAX_KARMA}"));
        }
    }
    st.f1_down = f1;

    // Own rest (solo too). A rest the mod caused at that idol is consumed as not ours.
    let rest = read::<i32>(base + REST_NOW).unwrap_or(-1);
    let token = MOD_REST.lock().ok().and_then(|t| t.map(|(idol, at)| (idol, now.duration_since(at))));
    match classify_rest(rest, st.last_rest, token) {
        Some(Rest::Mod) => {
            if let Ok(mut t) = MOD_REST.lock() {
                *t = None;
            }
            log::line(&format!("karma: rest at idol {rest} was the mod's (friend / join / resync): Karma stays"));
        }
        Some(Rest::Own) => {
            let have = HELD.load(Ordering::Relaxed);
            if have > 0 || st.pending > 0 {
                log::line(&format!("karma: own rest at idol {rest}: clearing {have} Karma (pending {} dropped)", st.pending));
                st.clearing = true;
                st.cleared_from = have.max(st.cleared_from);
                st.next_tick = None;
            }
            st.pending = 0;
        }
        None => {}
    }
    st.last_rest = rest;

    if hands_off || st.next_tick.is_some_and(|t| now < t) {
        return;
    }
    st.next_tick = Some(now + TICK);
    if !calls_ok(st) {
        return;
    }
    let Some(player) = game::player() else { return };
    let Some(have) = held() else { return };
    HELD.store(have, Ordering::Relaxed);
    let safe = game::safe_for_reward(in_world_for).is_ok();

    if st.clearing && safe {
        match remove_all() {
            Some(0) => {
                st.clearing = false;
                HELD.store(0, Ordering::Relaxed);
                st.topup_at = (st.cleared_from > 0).then(|| now + TOPUP_AFTER);
                st.cleared_from = 0;
            }
            Some(left) => HELD.store(left, Ordering::Relaxed),
            None => {}
        }
    } else if st.pending > 0 && safe && !st.clearing {
        let n = st.pending.min(MAX_KARMA.saturating_sub(have));
        st.pending = 0;
        if n > 0 && items::grant_with_popup(&[GOODS | GOODS_ID, n, u32::MAX, 0]) {
            log::line(&format!("karma: gave {n} Karma (held {have} -> {})", have + n));
        }
    }

    let k = effective_level(HELD.load(Ordering::Relaxed), enabled, st.clearing);
    keep_effect(st, player.0, k);

    // After an own rest's clean-up: the effect is gone, max HP is back up; top the HP up once
    // with the idol's own refill (the rest refilled to the lowered max).
    if let Some(t) = st.topup_at
        && now >= t
        && safe
        && st.on == 0
    {
        st.topup_at = None;
        let (hp, max) = (player.hp(), player.max_hp());
        if let (Some(hp), Some(max)) = (hp, max)
            && hp > 0
            && hp < max
        {
            let rest: extern "C" fn(usize, i32) = unsafe { std::mem::transmute(base + PLAYER_REST_RVA) };
            rest(player.0, 100);
            log::line(&format!("karma: HP topped up after the clean-up ({hp}/{max} -> {:?})", player.hp()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_are_additive_and_capped() {
        assert_eq!(rates(0), (1.0, 1.0));
        let (hp, dmg) = rates(1);
        assert!((hp - 0.9).abs() < 1e-6 && (dmg - 0.98).abs() < 1e-6);
        let (hp, dmg) = rates(6);
        assert!((hp - 0.4).abs() < 1e-6 && (dmg - 0.88).abs() < 1e-6, "6 Karma: 40 % HP/posture, 88 % damage");
        assert_eq!(rates(9), rates(6), "never past the cap");
    }

    #[test]
    fn one_row_per_level() {
        assert_eq!(row_for(0), None);
        assert_eq!(row_for(1), Some(SP_ROWS[0]));
        assert_eq!(row_for(6), Some(SP_ROWS[5]));
        assert_eq!(row_for(7), None);
        let mut ids = SP_ROWS.to_vec();
        ids.dedup();
        assert_eq!(ids.len(), 6);
        assert!(is_karma_sp(SP_ROWS[3] as i32));
        assert!(!is_karma_sp(8600) && !is_karma_sp(-1) && !is_karma_sp(0));
    }

    #[test]
    fn level_follows_the_inventory_unless_off_or_clearing() {
        assert_eq!(effective_level(3, true, false), 3);
        assert_eq!(effective_level(9, true, false), MAX_KARMA, "an odd count is capped");
        assert_eq!(effective_level(3, false, false), 0, "setting off: inert");
        assert_eq!(effective_level(3, true, true), 0, "own rest: the effect goes at once");
        assert_eq!(effective_level(0, true, false), 0);
    }

    #[test]
    fn grants_stop_at_six() {
        assert!(room_for_one(0, 0));
        assert!(room_for_one(5, 0));
        assert!(!room_for_one(6, 0), "death while at 6: nothing");
        assert!(!room_for_one(5, 1), "a pending one counts");
        assert!(room_for_one(4, 1));
    }

    #[test]
    fn own_and_mod_rests() {
        let s = Duration::from_millis;
        assert_eq!(classify_rest(-1, -1, None), None);
        assert_eq!(classify_rest(100, 100, None), None, "the same frame value is no new rest");
        assert_eq!(classify_rest(100, -1, None), Some(Rest::Own), "solo rest");
        assert_eq!(classify_rest(100, -1, Some((100, s(500)))), Some(Rest::Mod), "the friend's mirrored rest");
        // Old bug: an own rest within 2 s of the friend's rest was taken for its echo.
        assert_eq!(classify_rest(200, -1, Some((100, s(500)))), Some(Rest::Own), "own rest at another idol");
        assert_eq!(classify_rest(100, -1, Some((100, s(5000)))), Some(Rest::Own), "a stale token");
    }

    #[test]
    fn sp_row_is_clean() {
        let mut template = vec![0u8; SP_ROW_SIZE];
        template[f::BITS_168] = 0xF8;
        template[f::BITS_169] = 0x04; // requestSOS, the v1 door bug
        template[f::BITS_16A] = 0xFF;
        template[f::SAVE_CATEGORY] = 5;
        template[f::STATE_INFO] = 95;
        template[f::SP_CATEGORY] = 140;
        let mut donor = vec![0u8; SP_ROW_SIZE];
        donor[f::BITS_167] = 0x29;
        donor[f::BITS_168] = 0x05;
        donor[f::BITS_174] = 0xFF;
        donor[0x20] = 0x55; // nothing else of the donor is taken
        let r = make_sp_row(&template, &donor, 6);
        let f32_at = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
        assert_eq!(i32::from_le_bytes(r[0..4].try_into().unwrap()), STATUS_ICON);
        assert_eq!(f32_at(f::EFFECT_ENDURANCE), -1.0);
        assert!((f32_at(f::MAX_HP_RATE) - 0.4).abs() < 1e-6);
        assert!((f32_at(f::MAX_STAMINA_RATE) - 0.4).abs() < 1e-6);
        for o in f::ATTACK_POWER_RATES {
            assert!((f32_at(o) - 0.88).abs() < 1e-6);
        }
        assert!((f32_at(f::DARK_ATTACK_POWER_RATE) - 0.88).abs() < 1e-6);
        assert_eq!(r[f::SAVE_CATEGORY] as i8, -1, "never saved");
        assert_eq!(u16::from_le_bytes([r[f::STATE_INFO], r[f::STATE_INFO + 1]]), 0);
        assert_eq!(u16::from_le_bytes([r[f::SP_CATEGORY], r[f::SP_CATEGORY + 1]]), 0);
        assert_eq!(r[f::BITS_169], 0, "no session request (requestSOS broke Inspect in v1)");
        assert_eq!(r[f::BITS_167], 0x29);
        assert_eq!(r[f::BITS_168], 0xB8 | 0x05, "NG+ factor bit cleared, target bits from the donor");
        assert_eq!(r[f::BITS_174], 0x07);
        assert_eq!(r[f::BITS_16A] & 0x02, 0);
        assert_eq!(r[0x20], 0);
    }

    #[test]
    fn mode_row_takes_the_common_value() {
        let a = [1u8, 2, 3, 4];
        let b = [1u8, 9, 3, 5];
        let c = [7u8, 9, 3, 6];
        assert_eq!(mode_row([&a[..], &b[..], &c[..]].into_iter(), 4), [1, 9, 3, 4]);
    }

    #[test]
    fn goods_row_is_a_counted_key_item() {
        // A Prayer Bead-like template: consumable flag, a sync id, a shop value.
        let mut tpl: EQUIP_PARAM_GOODS_ST = unsafe { std::mem::zeroed() };
        tpl.set_is_consume(true);
        tpl.set_sync_num_vary_id(3);
        tpl.set_sale_value(100);
        tpl.set_max_num(99);
        tpl.set_action_unlock_param_id(7);
        let r = make_goods_row(&tpl);
        assert_eq!(r.max_num(), MAX_KARMA as i16);
        assert_eq!(r.icon_id(), ITEM_ICON);
        assert_eq!((r.goods_type(), r.item_ui_display_type()), (1, 1));
        assert!(!r.is_consume() && !r.is_drop() && !r.is_deposit() && !r.is_discard());
        assert_eq!((r.sale_value(), r.sell_value()), (-1, -1));
        assert_eq!(r.sync_num_vary_id(), 0);
        assert_eq!(r.action_unlock_param_id(), 0);
        assert!(r.is_notice_log(), "the item-received popup");
    }

    #[test]
    fn texts_name_the_numbers() {
        for ru in [false, true] {
            let t = text_strings(ru);
            assert!(t[2].contains("10%") && t[2].contains("2%") && t[2].contains('6'), "{}", t[2]);
        }
        assert_eq!(text_strings(true)[0], "Карма");
        assert!(contains_cyrillic("Чётки") && !contains_cyrillic("Prayer Bead"));
    }

    #[test]
    fn the_karma_row_avoids_the_debug_unlock_rows() {
        // 910000-920150 and 999996-999999 carry "all actions unlocked" sets in vanilla.
        assert!(!(910_000..=920_150).contains(&GOODS_ID) && !(999_996..=999_999).contains(&GOODS_ID));
    }
}
