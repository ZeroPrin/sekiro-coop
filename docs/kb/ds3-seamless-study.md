# DS3 Seamless Co-op v0.1.1 (LukeYui): static study for Sekiro Together

Research note, 2026-11-14. Static analysis only, for learning. Nothing from the archive was run, loaded
or installed, and no game was started. Decompiled code is not reproduced here: the logic is described
in our own words, with at most short identifiers. Nothing below may be copied into our source.

Evidence tags:
- **[DIS]**: read in the mod's disassembly or decompilation (Ghidra 12.2).
- **[XMAP]**: the mod's AOB pattern matches Sekiro 1.06 byte for byte (exact, unique hit in the
  carved image `E:\RE\carve\sekiro_mem.exe`). The Sekiro function was then decompiled.
- **[INF]**: inference from names, constants, data flow or public DS3 knowledge.
- **[UNK]**: needs the DS3 1.15.2 binary.

Work files (outside the repo): extraction `E:\RE\work_ds3_seamless\src\`, scripts
`E:\RE\work_ds3_seamless\tools\` (`petriage.py`, `emu_patterns.py`, `decode_slots.py`, `xmap.py`,
`dec_at.py`, `disasm.py`), outputs `E:\RE\work_ds3_seamless\out\` (`all.c` full decompile,
`patterns.tsv`, `slots.tsv`, `xmap.tsv`, `hookblocks.txt`, `eventlists.txt`, `sek1.c`/`sek2.c`
Sekiro decompiles). Ghidra projects: `E:\RE\ghidra_ds3_seamless\` (mod) and
`E:\RE\ghidra_ds3_seamless\sekiro\sekiro_quick` (copy of `ghidra_quick`).

---

## 0. Short answer

1. **It is ERSC's design, one generation earlier and without a protector.** The mod puts the
   players into the game's **own** session, so they are native remote `PlayerIns`, and it replaces
   only the **transport** under the game's packet layer: send-to-member, receive-from-member and
   receive-from-any are rerouted to `ISteamNetworkingMessages`. Every native packet type, including
   the DS3 equivalents of our NetAI types 4/5/22/23/70/71, flows unchanged through the tunnel. [DIS]
2. **Enemy AI authority is left to the game.** No hook touches the ownership distribution, the
   priority function or an "owned here" predicate. The DS3 equivalents of `0xA1CED0`, `0xA1DD00`,
   `0xA1D550`, `0xA4A650` and `0x9F0230` appear nowhere in the 150 recovered patterns. Frozen enemies
   are not repaired by an authority mechanism. The mod instead keeps the session object alive across
   loads and warps, so the native host arbiter never disappears. [DIS for "no hook"; INF for "why"]
3. **Enemy death is the mod's own state.** It keeps a per-map-block bitset of dead enemy netIds
   (`netId = block<<11 | index`, the same encoding as our SPEC §3). It broadcasts each death (mod
   message 12), applies the set when a block loads (the enemy is killed on load), seeds the set from
   the save's `"CHR "` chunk, and clears it on rest. Breakable objects get the same treatment (message
   13). This is how "killed by anyone stays dead for everyone until a rest" works. [DIS]
4. **Bosses and fog walls are handled at the EMEVD condition layer.**
   - `EcTestMultiplayState` answers "Host" for every member and "Client" never.
   - `EcTestIsMyWorld` and the C++ `IsMyWorld` answer from per-event allow lists.
   - Per-event argument rewriters redirect "player 10000 plays the fog-walk animation" to the player
     who actually triggered the event. That player is carried in the native event-sync packet, which
     the mod also hooks.

   No call to `SetNetworkUpdateAuthority` was found. Boss authority therefore stays the game's own
   (Forced on whichever machine runs that branch). [DIS mechanism; UNK who owns the boss in practice]
5. **Death becomes spectating, not "send home".** The native Lua-imitation "player dead / phantom
   return" handlers are replaced or no-op'ed. While another player is alive, the death reload is held.
   The camera and its position source are redirected to the spectated player's `PlayerIns`. A revive
   message restores HP. On a wipe the normal reload runs. [DIS]
6. **Scaling uses synthesised SpEffect rows plus the native multiplayer-correction hook.**
   - At param load the mod clones one SpEffect row into 3 categories × 5 player counts, with HP and
     damage rates scaled from the ini.
   - It hooks the DS3 twin of Sekiro's MultiPlayCorrection `0x9E9A90` (exact pattern hit) and
     `_UpdateMultiDoping`, so enemies get the row for the current player count and are re-doped when
     the count changes. [DIS + XMAP]

---

## 1. Triage

### 1.1 Archive

`DS3 Seamless Co-op v0.1.1-1895-0-1-1-1741470313.zip`, 1 287 892 B, SHA-256 `DA7D7D42…C72C8580`.

| File | Size | SHA-256 (prefix) | Notes |
|---|---|---|---|
| `ds3sc_launcher.exe` | 178 688 | `EE7C8F74751154DF` | PE32+, built 2024-11-14, PDB `C:\Users\Luke\source\repos\ds3sc\x64\Release\ds3sc_launcher.pdb` |
| `SeamlessCoop/ds3sc.dll` | 1 807 872 | `A7869888C98F36C4` | PE32+, built **2025-03-08 21:41 UTC**, PDB `…\ds3sc\x64\Release\ds3sc.pdb` |
| `SeamlessCoop/crashpad/crashpad_handler.exe` | 824 832 | `D799B428ECC200A4` | stock Chromium crashpad (2023-02-08), PDB `C:\Users\Luke\crashpad\…` |
| `SeamlessCoop/ds3sc_settings.ini` | 2 012 | `D961DAF86B88C5C3` | settings (§1.4) |
| `SeamlessCoop/locale/english.json` | 2 567 | `C1CD16184C4B6BA2` | UI strings |
| `crashdumps/attachments/`, `crashdumps/reports/` | – | – | empty folders |

The archive contains no README and no licence text. The only author text is in the binaries:
`Dark Souls III: Seamless Co-op mod by Yui [ YuiNexus Version 2 ]` and
`Dark Souls III Seamless Co-op v0.1.1 by Yui` (version string `0.1.1`). Locale credit: `"CREDITS": "LukeYui"`.
The Nexus terms (no re-upload, no modification, no porting) come from the brief and are respected:
nothing is redistributed or reused.

### 1.2 ds3sc.dll

- **Sections:** plain MSVC sections. Entropy: `.text` 1.26 MB 6.43, `.rdata` 6.24, `.data` 4.64,
  `.pdata` 6.14. Also `.00cfg`, `.gxfg` (CFG), `.retplne`, `.tls`, `_RDATA`, `.rsrc` and `.reloc`.
  There is no overlay and no extra section.
- **Imports:** full IAT:
  - KERNEL32 171 (it includes `Suspend/ResumeThread`, `Get/SetThreadContext` and `Thread32First`:
    MinHook-style thread freezing);
  - USER32 3, steam_api64 4 (`GetHSteamPipe/User`, `Register/UnregisterCallback`), WS2_32 33,
    ADVAPI32 14 (CryptoAPI), CRYPT32 16, WLDAP32 18, Normaliz 1. That is static libcurl.
- **Export:** `modengine_ext_init` (ModEngine 2 extension entry), as in ERSC.
- **Statically linked:** libcurl (schannel), crashpad client, nlohmann::json, std::format, an
  AES-style block cipher ("Plaintext length must be divisible by"), and the key-like string
  `Tu.gwF*yX(9I=JC?YlX0Ps?)9+9#dFi4`, which is used next to the lobby password.
