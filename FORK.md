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

## Loadout sync (experimental, `[DEBUG] loadout_sync`, default 1; not tested live yet)

**Bug.** The friend's native replica (`native.rs`, a PlayerIns with its own PlayerGameData)
never got the friend's equipment. `create` fills the replica's PGD from the first existing
CharaInitParam row (the dev test's row 1080100 does not exist in retail), and the deferred
dress called `ghost::dress_like(remote, player)` with the LOCAL player: the observer's 25
ChrAsm slots (`ChrIns+0x2140`, ids at `+0x24 + 4*slot`, set through `0x9CCBA0`), once.
Nothing about equipment went over the network (the snapshot has pose, animation and HP
only). Seen live: the replica wore the observer's prosthetics, and every combat art of the
friend played as Whirlwind Slash (the template's default art).

- **Message**: bulk kind 0xA8 (`loadout::BULK_LOADOUT`; far from upstream's 41-50 run, reliable, to every peer; the host
  relays). Body (`coop_net::encode_loadout`): magic `LD`, layout version 1, the sender's 25
  ChrAsm ids, `art` and `extra[3]` (i32 each, reserved for the combat art / skills, -1 for
  now). Sent when our ids change and every 3 s as a refresh (late join, lost packets; only
  while no bulk chunk is unacked). An older build leaves it unread.
- **Receiver**: the last loadout per peer slot (`loadout::of`). The deferred dress
  (`native_dress_delay_ms`) now puts the PEER's ids on its replica (`ghost::dress_with`,
  same `0x9CCBA0` loop) and then reloads the parts through PlayerIns vtable slot 21
  (`0xA30A50` -> `0xA361C0`, RE-FINDINGS-2 § Solid ghost (c)), only when the slot holds
  exactly that function and the replica is in the player chr set. A newer loadout that
  differs from the one applied re-dresses it (not only once). No loadout yet when the dress
  is due: it wears ours as before, and the first loadout that arrives re-dresses it.
  `loadout_sync = 0`: nothing is sent and the replica wears ours (the old behaviour).
- **Log lines**: `loadout: sent [...]` (ours, on change only), `loadout: peer N wears
  [...]` (theirs, on change), `ghost: dressed like peer slot N, K slots changed, ...`,
  `native: parts of 0x... reloaded after K slot(s) changed` or `native: part reload of 0x...
  skipped: ...`, `native: new loadout from peer slot N, re-dressing ...`, and the fallback
  `native: no loadout from peer slot N yet, ... wears ours for now`.
- **Live test**: both on this build. The observer equips something the friend does not
  have (Ichimonji as the art, a different prosthetic) before the session and again during
  it. Before the fix the replica showed the observer's prosthetics and `ghost: dressed like
  player`; now the observer's log has `ghost: dressed like peer slot N` with the friend's
  ids (they match the friend's `loadout: sent` line), the friend's prosthetics are visible
  on the replica, and a change of the friend's prosthetic shows within ~1 s with a
  `re-dressing` line. Watch for a crash right after `parts of ... reloaded` (the reload
  call on a live remote is new); `loadout_sync = 0` turns it off.
- **Open: the combat art.** Equip ids do not carry it; the replica's PGD keeps the
  template's `cult_sword_id` (CharaInitParam row `+0x110`), so the friend's arts still play
  as Whirlwind Slash. `0x7B7470` / `0x7B7120` (PGD_FILL_A/B) read that row into some PGD /
  EquipGameData field: find the write target in IDA, then send `art` and write it there
  (`TODO(IDA)` in `native.rs` `dress_from_peer` and in `loadout.rs`).

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
