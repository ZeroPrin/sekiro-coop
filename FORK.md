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