- **Steam interfaces by version string:** `SteamMatchMaking009`, `STEAMAPPS_INTERFACE_VERSION008`,
  `SteamFriends017`, `SteamNetworking005`, `SteamNetworkingUtils003`, `SteamNetworkingMessages002`,
  `SteamUser021`, `SteamUtils010`.
- **Source modules** (from assert paths): `hooks`, `mod_preferences`, `sprj_session_manager`,
  `sprj_menu_man`, `sprj_world_chr_man`, `sprj_map_item_man`, `sprj_event_flag_man`, `sprj_game_man`,
  `sprj_game_data_man`, `sprj_emk_system`, `param`, `networking`, `yui_nexus_2`,
  `ynx2_seamless_manager`, `mod_message_repository`.
- **Hook signatures leak game type names** (`CreateHooksClassHook<…>` lambdas): `SteamSessionLite`,
  `SteamSessionMemberLight`, `SprjPacketStorage`, `FrpgSessionManager`,
  `SoloParamRepositoryLookupContainer`, `GameDataManCtrl`, `PlayerDeadEventInfo`, `BlockClear2Info`,
  `BonfireFirstLvUpInfo`, `SprjLuaEventScriptImitation`/`LuaWarp1Info`, `PlayerSpawnInfo`,
  `SprjWorldChrMan`, `PlayerData`, `SprjEmkEventIns`, `EnemyIns`, `PlayerCtrl`,
  `PlayerGaugeComponentStruct`, `SprjWorldObjMan`, `ObjIns`, `PartyMemberInfo`, `SprjLuaEventMan`,
  `ParseEmkEventData`, `SteamPlayerData`, `EcTestConditionVariableStruct`,
  `EcTestMultiplayStateStruct`, `SprjEventChrEntity`, `SprjEventFlagMan`.
