# share-pickups fork

Unofficial branch of Sekiro Together, based on the newest public source (1.0.9-beta,
commit `e11f68b`; the 1.0.10 / 1.0.11 sources were not published when this was made).
Version string: `1.0.9-pickups.3-test7`. Everybody in a session needs the same build.

> **Test build `1.0.9-pickups.3-test7`: nothing in the sections below marked experimental or
> "not verified live" has been verified in a live game.** `test2` merged the branches
> `base-fixes`, `objact-sync`, `loadout-sync`, `items-unlock-fix`, `pos-rate` and `pvp-native`
> onto `share-pickups`; it had one live test (2026-11-12). `test3` adds the fixes made after it:
> `prosthetic-active` (active prosthetic slot), `zone-flicker` (block-border grace for the
> native remote, `docs/IMPLEMENTATION-NOTES.md`), `objact-doors2` (one-loop doors at mode 1)
> and `pvp-ff` (friendly fire, team 28). `test3` was never played live. `test4` adds `remote-fx`
> (the friend's art slashes / sounds and full-detail parts, `docs/RE-REMOTE-FX.md`) and `wardrobe`.
> `test5` follows the test4 live test (2026-11-12): the `zone-flicker` block-border grace is
> reverted (session engage / remote creation as in test2; its spectate wait hint stays), the
> friend's character updates every frame at any distance (`remote_every_frame`), objact flag /
> fx mirror log spam capped, friendly fire in the Co-op Settings page and the panel,
> `rest_refills_partner`, Cyrillic glyphs in the overlay font. Merged from `fx2`: the forced
> SpEffect mirror (`fx_speffect_mirror`: the art-gate SpEffects 106000 / 140310 / 140510 /
> 140610, pure stateInfo flag carriers, are applied to the friend's replica), the TAE debug
> hook fix (slot 2), real part loads only and the wardrobe combination rules (the Sculptor's
> skin, head and legs are no longer offered). Enemy sync is as in test2 except the death hold
> (`persist_wait_for_down = 1`): when the host dies first and spectates and the client dies
> last, the client keeps the host's old-world HP 0 off its fresh enemies until the host has
> reloaded (plus a 1.5 s tail); log `enemies: death hold on` / `death hold off: N host HP
> writes kept off`. `test6` adds `dead_players_untargetable` (enemies stop noticing a dead player while
> another lives; `docs/RE-CORPSE-TARGET.md`, log `corpse:`) and keeps a boss with the living player while
> the other is down (no ownership see-saw; 10 s fresh-take veto on stealth deathblows only: behind,
> crouching, plunging, wall hug, ledge hang; posture deathblows always pass). Lift / lever fix
> (Senpou temple lift, 2026-11-12): only door / chest ObjActs make fallback door flags or open the
> own-world merge window, a lift script's busy / delay flags (`lifts.rs`, common_func 20005621) are
> never merged into a joiner's own world, ObjAct animations come only from ObjActParam rows placed in
> maps. Review fixes: `corpse:` hysteresis (0.5 s dead / 1 s no state), stray team 0 restored, hide byte
> cleared on warps; the auditor ranks bosses before elites and labels elites; throwprobe logs posture
> and the AI target with a `+0 s` line (budget 536 lines). Known leftover: a solo world whose Senpou lift
> was killed by a test5 session stays dead (not repaired automatically; joining a host whose lift works
> restores it in the co-op world). `test7` adds an observe-only log of how enemies choose between the players (`ai_retarget`, `aitarget:` lines, section below; scores are never changed). The oracle is a playtest with both logs.

## New settings in this build

A fresh `sekiro_coop_settings.ini` is written with these defaults; a key missing from an
existing ini takes the same built-in default. PvP is off unless the host turns it on.

| Setting | Section | Default | Feature (section below) |
|---|---|---|---|
| `share_pickups` | GAMEPLAY | 1 | Shared map pickups (pickups.1-.3) |
| `dead_players_untargetable` | GAMEPLAY | 1 (local per PC) | test6: `docs/RE-CORPSE-TARGET.md` |
| `stealth_deathblow_needs_no_combat` | GAMEPLAY | 0 (experimental) | test6: `docs/RE-CORPSE-TARGET.md` §6 |
| `shop_stock_per_player` | GAMEPLAY | 1 (next start) | Base fixes from 1.0.11 |
| `getup_deathblow_veto` | GAMEPLAY | 1 | Base fixes from 1.0.11 |
| `persist_wait_for_down` | GAMEPLAY | 1 | Base fixes from 1.0.11 |
| `stealth_hang_cover` | GAMEPLAY | 1 | Base fixes from 1.0.11 |
| `log_per_run` | DISPLAY | 1 (next start) | Base fixes from 1.0.11 |
| `send_logs_key` | DISPLAY | 118 (F7; 0 = off) | Base fixes from 1.0.11 |
| `fix_debug_item_unlocks` | DEBUG | 1 | Co-op items unlocked diving |
| `ai_retarget` | DEBUG | 1 (0 off; 1 observe and log, scores never changed) | Enemy target choice, observed |
| `sync_objact` | GAMEPLAY | 1 (0 off; 1 every kind: doors, gates, chests, levers / lift, bells, rotating walls, windows; 2 = 1) | ObjAct sync |
| `objact_debug` | DEBUG | 0 | ObjAct sync |
| `objact_sender` | DEBUG | hook (or poll) | ObjAct sync |
| `loadout_sync` | DEBUG | 1 | Loadout sync |
| `player_packet_hz` | ADVANCED | 30 (12 = the game's own, nothing patched) | Player packet rate |
| `native_pin_delay_ms` | DEBUG | 0 (auto) | Player packet rate |
| `friendly_fire` | GAMEPLAY | **0** (the host's value is used; also in Options > Co-op Settings page 1 and the F-key Mod settings panel, a client sees the host's value locked) | Friendly fire |
| `friendly_fire_in_boss_fights` | GAMEPLAY | 0 | Friendly fire |
| `pvp_dry_run` | DEBUG | 0 | Friendly fire |
| `pvp_death_guard` | DEBUG | 1 (patches `0x66CE5C` even with PvP off) | Friendly fire |
| `wardrobe`, `wardrobe_key`, `wardrobe_pad`, `wardrobe_pad_button`, `wardrobe_reload` | DISPLAY | 1, 113 (F2), 1, none, 0 | Wardrobe |
| `wardrobe_skin`, `wardrobe_head`, `wardrobe_original`, `wardrobe_added`, `wardrobe_owner` | DISPLAY | game, skin, empty, empty, empty (written by the panel; older builds' `wardrobe_legs` / `wardrobe_all_parts` are ignored) | Wardrobe |
| `fx_speffect_mirror` | DEBUG | 1 (0 off, 2 = also enchant states) | Friend's art effects (`docs/RE-REMOTE-FX.md`) |
| `native_full_parts` | DEBUG | 1 | Friend's art effects (`docs/RE-REMOTE-FX.md`) |
| `rest_refills_partner` | GAMEPLAY | 1 (as before) | 0 = the friend's rest respawns enemies and wakes a dead you, but does not refill a living you (HP, gourd, items). NOT verified live |
| `remote_every_frame` | DEBUG | 1 | The friend's character updates every frame at any distance (0 = only while spectating) |
| `fx_debug` | DEBUG | 0 | Friend's art effects (`docs/RE-REMOTE-FX.md`) |

Reliable bulk kinds added by the fork: 46 (pickups), 0xA7 (ObjAct), 0xA8 (loadout), 0xA9
(PvP); `net::tests::bulk_kinds_are_unique` checks them against the upstream kinds. The doctor
compares `friendly_fire` (notice), the me3 packages (notice) and, log only, `sync_objact`,
`loadout_sync`, `player_packet_hz`, `shop_stock_per_player`, `fix_debug_item_unlocks`; an older
build that does not send a key is not compared. PlayerRaw types of the mod: 0xAF63 (artfx),
0xAF64 (remotefx gate SpEffects), 0x4E53xxxx (nsession); `net::tests::player_raw_types_are_unique`.
The full-detail parts hook (`remotefx.rs`, `0xA363BC`) only ever clears the low-detail flag of a
friend's model; the local player's build (wardrobe reload included) passes 0 there and its
model is excluded by address, and a replica re-dressed by the loadout keeps its ChrAsmModel, so
the hook still sees it.

## What it adds: shared map pickups (`[GAMEPLAY] share_pickups`, default 1)

An item one player picks up from the map while connected (treasure, chests, items on
corpses: a one-time map lot with one sure goods slot) is given to the other player too, and
that lot's flag is set on their side, so they cannot pick it up again.

- **Detection** (`keyitems.rs`, `KeyItems::pickups`): the map treasure rows of
  ItemLotParam (`classify_lot` = `MapTreasure`, one sure goods slot, not a co-op item) are
  indexed by lot flag (`LotIndex::map`, `map_by_flag`). Once per load, when the world has
  settled (10 s) and a session is up, every one of those flags is read; after that our own
  flag writes (the SetEventFlag ring in `hook.rs`, bank 0) that switch one of them on are
  our new pickups (`scan_pickups`). Pickups made alone or before the session are never sent:
  the snapshot is taken again after every load and while not connected nothing is watched.
- **Message**: bulk kind 46 (`BULK_PICKUP`), the ledger format with reason 2
  (`LEDGER_PICKUP`), sent to every link (host <-> each client). An older build leaves it
  unread.
- **Receiver** (`apply_pickups`), 2 s later and only when `game::safe_for_reward` allows:
  per row, its lot flag off here -> its sure goods through GrantItems (small pickup log) and
  the flag set; a unique item already owned only gets the flag; flag already on = already
  picked up here, nothing. The flag write we make is not sent back (`pickup_echo`).
- **Not covered**: shops (their flags are ShopLineupParam flags), lottery lots (no sure
  slot), pickups of 3+ player sessions between two clients (links are host <-> client),
  and a pickup in the first ~10 s after a load / warp / death reload (the map flags are
  read when the world has settled; a pickup before that counts as an old one).

## Taking the item off the map (1.0.9-pickups.2)

Live test of pickups.1 (2026-11-09): setting the lot flag does not remove an item that is
already lying on the loaded map; the friend picked the host's Pellet up a second time.
From a memory dump of the running 1.06 exe (layout checked live on 133 loaded treasures):

- MapItemMan (`*(base+0x3D6CDC0)`) keeps map items in linked lists per kind
  (`handle & 0xF`): 1 `+0x20`, 2 `+0x40`, 3 `+0x30` (map treasure), 4 `+0x50`, 5 `+0x68`
  (enemy drops, no flag). Node: `+0x20` handle, `+0x28` lot flag, `+0x30` position,
  `+0x54` item count, `+0x58` items.
- `0x91FCF0 PickUpMapItem(mim, handle, bool)` = the player's pickup: `0x91D450(handle, 1)`
  then GrantItems with the node's items. `0x91D450 MapItemPicked(mim, handle, picked)` sets
  the node's flag (`0x921E40`) and takes the node off the map (`0x91FAD0`): glow and prompt go.

So the receiver now, after giving the friend's pickup, finds the loaded nodes with that
lot flag (the sure slot's flag or the row's) and calls `0x91D450(handle, 1)`: the item
disappears as if picked up, nothing is given twice. And `0x91FCF0` is detoured: a pickup of
a node whose flag the friend's pickup set here (and is on) is refused the same way (node
off the map, no GrantItems). Log lines: `pickup: lot L taken off the map here (handle ...)`,
`pickup: a second pickup refused ...`.

Still open: chests. If a chest is an ObjAct whose item node only spawns when it is opened,
the chest itself stays closed for the receiver (opening it gives nothing). Enemy drops
(kind 5, no lot flag) are not shared.

## Fixes to the 1.0.9 base

- **False wipe** (1.0.9-pickups.3, `persist.rs`). A wipe was "every player's committed death
  within 90 s", even when the first player had already reloaded and was alive again. Live
  2026-11-09: the host died, reloaded and played on for 79 s, the friend then died, and both
  sides cleared the dead set, the paid kills and the boss records (killed enemies and damaged
  minibosses would come back on the next load). Now a death stops counting once that player
  loaded after it and is alive (`death_over`: ours after our death reload, a peer's after its
  last hidden snapshot); checked after the wipe test, so a death that comes in during the other
  player's death reload is still a wipe. Log line: `persist: ... is back after ... death: it no
  longer counts for a wipe`.

## Base fixes ported from 1.0.11

The 1.0.11-beta source is not public. These were rebuilt from its release notes, its ini
comments, the log lines in its DLL and our players' 1.0.11 logs (catalogue:
`docs/FIXES-1.0.11-vs-FORK.md`, Top-10). Branch `base-fixes`. Every item is a unit-tested
rule plus game-side wiring; **none of them is verified live yet**: the oracle is a playtest
with the log lines below.

| Item | Setting (default) | Log lines to look for | Status |
|---|---|---|---|
| Per-run log files, F7 bug-report zip, crash zip (`log.rs`, `logzip.rs`) | `log_per_run` (1, next start), `send_logs_key` (118 = F7, 0 = off) | `log: this run also goes to ...logs\<date_time>.log`, `logzip: wrote N files ...`, `logzip: the previous run crashed / had no exit line: wrote ...` | unit tests (zip layout, CRC, names, pruning, password hidden); not run in game |
| Fresh-life deathblow veto, 10 s (`nsession.rs dblock`, `livesync.rs`) | `getup_deathblow_veto` (1) | `deathblow: enemy E netId N lost a life (seen here / the friend's deathblow): no new deathblow on it here for 10 s ...`, `deathblow: our attempt on N refused: ... (fresh-life veto)`, `fresh-life vetoes` in the 10 s stats | unit tests; uses the existing gate detour `0xBE3810` (live since 1.0.9) |
| Boss lives desync line (`livesync.rs`) | none (log only) | `livesync: DESYNC enemy ... for 15 s ...`, `... lives agree with the other players again after N s` | unit tests |
| Limited shop stock per player (`world.rs shop_counter_bits`) | `shop_stock_per_player` (1, next start) | `world: ... per player (... shop rows, 75 shop counter bits, ...)` | unit test with the real 1.06 rows; width fitted to 1.0.11's own count (75) [ARV] |
| Restores wait for a down peer (`persist.rs`) | `persist_wait_for_down` (1) | `persist: our death reload while peer(s) [..] are down: ...`, `persist: the down peer(s) settled N ms after our death reload: ...` | unit test for the end rule; builds on the false-wipe fix |
| Ledge-hang / wall-hug stealth (`enemies.rs`) | `stealth_hang_cover` (1) | `stealth: our throw state ... (hang/cover): enemies within 12 m run on our AI ...`, `throw: handoff ... reason hang/cover`, `stealth: claiming enemy ... (..., hang/cover, ...)` | unit tests; reads ThrowModule `+0x79` (HKS act 160, RE-BACKSTAB.md) [VAR in IDA, not read live by the fork yet] |
| Co-op items when the temple idol is lit (`items.rs`) | under `connection_items` | `items: the temple idol was just lit: giving the co-op items` | unit test for the edge |
| me3 file mod warning (`me3scan.rs`, `doctor.rs`) | none (log + notice only) | `doctor: me3 profile ...: N package(s) ...`, `doctor: WARNING me3 file mods differ: ...` | unit tests (parser, fingerprint, warnings) |

Notes:

- The per-run file is a second copy of every line; log.txt is unchanged (append-only, moved
  to log.old.txt at 8 MB), so `upload-log.ps1`, testsync and the existing instructions still
  work. Zips and run logs live in `%LOCALAPPDATA%\sekiro-coop\logs\` (20 runs, 10 zips kept).
- The veto refuses every throw of our player on that enemy at the gate, as the peer lock
  does; 1.0.11 used the same gate (its stats line counts both).
- The shop counter width (`eventFlag + 1 ..= eventFlag + floor(log2 sellQuantity)`) is
  inferred, not read in the game's shop code: it is the only simple width that reproduces
  1.0.11's "75 shop counter bits" with the same 1084 other per-player flags.

## Co-op items unlocked diving and every skill (`[DEBUG] fix_debug_item_unlocks`, default 1)

The co-op items sit on EquipParamGoods rows 999997 (Kindred Bell), 999998 (Kindred Charm) and
999999 (Severed Thread); 1.0.11 also uses 999996 (Rival's Charm). In vanilla these rows are
FromSoftware debug items: their `actionUnlockParamId` (+0x28) is their own id, and
ActionUnlockParam 999999 unlocks every action, 999998 all but action03 (Mortal Blade), 999997 all
but 03 and 06, 999996 all but 20-23. The owner of the items could dive without Mibu Breathing
(HKS `env(3033, 0)`), use the Mortal Blade, ninjutsu and the skill actions without having them.

How the game handles it (RE 2026-11-11, dump + Ghidra, details in `docs/RE-NOTES.md`):
EquipInventoryData (EquipGameData+0x168 = PlayerGameData+0x3E0) keeps 30 counters at +0x90, one
per action. `0x7A9A80` adds the item's quantity to every action of its unlock set when it is
added or removed (goods: +0x28, weapons/skills: EquipParamWeapon+0x24C), with the params of that
moment. `0x7A8200` zeroes the counters and adds the whole inventory again; the save loader
(`0x7A8140`, [ARV]) and the debug param reload (`0x10B2AB0`) call it. `0x7A7FE0` = "counter > 0 or bit in
+0x118" answers HKS `env(3033, n)` (case 0xBD9 of `0xB36410`). The mod rewrote our rows only
seconds after the save had loaded, so the counters already held the debug sets and kept them.
In the 2026-11-11 dump the counters matched "inventory x vanilla rows" exactly (action00 = 4: one
from each of 999996-999999, no Mibu Breathing owned).

Fix (`items.rs`): the ids stay (saves, the 1.0.11 Rival's Charm and upstream builds use them).
- +0x28 of rows 999996-999999 (and of the rows this run chose) is set to 0 as soon as
  EquipParamGoods is loaded, at the title before any save load; checked again every second.
  Log: `items: debug unlock rows, vanilla actionUnlockParamId: 999996=999996, ... -> 0 (zeroed,
  before the save load)`.
- After every save load (and whenever a row had to be zeroed again in the world) the game's own
  `0x7A8200` rebuilds the counters once (prologue bytes checked first). Owned items such as Mibu
  Breathing (2420), Mortal Blade (2400) or ninjutsu (2100/2110/2120 -> 2301) count as before;
  only the debug sets drop out. Log: `items: action unlocks rebuilt (debug unlocks removed):
  before [...] after [...]; locked again: [a00 dive (Mibu Breathing), a03 Mortal Blade, ...]`,
  or `items: action unlocks rebuilt, unchanged [...]` once the fix is in place.
- The row writer sets `actionUnlockParamId = 0` explicitly; `items: chosen rows, vanilla
  actionUnlockParamId: ...` lists the vanilla values.
- `fix_debug_item_unlocks = 0` turns the zeroing and the rebuild off (the old behaviour).

Existing saves need nothing: the save holds items, not counters. A player who learned to dive
only through this bug cannot dive any more after the fix until they get Mibu Breathing.

Note for upstream (Tihvert): 1.0.11 has the same bug on 999996-999999. The fix is the two parts
above: zero `actionUnlockParamId` of those rows before the save loads, then call
`0x7A8200(PlayerGameData + 0x278 + 0x168)` once in the world.

## ObjAct sync (experimental, `[GAMEPLAY] sync_objact`, default 1; NOT verified live)

Problem: in 1.0.9 only event flags travel. A door, gate or chest the friend opened stays
closed here, and even where the object was replayed (doors.rs, the game's own type 45) the map
script did not go on: the door templates (common_func 20005616 / 20005615, m11_01 11115620)
and the chest templates (20005510 / 20005511) wait for `ObjActEventFlag(ObjActEntityID)` before
they set the door's flag, disable the ObjAct, deactivate the blocker and rebuild the navmesh,
or enable the chest's treasure. Doors also "close again after an area reload" (upstream issue
#3, open in 1.0.11 too).

### What RE says (Sekiro 1.06, dump 2026-11-11, Ghidra copy `E:\RE\ghidra_objact`) [VAR]

- `0x6BA7B0(block, index, actor)` = the local start: sets `info+0x70/+0x71/+0x72 = 1`, takes the
  value from `state->vt[6]`, builds the request (`0x6BCBC0`) and runs `0x6BC1B0(block, req, 0)`.
  Prologue `48 89 74 24 10 44 89 44 24 18 57 48 83 EC 70` (15 bytes, position independent).
  The closed 1.0.11 detours the same function (`objsync ObjAct start (0x6BA7B0)`).
- `0x6BA910(block, index, actor, value)` = the replay (type 45 receive `0x6BE740`, doors.rs):
  `state->vt[5](value)` (door / one-loop states: `SetEventFlag(flag, value != 0)`; one-state:
  nothing), then `+0x70 = ObjActParam.isEventKickSync`, `+0x71 = 1`, `+0x72 = 0`, no packet 45.
- `ObjActEventFlag(X)` = `EcTestObjActExecuted` (test `0x8FCB90`): true while the list
  `[[0x143D56058]+0x70]` holds `{0, X}`. The list is cleared once per event step (`0x6C76D0`,
  right after the EMEVD update `0x67D180`). Its only writer is `0x6B2AF0(info)`: if `+0x70` and
  `+0x0C != 0`, push `{0, info+0x0C}`, then `+0x70 = 0`. `0x6BC1B0` calls it at its end when
  `eventKickTiming == 1`; the info update `0x6B35D0` when the animation ends and timing is 0.
- Every door and chest row (9999xx) has isEventKickSync 0, eventKickTiming 1: **a replay never
  fires ObjActEventFlag**, only the local start does. The actor only moves / animates that
  character; the kick does not depend on it.
- info fields: `+0x0C` = MSB ObjActEntityID (dump vs `objacts_msb.tsv`: 31 infos of m11_01,
  m11_02, m13_00 all match), `+0x50` = object handle (not an entity id), `+0x78` = ObjActParam id,
  `+0x80` = its row (`+0x1B` isEventKickSync, `+0x25` eventKickTiming, dump vs csv).
  `state->vt[4]` (run on `+0x72`) toggles the flag: `SetEventFlag(flag, !flag)`.

So the receiver replays with `0x6BA910(block, index, -1, value)` and then kicks the EMEVD:
`info+0x70 = 1; 0x6B2AF0(info)`, the same push the opener's own start makes. Calling
`0x6BA7B0` on the receiver would kick too, but it also toggles the door flag (the friend's
flag, already applied here, would go off) and sends packet 45 at stage 4.

### Design (`objact.rs`)

- **Sender** (`[DEBUG] objact_sender = hook | poll`, default hook). `hook`: a detour on
  `0x6BA7B0` (bytes checked at start; on a mismatch a log line and the poll sender). The detour
  only notes the call (block, index, actor, run state and flag before, caller); the frame task
  sends it when the actor is our player's handle (`chr+0x08`) and the info started (run state
  3). `poll`: the 50 ms run-state walk of the first version (fallback; `+0x72` evidence). The
  walk runs in both modes, the receiver needs its facts. What goes out: "What is synced"
  below; a door's sub infos go out once per 2 s.
- **Message v2**: bulk kind `0xA7`, 29 bytes: magic `0x4F`, version 2, reason 1, state class,
  target value, map id, ObjActEntityID, info index, sub index, ObjActParam id, sequence number.
  A version 1 message (first objact build) is refused with a log line: both players need this
  build.
- **Receiver** (game thread, 300 ms after arrival, retried 5 s, at most 32 waiting): the info
  by ObjActEntityID in the loaded block with that map id; by index / sub only when that id is
  not in the block (an index whose own id differs is refused). Then `receive_plan`:
  replay + kick when idle since the load; **kick only** when it already ran here as a replay
  (`+0x72 = 0`, e.g. the game's type 45 at stage 4) and was not kicked; nothing when doors.rs /
  we replayed it in the last 3 s, it was replayed or kicked since the load, it ran here as a
  local start, our own player used it, a door toggling closed
  (a one-loop door's target 0 too), an unknown class. The object check (`doors::object_view`) stays: already at that animation =
  nothing. The kick is skipped when isEventKickSync = 1 (the replay kicked), a kick is pending,
  the info has no ObjActEntityID, `0x6B2AF0` has other bytes, or the event manager is missing.
  doors.rs kicks after its own `0x6BA910` too (only with `sync_objact` on).
- **Journal** (reload persistence): every executed ObjAct of the session, ours and the peers',
  by (map id, ObjActEntityID), at most 256 (oldest dropped), only one-state / door states that
  opened, doors and chests only (a door closing leaves it; one-loop never: a one-loop door
  loads from its flag, a lift must not move on a reload; levers / rotating walls never). Cleared
  on a new session, never written to disk. Once per second, an info of a journaled ObjAct that
  has been loaded and watched for 3 s, has not run since its load and whose object is not at
  that animation is replayed and kicked once per load (`journal:` line). This is the fix for
  doors that close again after a reload.
- **Chests** (ObjActParam 999980 / 999985 / 999990, one-state, kick sync 0, timing 1) take the
  same path: the kick lets 20005510 / 20005511 enable the treasure, so the item node spawns
  here and share_pickups sees it. A chest the friend already looted: its lot flag came with the
  pickup, the `0x91FCF0` refusal of the pickups section still applies.
- Nothing runs in a Remnant memory.

### What is synced, and one-loop doors (2026-11-12, after the first live test)

Live test 2026-11-12: host -> client worked (one-state HUB door 1113712, ObjActParam 999941).
Client -> host failed: the m11_01 gate 1113602 (ObjActParam 999900, flag 61110602) is a
**one-loop** state and was cut off at mode 1 ("a lift or a lever?"). Only its flag reached the
host; the host then used its still closed gate, the toggle switched 61110602 off, that went
back to the client: both ended with the flag off, so the gate loads closed with its ObjAct
disabled (20005616 set 11110602).

Data (`E:\RE\sekiro-data\objacts.md`, every MSB ObjAct): EVERY bar door and gate in Sekiro is
one-loop (stateType 2), sliding / HUB doors and chests are one-state, and the temple lift levers
are one-state too. So the class says nothing; the ObjActParam row does (`doors::kind`, table in
the doors.rs header): 999900-999979 and 101050 = Door, 999980-999999 = Chest, everything else
(levers 205010/205011, bells 2051xx / 252555, rotating walls 9007x, rotating floors 9006x, the
windows 1100000, hidden tatami, -1) = Other. Mode 1 syncs Door and Chest of any class (send,
replay, journal). Since test3 (user decision: "it should just always work") Other syncs live at
mode 1 too; only its persistence stays off ("Every kind live" below).

One-loop doors [VAR, Ghidra `E:\RE\ghidra_doors2`]: `vt[4]` `0x6B64A0` toggles the flag on every
local start, `vt[5]` `0x6B6810` sets it, and the requested animation (`0x6B1DE0` = `vt[8]` +
objAnimId, one-loop `vt[8]` = `0x6B6B40` returns 0) is the same opening animation every time:
there is no closing execution, the flag is a use parity, "open" = flag ON (the load restores
the object from it [ARV]: 20005616 has no `ReproduceObjectAnimation`). Hence:
- target state of a one-loop door is always 1; a target 0 is refused;
- receiver: `0x6BA910(.., 1)` sets the flag to 1 (no toggle) and plays the animation, unless
  the object is already at it (`reached`); flag first or message first, both end at flag ON +
  open, the second is a no-op; doors.rs also replays a friend's ON for a one-loop door flag
  (61xxxxxx, never seen by its map-flag path before) when its door is loaded and closed, and
  counts a one-loop door at its animation as open (not only while it plays);
- sender: our player using a one-loop door whose flag is on already (the case above) toggles
  it off; the `0x6BA7B0` detour notes ON -> OFF of our own player and doors.rs **guard 5**
  (`keep_open`) writes it back on without sending the OFF. A script's OFF (m11_00 11100600
  closes gate 61100610 in phase 8301) is no start of ours: untouched, and guard 2/3 never apply
  to one-loop door flags;
- not journaled: a reload sets the door up from its flag.

Log lines: `doors: our player used one-loop door F whose flag was on already ...: kept on`,
`objact: we used ObjAct ... one-loop door whose flag was on already ...`, `doors: ... door flag F
is on but its door (..., one-loop door, ObjActParam P) was closed here: opened (...); object
anim A playing B, the open execution plays W`. The `doors.rs's replay ...: EMEVD not kicked: no
ObjActEntityID` line (windows, after every load) is said once per object and session.

### Every kind live (test3)

`sync_objact = 1` sends and replays the live executions of every kind with a known state class
(2 is accepted as an alias of 1, 0 stays off). Persistence stays door / chest only: the journal
and doors.rs's "replay from the flag" (`reopens`) never touch levers, bells, walls, windows.
Non-doors are `repeatable` (objact.rs): every message is one event (no "once per load"), the
replay is skipped only while the object plays that animation right now (the game's own
`0x8777F0` = `anim == want && obj+0x128 & 4` [VAR]) or, for a flag state, when the flag already
equals the sender's and the object is at that animation; a type 45 replay a moment ago gets only
the kick, a local start a moment ago (the receiver's own use, a script's `ForceUseObjact(10000)`
on both sides) nothing. Target = the sender's flag after its execution. Guard 5 and the
"always open" rule stay door-only.

| kind (ObjActParam, class) | live | how the world state converges | after a reload |
|---|---|---|---|
| doors, gates (9999xx, 101050) | yes | replay + kick; one-loop flag ON; journal | yes (flag / journal) |
| chests (99998x/5/90) | yes | replay + kick (treasure); lot flags | yes |
| temple lift levers (m20 12000651-654, 205011 / -1, one-state) | yes | the kick drives the receiver's own 20005621; a kick for the other branch is ignored, so one pull makes both lifts agree; 12000656 travels as a flag | both lifts reset to the 20005620 pose (12000655, set only in the invader branch: down). A player who reloads while the other does not has his lift down until the next pull |
| Bell Demon's bell (m20 12000660, -1, one-state) | yes | the kick runs the receiver's 12005200: if 9830 is off it awards item lot 3400 (Bell Demon's Bell) and sets 9830 (the burden) there too; only when the receiver's 12005200 waits already (12000205 = he inspected the bell) | 9830 is a flag; the item only by the kick |
| corridor bell (m20 12000825, 205101, one-state) | yes | kick -> 12005826 (transient event) | nothing persistent |
| m25 bell (252555, one-loop) | yes (script-forced on both: refused as a local start) | its script on each side | flag 62500570 |
| rotating walls (9007x, one-state, no flag, no EMEVD) | yes | replay plays the same turn (the walls are symmetric) | default pose for both: same |
| windows (1100000, one-loop, no ObjActEntityID) | yes | target = sender's flag; flags 61110703 / 705 via the flag sync; 11115614 forces them in phase 8302 | from the flag |
| hidden tatami (m10 1005671, -1, one-loop) | yes | sender's flag | from flag 61000701 |
| rotating floors (m15 9006x, one-loop) | **no** (`never_live`): they carry the user (player anim 7104xx), a replay with no actor would flip a floor under a player standing on it | flags 61500700 / 710 via the flag sync | from the flags |

Lift statics: our replay is of the lever ObjAct only; the platform moves through the receiver's
own script (`ForceAnimationPlayback(2001650, 10/11)`), the same path as his own pull. A player
standing on it rides it; one standing under it is in the situation the vanilla invader branch of
20005621 (`SetNetworkconnectedEventFlag`) creates too.

Log: `not replayed: the object plays that execution's animation right now`, `already in that
state here (...)`, `it ran here a moment ago as a local start ...`, `not sent: a rotating floor
(ObjActParam 9006x) carries its user ...`.

Live check: A opens gate 1113602 (or any bar door, 9999xx one-loop). A: `we used ObjAct ...
one-loop state ... sending as #N (target state 1)`. B: `peer ... here: ... replayed ... EMEVD
kicked` (or `already at that execution's animation` when doors.rs opened it from the flag a
moment earlier), B can pass, `61110602` and `11110602` on for both. B must NOT be able to use
the gate any more; if B does use it (replay late), B's log has `kept on` and neither log shows
`61110602 = false`. Both reload: the gate is open for both.

Log lines (prefix `objact:`):
- `0x6BA7B0 hooked ...` / `NOT hooked ...` at start; `sender: hook | poll ...; journal: N` once.
- `we used ObjAct: map ..., ObjActEntityID E, ObjActParam P, <class> state, flag F before B now
  A, run state R0 -> R1, actor A (our player H), caller sekiro+0x...; <why>: sending as #N
  (target state T) | not sent: <why>; journaled (n in the journal)`, then `sent our ObjAct #N`.
  The first 5 calls per load by another actor are logged too (to catch a wrong handle).
- `peer P ran ObjAct #N: map, ObjActEntityID, index, sub, ObjActParam, class, target; journaled`.
- `peer P's ObjAct #N here: block slot, info index (found by ObjActEntityID | by index), ...;
  replayed with 0x6BA910(...) ..., run state now 3; EMEVD kicked: ObjActEventFlag(E) holds ...`
  or `it ran here already as a replay without its EMEVD kick ...: EMEVD kicked ...` or
  `not replayed: <why>`; then the info `1 s after` / `3 s after` with `info+0x70/+0x71/+0x72`.
- `doors.rs's replay (block ..., info index I): EMEVD kicked ... | EMEVD not kicked: <why>`.
- `journal: our | a peer's ObjAct of this session (...), after its load: replayed ...; EMEVD
  kicked ...` (only when something was done, everything with `objact_debug`).
- With `objact_debug = 1`: every execution seen by the walk, every map flag write, raw info
  dumps, the first bytes of `0x6BC1B0` / `0x6BA7B0` / `0x6BA910` / `0x6B2AF0`.

Verified by RE (dump, not live): entry points and arguments, prologue bytes, the
ObjActEventFlag mechanism and its writer, the info and param offsets, the flag toggle. Not
verified live: that the kick from the frame task is seen by the next EMEVD step (ordering
[ARV]: our push lands outside the `0x811C90` event step, so the next step evaluates it before
its clear), that `chr+0x08` of our player equals the actor `0x6BA7B0` gets (dump: the player's
`chr+0x08` is `0x10050000`, a chr-type handle), the journal timing after a reload, chests
spawning their item node here.

Live test protocol (both players on this build, `objact_debug = 1`, `native_session_stage = 3`):
1. Start: each log has `objact: 0x6BA7B0 hooked` and `sender: hook`.
2. Same area, a one-state door (m11_01 sliding door 1113713 / ObjActParam 999970, or any
   9999xx door). A opens it. A's log: `we used ObjAct ... sending as #N`. B's log: `peer ...
   here: ... found by ObjActEntityID ... replayed ... EMEVD kicked`. B walks through the door
   (the blocker must be gone), and the door's script flag is set on B (`flag ... = true` lines).
3. A chest (999980): A opens it, B sees it open; the item node: B's pickup is refused or
   shared as share_pickups says.
4. B reloads the area (rest at an idol or quit to menu and back): `journal: ... replayed ...`
   or nothing because the door loaded open; B can still pass.
5. Repeat 2 with `native_session_stage = 4`: B should log `it ran here already as a replay
   without its EMEVD kick ... EMEVD kicked` (type 45 first) and pass.
6. If nothing is sent: look for `not sent: the actor is not our player` (handle) and retry with
   `objact_sender = poll`.

Open: keyed doors open on the receiver without the key (flags are shared anyway); ringing the
Bell Demon's bell gives the receiver the burden too (below); rotating floors are not live.

## Loadout sync (experimental, `[DEBUG] loadout_sync`, default 1; not tested live yet)

**Bug.** The friend's native replica (`native.rs`, a PlayerIns with its own PlayerGameData)
never got the friend's equipment. `create` fills the replica's PGD from the first existing
CharaInitParam row (the dev test's row 1080100 does not exist in retail), and the deferred
dress called `ghost::dress_like(remote, player)` with the LOCAL player: the observer's 25
ChrAsm slots (`ChrIns+0x2140`, ids at `+0x24 + 4*slot`, set through `0x9CCBA0`), once.
Nothing about equipment went over the network (the snapshot has pose, animation and HP
only). Seen live: the replica wore the observer's prosthetics, and every combat art of the
friend played as Whirlwind Slash (the template's default art).

- **Ids, not handles** (RE of a live dump, 2026-11-11, `E:/RE/notes-2026-11-11.md`). Layout 1
  sent the ChrAsm slots (`ChrIns+0x2140`, `+0x24 + 4*slot`): those are gaitem HANDLES
  (`0x8080xxxx`, `0x907Fxxxx`, identical to `PGD+0x310` ChrAsm), and the getter `0x9CBE40`
  resolves them through the LOCAL player's inventory, so on another machine they mean nothing.
  The real ids are in EquipGameData (`PGD+0x278`): `+0x374` i32[25] by ChrAsm slot
  (`PGD+0x5EC`), category bits included. Dump values: slot 0 = 70100 (Shuriken), **slot 1 =
  5200 (Nightjar Slash): the right "weapon" is the equipped combat art** (5100 = Whirlwind
  Slash, the template's), 2 = 73000 (Axe), 4 = 72000 (Flame Vent), 3/5 = 110000 (bare hands),
  12..15 = protectors (`0x1000_0000 | id`), the rest -1. That is why the friend's arts played
  as Whirlwind Slash: the replica's PGD kept the template's slot 1.
- **Message**: bulk kind 0xA8 (`loadout::BULK_LOADOUT`; reliable, to every peer; the host
  relays). Body (`coop_net::encode_loadout`): magic `LD`, **layout version 2**, the sender's
  25 EquipGameData ids (GameDataMan+8 -> PGD -> `+0x5EC`), `art` and `extra[3]` (`art`
  reserved, -1, the art is slot 1; `extra[0]` = the active prosthetic, next section). Sent when our ids change and every 3 s as a refresh (late join, lost
  packets; only while no bulk chunk is unacked). A layout-1 body (the previous build) is
  ignored and logged once per peer.
- **Receiver**: the last loadout per peer slot (`loadout::of`). When the deferred dress is due
  (`native_dress_delay_ms`), and again whenever a newer loadout differs, `loadout::apply`
  replays on the replica's OWN PlayerGameData (`Remote.pgd`) what the game does when it fills
  a PGD from a CharaInitParam row (`0x7B7470`, the same call `create` uses on that PGD) and
  the equip menu (`0xD592F0`), for each slot whose id differs from the PGD's `+0x5EC`:
  - item already in that PGD's inventory (`0x79D7E0(egd, &id)` >= 0): its entry's handle
    (`0xC3C9B0` / `0xC3CB00`, the menu's way), then equip;
  - else a new gaitem handle `0xC37740(GaitemMan 0x143D888A8, &h, &id)`, added to the PGD's
    inventory `0x79A710(egd, id & 0xF0000000, &h, 1, 1, 0, 1)` (count 1, no "new" mark, no
    auto-equip; fill_a passes 0 for the last one and lets `0x7A12F0` auto-equip), then equip,
    then `0xC396C0(&h)` drops our reference as fill_a does;
  - equip = `0x79F8A0(egd, slot, &h, inv_idx, 0, 1, 0)` (fill_a's arguments): it builds the
    functor `0x7A46B0` -> `0x9CCBA0` on the PGD's ChrAsm (`egd+0x98`) and `0x7A52B0` writes
    `egd+0x24[slot]` = inventory index and `egd+0x374[slot]` = id.
  Then `loadout::sync_chr` copies the PGD's ChrAsm into the PlayerIns holders `+0x2140` and
  `+0x2198` with `0x9CCEE0(holder, pgd)` (what the PlayerIns ctor `0xA217A0` does, and the
  equip menu for a weapon swap), and the parts reload through PlayerIns vtable slot 21
  (`0xA30A50`, only when the slot holds exactly that function and the replica is in the player
  chr set). The menu's own follow-up `0xA2E1B0(chr)` is NOT called: it sends OUR equipment
  over the game's session (`PlayerIns+0x2038`), not a refresh.
- **Validation**: network ids reach a game call only through `loadout::plan`: slots 0..11 take
  a weapon id (category 0) that is a row of EquipParamWeapon, 12..16 a protector id (category
  1) that is a row of EquipParamProtector (`SoloParamRepository`, `find_index`). Anything else
  is skipped with a log line: -1 (unequip is not replayed), accessories / slots 17..24 (fill_a
  adds them through `0x79BF90`, not replayed), a wrong category, a missing row. Unit tests:
  `loadout::tests`, `protocol::tests::loadout_round_trip_and_rejects`.
- **Fallback**: no loadout yet (the friend on an older build, or `loadout_sync = 0`), and N0
  clones: `ghost::dress_like` as before, OUR handles into the replica's holder. Kept on
  purpose: without it the template replica has no visible body (ghost.rs). The first loadout
  then re-syncs the holders from the PGD (even when no slot differed), replacing our handles.
- **Log lines**: `loadout: sent ids [...]` (ours, on change), `loadout: peer N ids [...]`
  (theirs, on change), `loadout: peer slot N slot S a -> b applied (ret inventory I, added /
  in inventory; slot now Some(b))`, `loadout: peer slot N slot S id X skipped (NoRow /
  Empty / ...)`, `loadout: peer slot N K slot(s) applied, H ChrAsm holder(s) of 0x... synced
  from its PGD`, `loadout: refresh of 0x...: parts reloaded (vtable slot 21)` or `... skipped:
  ...`, `native: new loadout from peer slot N, re-dressing ...`, and the fallback `native: no
  loadout from peer slot N yet, ... wears ours for now`.
- **Verified vs not.** From the dump (static, Ghidra): the id array and its values, the
  handle nature of the ChrAsm slots, fill_a's call sequence and arguments, the menu path, the
  PlayerIns ctor's holder copy, that `0x79A710` / `0x79F8A0` work on the EGD they are given
  (inventory `egd+0x168`, ChrAsm `egd+0x98`) and that their side effects on the LOCAL player
  (`0xA2AA00` item-count bookkeeping, `0x7A1B60`) only fire for goods ids
  (`0x400001F4`, `0x4000044C/D`, `0x40000456`), never for weapons / protectors. NOT tested
  live: that the replica's model and moveset follow (the combat art in particular may also
  need the hand/style refresh `0xA26150` the menu calls for weapon swaps, not done), that the
  gaitem manager's new handles behave on a remote PGD over a long session, and the crash risk
  of the vtable-21 reload on a live remote (`loadout_sync = 0` turns all of it off).
- **Live test**: both on this build. The friend equips a combat art and prosthetics the
  observer does not have before the session and changes the art during it. The observer's log
  must show `loadout: peer N ids` matching the friend's `loadout: sent ids`, `slot 1 ... ->
  <art> applied`, the holder sync and the reload; the friend's art must play as itself on the
  replica, the friend's prosthetics must be visible.

## Active prosthetic (part of loadout sync, `[DEBUG] loadout_sync`; NOT verified live)

**Bug (live test 2026-11-12).** The replica always showed the tool of the FIRST prosthetic
slot. On "switch prosthetic tool" the other side saw the switch animation and sparks, but the
tool did not change and the switch sound was missing (both directions). The loadout carried
the 25 ids only; the game sends no equipment packet in our session (only types 1 and 24).

### What RE says (static, Ghidra copy `E:\RE\ghidra_prost`, dump 2026-11-11) [VAR]

- **ChrAsm** (class vtable `0x2A1EDD0`; `PGD+0x310` and the PlayerIns holder `chr+0x2140`):
  `+0x08` arm style, `+0x0C + 4*hand` the ACTIVE slot index of that hand (i32; getter
  `0xBF1480(asm+8, hand)`, setter `0xBF1640(asm+8, hand, idx)`; copy `0x9CC9A0` and
  save/load `0x9CC790` / `0x9CC8A0` carry `+0x08..+0x20`), `+0x24` the 25 handles. Hand 0 =
  left = the prosthetic: active ChrAsm slot `2 * idx` (`0x9CB210`, `0xE72220`), idx 0..=2.
  Dump: local `PGD+0x318` = 1 (arm style), **`PGD+0x31C` = 1 = slot 2 = 73000 (Axe)**,
  `PGD+0x320` = 0; the local holder `chr+0x2140` has the same values.
- **The game's switch**: `0xB57E30` (handler with `*(arg+8)` = hand) computes
  `(PlayerIns vtable 0x420 (= holder+0xC+4*hand) + 1) % 3`, skips a slot holding 110000 (bare
  hands), and calls **`0xA26150(chr, hand, idx)`**: sets the holder index, arm style, `0x9F6780`
  / `0x9F6460`, `0xA284D0` (left) / `0xA28570` (right), copies holder -> PGD (`0x9CCDB0`) and
  sets `chr+0x2060` (equipment dirty). `0xB57E30` returns early unless the chr is the local
  player (`WorldChrMan+0x88`) or PlayerIns vtable `0x1B0` holds; `0xA284D0` also acts only
  for the local player. The equip menu (`0xD592F0`, `0xD5A480`) calls `0xA26150` too.
- **Why the replica stuck at slot 0**: every PlayerIns update (`0xA37560`) copies the PGD's
  ChrAsm INTO the holder (`0x9CC9A0(holder, pgd+0x310)`) when the chr-type table
  `0x143B0A3B0[type*0x14+1]` is 0: true for type 0 (local) and type 1 (our replicas). If the
  left/right hand weapon changed by that copy it calls `0x7E3C60(.., 1)` and `0xA23AF0(egd)`,
  then updates `chr+0x120` and calls `0x9F6780` / `0x9F6460` (the same reactions the local
  switch gets). So the replica's PGD index (template, 0) is the master; writing a holder alone
  would be overwritten the next frame.

### What the mod does (`loadout.rs`)

- **Sender**: `extra[0]` (`coop_net::LOADOUT_EXTRA_ACTIVE_PROSTHETIC`) = local `PGD+0x31C`.
  A change makes the loadout differ, so it goes out at once (not on the 3 s cadence). No
  layout change: older builds ignore `extra[0]`, and -1 from an older sender keeps the old
  behaviour.
- **Runtime check of the offsets** (sender): the ChrAsm vtable at `PGD+0x310`, arm style
  0..=3, index 0..=2, and the local holder `chr+0x2140 +0xC` equal to the PGD's. Failing 180
  ticks in a row disables only this feature (-1 sent, one log line).
- **Receiver** (`loadout::follow_active`, from `native.rs` every dress tick and in the dress
  itself before `sync_chr`): validates (`check_active`: 0..=2, the sender's slot `2*idx` holds a
  weapon id that is not 110000, the replica's PGD already holds that id), checks the replica's
  ChrAsm (vtable, ranges; failing disables this side once), writes the replica's `PGD+0x31C`
  only when it differs. No game function is called and no parts reload is done mid-switch: the
  game's update copies it and reacts. 400 ms later the holder must hold the index; if not, the
  old refresh runs (`sync_chr` + vtable-21 parts reload) and says so.
- **Sound** [ARV]: the switch sound is most likely part of the local path (`0xA284D0`, local
  only) or of the TAE keyed on the new tool; the missing sound is probably the same missing
  state (the replica's tool never changed). This fix may restore it; not verified.
- **Log lines**: `loadout: sent active prosthetic I (ChrAsm slot S, id X)`, `loadout: peer
  slot N active prosthetic a -> b applied on 0x... (ChrAsm slot S, id X)`, `loadout: peer slot
  N active prosthetic b: holder of 0x... followed (game swap)` or `... did not follow (...),
  fallback sync ..., parts reload ...`, `loadout: peer slot N active prosthetic I skipped
  (Unknown / OutOfRange / NotATool / NotDressed), replica keeps J` (once per reason), and
  `loadout: active prosthetic DISABLED ...` (check failed). Unit tests: `loadout::tests`
  (`check_active`, `prosthetic_slot`, `asm_sanity`, round trip),
  `protocol::tests::loadout_active_prosthetic_in_extra0`.
- **Live test**: both on this build, each with 3 different tools. Each switches several times.
  Owner's log: one `sent active prosthetic` per switch. Observer's log: `applied` then
  `followed (game swap)`; the replica must show the new tool (and the sound, if it comes back).
  A `did not follow` means the update copy does not run for the replica (the fallback then
  reloads parts). `NotDressed` lasting more than a moment means the ids did not apply.

## Wardrobe (`[DISPLAY] wardrobe`, default 1; NOT verified live)

Mix outfit parts on your own character while the world stays visible (`wardrobe.rs`). F2
(`wardrobe_key`) opens a small panel on the left: Skin / Head, Up/Down = row,
Left/Right = change, applied 200 ms after the last press (Enter = at once), Esc or F2 =
close. The panel closes by itself when a loading screen starts (`game::load_pending` in the
wardrobe tick, and the world reset of a load) and does not open again until the load is
over: left open it kept holding the keyboard (`ui.rs` `message_filter`) through the load,
and F7 / F11 did not get through (live 2026-11-13). Controller: right stick + right trigger while the panel is open (`wardrobe_pad`), an
open button only if `wardrobe_pad_button` names one. Nothing is read or written until a
choice other than "As in game" is made. The native Co-op Settings page has no rows for it:
the page shows only its first 10 rows (`optmenu_model::PAGE_ROWS`).

- **Choices.** Skin (torso + prosthetic arm + legs, and the default head): As in game, Wolf,
  Shura, Ashina, Tengu. Head: As skin, Wolf, Shura, Ashina, Tengu, the same list with every
  skin and no locks (user decision 2026-11-13); Shura's head brings its hair 304000 in
  protector slot 16, any other head empties that slot. Legs are not a choice: they are always
  the skin's own (103000 / 303000 / 403000 / 503000; user decision 2026-11-13 after live
  testing, mixed legs never matched anything), so the leg / torso texture pairs (Tengu's
  torso needs Tengu's legs, Shura's legs need Shura's torso) always come together. A
  `wardrobe_legs` line from an older build is not read: the skin's legs replace whatever legs
  are worn on the next apply. Known cosmetic caveat, accepted: Shura's head (`fc_m_0240`) on
  a non-Shura skin has no fire texture (`BD_M_9110_Fire_B_*` lives in Shura's torso
  `bd_m_9110`, `docs/RE-REMOTE-FX.md`). The Sculptor's skin and head are not offered (test5,
  user decision: the body is not rigged right, the head barely differs); `sculptor` in the
  ini acts as game / skin, and "As in game" still removes the added rows 200000-203000 (and
  153000, the tabi legs of older builds). The torso keeps the
  sword state worn: x01000 = Mortal Blade, x31000 = sword only, 141000 = no sword (Wolf only;
  the other skins use x31000); Sculptor 201000 has one torso. A bare arm (112000, before the
  prosthetic) or a missing arm (122000) is never touched.
- **How the game changes armour (static RE, `E:\RE\ghidra_wardrobe`) [VAR].** EMEVD 2003[83]
  `ForcePlayerArmorEquip` = case 0x53 of `0x904EF0`: on the local EGD (`PGD+0x278`) find the
  row in the inventory (`0x79D7E0`, no add), its entry's handle (`0xC3C9B0` / `0xC3CB00`),
  `0x79F8A0(egd, 12 + type, &h, idx, 0, 1, 0)`; Body 101000 keeps the costume family
  (`cur / 100000 * 100000 + 1000`). New Game+ (`0xA8D130`, ids 100000 141000 112000 103000)
  and the reflection-mode Mortal Blade (`0xA917B0` adds with `0x79A710`, `0xA919C0` removes
  with `0x79ECA0`) do the same. The wardrobe makes exactly these calls (adding a missing row
  first, as `0xA917B0`), then copies the PGD into the holder (`0x9CCEE0`, as the equip menu
  `0xD592F0` does for the local player). None of the game paths reloads parts: the per-frame
  update reacts to weapon slots only, the loader `0xA361C0` runs from PlayerIns vtable slots
  8/21/27/33/35. So one frame later the wardrobe calls slot 21 (`ghost::reload_parts`, used on
  replicas in the 2026-11-12 test without trouble; never on the local player before) [ARV].
  Default `wardrobe_reload = 0` leaves it out (the look may then change only at the next load;
  review 2026-11-12: 0 = exactly the game's own EMEVD path, the safer default until slot 21 on
  the local player is seen working live). The
  "Change Form" menu itself was not found; slot 16 for the hair is inferred from `0x79E230`
  reading protector kinds 0..4 [ARV].
- **Save and inventory.** Worn protectors are part of EquipGameData, which the save keeps
  (NG+ and EMEVD rely on it): the wardrobe applies only on a change, and after a load the worn
  ids already match. Rows the player does not own (Sculptor's set, 153000, costumes before
  their Memory) are added to the inventory (count 1, no "new" mark) and listed in
  `wardrobe_added`; protectors have no menu page in Sekiro, so they should not show anywhere
  [Г]. Skin "As in game" re-equips `wardrobe_original` (the 5 rows before the first change,
  brought to the story state of now: `restore_target` keeps a Mortal Blade / prosthetic the
  story gave meanwhile) and removes the added rows that are not worn and whose item-lot flag
  (50003400.., the game's award) is not set. The ini is per player, not per save: a second
  save gets the same skin, but the records belong to one character (`wardrobe_owner` = save
  slot + total sen earned, ownworld's fingerprint): another slot / a new character in the slot
  is left alone (`wardrobe: ... belong to save slot N`) until that character restores or the
  three keys are cleared.
- **Not fighting the game.** If only the torso / arm change under us (story: Mortal Blade,
  prosthetic) the skin is put on again with the new sword state. If the head, legs or slot 16
  change too (the game's Change Form, NG+) the wardrobe goes back to "As in game" and says so.
  Both are handled only once the blockers below clear (a Change Form menu / talk may change the
  parts over several frames; nothing is removed from the inventory under an open menu).
- **Safety.** First use byte-checks the 10 called functions (one `wardrobe: DISABLED` line on a
  mismatch); every row must be in EquipParamProtector. Nothing is applied on a load / less than
  5 s after it (gpuguard: the driver crash fired most after loads), dead, remnant, in a menu,
  talk, cutscene or a throw / deathblow (`enemies::throw_active`), and not twice within 400 ms.
  While the panel is open the keyboard's window messages do not reach the game
  (`MessageFilter::InputKeyboard`; if the game reads the keyboard through DirectInput it still
  sees the arrows [Г]) and the stuck watchdog ignores Esc. The parts reload runs outside the
  wardrobe lock; `panel_open` (window thread) is an atomic.
- **The friend.** Slots 12..16 travel in the loadout (`loadout.rs`), sent the frame they change.
  The replica side validates them (category 1, an EquipParamProtector row) and now also replays
  emptying slot 16 (`loadout::SLOT_EMPTIABLE`, the game's own way for slots 17..21), else the
  replica kept Shura's hair. Test `loadout::tests::wardrobe_mix_and_the_hair_slot`.
- **Log lines.** `wardrobe: panel opened / closed`, `wardrobe: panel closed (loading / world
  reset)`, `wardrobe: choice changed: skin S, head H`, `wardrobe: original outfit remembered [..]`, `wardrobe: slot N a -> b (from
  inventory / added to inventory / emptied)`, `wardrobe: applied ... -> ids [..] (worn now
  ..)`, `wardrobe: parts reload Ok(()) (holder follows the PGD: true)`, `wardrobe: change
  skipped for now (REASON)`, `wardrobe: the game changed ...`, `wardrobe: original outfit [..]
  restored`, `wardrobe: R removed from the inventory`. Friend: `loadout: peer N ids`, `slot 16
  ... -> emptied`.
- **Live test.** Solo first, default `wardrobe_reload = 0`: F2, Skin -> Tengu, wait: the log
  shows the slots and `parts reload off`; does the body change on screen without a load? If
  not, `wardrobe_reload = 1`, change again: `parts reload Ok`, the body changes (settles whether
  slot 21 is needed and safe on the local player). Then flick Left/Right fast: one apply per
  pause. Then "As in game": the old outfit and `restored`. In co-op the
  friend's replica must change within a second or two (`loadout: peer N ids`).
- Unit tests: `wardrobe::tests` (torso variants, legs follow the skin, every head with every
  skin, older builds' worn legs replaced, skin / head cycling, ini round trip, external
  changes, award flags, restore target, records owner).

## Player packet rate (experimental)

`[ADVANCED] player_packet_hz` (12..60, default 30; 12 = the game's own rate, nothing patched).
The friend's native remote is driven by the game's own player frame, packet type 1 (position,
rotation, HP, sync states, ~76 B). The game sends it every 5 frames (12/s at 60 fps, live log
`type 1: 120 (12.0/s, 9120 B)`), so the remote showed the friend 130-200 ms late at ~100 ms ping.

Found in Ghidra on the 1.06 dump of 2026-11-11 [VAR]:

- Sender `0x7EEB50` (PadManipulator slot 11): countdown `pad+0x360`, minus 1 per frame; at < 1
  (or with the force byte `chr+0x2201`) it builds the frame and sends it (`0x8DA080` →
  `0x8D7000`, session slot 1, type 1), then re-arms with `0x96F8A0(5)` (`mov ecx, 5` at
  `0x7EEF47`, `call` at `0x7EEF4C`).
- `0x96F8A0` reads the debug config key `ChrSync.FrameSkipCount` (UTF-16 at `0x2A110C0`) with
  that default. Its only caller is that call, so nothing else (no other packet type) uses it.
  The count is in frames, not time: 24/s at 120 fps.
- Receiver `0x7EC250` (NetworkManipulator slot 9) measures the time since the previous packet
  itself (`+0x3CC`, reset per packet) and `0x7EC9E0` divides by 1.2 × that. No fixed 1/12 s
  anywhere on the receive side; two packets in one frame snap (interval 0), as before.

What the mod does (`packetrate.rs`, `native.rs`):

- Start-up (setting above 12): checks the call site bytes and the prologue of `0x96F8A0`, then
  redirects `0x96F8A0` to a function that returns the frame count for the setting at the
  measured frame time (2 at 60 fps for 30 Hz, 4 at 120 fps), never more than the game's 5. Log:
  `native: player packet rate 12 -> 30 Hz (interval 33 ms; every 2 frames ...)`, and a line when
  the frame count changes. A change of the setting applies live; from 12 it needs a restart.
- Each side changes only its own sending. Use the same value on all machines, not above the
  other players' frame rate (the receiver plays out at most one frame per frame).
- Receiver, only while the friend's measured interval is under 60 ms (so nothing changes
  against a 12/s sender): the play-out may release half a frame early (the cadence is a whole
  number of the sender's frames), the type-1 queue keeps ~200 ms (6 frames at 30/s), and the
  position pin's delay (`native_pin_delay_ms`, [DEBUG], 0 = auto) follows the measured rate:
  play-out wait + 1.2 × interval − one frame, 60..130 ms (about 70 ms at 30/s). A fixed 130 ms
  at a faster rate made the remote read walking as dashing (2026-10-23, IMPLEMENTATION-NOTES § 7).
  The formula is the game's own 1.2 factor, not tuned live yet.
- `native stats` shows `our type 1 every N frames at F fps` and, per inbox, `play-out` and
  `pin delay`.

Bandwidth per direction and per receiving player, 76 B + 19 B PlayerRaw header + 28 B UDP/IP:
12/s ≈ 1.5 KB/s, 30/s ≈ 3.7 KB/s, 60/s ≈ 7.4 KB/s. The 60/s snapshots (~same size) were already
there.

Live checks: the sender's tee stats `type 1: ~300 (30.0/s ...)`; the receiver's `frame interval
33 ms`, `dropped old` near 0, `pin delay` ~70 ms; by eye: walk / run / dash look right, the friend
is closer to their real position. If walking reads as running, set `native_pin_delay_ms` by hand
(try 90-130) and report the value.

## Friendly fire on the native hit packet (experimental, `[GAMEPLAY] friendly_fire`, default 0)

Friendly fire, not duels: the friend stays an ally (no lock-on, no enemy HP bar, Sculptor's
Idols usable next to them, no deathblows or throws on each other), but careless hits hurt.
Damage uses FromSoftware's own protocol: the attacker's game detects the hit, the victim's
game computes it against the victim's current state (guard, deflect, dodge i-frames). 1.0.9
had no PvP at all; 1.0.11's friendly fire applied the friend's copy's hits locally (laggy,
"hits register half the time") and crashed on death.

First live test (2026-11-12, logs `E:\RE\logs-2026-11-12`): hooks installed, hits captured
and sent both ways (22 / 24 received), **0 applied**; team 29 made the friend a full enemy
(lock-on, native enemy HP bar with a debug name, idols unusable near them). Both are fixed
in the `pvp-ff` branch (below: "Fixes after the first live test").
Code: `crates/coop-dll/src/pvp.rs`, plus `native.rs` (`recv_side_send`), `net.rs` (bulk
dispatch), `scaling.rs` (ScaleCfg 11th byte), `doctor.rs`, `settings.rs`, `puppet.rs`.

Marks: [VAR] read in the Ghidra decompilation / disassembly of the 2026-11-11 process dump
(`E:\RE\notes-2026-11-11.md`, `E:\RE\work_pvp`), [ARV] inferred. Nothing here ran live.

### What the game does (Sekiro 1.06)
- [VAR] Damage router `0xB6A040(dmgModule, attacker, hit)`: action = `table[victimClass*7 +
  attackerClass]`, table `0x2A7C590` (or `0x2A7C660` when `hit+0xFC` != 0); classes
  `0xB6F4C0`: 1 local player, 2 remote player. Read from memory: `[1][2]` = **0** in both
  tables (a remote player's hit on us = `0xB6AB40` only: guard direction, hit SFX, no HP /
  posture), `[2][1]` = 4 / 1. (The earlier note "[1][2] = 1" misread the row: row 1 is
  `1,1,0,1,1,1,1`.)
- [VAR] Action 4 = damage module slot 10, `0xB72CC0` for players: `0xB686A0` serialises the
  hit into a 0xC0-byte record and calls `[[victim+0x2038]]+0x20`; in PlayerNetworkSession
  that is `0x1061D30` -> `0x8D9D40` -> `0x8E6FA0` (packet type 20 to the victim's owner), in
  PlayerDebugSession `0x1061880` = `jmp [vtbl+8](this, record, 0xC0, 20)`. Then
  `0xB68FF0(.., remote = 1)`: reaction only on the copy (`0xB6E6A0` writes HP / posture only
  for remote = 0).
- [VAR] Receive: `0xA37560` (PlayerIns update; `PlayerIns+0x2088` exists only for the local
  player) -> `0xB72A80(dmgModule, rx)`: `0x8D7B10(rx, &record)` (it ignores `rx` and walks
  FrpgSessionManager's member vector `0x8E4010`) returns the next type-20 record of any member
  and the PlayerIns whose session (`[[chr+0x2038]]+0x58`) is that member; `0xB680C0` rebuilds
  the hit (BehaviorParam id at record `+0x48`, not an AtkParam id); for throw kinds 1 and 3 (`hit+0xD9`, the stack byte
  `[rsp+0x109]` in `0xB72A80`) the record's u16 `+0xB0` is compared with our `chr+0x78` handle
  and the kind is zeroed on a mismatch; `0xB68FF0(dmgModule, attacker, hit, 0)` applies it.
- [VAR] Teams: `chr+0x74` (`0x9F64D0` derives it from the chr type `+0x70`, set by
  `0x9F5170`); hostility matrix `0x143B0A680` (34x34, `a*0x22+b`): 1 and 2 are Friend, 29 is
  Rival with everyone.
- [VAR] "Cannot die": `0xBD5DF0(data)` is true when `data+0x228 & 1` (among others); the HP
  setter `0xBD64E0` then keeps HP at 1 when it would go from > 0 to <= 0, before
  `0xB6E6A0`'s "HP < 1" death call (`chr.vtbl+0x168`).
- [VAR, independent review 2026-11-11 `E:\RE\work_review`; call site and xrefs re-checked]
  The 1.0.11 death crash `sekiro.exe+0x66CE96`: every applied hit with HP damage leaves a
  record in the per-frame damage list `[[0x143D56058]+0x38]` (`0xB6F690` -> `0xB6E150` ->
  `0x6D8950(list, victim, attacker, hit)`, cleared by `0x6D8C40` each event update). The
  event update `0x67FD9A` -> handler `0x66CDA0` -> check `0x66CF80` fires when our player is
  dead and the list holds a record "victim = us, attacker = another player"; then
  `call 0xA4A3F0` at `0x66CE5C` returns the attacker chr, `0xA29DD0` its session member,
  `0x8E40F0` finds no such member (-1), `0x7B2980(-1)` = NULL and `mov r8d,[rax+0x10]`
  crashes. So: a remote player's applied hit + our death in the same frame (an enemy, poison,
  a fall) = this crash.

### What the mod does
1. Setting `[GAMEPLAY] friendly_fire = 0|1`. The host's value is used by both (ScaleCfg byte
   11, sent with the scaling percentages; an older host = off). The doctor warns when the two
   ini values differ.
2. While on and linked: team 28 ("Coop NPC"; was 29 before `pvp-ff`) on the native remote of every friend whose Hello is fresh
   (never on our player, never for an older build; restored when off, when the Net goes away,
   on a hub reset (Drop) and in `shutdown`, only while the chr is still in the player set).
   The remote also gets the "cannot die" bit while its owner's snapshot says more than 1 HP
   and its type-1 frames are under 1 s old, so a local-only hit (`0x2A7C660`'s `[2][1]` = 1,
   `hit+0xFC` set) can never kill the copy; otherwise the bit goes and the game
   kills the copy the vanilla way (`pvp: replica held at 1 HP while its owner is dead` if
   seen). Boss fights: `[GAMEPLAY] friendly_fire_in_boss_fights = 0` (default) gives the
   copies their own team back and drops player hits while a boss gauge is shown
   (`game::boss_state`).
3. The laggy path: nothing to patch. Vanilla `[1][2]` is already 0 (reaction only); the hooks
   refuse to install if either table reads otherwise. A pass-through call-site hook at
   `0xB6A12A` (`call 0xB6AB40`) only counts these copy-on-us hits for the log.
4. Transport: native.rs's remote recv session slot 1 now hands type-20 records (0xC0 bytes) to
   `pvp::capture` (the peer is known from `this`). They go to the remote's owner as bulk kind
   `0xA9` (reliable, chunked): `{magic, version, kind, nonce, seq, held ms, record}`; the
   receiver drops duplicates per (nonce, seq). While on, each side sends a Hello every 10 s;
   hits go only to a peer whose Hello came within 35 s (an older build would keep our unknown
   bulk kind in its 24-entry inbox). nsession's packet routing is not used: its
   member-vector fakes do not cover `0x8D7B10`, and the game's attacker lookup by session
   member cannot find our remote (its session object answers slot 11 with an empty member).
5. Receiver: call-site patches inside `0xB72A80`, written with every other thread suspended,
   all or nothing, after checking each `call rel32` target: `0xB72B1D` / `0xB72BC7`
   (`call 0x8D7B10`) -> our queued records first (copied into the game's own buffer
   `0x143D85AE0`, attacker = the sender's native remote), then the original; `0xB72BB8`
   (`call 0xB68FF0`) -> for our records only: the "cannot die" bit on our data module for
   that one call, HP / posture logged before and after. Since `pvp-ff` the mod itself calls
   `0xB72A80(our dmgModule, sentinel rx)` from the frame task while hits are queued (the
   vanilla call never gets past its rx NULL check in our session, see below); `recv_hook`
   returns 0 for the sentinel once the queue is empty, so the original `0x8D7B10` never sees it.
6. Non-lethal duels: a hit that leaves us at 1 HP (or would have killed) ends the duel:
   `pvp: duel over, <name> wins`, a DuelOver message to the winner (`pvp: duel over, we
   win`), and for 5 s both sides drop that peer's player hits (per peer). A deathblow (hit
   type 5, `hit+0x28`) is not applied at all and ends the duel. Both players then get full HP
   and posture back (`posture::set_hp` / `add_posture`, the game's setters, as livesync's
   refill). **Lethal PvP is intentionally not supported** (see the death crash above).
7. Death guard (`[DEBUG] pvp_death_guard = 1`, independent of friendly_fire, installed when
   the first native remote exists, byte-checked, threads suspended): the call at `0x66CE5C`
   goes through `death_attacker_hook`, which calls `0xA4A3F0` and returns 0 when the result
   is one of our native remotes, so `0x66CDA0` leaves through its own `jz 0x66CEE0`
   (returns 0) instead of crashing. Log: `pvp: death event with a player attacker skipped
   (replica of <name>)`. Not done: clearing the attacker in the damage list itself (its
   record layout was not confirmed here).
8. `[DEBUG] pvp_dry_run = 1`: received hits are logged and not applied.
9. A peer that leaves or reconnects (new link epoch) loses its Hello, cooldown, duplicate
   filter and queued hits. A queued hit is served only while its sender's remote is still in
   the WorldChrMan player set (`native::live_remote`).

10. No deathblow / throw on a friend (since `pvp-ff`): the two chr finders' candidate checks
   are replaced in their vtables by wrappers that refuse every native remote and call the
   original otherwise: `SprjChrThrowModule::RequestThrow_AllChr` finder (slot `0x142A8EBD0`,
   `0xBDF930`) and `FdpChrCeremonyModule::_GetEnableExecuteCremonyChr` finder (slot
   `0x142A7BC20`, `0xB5DD30`); class names from RTTI. Both decide by M1
   (`0xC09AA0` = `M1[attacker team][candidate team]`), which passes for team 28. A received
   hit of type 5 is still never applied (item 6).

### Fixes after the first live test (`pvp-ff`, 2026-11-12)
- [VAR, Ghidra on the dump, project copy `E:\RE\ghidra_pvpff`, jobs `E:\RE\work_pvpff`]
  Why 0 hits were applied: `0xA37560` allocates `PlayerIns+0x2088` (`param_1[0x411]`) only
  while `0x9F3430(player)` = `[[0x143D7A1E0]+0x88] == player` (the game's own online player),
  then calls `0xB72A80(dmgModule, [player+0x2088])`; `0xB72A80` begins with `test rdx,rdx;
  jz 0xB72BFF`, so with a NULL rx none of our call sites ran. [ARV] that it was NULL in the
  test (not read live); the new one-time line `pvp: receive path: PlayerIns+0x2088 = ...`
  shows it. Fix: the frame task drives `0xB72A80` itself (item 5). Neither `0xB680C0` nor
  `0xB68FF0` (two call levels down) checks the team matrices, so the attacker's team 28 does
  not stop the application.
- Team 28 instead of 29 (RE `E:\RE\work_team\team_matrices.txt`): M1 `0x143B0A680`
  `[1][28]` is the same functor object as `[1][29]` (our hitboxes hit the copy, the hit filter
  `0xB68A50` passes, the router's `[2][1]` = 4 still makes the type-20 packet); M2
  `0x143B0CAA0` `[1][28]` / `[28][1]` are not hostile (no lock-on, so no enemy bar; the idol's
  enemy finder ignores it); enemy AI treats 28 like 2; `M1[28][1]` = F, so the copy's own
  hitboxes no longer hit us locally (the "local-vs-replica" counter should stay 0).

Install checks (all or nothing; `pvp: hooks NOT installed: ...` otherwise): the four call
targets, `[1][2]` = 0 in both tables and `[2][1]` = 4, the 19 bytes of `0x1061880` and
PlayerDebugSession vtable (`0x2B86048`) slot 4 = `0x1061880`, the first 9 bytes of
`0xB72A80` (`48 85 D2 0F 84 76 01 00 00`), both finder vtable slots holding their function
and those functions' first 8 bytes; at the first remote, its recv vtable copy's slot 4 is
logged (`pvp: remote session slot 4 = ...`).

Prosthetic tools, bullets and explosions [VAR, Ghidra `E:\RE\work_pvpff_bul`]: they take the
same path as the blade. Bullet hit dispatcher `0x995600` -> `0xB06360(hit, victim, bullet)`
(owner from bullet `+0x4BC`; an owner of type 1 = a chr) -> the hit filter `0xB68A50` (M1,
rows = attacker team, with the AtkParam's `+0x89` hit-friend / hit-enemy bits) -> victim
damage module slot 9 `0xB729E0` (the router `0xB6A040` is inside it) -> action 4 -> slot 10
`0xB72CC0` -> type-20 record. The record flags `+0x3B` bit 1 mark a bullet (`+0x4C` then
holds the BulletParam id; the victim reads the AtkParam from that row). The first live test
already captured and sent bullet records (behaviors 107200110 = BulletParam 720010 and
107200297 = BulletParam 295); like every other hit they were never applied (the receive
bug above). `0xC02140` / `0xC099F0` are SpEffect target filters, not bullet filters. So no
separate mechanism is needed. The old note "bullets go through `0x2A7C660`, local copy only"
was wrong. Log lines now say `melee` or `bullet` per hit.

Not covered: damage over time (burn, poison: SpEffects ticking locally, never a type-20
record [ARV]); bullets with no chr owner (traps: the router's no-attacker branch); lethal
friendly fire.

Open [ARV]: SpEffects carried by a hit (flame vent burn, status build-up) may be applied
through `0xC00130` / `0xC02140` with the attacker's copy as the applier; with team 28
`M1[28][1]` = functor A ("hit friend" bit only), so an effect flagged for enemies only
would be refused (team 29 gave C and would pass). The direct damage is not affected. Watch
for "flame vent hits but no burn" in the test.

Risks [ARV]: with team 29 the copy's swing played the vanilla reaction-only path on us (hit SFX,
maybe a guard spark) before the real hit arrived; with team 28 it should not hit us at all. The death crash above is reachable whenever
a friend's applied hit and our death fall into the same event update; the death guard (item 7)
closes it at the faulting lookup, untested live. (`0xB53BA0` is NOT that path: it reads the
`chr+0x1FF8 -> +0x88` module's `+0x18` / `+0x20`, written only by `0xBE14D0` from `0xB6F140`
for grab / link hits with `hit+0x88` >= 1.) The HP damage field `hit+0x1E0` is logged as int
or float (Ghidra typed it float).

### Log lines (`pvp:`)
- Attacker: `pvp: hit #N (local K) sent to <name> (peer P, ok true): behavior B (melee|bullet), record dmg
  [...], held H ms; us [x,y,z], replica [x,y,z] (D m); last type 1 of the victim T ms ago;
  ping R ms`.
- Victim: `pvp: hit #N from <name> received (behavior B (melee|bullet), +~L ms sender->here incl. H ms
  held there, Q ms queued here) -> applied hp a->b posture c->d (hit hp X posture Y, type T,
  result R)[, CLAMPED at 1 HP (duel lost)]`, or `... dropped: <why>` / `DRY RUN`.
- Once, at the first queued hit: `pvp: receive path: PlayerIns+0x2088 = 0x0 (NULL: the
  game's own receive never runs ...); game's online player is ours: false`.
- Every 10 s while on: `pvp: 10 s: our hits captured .. sent ..; friend's received ..
  applied .. (receive loop driven Nx); local-vs-replica hit dropped (laggy path, reaction
  only) ..; throw / deathblow on a friend refused ..; dropped: ...`.
- On / off with the reason, team changes (`team 2 -> 28 (Coop NPC ...)`), install result.

### Live test protocol (the first test is diagnostic)
1. Same build on both PCs; the HOST sets `friendly_fire = 1` (the client's value does not
   matter; the doctor notes a difference). Optional first round: both `pvp_dry_run = 1`.
2. Connect, meet in one block, wait until both logs show `pvp: player-vs-player damage ON`
   and `pvp: peer ...: team 2 -> 28`.
3. Check the ally side first: no lock-on on the friend, no enemy HP bar over them, an idol
   next to them can be used, no deathblow prompt on them (also not when their posture breaks).
4. Then hit each other for 5 minutes: single slashes, combos, block, deflect, dodge through
   a slash, every prosthetic tool you have (shuriken, firecracker, flame vent, axe, spear),
   a combat art, one round to the end (1 HP). Note the time of anything odd (F11 writes a
   marker).
5. Exchange `%LOCALAPPDATA%\sekiro-coop\log.txt` from both sides; match `hit #N` lines
   attacker -> victim (sent vs applied, latency, HP before / after, results of blocked /
   deflected / dodged hits, behavior ids and melee / bullet of tools), the 10 s counters, any crash.rs breadcrumb.
6. End the session (Severed Thread): `pvp: player-vs-player damage off (no link)`; a remote
   that is still there gets `team ... restored`.

## Enemy target choice, observed (`[DEBUG] ai_retarget`, default 1; test7, NOT verified live)

Groundwork for a later "who does the enemy fight" model; this build changes nothing in the game.
The engine scores every target record of an AI each frame and fights the highest
(`docs/RE-AI-TARGET.md` §3): `targetPriority + (1 - dist/1000) * hitF * topF + 50` for the current
target, except on replan frames. `aitarget.rs` detours the score function `0x61DE40`, calls the
original and returns its value unchanged. With `ai_retarget = 1` it samples the records whose target
is a player (ours or a friend's native remote, matched by chr handle) into a fixed table; the frame
task writes the `aitarget:` lines. It runs only where the enemy's AI runs (the owner); NetAI copies
have no targeting system. `ai_retarget = 0`: the hook returns at once (0 at start: not installed;
switching to 1 later installs it). Offsets and their sources are in the module header.

Log lines (only enemies that know both players, or events that involve the friend):
- `aitarget: <cNNNN id E (ptr, hp)> TARGET me -> peer1 (on a replan frame | NOT a replan frame; old
  held 12.4s): new ... | old ...`: the top record changed. Each candidate is written as `score S =
  dist D m x hitF H (hit Xs ago, w W) x topF T + bonus B [top] resid R; rec state N seen|unseen`.
  `resid` = score minus our reconstruction (targetPriority, or a stale distance).
- `aitarget: ... HIT by peer1 (hp N) registered on this PC's AI (top me): ...`: the hit timer of
  that player's record went back to 10 s. A `HIT by peer1` on the owner answers RE-AI-TARGET §9.3
  (the friend's hits reach the owner's AI).
- `aitarget: ... knows peer1 (hp N) now: ...`: a record for that player appeared.
- `aitarget: ... replan frame (no +50): <both candidates> | best X, top Y for Ns`: only on a replan
  frame with both players scored, at most once a second per enemy: the raw choice without the +50.
- `aitarget: ... last 10s: N replan frame(s) (M with 2+ players), K top change(s), hits seen me 2,
  peer1 1 (at least)`: how often the engine replans (§9.1); only for enemies with a replan frame, a
  hit or a top change in the window and the friend in their list (an idle boss stays silent). Hits
  are every rise of the record's hit timer; two hits between two samples count once.
- `aitarget: score calls N/s, M/s about a player; ... dropped (table busy), ... skipped (owner vtable
  not 0x142961690), ... with the replan state unknown`: cost and sanity check every 10 s.
- All `aitarget:` lines share a budget of 10 a second; the rest is counted in one
  `aitarget: N line(s) suppressed` line at most every 10 s (the same for dropped events).

Players are keyed by peer id (`me`, `peerN` = session slot N); the friend's distance is measured
to the native replica's own position, as the AI does. The hook is installed in two phases (the
trampoline is published before the jump is written); switching 0 -> 1 in the world installs it
with the native session's 250 ms freeze budget and retries a busy install on a later frame.

Not seen: records frozen above 100 and records about non-players. Damage share per player is not
tracked by the crate; the hit timer per record (seconds since that player's last hit) stands in.

## Working on this fork

- Repository: https://github.com/ZeroPrin/sekiro-coop, branch `share-pickups` (the fork's
  default branch). `main` stays a copy of the original author's `main`.
- New work: a branch from `share-pickups`, then a pull request into `share-pickups`.
- Bump `MOD_VERSION` in `crates/coop-dll/src/ui.rs` (`1.0.9-pickups.4`, ...) with every build
  you give to other players: everybody in a session needs the same build.
- When the original author publishes newer source (1.0.10 / 1.0.11), the pickup commits can be
  rebased onto it (`git remote add upstream https://github.com/Tihvert/sekiro-coop`).

## Build

`build-public.cmd` (VS 2022 Build Tools with MSVC x64 and Windows SDK 10.0.26100, Rust
stable). Unit tests: `cargo test --release -p sekiro_coop --no-default-features --features dev-native`.
