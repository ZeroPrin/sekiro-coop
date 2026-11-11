# share-pickups fork

Unofficial branch of Sekiro Together, based on the newest public source (1.0.9-beta,
commit `e11f68b`; the 1.0.10 / 1.0.11 sources were not published when this was made).
Version string: `1.0.9-pickups.3`. Everybody in a session needs the same build.

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
  walk runs in both modes, the receiver needs its facts. One-loop states only with
  `sync_objact = 2`; a door's sub infos go out once per 2 s.
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
  local start, our own player used it, a one-loop below mode 2, a door toggling closed, an
  unknown class. The object check (`doors::object_view`) stays: already at that animation =
  nothing. The kick is skipped when isEventKickSync = 1 (the replay kicked), a kick is pending,
  the info has no ObjActEntityID, `0x6B2AF0` has other bytes, or the event manager is missing.
  doors.rs kicks after its own `0x6BA910` too (only with `sync_objact` on).
- **Journal** (reload persistence): every executed ObjAct of the session, ours and the peers',
  by (map id, ObjActEntityID), at most 256 (oldest dropped), only one-state / door states that
  opened (a door closing leaves it; one-loop never: a lift must not move on a reload). Cleared
  on a new session, never written to disk. Once per second, an info of a journaled ObjAct that
  has been loaded and watched for 3 s, has not run since its load and whose object is not at
  that animation is replayed and kicked once per load (`journal:` line). This is the fix for
  doors that close again after a reload.
- **Chests** (ObjActParam 999980 / 999985 / 999990, one-state, kick sync 0, timing 1) take the
  same path: the kick lets 20005510 / 20005511 enable the treasure, so the item node spawns
  here and share_pickups sees it. A chest the friend already looted: its lot flag came with the
  pickup, the `0x91FCF0` refusal of the pickups section still applies.
- Nothing runs in a Remnant memory.

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

Open: keyed doors open on the receiver without the key (flags are shared anyway); lifts and
levers (one-loop) stay off unless both set `sync_objact = 2` and are not journaled.

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