- **RTTI names used to locate vtables in the game:**
  `ChrFinderImp@_UpdateMultiDoping@WorldChrManImp`, `SprjEmkEventIns`, `EcTestIsMyWorld`,
  `EcTestChrSosInvadeType`, `EcTestMultiplayState`, `EcTestChrType`, `EcTestChrDead`,
  `EcTestSexType`, `EventChrEntity`, `SprjEnemyDamageModule`, `SprjRemoMan`,
  `DLNRD::SteamMatchingSystem`.
- **Singletons** are reached by name with null asserts: `SprjSessionManager`, `FrpgSessionManager`,
  `SprjWorldChrMan`, `SprjEventFlagMan`, `SprjGameMan`, `SprjGameDataMan`, `SprjMenuMan`,
  `SprjMapItemMan`, `NewMenuSystem`, `SoloParamRepository`, `StayParamRepository`.

### 1.3 Protection

**Not protected.** No commercial protector, no virtualisation, no packer (normal entropy, complete
IAT, entry point in `.text`). `IsDebuggerPresent` appears only in CRT fail-fast code. Unlike ERSC
(Themida/WinLicense), this binary is fully readable.

The one obstacle is **xorstr-style string obfuscation**: ini keys, URLs, Steam lobby keys and **all
AOB pattern strings** are rebuilt on the stack with AVX XOR at run time. I recovered the patterns
statically with Ghidra's p-code emulator (`tools/emu_patterns.py`, which emulates only the four
resolver functions and nothing else). There were 150 patterns.

- **Scanner:** it parses `"48 8B 0D ?? …"` text, anchors on the rarest byte with a frequency table,
  and resolves RIP-relative or `call` targets. It is the same scanner family as in ERSC.
- **The table has four parts:**
  - slots 2–38: game functions the mod calls;
  - slots 39–131: hook sites;
  - slots 137–156: globals;
  - slots 157–168: vtables found through RTTI.
- **Hook kinds** (`out/hookblocks.txt`):
  - E9 function replacement into a `std::function` thunk;
  - E8 call-site retargeting, where the handler gets the original callee as a member pointer;
  - trampoline detours;
  - vtable slot swaps.
- **Hard-coded game RVAs** exist only for crash fixups, in an exception-filter map at init.

**Launcher:** sets `SteamAppId`, starts `DarkSoulsIII.exe` and injects `SeamlessCoop//ds3sc.dll`
with the classic `VirtualAllocEx` / `WriteProcessMemory` / `CreateRemoteThread(LoadLibrary)` sequence.

### 1.4 Settings ini (features)

| Key | Default | Meaning / where it is implemented |
|---|---|---|
| `allow_invaders` | 1 | Mod-run invasions ("Malcontent"); Blasphemous Pendant |
| `death_debuffs` | 1 | Rot Essence debuff on death, cured at a bonfire (SpEffect applied in the death replacement, §4.3) |
| `overhead_player_display` | 2 | 0–5: normal, none, ping, SL, deaths, SL+ping (message 26 stats, `"Deaths: "`) |
| `skip_intros` | 1 | Skips the logos |
| `sync_progress_as_guest` | 1 | A guest saves events done in the host's world (the IsMyWorld allow lists, §5) |
| `game_boot_volume` | 5 | – |
| `enemy_health/damage/posture_scaling` | 35 / 0 / 15 % per player | Synthesised SpEffect rows (§7) |
| `boss_health/damage/posture_scaling` | 100 / 0 / 20 % per player | Same, boss category |
| `cooppassword` | – | Steam lobby key (lobby keys like `lobby_breakin_lobby_ynxsrs_%s_%u`, hashed with the key string above) |
| `save_file_extension` | `co2` | Separate save file |
| `mod_language_override` | – | Locale JSON |

**Items** (locale): Crystalline Pendant (host), Stone Pendant (join), Blasphemous Pendant (invade),
Rusted Pendant (leave), Ominous Tome (world rules: friendly fire / PvP), Taunter's Orb (lure
invaders). They are granted automatically when missing (§9, slot `0x1E8`).

---

## 2. Q1: architecture, transport, session model

