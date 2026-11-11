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
