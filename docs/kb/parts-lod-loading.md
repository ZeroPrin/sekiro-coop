# Player part loading: full vs `_L`, texture `_l` fallback, missing files, face-set LODs (2026-11-15)

Read-only RE on `sekiro.exe` 1.06 (carved image `E:\RE\carve\sekiro_mem.exe` from the 2026-11-11 dump). Ghidra
project copy `E:\RE\ghidra_lod\sekiro_quick` (copied from `ghidra_fx2`, functions created and decompiled on
demand). Scripts and outputs: `E:\RE\work_lod\` (`run.py` + `g.ps1` runner, `j_dec.py` / `j_dis.py` / `j_ctx.py`
jobs, `xref.py` / `refs.py` byte-scan xrefs, `rtti.py`, `fstr.py` strings used by a function, `vtscan.py`;
decompiled outputs `d*.txt`, disassembly `s*.txt`, context `c*.txt`; data tools `py\facesets.py`, `py\texdup.py`,
`py\bndls.py`; `facesets_vanilla.txt`). All RVAs are relative to `0x140000000`.

Tags: **[VAR]** read in disassembly/decompilation (or in game data) this pass; **[ARV]** inferred from code that
was read; **[LIVE]** seen in a mod log. Nothing here was run in game.

Dump caveat: none of the functions used below is detoured in the dump (prologues checked: `0xA361C0`,
`0xE7B000`, `0xE7CAB0`, `0x10C8600`, `0x10D1CE0`, `0x1091010`, `0xF5BF40`, `0xF78780`, `0xF728A0`, `0xECFFC0`,
`0xFC2D80`, `0xFC3030`). The call at `0xA363BC` is still the vanilla `E8 3F 4C 44 00` (the dump predates the
`native_full_parts` hook).

## 0. Answers in one table

| Q | Answer | Confidence |
|---|---|---|
| 1 | Part names are built in `0x10C8600` (protectors) and `0x10D15A0` (weapons): prefix + `M_`/`F_`/`A_` + `%04d` + `_L` when the per-character flag `low` is set. `low` comes from the PlayerIns ChrAsm builder `0xA361C0`: `vt[0x7C8](chr) \|\| (chr is not the main player && chrTypeTable[type].byte1 == 0)`. One flag per character (stored at ChrAsmModel+0x2B), applied to all its parts. No param field, no distance, no quality option. Re-evaluated every frame; a change reloads every part | VAR |
| 2 | The `_l` texture suffix is not tied to the `low` flag. For every FLVER material texture the GX layer (`0xF5BF40`, `0xF78780`) looks up the name `X` and also `X + "_l"`, wraps both in `SprjMiddleGxTexture`, and that object draws `X` whenever `X` is resident, else `X_l` (`0xF728A0` & co.). A `_L` model therefore shows full textures whenever the full texture set happens to be loaded | VAR (lookup and the wrapper), ARV (which of the two bindings the draw uses) |
| 3 | Protector parts: the loader checks existence first (`0xE7CAB0`: already in `PartsbndRepository`, else a file-exists query on `parts:/X.partsbnd.dcx`). A missing `_L` file fails both tries, the slot gets no resource and the part is not drawn. There is no fallback to the full file and no load request, so no stall. The vanilla "retry" only clears an argument that the name builder ignores. Weapons (`WP_`) have no existence check | VAR (code path), ARV (missing slot = invisible part) |
| 4 | Every vanilla part mesh has face sets LOD0, LOD1 (`0x01000000`), and in most cases LOD2 (`0x02000000`), plus the same three with `0x80000000`. LOD2 is missing only on tiny meshes (chains, fray, small cloth), and LOD1 is never missing. The face-set selection itself (GX, `LodQuality` graphics-config key) was not traced. A FLVER with only LOD0 is not vanilla-like: by analogy with the vanilla LOD2 omissions, such meshes most likely vanish at LOD1 distance | VAR (data), ARV (render behaviour) |
| 5 | `native_full_parts` (default 1, read live): the `call 0xE7B000` at `0xA363BC` in `0xA361C0` goes to `build_hook`, which passes `low = 0` for a friend's replica ChrAsmModel. It covers only PlayerIns built by `0xA361C0` whose model is a registered replica. It does not cover the ReplayGhostIns fallback puppet. It does not need to cover the local player (always full), cutscenes (`0xECFFC0`, always full) or the Kick/Sign menu portraits (`low = 1` there, not used by the mod). Hook installation is byte-checked and has been seen working live | VAR, LIVE (test9 log) |
| 6 | Recommended: ship only full files for custom ids and add a 5-byte engine patch at `0xE7B13B`, so that the existing (dead) retry asks for the name without `_L`. Vanilla parts keep their `_L` behaviour, and any character that asks for a missing `_L` gets the full file. Keep `native_full_parts`. Section 6 has the details | design |

## 1. Where the name is built and what decides `_L`

### 1.1 Call chain [VAR]

`PlayerIns` per-frame update (`0xA30A50`, `0xA37480`, `0xA39030`, `0xA38A60`; first build in `0xA2BF20`)
-> `0xA361C0(chr)` -> `0xE7B000(asm = chr+0x2148, holder = chr+0x2140, face = PGD+0x740, a4 gender, a5, a6 low, a7, a8 hideAll = chr+0x2267, a9)`
-> per slot: protectors `0xE7CAB0` -> thunk `0x10C99D0` -> `0x10C8600`; weapons `0x10D1CE0` -> `0x10D15A0`.

`0xE7B000` is cheap when nothing changed (the 2026-11-12 logs show two calls per replica per frame). It runs only
when `asm+8 < 1 && asm+0xC < 1 && asm+0x10 == 0`. It stores `a4 -> +0x2A`, `a6 (low) -> +0x2B`,
`a7 -> +0x2C`. If any of the three changed, it sets `+0x28 = 1`, which forces every one of the 33 slots to rebuild.
It then compares part ids per slot (`slot+0x30` current, `+0x34` wanted) and requests loads (`asm+8 = 2`,
for protectors also `asm+0xC = 2`).

### 1.2 Name format [VAR]

`0x10C8600(out, EquipParamProtector row, gender a4, low)`:

- prefix = table `0x143B16100[row+0xD0]`: 0 `AS_`, 1 `AM_`, 2 `BD_`, 3 `FC_`, 4 `HR_`, 5 `HD_`, 6 `LG_`, 7 `WP_`;
- row `+0xD1`: 0 -> `A_`; 1..3 -> `M_` if `a4` else `F_`; 4 -> `M_`;
- `%04d` of `equipModelId` (u16 at row `+0xA0`, the field the Reed patch writes);
- `_L` (UTF-16 string `0x142B99380`) if `low`.

The 5th argument (`a7`, forwarded by the thunk `0x10C99D0`) is not read by `0x10C8600`. `0xE7CAB0` then
formats `%s.partsbnd.dcx` (`0x142B2F7F0`) and `parts:/%s.partsbnd.dcx` (`0x142B2F810`), and on success returns
`parts:/%s.partsbnd` (`0x142A5A580`). The weapon builder `0x10D15A0` is the same with row `+0xE0/+0xE1/+0xB8`
(+ an id offset) and the same `_L`.

No EquipParamProtector field selects the low model. The only param bytes read are `+0xA0`, `+0xD0` and `+0xD1`.

### 1.3 The `low` flag (6th argument) in `0xA361C0` [VAR]

```
low = vt[0x7C8](chr)                                  // PlayerIns 0xA2AE70: return 0
      || (!is_main_player(chr)                         // 0x9F3430: chr == [WorldChrMan 0x143D7A1E0]+0x88
          && chrTypeTable[chr+0x70].byte1 == 0)        // 0xBF1B90: byte at 0x143B0A3B1 + 0x14*type, type < 14