**Session.** The players are real members of the game's session: `FrpgSessionManager` and
`SprjSessionManager`, with `SteamSessionLite`/`SteamSessionMemberLight` at the DLNRD layer. Remote
characters are created by the game's own remote-`PlayerIns` path. The mod hooks the call site around
the member-valid check and the remote-player creator.
- [XMAP] These are Sekiro `0xA7AA24 → 0x1BD9510` (SessionMember valid) and
  `0xA7AA82 → 0xA7A840` (remote PlayerIns create). Both are exact hits.
- **Discovery:** Steam lobbies with a password key.
- **Persistence:** call sites that would leave or disband the session during loads and warps are
  forced to "stay". Example [XMAP]: Sekiro `0xA76B62` calls `0x832180` (`GameMan+0xD28 ∈ {0xB,0xC}`);
  when that check fails, the code calls the session-leave functions `0x8E4790`/`0x8E4A20` and then
  `0x8E4C80`. The mod forces the check true.

**Transport** [DIS], identified from the hooks plus [XMAP]:
- **Send-to-member** (Sekiro `0x1C14340`) is replaced. The game packet becomes one Steam message
  `[u8 sendKind][u8 packetType][u8 0][u32 len][payload]`, sent with
  `ISteamNetworkingMessages::SendMessageToUser` to the member's SteamID.
  - Reliability: kind 0 → unreliable-no-delay (5), or reliable (9) for a configured type set.
    Kinds 1/3 → reliable. Kind 2 → unreliable.
  - Channel: kind 3 uses the mod's own second channel.
- **Receive-from-member** (Sekiro `0x8EA440`) and **receive-from-any** are replaced by
  **per-member, per-type queues**: 78 queues for game types (`type < 0x4E`), and a second family of
  30 for another receive path.
  - Type **57** is special: it is polled across all members.
  - Some types are dropped unless the native FSM state (`FSM+0xDC`) is 5.
- **A per-frame pump** (hooked tick, `SteamSessionLite` signature) calls `ReceiveMessagesOnChannel`
  (up to 256 messages). Mod messages are dispatched by `byte0` through a map of `std::function`s;
  game packets go into the queues.

Compare our `SPEC-NATIVE-SESSION`, which swaps the five session send/recv entry points in Sekiro.
DS3SC does it **one layer lower** (DLNRD member send/recv). The game's session manager therefore
still sees a "real" Steam session, member list and states, and needs no fake vectors.

**The mod's own messages** (`sendKind 3`, `byte1` = id; sender function in brackets):

| Id | Payload | Meaning [DIS unless INF] |
|---|---|---|
| 2, 3, 4 | – | join/handshake family (`0x8DCB0`) [INF] |
| 5–8 | – | session/roster family (`0x8E770`) [INF] |
| 9, 15, 28 | 1 B / … | death/spectate state machine (`0x4B100`). **15 = "I died"** |
| 10, 11, 17+2k | – | member drop/kick (`0x8CCC0`) |
| **12** | u16 netId | **enemy died → mark dead on all** (`0x523F0`) |
| **13** | u32 objId | **breakable object destroyed** (`0x52530`) |
| **14** | u8 | **revive done / alive again** (`0x4CCB0`, `0x4A4C0`) |
| **16** | flag blob | **event-flag snapshot to a joining member** (`0x4E9C0` → `0x88B40` serialiser) |
| **20** | u8 | **host rest → world reset for all** (`0x4E070`) |
| **21** | u8 | **guest rest → ask host to reset** (`0x4E070`) |
| 22 | 8 B | host-only Lua action relayed (`0x4BFA0`) [INF: LuaWarp1] |
| **23** | 8 B | **EMEVD WarpPlayer relayed by the host** (`0x4C480`) |
| 24 | flag | event-flag set sync (`0x4F4D0`, `0x88F90`) |
| **25** | u32 | **block-clear / boss reward relayed** (`0x4BBB0`) |
| 26 | 4×u16 | own player stats (HP/FP…) for the overhead display (`0x52670`) |
| 27, 29 | u32 / … | misc (`0x52C30`, `0x5A4D0`) |

---

## 3. Q2: hooks by subsystem

The slot is the offset in the mod's address table. "Sekiro" is the exact unique pattern hit (§4)
where there is one. Handler RVAs are inside `ds3sc.dll`.

**Transport and params**

