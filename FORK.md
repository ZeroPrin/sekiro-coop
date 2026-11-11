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

## PvP on the native hit packet (experimental, `[GAMEPLAY] friendly_fire`, default 0)

Not tested in play. Player-vs-player damage with FromSoftware's own protocol: the attacker's
game detects the hit, the victim's game computes it against the victim's current state
(guard, deflect, dodge i-frames). 1.0.9 had no PvP at all; 1.0.11's friendly fire applied the
friend's copy's hits locally (laggy, "hits register half the time") and crashed on death.
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
  the hit (AtkParam id at record `+0x48`); for throw kinds 1 and 3 (`hit+0xD9`, the stack byte
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
2. While on and linked: team 29 on the native remote of every friend whose Hello is fresh
   (never on our player, never for an older build; restored when off, when the Net goes away,
   on a hub reset (Drop) and in `shutdown`, only while the chr is still in the player set).
   The remote also gets the "cannot die" bit while its owner's snapshot says more than 1 HP
   and its type-1 frames are under 1 s old, so a local-only hit (`0x2A7C660`'s `[2][1]` = 1,
   `hit+0xFC` = bullets [ARV]) can never kill the copy; otherwise the bit goes and the game
   kills the copy the vanilla way (`pvp: replica held at 1 HP while its owner is dead` if
   seen). Boss fights: `[GAMEPLAY] friendly_fire_in_boss_fights = 0` (default) gives the
   copies their own team back and drops player hits while a boss gauge is shown
   (`game::boss_state`): no lock-on onto the friend mid-boss, allied NPCs (teams 25-28 are
   hostile to 29) stay friendly.
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
   that one call, HP / posture logged before and after.
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

Install checks (all or nothing; `pvp: hooks NOT installed: ...` otherwise): the four call
targets, `[1][2]` = 0 in both tables and `[2][1]` = 4, the 19 bytes of `0x1061880` and
PlayerDebugSession vtable (`0x2B86048`) slot 4 = `0x1061880`; at the first remote, its recv
vtable copy's slot 4 is logged (`pvp: remote session slot 4 = ...`).

Not covered: prosthetic / bullet hits (through `0x2A7C660` [ARV]: local copy only, no type 20),
throws (the game cancels them: the record's target handle is the sender's), lethal PvP.

Risks [ARV]: the copy's swing still plays the vanilla reaction-only path on us (hit SFX,
maybe a guard spark) before the real hit arrives. The death crash above is reachable whenever
a friend's applied hit and our death fall into the same event update; the death guard (item 7)
closes it at the faulting lookup, untested live. (`0xB53BA0` is NOT that path: it reads the
`chr+0x1FF8 -> +0x88` module's `+0x18` / `+0x20`, written only by `0xBE14D0` from `0xB6F140`
for grab / link hits with `hit+0x88` >= 1.) The HP damage field `hit+0x1E0` is logged as int
or float (Ghidra typed it float).

### Log lines (`pvp:`)
- Attacker: `pvp: hit #N (local K) sent to <name> (peer P, ok true): atk param A, record dmg
  [...], held H ms; us [x,y,z], replica [x,y,z] (D m); last type 1 of the victim T ms ago;
  ping R ms`.
- Victim: `pvp: hit #N from <name> received (+~L ms sender->here incl. H ms held there, Q ms
  queued here) -> applied hp a->b posture c->d (hit hp X posture Y, type T, result R)[,
  CLAMPED at 1 HP (duel lost)]`, or `... dropped: <why>` / `DRY RUN`.
- Every 10 s while on: `pvp: 10 s: our hits captured .. sent ..; friend's received ..
  applied ..; local-vs-replica hit dropped (laggy path, reaction only) ..; dropped: ...`.
- On / off with the reason, team changes, install result.

### Live test protocol (the first test is diagnostic)
1. Same build on both PCs; the HOST sets `friendly_fire = 1` (the client's value does not
   matter; the doctor notes a difference). Optional first round: both `pvp_dry_run = 1`.
2. Connect, meet in one block, wait until both logs show `pvp: player-vs-player damage ON`
   and `pvp: peer ...: team 2 -> 29`.
3. Duel for 5 minutes: single slashes, combos, block, deflect, dodge through a slash, a
   prosthetic, a combat art, one duel to the end (1 HP). Note the time of anything odd (F11
   writes a marker).
4. Exchange `%LOCALAPPDATA%\sekiro-coop\log.txt` from both sides; match `hit #N` lines
   attacker -> victim (sent vs applied, latency, HP before / after, results of blocked /
   deflected / dodged hits), the 10 s counters, any crash.rs breadcrumb.
5. End the session (Severed Thread): `pvp: player-vs-player damage off (no link)`; a remote
   that is still there gets `team ... restored`.

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