```

`chrTypeTable[t].byte1`: types 3, 4, 10, 11 = 1 (full detail); 0, 1, 2, 5, 6, 7, 8, 9, 12, 13 = 0 (low). The
mod's replica (PlayerIns type 1) is therefore low in vanilla.

`ReplayGhostIns` (remnants, and the mod's ghost fallback puppet) overrides `vt[0x7C8]` with `0xA39780`. It returns
true for type 11, otherwise a fade value test (`+0x22A8 < 0`, or `<= 0` when `+0x22A4 != 0`). So remnants are
low.

### 1.4 Per character, runtime, distance

- **Per character:** one byte per ChrAsmModel (`+0x2B`), passed to every protector and weapon name of that
  character [VAR].
- **Runtime:** re-evaluated on every call. A changed flag sets `+0x28`, and all slots reload with the new names
  [VAR]. Type and locality rarely change; a ReplayGhostIns can flip through its fade value [ARV].
- **Not distance, not graphics settings, not session role as such.** The inputs are chr type, "is the main
  player" and the vtable hook [VAR]. Session role matters only through the chr type the session gives the
  character.

### 1.5 Other callers of `0xE7B000` (other model builders) [VAR]

| caller | what | `low` passed |
|---|---|---|
| `0xA361C0` | every PlayerIns (local player, replicas, ReplayGhostIns) | rule 1.3 |
| `0xECFFC0` (`0xED004E`, `0xED00AD`) | `SprjRemoRes` cutscene actors (step names `SprjRemoRes::STEP_*`, `0x142B3FE68`): the cutscene's player model and the PGD-based actors | constant 0 (`[rsp+0x28] = r14b`) |
| `0xFC2D80` / `0xFC3030` | `CSMenuAsmModelRend` (`STEP_Wait_Play` / `STEP_Wait_Setup`): flags at `+0x288..+0x28B` / `+0x420..+0x423` (gender, a5, **low**, a7), copied from the request block `+0x5B8..+0x5BB` | ctor default 0 (dword `+0x288`/`+0x420` = 1 = gender only). The setter `0xFC29B0` (`mov [rcx+0x5BA], dl`) is called with 1 only from the **Kick** menu (`0xD9F2E0`, `SYSTEX_Menu_KickFace%02d`) and the **Sign** menu (`0xDE5810`, `SYSTEX_Menu_SignFace`). The ChrMake previews (`0xCA6660`, `0xD70D60`) keep 0 |

Sekiro has no 3D player model on the status or equipment screens [ARV]. The CharaInitParam-based cutscene
costumes (e.g. 15010, the Sculptor ending) are cutscene actors, so they go through `0xECFFC0` with `low = 0`
[ARV: the CharaInit -> Remo link was not traced].

`0xA7FC80` formats `parts:/%s%04d_L.partsbnd.dcx` for a hard-coded preload list (`0xA7D6B0`: `AM_M_` 9220,
9260, 9270, … ids that are not in Sekiro's archives). This is a leftover, irrelevant to Sekiro parts [ARV].

## 2. The `_l` texture suffix

### 2.1 Lookup at FLVER material setup: `0xF5BF40` (GXFLV) [VAR]

For each texture of a material (name = base name of the FLVER path, or of the MTD sampler path when the FLVER path
is empty), the function fills a 0x28-byte record:

1. `[0] = find_or_create(TexRepo 0x143D90038, X)` (`0xF76DD0` -> `0xF76CD0`, a placeholder entry when absent).
2. If `0xF434E0([0])` is false (that is, `X` is not one of the `other:/SysTex.tpf` textures, list at
   `0x143D8ED78`), then `[1] = find_or_create(X + "_l")` (the string `_l` at `0x142B56228`, appended at
   `0xF5C429`). Otherwise `[1] = [0]`.
3. `[2] = GX handle of [1]` (`0xF786A0`).
4. `[3] = 0xF78780([0])`: `find(X + "_l")` (find only, `0xF75E70`, `_l` appended at `0xF787E6`), then
   `SprjMiddleGxTexture(primary = X, secondary = X_l or X itself)` (`0xF72510`, vtable `0x142B5CAB8`), held by a
   `GXFlverTexture` (`0x1429A97E0`).

### 2.2 Choice at draw time [VAR]

Every `SprjMiddleGxTexture` method (`0xF728A0`, `0xF72920`, `0xF72680`, `0xF728E0`, `0xF72750`, `0xF72860`,
`0xF72960`, `0xF727D0`, `0xF726E0`, `0xF72820`) calls `primary->vt[0x68]()` (ready/valid). If it returns true, the
call goes to the primary (full `X`), otherwise to the secondary (`X_l`).

### 2.3 Consequences

- Nothing in this path knows about ChrAsmModel+0x2B. It is generic for every FLVER: "draw `X`; while `X` is not
  resident, draw `X_l` if that exists" [VAR].
- Vanilla `_L` partsbnd: the FLVER is byte-identical to the full one in 38 of the 39 pairs with a FLVER in
  `orig\parts` (SHA-1 check). The exception is `hd_m_9120`, whose `_L` FLVER names `HD_M_9120_a_l` explicitly.
  `fc_m_0000` has no FLVER. Its TPF holds only `X_l`. The model draws `X_l`, unless the full TPF
  is loaded by someone else (for example the local player wearing the same part), in which case it draws the
  full `X` [VAR mechanism; ARV that the record's `[3]` is the material binding (a full model's `[1]` is an empty
  `X_l` placeholder, so `[2]` cannot be the normal binding)].
- **What a full model in the `_L` slot must contain:** a FLVER (its entry name does not matter for lookup; the
  PartsbndResCap registers each child by its own entry name and keeps typed slots, `0x1091FE0` [ARV]) and a TPF that
  holds either `X_l` names (vanilla style, any resolution) or full `X` names. Full names collide with the full
  file's TPF by name; vanilla never ships the same texture name in a full and an `_l` file (`py\texdup.py`: 22
  names shared only between different part ids, e.g. `bd_m_9500`/`bd_m_9530`).
- **Explicit FLVER paths (Reed):** with a byte-identical `_L` FLVER that names `X`, the `_l` TPF only needs
  `X_l`, because the engine adds `_l` itself. The current Reed `_L` FLVERs name `X_l` explicitly. That also works
  (`[0] = X_l`, secondary not found, so it falls back to itself), but then the replica never upgrades to the
  full `X` when it is resident.

## 3. A missing `_L` file

### 3.1 Protectors (BD/AM/LG/FC/HD) [VAR]

Load step of `0xE7B000` (the `asm+0xC` countdown branch, slots 1..4, at `0xE7B106`-`0xE7B153`):

```
ok = E7CAB0(asm, &path, gender=+0x2A, low=+0x2B, a7=+0x2C, id)     // 0xE7B126
if (!ok) ok = E7CAB0(asm, &path, gender, low, a7=0, id)            // 0xE7B14C, retry
if (ok) slot+0x40 = request(0x143D59318 resource man, path)        // 0x140728060
else    slot+0x40 = 0                                               // 0xE7B1A6 path
```

`0xE7CAB0` returns true only if `X(_L).partsbnd.dcx` is already registered in `PartsbndRepositoryImp`
(`0x143D95F18`, map at `+0x98`, `0x1091010`; filled by `PartsbndFileCap` `0x706830`), or if a file-exists
query on `parts:/X(_L).partsbnd.dcx` succeeds (`0x1A0D110` file operator with `[0x143F08938]`, `0x1A0D2D0`
-> device `vt+0x98`/`+0x118`).

The retry is dead code in 1.06: it changes only `a7`, which the name builder ignores (1.2). A missing `_L` file
therefore gives `slot+0x40 = 0`. When the countdown swaps it into `+0x38`, the part has no resource: nothing is
requested from disk (no stall), and the part is not drawn [ARV: an empty slot means no model]. There is no
fallback to the full file.

me3: a missing `_L` name exists neither in the package mapping nor in the archives, so the check fails as
described. A new full name that exists only in the package is found only if this exists query goes through the
device path me3 hooks [ARV; Reed's new names in game would show it].

### 3.2 Weapons (`WP_`)

The weapon paths (`0x10D1CE0` -> `FUN_140728060`) request `parts:/%s.partsbnd` with no existence check [VAR]. A
missing file reaches the resource manager. Not analysed (custom weapons are out of scope).

## 4. FLVER face-set LODs

Data [VAR] (`py\facesets.py` over all 78 vanilla part FLVERs in `E:\RE\work_parts_patch\orig\parts`,
`facesets_vanilla.txt`): FLVER 0x2001A, per mesh face-set flags `0x0`, `0x01000000`, `0x02000000`,
`0x80000000`, `0x81000000`, `0x82000000` (SoulsFormats: LodLevel1, LodLevel2, bit 31 = "MotionBlur"/alternate set).
Every mesh of every part has LOD0, LOD1 and the `0x8…` copies. LOD2 is missing on only 13 tiny meshes (arm chains of
384 faces, fray/frary cloth of 6-222 faces, `bodyhair` with 6 faces). The `_L` FLVERs are identical, so they have
the same LODs. Reed's `bd_m_9045` keeps the vanilla set (27/27/26).

Selection at render time: not traced. The GX layer has a `LodQuality` graphics-config key (`0x142BD1218` in the
GRAPHICSCONFIG table `0x143B1D710`), `g_GX_DisplayableMaxLodLevel` and debug "LOD Dist Scale" [VAR strings].
Distance or screen size plus that setting drive it [ARV]. Since vanilla drops LOD2 only on meshes that are meant
to disappear when far, a mesh without a LOD-n face set is most likely simply not drawn at LOD n [ARV]. **Author
custom parts with LOD1 and LOD2 face sets** (copies of LOD0 are fine, as in vanilla small meshes where counts are
equal) **and the `0x80000000` copies.** Do not ship LOD0-only FLVERs.

## 5. The mod's `native_full_parts`

`crates/coop-dll/src/remotefx.rs`, key `[DEBUG] native_full_parts` (default 1, re-read live every 2 s,
`settings.rs:480`) [VAR source]:

- **Patch:** the rel32 of `call 0xE7B000` at `0xA363BC` (inside `0xA361C0`) goes to a near stub
  `mov rax, build_hook; jmp rax`. It is installed once (`BUILD_TRIED`), lazily: on the first `remotefx::creating(true)`
  around native.rs's player factory, or on the first `tick` while the key is on. It is installed only if the
  bytes are `E8 rel32` to `0xE7B000`; otherwise a log line and nothing is written.
- **Logic:** `build_hook` replaces `a6` (low) with 0 when `is_replica_asm(asm)`: not the local player's model
  (`LOCAL_ASM`) and (inside the factory call `CREATING`, or the asm is `*(replica+0x2148)` of a peer's native
  chr, or of a chr just created by native.rs, `CREATED`, with a TTL of 600 ticks). Then it calls the original. It
  logs real part loads (first 40). With the key off, it passes the arguments through.
- **Reliability:** high for the case it targets. The flag is applied before `0xE7B000` stores `+0x2B`, so names,
  the comparison and the reload are all consistent. Live in test9 (2026-11-14): "hook installed", then "friend's
  model … loads parts full-detail (the game asked for _L)" [LIVE].
- **Coverage:**

| character / view | builder | vanilla `low` | with `native_full_parts = 1` |
|---|---|---|---|
| local player, any state, wardrobe reload included | `0xA361C0` | 0 | 0 (excluded by address anyway) |
| friend's native replica (PlayerIns type 1), creation and re-dress (loadout keeps the ChrAsmModel) | `0xA361C0` | 1 | **0** |
| ghost fallback puppet (`ghost::spawn_plain`, ReplayGhostIns; only while native is off or failing) | `0xA361C0` | `0xA39780` or type rule, low in practice [ARV] | **unchanged, not covered** (its asm is not registered) |
| vanilla remnants (ReplayGhostIns) | `0xA361C0` | 1 | unchanged |
| cutscene actors, CharaInitParam costumes such as 15010 | `0xECFFC0` | 0 | 0 (not hooked, not needed) |
| menu models: Kick / Sign portraits | `0xFC2D80`/`0xFC3030` | 1 | unchanged (FromSoft online menus) |
| menu models: ChrMake previews | same | 0 | 0 |

So for "everyone sees one full model", the hook alone works only while the setting is 1, the patch is installed,
and the friend is shown natively. In every other case `_L` is asked for, and with no `_L` file the part is
**invisible** (section 3).

## 6. Recommendation

| option | what must be true | what breaks / costs | verdict |
|---|---|---|---|
| (a) rely on `native_full_parts`, ship no `_l` | hook installed, key = 1, friend shown natively | ghost fallback, `native_full_parts = 0`, a failed byte check and remnants all ask for `_L` and get an **invisible part** (3.1) | not enough alone |
| (b) ship `_l` files = full content | `_l` TPF holds full-res pixels under `X_l` names (keep the FLVER byte-identical to the full one, so the engine adds `_l`; no explicit `_l` paths needed), `_L` entry names for FLVER/TPF/clm2/hkx as vanilla does | doubles disk and memory for that character, and keeps the "duplicate content" chore. Full-name textures or a byte copy of the full file would put same-named resources (TPF, clm2, hkx, textures) into two partsbnds at once, which vanilla never does [ARV risk] | works without code, but it is the workaround the user wants to avoid |
| (c) **engine fallback "no `_L` file -> full file"** | 5-byte patch at `0xE7B13B` in the dead retry: `44 0F B6 4F 2B` (`movzx r9d, byte [rdi+0x2B]`) -> `45 31 C9 90 90` (`xor r9d, r9d; nop; nop`). Byte-check before writing, like the existing hooks | only protectors (weapons have no existence check, 3.2). Vanilla is unchanged: every vanilla part has its `_L`, so the first try succeeds, and the retry only ever ran when the first try failed | **recommended** |
| (d) per-id param switch | none exists: the name builder reads only `+0xA0/+0xD0/+0xD1`; `low` is per character | n/a | not available |

**Recommended setup:** (c) + keep (a). Custom outfits ship only `bd_m_XXXX`, `am_m_XXXX`, `lg_m_XXXX` and
`fc_m_XXXX` (full detail, all LOD face sets, section 4). Each character that asks for `XXXX_L` (vanilla-low
replica, ghost fallback, `native_full_parts = 0`, menus) now gets the full file. If that file is already loaded
for another character, `PartsbndRepository` shares it, which is how vanilla full-detail players of types
3/4/10/11 work. The ChrAsmModel's `+0x2B` stays 1, but nothing else reads it for protectors (the texture fallback
is name-based, section 2).

- Offer a custom outfit only while the patch is installed (the same gate as Reed's file check). Without the
  patch and without `_l` files, a low-detail viewer gets an invisible part.
- With (c) in place, Reed's eight files can drop to four (no `_l`), and the `_L` FLVER path rewriting in
  `SPEC.md` 1.4 is no longer needed.
- The existing wardrobe parts package (patched vanilla ids that have their `_l`) is unaffected.

**To verify in game (solo first, then co-op):**

1. Custom part with no `_l` file, friend's replica with `native_full_parts = 0`: the part is visible and
   full-detail (proves the fallback). Before the patch, the same setup should show the part missing (proves
   3.1).
2. The same with `native_full_parts = 1`: no change in behaviour, and no extra loads in the `fx: parts:` lines.
3. Vanilla outfit on the replica with `native_full_parts = 0`: still the `_L` look (the patch did not change
   vanilla).
4. Ghost fallback puppet (force native off) wearing the custom outfit: visible.
5. Me3 package with a new full name only (no archive file): the existence check passes (the part appears).
6. Re-dress the replica, warp at an idol, sit through a cutscene and a menu while wearing it: no stall, no
   invisible part.
7. Distance: walk away from the replica until LOD1/LOD2 kick in. No mesh pops out, which checks that the face
   sets of section 4 are present.