| Slot | Kind | Sekiro | Handler | What |
|---|---|---|---|---|
| 0x140 | E9 | – | 0x46C30 | per-tick pump (receive mod + game messages) |
| 0x148 | E9 | `0x1C14340` | 0x46C80 | send-to-member → Steam Messages |
| 0x150 | E9 | `0x8EA440` | 0x46CE0 | receive-from-member ← queues |
| 0x158 | detour | – | 0x46D30 | receive-from-any (type 57 across members) |
| 0x138 | detour | – | 0x46920 | init-time protection/patch helper [INF] |
| 0x160 | E8 | `0x6F4F09`→`0x114CFD0` | 0x46DF0 | after param load: synthesise rows (scaling, items) |
| 0x168/170/180 | E8 | 0x180→`0x10C74C0` | 0x49170/49440/495E0 | param-row lookup override (mod rows for tables 2–3, 14–15, 57–58) |
| 0x188 | detour | `0x2603620` (MsgRepo lookup) | 0x496B0 | mod item names/texts |

**Session role gating** (call sites → mod role bits; state bit0 = in session, bit1 = host,
bit2 = invader)

| Slot | Sekiro call site → callee | Replacement |
|---|---|---|
| 0x270 | `0x9186E2`, `0x918DCB` → `0x8E4730` ("in online session") inside the enemy item-lot drop code | "am I an invader" → **enemy drops stay on in co-op** |
| 0x268 | `0x69A726`, `0x69C1DF` → `0x68A9D0` (`S+0x184==6`, IsClient) in Lua-imitation events | "am I an invader" |
| 0x280 | `0x11C3847` → `0x8E4750` (members > 0) | invader or flag |
| 0x248–0x260 | (loose/many) | "am I an invader" |
| 0x1B0/0x278/0x2B8 | – | "not an invader" |
| 0x318 | `0xA76B62` → `0x832180` | true → **skip session leave on that load path** |
| 0x398–0x3B0 | `0x6D75F2`, `0x76E0B2`, `0x76FADB` → `0x832050` (GameMan+0xBA6) | "in session" |
| 0x300 | `0xA73D24` → `0x832050` | guest: false once, then true |
| 0x240 | E9 on `0x74B090` (compare `0x832050()` with the argument = native IsMyWorld) | IsMyWorld from event allow lists |
| 0x220 | – | native client-state query → mod role |

**Death, spectate, revive**

| Slot | Sekiro | Handler | What |
|---|---|---|---|
| 0x1B8, 0x1C0 | `0x697460`, `0x7A7700` (Lua-imitation death/return, events 4000/4050/4059) | no-op | **native "phantom return/death" handler disabled** |
| 0x1D8 | `0x695E00` (PlayerDeadEvent) | 0x4B900 | non-invader death: soul penalty (param 28), death-debuff SpEffect, menu reset |
| 0x1D0 | (loose) | 0x4B100 | death state machine: count, message 15, hold reload while someone is alive (spectate), revive flag, wipe → reload |
| 0x360 | (loose `0x5B40B1`→`0x6045E0`) | 0x51990 | "local player chr" for camera → spectated player's `PlayerIns` |
| 0x368 | `0x7EC8D4` → `0x7DCCE0` (chr position getter) | 0x51AE0 | position source → spectated player |
| 0x218 | `0xA2F40C` → `0x8D8370` (receive native packet **0x44** from member) | 0x4CCB0 | replaced by mod message 14 (revive: HP = max, sync) |

**World state (enemies, objects, flags, rest)**

| Slot | Sekiro | Handler | What |
|---|---|---|---|
| 0x378/0x380 | (loose) | 0x51E80/51F80 | allocate per-block bitsets: objects 8192 bit, chrs 2048 bit |
| 0x3C8 | – | 0x523F0 | enemy death → dead bit + **message 12** |
| 0x3D0 | – | 0x52530 | object broken → bit + **message 13** |
| EventChrEntity vt | – | 0x53570 | event-chr death → dead bit |
| 0x390 | (loose) | 0x521D0 | **block chr load: kill every chr whose bit is set** (unless NpcParam `+0x149` bit 3); re-dope |
| 0x388 | (loose) | 0x52090 | block object load: destroy broken objects |
| 0x3E8 | – | 0x527A0 | seed the dead set from the save `"CHR "` chunk (ver `0x17022200`) |
| 0x230 | – | 0x4E070 | rest/reset: host clears sets + sets WCM respawn flag (`+0x65C`) + **message 20**; guest sends **message 21** |
| 0x408 | – | 0x52AA0 | clear both sets before the native world reset |
| 0x2D0 | `0x6C4520` (SetEventFlag) | 0x4F8F0 | flag sync |
| 0x2B0, 0x2F8 | `0xB60B06`, `0x10CD64A` → `0x6C3E60` (GetEventFlag) | 0x4F3B0 | flag read override |
| 0x2E8, 0x2F0 | `0xD586A8`→`0x6AAAB0`, `0xD5831F`→`0x6AA860` (ESD/EMEVD flag wrappers) | 0x4F920/4F940 | same for scripts |
| 0x288 | – | 0x4E9C0 | joining member → **message 16** flag snapshot |
| 0x298, 0x2C0, 0x2C8 | – | 0x4EDA0, 0x4F4D0, 0x4F760 | EventFlagMan side of sync (message 24) |

**EMEVD / scripts / bosses**

| Hook | Sekiro | Handler | What |
|---|---|---|---|
| `EcTestMultiplayState` vt | – | 0x532C0 | states 2/3/5 → native; **0 (Host) and 4 → "in session"; 1 (Client) → false**; listed events → false |
| `EcTestIsMyWorld` vt | – | 0x52FC0 | true, except a guest needs the event on an allow list (two more lists gated by settings) |
| `EcTestChrType` vt | – | 0x53430 | listed events → false; event 13005893 forces type 0 |
| `EcTestChrSosInvadeType`, `ChrDead`, `SexType` | – | thin wrappers | – |
| `SprjEmkEventIns` vt | – | 0x52EB0 | **native event-sync receive**: maps the sender to entity `10001+slot` and stores it in the event instance (`+0x58`) |
| per-event callbacks | – | 0x88A70… | in events 13005890, 14000121/130, 14005101, 14105500/510, 20005640, 20005800, 20005840: an instruction aimed at player **10000** with fog-walk anims 60060/60070/60210/69040/69050/91040 is redirected to the triggering player |
| 0x200 | `0x68E7F0` (EMEVD WarpPlayer) | 0x4C480 | host runs it and relays (message 23); a guest ignores its own scripted warps |
| 0x1E0 | `0x69AFB0` (Lua-imitation, IsClient → `0x68E5C0(id)`) | 0x4BBB0 | relays (message 25) [INF: boss/block-clear reward] |
| 0x1F8 | `0x69A2D9` → `0x688320` (block-clear reward calc) | 0x4C0B0 | reward hook |
| 0x310 | (loose) | 0x4FBC0 | ESD/talk env queries: ids 0x67, 0xF0, 0xF1, 0x1C8 forced; plus mod ids |
| 0x3F0 | `0x67A143` → `0x89CAA0` | 0x52940 | "Pseudo multiplayer events are currently not available online" |

**Chr / team / scaling**

| Hook | Sekiro | Handler | What |
|---|---|---|---|
| 0x338 | `0x9F1510` (GetTeam) | 0x512A0 | **team override**: co-op players 1, invader 16, PvP 15/31 by world rule |
| 0x328 | – | 0x50FC0 | phantom kind per player (1 Wanderer / 2 Malcontent) from the mod roster |
| 0x210 | `0xA7AA82` → `0xA7A840` | 0x4CB40 | after native remote creation: remote `PlayerIns +0x110 = 0`, `+0x1A08 |= 1` [UNK fields] |
| 0x358 | `0x9E9A90` (MultiPlayCorrection) | 0x51660 | scaling SpEffect by NPC category × player count |
| 0x350 | – | 0x51530 | `_UpdateMultiDoping`: re-dope all chrs when the player count changes |

---

## 4. Q3: cross-map to Sekiro 1.06

`out/xmap.tsv` has all 150 patterns. **50 match Sekiro exactly** (the same bytes, wildcards
included), and **44 of those are unique**. That is strong evidence that DS3 1.15.2 and Sekiro 1.06 share these functions
almost verbatim. Loose matches (register bytes wildcarded) are noted only where they are unique.

| Mod slot | Sekiro RVA | Known in our docs? | Implication |
|---|---|---|---|
| send/recv member (0x148/0x150) | `0x1C14340`, `0x8EA440` | no | **lower-layer transport seams** below FSM `0x8E6FA0`/`0x8E6B50` |
| remote create (0x208/0x210) | `0xA7AA24`→`0x1BD9510`, `0xA7AA82`→`0xA7A840` | yes (SPEC §3) | the same chain as our native remote |
| packet 0x44 recv (0x218) | `0xA2F40C`→`0x8D8370` | no | native "remote player revived/respawned" packet (68, 4 B) |
| session-leave guard (0x318) | `0xA76B62`→`0x832180` | no | **where Sekiro disbands the session on load** |
| GameMan+0xBA6 sites (0x300, 0x398–0x3B0) | `0xA73D24`, `0x6D75F2`, `0x76E0B2`, `0x76FADB` | `0x832050` yes (SPEC §1d) | it is "in another world"; `0x74B090` is native IsMyWorld |
| IsMyWorld (0x240) | `0x74B090` | no | EMEVD IsMyWorld implementation |
| online check in drops (0x270) | `0x9186E2`, `0x918DCB` → `0x8E4730` | `0x918090` yes (drops need `0x8E4730()==0`) | **in a native session drops are off unless these sites are flipped** |
| IsClient (0x268) | `0x69A726`, `0x69C1DF`→`0x68A9D0` | type helpers yes (SPEC §5) | Lua-imitation event gating |
| death/return handlers (0x1B8/0x1C0/0x1D8) | `0x697460`, `0x7A7700`, `0x695E00` | `0x69D310` family yes | **the native phantom death/return C++ events** |
| block clear (0x1E0, 0x1F8) | `0x69AFB0`, `0x69A2D9`→`0x688320` | no | boss/area reward for clients |
| EMEVD WarpPlayer (0x200) | `0x68E7F0` | yes (spectate notes) | host-relayed warps |
| flags (0x2B0/0x2D0/0x2E8/0x2F0/0x2F8) | `0x6C3E60`, `0x6C4520`, `0x6AAAB0`, `0x6AA860` | yes | the same choke points we use |
| GetTeam (0x338) | `0x9F1510` | yes (RE-ENEMY-TARGETING) | team override point |
| MultiPlayCorrection (0x358) | `0x9E9A90` | yes (SPEC-DEBUFF-SCALING) | **the DS3 scaling hook point exists in Sekiro** |
| spectate pos (0x368) | `0x7EC8D4`→`0x7DCCE0` | no | camera/physics position source |
| calls the mod makes | `0x6D5B20` (GetChrInsByEntityId), `0x6C3610`/`0x6C4240` (flag apply / IsFlagNetSynced), `0xA4D0A0` ("CHR " writer), `0x8DAE90` (join info 8/60), `0xA25650` (soul reward calc), `0x1072030`, `0x8E65E0`, `0xEA2690` | partly | the same toolbox |
| globals | `0x3D55600` EventMan, `0x3D55FE8` EventFlagMan, `0x3D67C90` FSM, `0x3D94970`, `0x3D56058`, `0x3E9F748`, `0x3D8A140` NewMenuSystem, `0x3F0FB80`, `0x298E880` | mostly yes | confirms singleton set |

The DS3-only hooks did not match: `_UpdateMultiDoping`, the dead-set block loads, the param lookups,
and the EcTest vtables, which are reached through RTTI. The EcTest classes need RTTI lookups in
Sekiro.

---

## 5. Q4: enemy AI authority

- **Distribution: untouched.** [DIS, high] No hook, patch or call reaches the DS3 equivalents of the
  ownership code. The 22/23-style traffic is just one more queued packet type in the tunnel.
  Distance-priority distribution, timers and the host arbiter are therefore native.
- **Frozen or ownerless enemies.** [INF] There is no repair logic in the DLL. What the mod protects
  instead is the session itself, so the native arbiter keeps working:
  - it neutralises the session-leave paths on loads (0x318);
  - it keeps the role answers stable;
  - it keeps remote `PlayerIns` alive.

  The DS2 changelog lines ("enemies freeze after someone disconnected") have no DS3 counterpart in
  this build. In DS3 the native code already reassigns on the next 23 after a member leaves, provided
  the session survives. **To verify with the DS3 binary.** [UNK]
- **A player who dies, loads or disconnects.** [DIS]
  - Death no longer removes the player from the world (spectate), so their PlayerIns and their
  share of the distribution stay.
  - A disconnect goes through the member-drop handler (`0x8CCC0`, messages 10/11).
  - Enemy **death** is decoupled from the owner: whoever kills it broadcasts message 12, and every
    machine kills that netId at its next block load. A NetAI copy whose owner left therefore cannot
    "come back to life".
- **Hand-over.** Nothing is added; it is native.

## 6. Q5: bosses

- **Fog walls / boss start.** [DIS]
  - MultiplayState is "Host" everywhere, so the host-version fog logic runs on every machine.
  - IsMyWorld and ChrType answer from allow lists.
  - Fog-traversal instructions are retargeted to the triggering player (the per-event callbacks plus
    the EmkEventIns sender mapping). Any member can open the fog and start the fight for everyone,
    and only that member walks through.
- **Authority.** No `SetNetworkUpdateAuthority` call and no change to it. [DIS for "none found"]
  - [INF] In DS3 the Forced call sits behind IsMyWorld/own-world conditions in the boss events.
    With IsMyWorld forced, every member may execute Forced locally.
  - Which machine really simulates the boss under DS3SC is **[UNK]**: it needs the DS3 2004[28]
    handler.
- **Spectate after death:** [DIS] the reload is held while another player lives, and the camera and
  position follow the spectated `PlayerIns`.
- **Wipe:** [DIS] the native reload runs once nobody is alive.
- **Revive:** [DIS] replaces native packet 0x44 with message 14 (HP to max), triggered by a pending
  flag set by the state machine. [INF] It fires at boss end or rest.
- **Reward:** [DIS + INF] the block-clear (boss defeat) event is relayed to all (message 25), and the
  reward path is hooked (0x1F8).

## 7. Q6/Q7: targeting, scaling, rest, flags

- **Targeting** [DIS]: the mod only rewrites teams (GetTeam `0x9F1510`: co-op 1, invader 16). AI
  target selection is native over native remote PlayerIns. There is no aggro or "lose interest" logic.
- **Scaling** [DIS]: at param load, SpEffect row 140201 is cloned into 15 rows (ids from `0x7FFF0000`):
  3 categories × 1–5 extra players.
  - The HP rate field (`+0x10`) is `1 + n·health%`; the attack-rate fields (`+0x38…+0x44`, `+0x1E4`)
    are `1 + n·damage%`.
  - The MultiPlayCorrection hook picks a category from NpcParam (`+0x1D0`/100) and applies the row
    for the current count. A count of 0 removes it.
  - The `_UpdateMultiDoping` hook re-runs this when the count changes.
  - Where the posture rate is written was not traced. [UNK]
- **Rest** [DIS]: a host rest clears the dead sets, sets the WCM respawn flag and broadcasts message 20.
  A guest rest asks the host (message 21).
- **Flags** [DIS]:
  - SetEventFlag and the script wrappers are hooked; the game's `IsFlagNetSynced` (`0x6C4240`) is
    called.
  - A joiner gets a full flag snapshot (message 16).
  - The dead set is seeded from the save's `"CHR "` chunk.
  - Lists in `out/eventlists.txt`: a 171-flag list (bonfire/NPC/quest flags 70000xxx, 6020–6030,
    9300–9322), most likely flags that are never synced, and 12 per-event ids.

---

## 8. Q8: ranked transfers to Sekiro, and what needs the DS3 binary

**Transfer, by value per effort:**

1. **Dead-enemy registry keyed by netId** (messages 12/13): broadcast, apply on block load, seed from
   `"CHR "`, clear on rest. Our netId encoding is identical (`block<<11 | idx`), and our
   `0xA4D0A0` writer is the same chunk. This is the single best fix for "dead here, alive there"
   after reloads.
2. **Guard the session-leave path on loads:** Sekiro `0xA76B62 → 0x832180` (exact hit). It is needed
   whenever we lean on the native session (SPEC-NATIVE-SESSION). Persistence is what keeps the native
   arbiter alive.
3. **Enemy drops in session:** flip `0x8E4730` at `0x9186E2`/`0x918DCB` (exact hits) if we ever enter
   a native session; otherwise `0x918090` refuses drops.
4. **Death → spectate state machine:** hold the reload while a partner lives. Redirect the camera
   position source (`0x7EC8D4 → 0x7DCCE0`) and the camera's player getter. Disable the native
   phantom death/return C++ events (`0x697460`, `0x7A7700`, `0x695E00`). Revive by message.
5. **Host-relayed scripted warps** (`0x68E7F0`) and block-clear rewards (`0x69AFB0`, `0x688320`).
6. **Scaling via cloned SpEffect rows + MultiPlayCorrection `0x9E9A90`.** Sekiro has the same hook
   point (SPEC-DEBUFF-SCALING). It replaces our planned `0xC04EA4` multiplier patch with per-category
   rows.
7. **Team override at `0x9F1510`** for PvP/friendly-fire rules.
8. **Transport one layer lower** (`0x1C14340`/`0x8EA440`): an alternative seam for our native mode,
   with per-type queues and a reliable set.
9. **EMEVD**:
   - MultiplayState/IsMyWorld answers and fog-walk retargeting are irrelevant while we run outside
     the native session (scripts already see single-player);
   - they become mandatory if we move inside it.

**Needs the DS3 binary:**
- Which machine simulates a boss under DS3SC: does the Forced authority run on every member, and
  does the 2004[28] handler compare player slots?
- The real identity of the 13 unmatched DS3-only hook sites: `0x1A0`, `0x228`, `0x330`, `0x348`,
  `0x370`, `0x3F8`, `0x418` and others.
- What native packet 57 is, and which types the reliable/drop sets contain.
- The remote `PlayerIns +0x110`/`+0x1A08` writes after creation.
- Whether DS3's native distribution really reassigns cleanly after a member drop, which is our
  frozen-enemy question.
- Where the posture scaling is written.
