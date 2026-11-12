# База знаний: выбор цели врагами (AI targeting) в Sekiro, DS3 и Elden Ring

Дата сборки: 2026-11-12. Цель документа: объяснить, почему в Sekiro Together враги и боссы «прилипают» к одному игроку, и показать, какими рычагами можно сделать смену цели умнее. Документ охватывает Sekiro 1.06 (SDT), Dark Souls 3 (DS3) и Elden Ring (ER).

## Как читать

Каждое утверждение помечено одним из тегов:

- **[S]**: прямо в источнике. Ссылка вида `ALIAS:path:line(-line)`, алиасы расшифрованы в таблице источников. Для git-репозиториев алиас привязан к коммиту.
- **[S-RE]**: прямо в декомпилированном коде `sekiro.exe` 1.06, но взято из нашего прежнего RE-документа (IDA 9.3, 2026-10-23), а не перепроверено здесь.
- **[I]**: мой вывод. Рассуждение приведено рядом.
- **[C]**: утверждение сообщества, кодом не подтверждено.

Всё, что я искал и не нашёл, собрано в конце в разделе «Unverified / не найдено».

## Источники

| Алиас | Что это | Commit / версия (дата) | Лицензия | Что читалось |
|---|---|---|---|---|
| `SDT-AI` | AI-скрипты Sekiro 1.06. Все 12 файлов `script/*.luabnd.dcx` извлечены `SekiroTool extract` из `Data*.bdt` в `E:\RE\work_ai\raw\` (только чтение, игра не тронута). Распаковка: `SekiroTool undcx` и WitchyBND 3.0.1.0 (`E:\RE\work_ai\bnd\`). Декомпиляция: DSLuaDecompiler katalash@c27340ab1b898a584748dc7e43a516cc1f1684f6, получилось 673 файла в `E:\RE\work_ai\dec\<bnd>\*.lua`. Номера строк относятся к выводу декомпилятора. | Sekiro 1.06 (Steam) | собственность FromSoftware | `aicommon-luabnd/{ai_define,common_logic_func,table_ai_common,clear_target,non_battle_act,after_attack_act}.lua`, `m11_01_00_00-luabnd/{710000_logic,710000_battle}.lua`, `m10_00_00_00-luabnd/105000_battle.lua`. Частоты вызовов API по всем 673 файлам сведены в `E:\RE\work_ai\api_counts.txt`. |
| `CSV` | Параметры Sekiro 1.06 в CSV, выгружены SekiroTool с именами строк из Paramdex | `E:\RE\sekiro-data\params\csv\` | собственность FromSoftware | `NpcThinkParam.csv` (771 строка), `SpEffectParam.csv` (3096 строк) |
| `EVT` | EMEVD Sekiro 1.06, декомпиляция DarkScript3 3.6.3 | `E:\RE\sekiro-data\event\js\` | собственность FromSoftware | grep по `ForceCharacterTarget`, `ClearCharactersAITarget` |
| `EXE` | Вырезанный из памяти образ `sekiro.exe` 1.06. Смещения в таблице файловые, это не RVA. | `E:\RE\carve\sekiro_mem.exe` | собственность FromSoftware | только поиск строк: имена Lua-биндингов (`E:\RE\work_ai\exe_lua_bindings.txt`) и RTTI |
| `PDX` | soulsmods/Paramdex, https://github.com/soulsmods/Paramdex | ff7245e524329bc3eab00036723d2bd53384cedf (2026-03-06) | файла лицензии нет | `SDT/Defs/{NpcThinkParam,NpcParam,SpEffect}.xml`, `ER/Defs/{NpcThinkParam,NpcParam,SpEffect}.xml`, `DS3/Defs/{NPC_THINK_PARAM_ST,SP_EFFECT_PARAM_ST}.xml`. SDT и ER содержат оригинальные японские описания полей; DS3-деф содержит только имена, описаний в нём нет. Клон: `E:\RE\kb-sources\ds3\Paramdex`. Клон `E:\RE\kb-sources\er\Paramdex` неполный, его не использовал. |
| `FRS` | vswarte/fromsoftware-rs, https://github.com/vswarte/fromsoftware-rs | 57023153d786c4b13fe95d9170e6dcd9d35093ea (2026-10-24) | MIT-NON-AI / Apache-2.0-NON-AI (нельзя использовать для обучения ML) | `crates/eldenring/src/cs/{targeting,chr_ins,team_relation,character_type_properties}.rs`, `crates/sekiro/src/sprj/*` (AI-структур в крейте Sekiro нет) |
| `RET` | Наш RE-документ: `sekiro-coop@47b77e46285def3256b3348d47d9ab2fd987391e:docs/RE-ENEMY-TARGETING.md` | 2026-10-28 | репо проекта | разделы a и b: цепочка `SprjTargetingSystem`, список осведомлённости, частота обновления AI |
| `IMPL` | `sekiro-coop@4e0588146762501d8995ed62d3115e3509ab7f8e:docs/IMPLEMENTATION-NOTES.md` | 2026-11-09 | репо проекта | строка 687: пакет удара 21 и атакующий |
| `SM` | soulsmods/soulsmods.github.io (каталог EMEDF) | 7bdbe2e049f9cea445cacfd59432384ed734aa2d (2026-09-13) | нет | `emedf/sekiro-emedf.html`, `emedf/er-emedf.html` |
| `RDL` | eladidu/readable-ds-lua, ветка `main`: декомпилированные боевые скрипты Soul of Cinder (DS3) | e56c5cc0cac6b5328c24880dfef7ffb7819d5b69 (2024-08-13) | нет | `Soul of Cinder/*.dec.lua` |
| `RDL-doc` | тот же репо, ветка `gh-pages`: Doxygen-справочник «Documentation of the Elden Ring AI» | 4c78203276c9dd5dbaf8b6d769a30d807cd41233 (2024-08-15) | нет | `docs/d1/d81/class_ai_func.html`, `docs/_ai_constants_8hpp_source.html`, `docs/df/d02/class_goal_native.html`, `docs/d4/d58/class_goal_func.html` |
| `NvD` | TuriansNotBad/NPCs_vs_DS3 (рукописный AI на Lua для DS3) | c1e0a3ba72147c69f0a5fc278c75021c07305ffe (2019-10-20) | MIT | README, 6 файлов `.lua` |
| `SR` | thefifthmatt/SoulsRandomizers | 0f4c316ac5728fed7db78e3a54bd307cfd43bd8e (2026-10-26) | LICENSE.md, «mostly all rights reserved» | `RandomizerCommon/{Sekiro/SekiroEnemyEditor.cs,Elden/EldenEnemyEditor.cs,Enemy/NpcEdits.cs}` |
| `LDEC` | https://github.com/nex3/DSLuaDecompiler README (fetched, цитаты в 6.3) | — | — | формат AI-скриптов (Lua 5.0.2) против HavokScript |
| `NEGA` | https://nega.tv/posts/low-tech-ai-of-elden-ring.html (блог, 2026-06-23) | — | — | цитаты про goal stack |
| `ST1`, `ST2` | https://steamcommunity.com/app/1245620/discussions/0/4343240316590129686 и …/4417479689928856889 | — | — | только [C] |

Клоны `RDL`, `RDL-doc`, `NvD` и `SR` лежат в `E:\RE\kb-sources\ai\`. Код из них не запускался.

---

## 1. Архитектура AI у FromSoftware

### 1.1 Слои

1. **Параметры.** Враг в MSB ссылается на строку `NpcThinkParam`. В строке лежат `logicId` (ID логики, `PDX:SDT/Defs/NpcThinkParam.xml:3` «スクリプトで作成したロジックのIDを設定します») и `battleGoalID`, а также чувства (зрение, слух, нюх), таймеры забывания и поля целевой системы. [S]
2. **Lua-логика и цели (goals).** Логика (`Logic.Main` и `Logic.Interrupt`) раз в тик решает, какую top goal поставить. Боевая цель (`Goal.Activate/Update/Interrupt`) раскладывает её на sub-goals (`AddSubGoal(GOAL_COMMON_*)`). Примеры: `SDT-AI:aicommon-luabnd/011001_logic.lua` (`RegisterTableLogic`, `Logic.Main`, `Logic.Interrupt`) и `SDT-AI:aicommon-luabnd/011000_battle.lua` (`RegisterTableGoal`, `Goal.Activate` → `AddSubGoal(GOAL_COMMON_Wait, …)`). [S] Для ER и DS3 то же самое описано в `RDL-doc:docs/df/d02/class_goal_native.html:156-157`: «Interrupt sub goal… call goal's interrupt function… InterruptTableGoal_Common (Goal.Interrupt) for every interrupt type». В `NEGA` сказано: «Each frame Actors will update the Goal on top of their stack of Goals». [S]
3. **Движковая целевая система (target system).** Lua не выбирает, с кем драться. Каждая цель адресуется абстрактным слотом `TARGET_ENE_0`, а заполняет его движок.
   - Sekiro: класс `SprjTargetingSystem`, 14 слотов поиска `TargetSearchSysSlot_*`. Их имена в `EXE`: `NormalEnemy`, `NormalFriend`, `SoundPos`, `LastSightPos`, `LastTarget`, `LastMemoryTargetPos`, `LowHpFriend`, `CorpsePos`, `IndicationPos`, `Hallucination`, `SoulCoin`, `PredictionPos`, `FixedPos`. [S] Цепочка вызовов: `SprjTargetingSystem::Update 0x61FF40 → 0x623CA0 → 0x6250A0 SearchHostile → 0x62BF30` (`RET:24-31`). [S-RE]
   - ER: `CSTargetingSystemBase` содержит `CSTargetSearchSys` с `search_slots: [usize; 14]` (`FRS:crates/eldenring/src/cs/targeting.rs:165-184`). Индексы слотов: `Enemy = 0, Friend = 1, Sound = 2, LastSight = 5, LastTarget = 6, LowFriend = 7, Corpse = 8, LastMemory = 11` (`:186-196`). [S] Устройство в ER и Sekiro одинаковое (14 слотов, те же роли) [I].
4. **EMEVD (события).** Могут принудительно задать или сбросить цель: `ForceCharacterTarget` и `ClearCharactersAITarget` (раздел 2.5). [S]

### 1.2 Что решает Lua, а что движок

- **Lua в Sekiro не может назначить конкретную цель.**
  - Константы целей в `SDT-AI:aicommon-luabnd/ai_define.lua:22-35`: `TARGET_NONE=-2, TARGET_SELF=-1, TARGET_ENE_0=0, TARGET_FRI_0=10, TARGET_EVENT=20, TARGET_LOCALPLAYER=21, … TARGET_TYPE_BOIDS_COMMAND=60`. Слотов `TARGET_ENE_1..n` нет. [S]
  - По всем 673 скриптам: `TARGET_ENE_0` встречается 29709 раз, `TARGET_FRI_0` 273 раза, `TARGET_LOCALPLAYER` 183 раза (`E:\RE\work_ai\api_counts.txt`, подсчёт grep). [S]
  - Методов `ChangeTarget*`, `SetTarget(...)`, `SelectTarget`, `*Hate*`, `*Aggro*` нет ни в скриптах, ни среди строк `EXE`: `ChangeTarget` 0 вхождений, `Hate` 0, `Aggro` 0, `SelectTarget` 0. Строчное `hate` встречается только внутри `whatever`/`alphatex`. [S]
  - Единственное, что Lua может сделать с целью, это **сбросить** её: `ClearEnemyTarget()` (51 вызов), `ClearSoundTarget`, `ClearLastMemoryTargetPos`, `ClearIndicationPosTarget`, `ClearCorpsePosTarget` (`SDT-AI:aicommon-luabnd/clear_target.lua:5-18`). После сброса цель снова выбирает движок. [S для API; [I] для «движок выбирает снова»]
- **В DS3 и ER картина та же.**
  - В пяти декомпилированных скриптах Soul of Cinder используются только `TARGET_ENE_0` (428 раз) и `TARGET_SELF` (341 раз), и ни одного вызова поиска или смены цели (`RDL`, подсчёт ресёрч-агента; пример `RDL:Soul of Cinder/LastBoss_LargeSword_battle.dec.lua:177`). [S]
  - Моддер прямо пишет: «selecting specific targets for the NPC (something which is unfortunately not available in the lua scripts)» (`NvD:README.md:4`), и поэтому делает это через EMEVD. [S]
  - Автор `NEGA` по декомпилированным скриптам приходит к выводу: «Aggro and Targeting seems to be handled outside». [S как цитата; сам тезис относится к [I] автора]
  - В справочнике `RDL-doc` для ER символов hate/aggro/ChangeTarget нет. Перечисление `TARGET` там расширено: `TARGET_SEARCH=80, TARGET_SOUND=81, TARGET_HOSTPLAYER=82`, а `TARGET_ENE_1` тоже нет (`RDL-doc:docs/_ai_constants_8hpp_source.html:147-166`). [S]

### 1.3 Прерывания (interrupts), связанные с целью

Таблица `INTERUPT_*` в Sekiro (`SDT-AI:aicommon-luabnd/ai_define.lua:72-131`, коды 0..57) по составу и номерам совпадает с ER (`RDL-doc:docs/_ai_constants_8hpp_source.html:248-305`, 0..56). [S] К цели относятся следующие коды:

- `INTERUPT_FindEnemy = 0`
- `INTERUPT_Damaged = 2`
- `INTERUPT_Damaged_Stranger = 3`
- `INTERUPT_ForgetTarget = 18`
- `INTERUPT_FriendRequestSupport = 19`
- `INTERUPT_LoseSightTarget = 31`
- `INTERUPT_TargetOutOfRange = 35`
- `INTERUPT_ChangeSoundTarget = 46`

(`ai_define.lua:73,75,76,91,92,104,108,119`) [S]

Сколько раз скрипты Sekiro проверяют каждое прерывание через `IsInterupt(...)`: `Damaged` 246, `LoseSightTarget` 26, `TargetOutOfRange` 14, `Damaged_Stranger` 1, `ChangeSoundTarget` 1, `FindEnemy` 0, `ForgetTarget` 0 (как отдельная проверка) (`api_counts`). [S]

`Goal.Damaged` в боевых скриптах отвечает только за реакцию на удар (уклон, контратака) и цель не трогает. Пример для Гэнъитиро: `SDT-AI:m11_01_00_00-luabnd/710000_battle.lua:751-775` (`SpinStep`, `ComboFinal` по `TARGET_ENE_0`). [S] Если враг после удара разворачивается к ударившему, это делает движок, а не Lua [I: в Lua нет API выбора цели, см. 1.2].

`INTERUPT_Damaged_Stranger` по названию похож на «урон от того, кто не является текущей целью». Документального подтверждения этому нет ни в одном источнике [I]. В Sekiro он используется ровно один раз: в `NonBattleAct_Interupt` урон любого вида обрывает небоевое поведение (`SDT-AI:aicommon-luabnd/non_battle_act.lua:195-200`). [S]

### 1.4 Какие API вокруг цели есть в Sekiro

Найдены через `SDT-AI` (частоты использования) и `EXE` (строки биндингов в районе файловых смещений `0x294c000–0x2956000`).

| API | Вызовов в скриптах | Смысл |
|---|---|---|
| `GetDist(TARGET_ENE_0)` и прочие `GetDist*` | 4040 | дистанция до цели [S] |
| `IsBattleState` / `IsFindState` / `IsCautionState` | 187 / 177 / 39 | состояние осведомлённости [S] |
| `ClearEnemyTarget` | 51 | сброс цели (единственный рычаг смены) [S] |
| `GetTeamOrder(ORDER_TYPE_Role)` | 770 | роль в отряде: атака, окружение (`Torimaki`), зритель (`Kankyaku`) по отношению к общей цели ENE_0 (`SDT-AI:m10_00_00_00-luabnd/105000_battle.lua:38-40`; константы в `ai_define.lua:145-151`) [S] |
| `GetTeamRecordCount` / `AddGoalScopedTeamRecord` | 8 / 8 | сколько союзников уже атакует или обходит ту же цель (`SDT-AI:aicommon-luabnd/after_attack_act.lua:13-14`) [S] |
| `GetCurrTargetType` / `GetPrevTargetState` | 12 / 10 | тип и состояние текущей цели, `AI_TARGET_TYPE__NORMAL_ENEMY=3, SOUND=4, MEMORY_ENEMY=5…` (`ai_define.lua:489-500`) [S] |
| `GetTopNormalEnemyForgettingTime` | 1 | сколько времени цель не видна (`common_logic_func.lua:44`) [S] |
| `IsSearchTarget(TARGET_ENE_0)` | 12 | «знает ли AI о цели» (для ER: `RDL-doc:…class_ai_func.html:8302`) [S] |
| `GetHpLastTarget` | 19 | HP последней цели (логики боссов, например `710000_logic.lua:8`) [S] |
| Есть в `EXE`, в скриптах **не используются** | 0 | `AddFriendAttackedTarget` (по `RDL-doc:…class_ai_func.html:1228`: «Notifies targeting system that someone in the team attacked target»), `ShiftCurrTargetBattleState`, `HasLastTarget`, `GetHpRateLastTarget`, `MemoryRelativeTarget`, `SetTargetOriginRange`, `AddTeamRecord`, `IsExistTeamRecord`, `GetDamageLastFrame`, `GetLatestAttackedDir`, `GetNumFriend` [S наличие строки; смысл частично по ER-доке] |

---

## 2. Модель цели в движке

### 2.1 Что такое «цель»

- **`TARGET_ENE_0` — один слот.** Это текущая лучшая враждебная цель системы поиска (слот `NormalEnemy`/`Enemy`). Тип цели (`AI_TARGET_TYPE__*`) может быть настоящим врагом, звуком, «памятью» о враге, позицией «присутствия» (`IndicationPos`) или трупом (`ai_define.lua:493-500`). [S] Слоты `LastSightPos`, `LastMemoryTargetPos`, `LastTarget` есть в обеих играх (`EXE` RTTI, `FRS:targeting.rs:186-196`). [S]
- **Состояния.** В Sekiro: `AI_TARGET_STATE__NONE/CAUTION/FIND/BATTLE` (`ai_define.lua:489-492`). В ER их больше: `NONE/MEMORY/AWARE/SEARCH/SEARCH2/CAUTION/BATTLE` (`RDL-doc:…ai_constants…:597-603`). [S]
- **Кандидаты в Sekiro** [S-RE]:
  - Все chr из глобального списка с враждебной командой по матрице `0x143B0CAA0` (34×34). Фильтров по дистанции, по типу chr и по признаку «главный игрок» нет (`RET:36-46`).
  - Список кандидатов включает всё множество игроков `WCM+0x40`, то есть и реплику друга (`RET:48-52`).
  - Команды 6/7/13/24/33 считают команду 2 враждебной (`RET:58-66`).

### 2.2 Осведомлённость (awareness) и выбор

- **[S-RE]** Список осведомлённости `a1+0x40..0x48` (записи по 40 байт) обновляется функцией `0x612400`. Цель выбирается, когда осведомлённость достигает `>= 100.0`, и побеждает наибольшее значение (`RET:26-27`). Восприятие по каждому кандидату идёт через зрение `0x61BB70` и слух `0x61C940` (`RET:31-33`).
- **[S] Поля, которые движок, судя по японским описаниям, кладёт в оценку цели** (`PDX:SDT/Defs/NpcThinkParam.xml`):
  - `targetSys_DmgEffectRate = 100` (`:200`): «ダメージ影響率[％] — ダメージ影響率取得(ターゲットシステム評価情報)», то есть «доля влияния урона, % (данные оценки целевой системы)». Это прямое указание, что **урон входит в оценку цели**.
  - `aroundTargetIncrementPoint` (`:397`): «1秒間に増加する発見ポイント[point/s]», прирост «очков обнаружения» в секунду (в зоне периферийного зрения). Есть и вариант для позиции присутствия, `IndicationTargetIncrementPoint` (`:476`).
  - Три конуса зрения: `eye_dist_perceive` («認識», `:23`), `eye_dist_normal` («通常», `:122`), `eye_dist_around` («周辺», `:421`).
  - `ignoreStealthTime` (`:461`): «視界による戦闘ターゲットの強制認識時間», то есть время принудительного распознавания цели боя.
- **[I] Вывод.** Осведомлённость о каждом кандидате набирается из видимости (очки в секунду) и из урона (с весом `targetSys_DmgEffectRate`). Слот ENE_0 занимает лидер, у которого 100 или больше. Точная формула (есть ли бонус удержания текущей цели, затухание, учёт дистанции) в источниках не описана, это вопрос к RE (раздел 6.5).

### 2.3 Таймеры забывания

В SDT есть: `SightTargetForgetTime` (по умолчанию 600, `:85`), `SoundTargetForgetTime` (`:175`), `MemoryTargetForgetTime` (`:575`), `IndicationTargetForgetTime`, `CorpseTargetForgetTime`. [S]

Общая логика `COMMON_HiPrioritySetup` сбрасывает цель в таком случае: дистанция до ENE_0 на 10 м больше «поводка», заданного событием (`GetEventRequest(2)`), цель есть, и `GetTopNormalEnemyForgettingTime() >= 5`. Тогда вызываются `ClearEnemyTarget`, `ClearSoundTarget`, `ClearIndicationPosTarget` и `ClearLastMemoryTargetPos` (`SDT-AI:aicommon-luabnd/common_logic_func.lua:44-51`). [S]

### 2.4 Скриптовые триггеры сброса цели в Sekiro (SpEffect → прерывание → `ClearEnemyTarget`)

| SpEffect | Имя строки (`CSV:SpEffectParam.csv`) | Где сбрасывается цель |
|---|---|---|
| `110010` на ENE_0 | «復活の術_死んだふり効果» (Воскрешение, эффект «притворился мёртвым»), строка 1556 | `common_logic_func.lua:16-23`: если у цели есть 110010 и у себя нет 205090/205091, вызываются все `Clear*` [S] |
| `107710` на себе (`COMMON_SP_EFFECT_ENEMY_TURN`, `ai_define.lua:552`) | «義手忍具_敵回し_敵回されリアクション» (протез: реакция на «敵回し», веер или воронье перо), строка 1467 | `table_ai_common.lua:204-210`: `ClearEnemyTarget()`, если у себя нет 240100 («【共通AI分岐】敵回しでPCを見失わない», строка 2136) [S] |
| `220010` на себе (`COMMON_SP_EFFECT_BLOOD_SMOKE`) | «敵リアクション再生_めくらまし（血煙）», строка 2030 | `table_ai_common.lua:211-215` (мелкие враги) и `710000_logic.lua:34-36` (Гэнъитиро всегда) [S] |
| `109230` HIDE_IN_BLOOD, `8300` SMOKE_SCREEN + `109203` HIDE_IN_BUSH | — | `table_ai_common.lua:220-223`, `common_logic_func.lua:25-31` (только мелкие враги, «ZAKO») [S] |

Подписку на эти эффекты включает `COMMON_HiPrioritySetup` (`common_logic_func.lua:1-13`). Её вызывают **171 из 210** файлов `*_logic.lua` (grep). [S]

### 2.5 EMEVD может принудительно задать цель

- **Sekiro** [S]:
  - `ForceCharacterTarget(int targeterEntityId, int targetedEntityId)`, инструкция 2004[65] (`SM:emedf/sekiro-emedf.html:2872-2875`).
  - `ClearCharactersAITarget`, инструкция 2004[16] (`:2508-2511`).
  - `EnableAIStateChangeFromEnemyTeam`, инструкция 2004[67] (`:2879-2882`).
- **Ванильные события Sekiro** [S, grep по `EVT`]:
  - `ForceCharacterTarget` вызывается 94 раза, из них **78 раз с целью `10000`**. Например `EVT:common_func.emevd.dcx.js:1214` `ForceCharacterTarget(chrEntityId, 10000);` и `m11_01_00_00.emevd.dcx.js:824-825`.
  - `ClearCharactersAITarget` вызывается 12 раз.
- **ER** [S]: `ClearCharactersAITarget` 2004[16] и `SetCharacterEventTarget` 2004[11] (`SM:emedf/er-emedf.html:2386-2389, 2435-2438`). Есть условие `IfCharacterTargetedBy(..., aggressorEntityId, targetEntityId, ..., numberOfTargetCharacters)` (`:656`). Это значит, что ER-события умеют проверять, кого именно атакует враг.
- **[I]** `10000` по контексту (засада «нацелиться на игрока» после пробуждения, `common_func.emevd.dcx.js:1196-1215`) — это entity id главного (локального) игрока. Это общепринятое значение в модинге, но прямой цитаты я не нашёл. На машине хоста засады и скриптовые агро-события Sekiro жёстко направляют врага на хоста.

### 2.6 Различия Sekiro и ER

| Аспект | Sekiro | ER |
|---|---|---|
| Пороги поиска в параметрах | нет; порог 100 зашит в код (`RET:27`) [S-RE] | `searchThreshold_Lv0toLv1 = 10`, `searchThreshold_Lv1toLv2 = 70` (`PDX:ER/Defs/NpcThinkParam.xml:398,405`), отдельные `searchTargetLv1/2ForgetTime` (`:87,95`) [S] |
| AI призыва (buddy) | нет | `isBuddyAI` «バディ用の思考か» (`:500`) [S] |
| Слух | `ear_dist` | `ear_listenLevel`, `soundBehaviorId01..08` (`:303, 593…`) [S] |
| Константы целей | до 60 | плюс `TARGET_SEARCH=80, TARGET_SOUND=81, TARGET_HOSTPLAYER=82` [S] |
| Фильтр «участвует в поиске цели» | — (в FRS нет крейта) | `include_in_ai_target_search` («will be disabled for ghosts», `FRS:crates/eldenring/src/cs/character_type_properties.rs:9-11`) [S] |
| Блок полей для нескольких целей в `NpcThinkParam` | нет | **нет**. Полный diff SDT↔ER (раздел 3) показывает только поля восприятия, оружия и дальней атаки [S] |
| `targetSys_DmgEffectRate`, `TeamAttackEffectivity` | есть | есть (`:234, :240`) [S] |
| SpEffect `targetPriority` | есть | есть (раздел 4) [S] |

---

## 3. NpcThinkParam: поля, связанные с целью, SDT / DS3 / ER

Деф DS3 (`PDX:DS3/Defs/NPC_THINK_PARAM_ST.xml`) содержит только имена и типы, японских описаний в нём нет. Часть полей в нём названа `Unk1..Unk7` (`:80-90`). Типы `SightTargetForgetTime`/`SoundTargetForgetTime` объявлены как `s16` (`:31-32`), причём `SoundTargetForgetTime` с дефолтом 16448 = `0x4040`, что похоже на старшую половину float 3.0. Поэтому раскладку DS3 я считаю ненадёжной [I].

| Поле | SDT (`PDX:SDT/…NpcThinkParam.xml`) | DS3 (`…NPC_THINK_PARAM_ST.xml`) | ER (`PDX:ER/…NpcThinkParam.xml`) | Описание (JP, из SDT/ER) |
|---|---|---|---|---|
| `targetSys_DmgEffectRate` | u16 = 100, `:200`, смещение 0x4E [I: вычислено по дефу] | s16 = 100, `:36` | u16 = 100, `:234` | ダメージ影響率取得(ターゲットシステム評価情報) |
| `TeamAttackEffectivity` | u8 = 25, `:206`, 0x50 | u8 `teamAttackEffectivity`, `:37` | u8 = 25, `:240` | チーム内の同時攻撃人数を決めるための値。値を大きくすると、同時に攻撃参加できる人数が少なくなる (сколько членов отряда одновременно атакуют одну цель; это не выбор цели) |
| `SightTargetForgetTime` | f32 = 600, `:85`, 0x1C | s16, `:31` | f32 = 600, `:111` | 視覚ターゲット忘れる時間 |
| `SoundTargetForgetTime` | f32 = 150, `:175` | s16 = 16448, `:32` | f32 = 3, `:209` | 音ターゲット忘れる時間 |
| `MemoryTargetForgetTime` | f32 = 150, `:575`, 0x9C | — | f32 = 150, `:545` | 記憶ターゲット忘れる時間 |
| `BattleStartDist` | u16, `:183`, 0x48 | s16 = 20, `:33` | u16, `:217` | 戦闘開始距離[m] |
| Точки обнаружения | `aroundTargetIncrementPoint` f32, `:397`, 0x68 | — | заменены порогами `searchThreshold_Lv0toLv1/Lv1toLv2`, `:398/405` | 1秒間に増加する発見ポイント / 索敵LvN閾値 |
| Конусы зрения | `eye_dist_perceive/normal/around` (`:23/122/421`) | `eye_dist = 20`, `:21` | `searchEye_dist` (`:42`), `eye_dist` (`:148`), `battleEye_updateDist` (`:441`) | 視覚_距離 (認識/通常/周辺 / 索敵/戦闘上書き) |
| `ignoreStealthTime` | u16, `:461` | — | — | 戦闘ターゲットの強制認識時間 |
| `callHelp_*`, `platoonReply*` | есть | есть | есть | вызов союзников |
| `thinkAttr_doAdmirer` | u8, `:300` | `thinkAttrDoAdmirer`, `:52` | u8, `:330` | 取巻き役になるか (роль «окружающего») |
| `isBuddyAI` | — | — | u8, `:500` | バディ用の思考か |
| Поля про «ближайшего», «атакующего», «случайного», «с наименьшим HP», «hate» | **нет** | **нет** | **нет** | — |

Всё остальное, что отличает ER от SDT в diff по полям (`E:\RE\work_ai\think_{SDT,ER}.tsv`), относится к оружию в ножнах и наголо, дальней атаке (`rangedAttackId`, `spEffectId_RangedAttack`), прыжкам и падениям в навигации, «каравану» и звуковым поведениям. **Блока полей ER для боя с несколькими целями в `NpcThinkParam` нет.** [S: diff]

В `NpcParam` к цели относятся только `teamType` («NPCの攻撃が当たる/当たらない、狙う/狙わない設定», `PDX:SDT/Defs/NpcParam.xml:777`, ER `:742`) и поля lock-on. Полей приоритета или ненависти нет. [S]

### 3.1 Распределение значений в Sekiro (`CSV:NpcThinkParam.csv`, 771 строка)

Группы выделены по префиксу ID строки: `1xxxxxxx` — рядовые враги, `5xxxxxxx` — боссы и крупные, `7xxxxxxx` — человеческие NPC и боссы-люди. Скрипты: `E:\RE\work_ai\scripts\{dist,grp}.py`.

| Поле | Все строки | 1xxxxxxx (559) | 5xxxxxxx (69) |
|---|---|---|---|
| `targetSys_DmgEffectRate` | 100 в 769 строках, **0 только у «鬼仏» (идол) 70210000/70219000** | 100 везде | 100 везде |
| `TeamAttackEffectivity` | 34 (323), 0 (134), 100 (74), 26 (69), 17 значений | 34 (308) | 100 (34), 0 (25), 75 (8) |
| `SightTargetForgetTime` | 15 (317), 10 (126), 99999 (95), 3600 (69) | 15 (277), 10 (115) | **99999 (40)**, 10 (10), 3600 (8) |
| `MemoryTargetForgetTime` | 150 (542), 99999 (75) | 150 (476) | **99999 (40)** |
| `aroundTargetIncrementPoint` | 25 (491), 0 (239) | 25 (437) | **0 (49)**, 1000 (9) |
| `ignoreStealthTime` | 0 во всех 771 строках | — | — |
| `callHelp_MyPeerId/CallPeerId` | 0 во всех 771 строках | — | — |

Ключевые боссы:

| ID | Босс | `TeamAttackEffectivity` | `SightTargetForgetTime` | `MemoryTargetForgetTime` | `aroundTargetIncrementPoint` | `BattleStartDist` | Строка CSV |
|---|---|---|---|---|---|---|---|
| 71000000 | «ライバル» (Гэнъитиро), logic 710000 | 100 | 99999 | 99999 | 0 | 25 | 653 |
| 54000000 | «蘇った剣聖» (Иссин) | 100 | 99999 | 99999 | 0 | 25 | 633 |
| 50900000 | «お蝶» (Леди Бабочка) | 26 | 99999 | 99999 | 0 | 60 | 616 |
| 51000000 | «夜叉猿» (Обезьяна-страж) | 100 | 99999 | 99999 | 10 | 50 | 618 |

Строка 50709000 «長の梟 サンプル»: 100 / 99999 / 99999. [S]

**[I] Вывод.** Боссы Sekiro практически никогда не забывают текущую цель. Таймеры забывания, которые могли бы запустить новый поиск, у них стоят на 99999 с. Периферийное накопление очков обнаружения у большинства из них выключено. Вес урона одинаковый у всех: 100 %.

---

## 4. Что делают ER и DS3 в бою нескольких игроков

1. **SpEffect `targetPriority` — штатный рычаг для мультиплеера во всех трёх играх.** [S]
   - `PDX:SDT/Defs/SpEffect.xml:481-486` и `ER/Defs/SpEffect.xml:480-485`, в DS3 `SP_EFFECT_PARAM_ST.xml:71`. Описание: «ターゲット優先度加算分 — マルチプレイ時、敵から優先的にターゲットとして狙われるようになる。プライオリティの加算。０がデフォルト。プラス値でよく狙われるようになる。マイナスは、－１まで。» («Добавка к приоритету цели. В мультиплеере враги начинают предпочтительно выбирать носителя целью. 0 по умолчанию, плюс — чаще целятся, минус — до −1.»)
   - Диапазон: `Minimum -1`, `Maximum 10`.
   - Смещение поля в строке SpEffect Sekiro: `0xF8` [I: вычислено по дефу `E:\RE\work_ai\scripts\offs.py`].
   - **В Sekiro поле используется только в 4 строках, и везде со значением −1** (`CSV:SpEffectParam.csv:2070,2071,3076,3078`): «イベント用_完全姿隠し», «イベント用_白目», «【NTC】皇子_ヘイトテスト» («тест ненависти»), «【NTC】皇子_誘導幻影». [S]
   - **[I]** Значит, движок Sekiro унаследовал механизм приоритета целей «для мультиплеера», и разработчики его проверяли: строка «ヘイトテスト». Но ни один эффект игрока или союзника его не использует.
   - Как это поле используют ER и DS3 (какие эффекты призывов, фантомов, ростов Мимика его выставляют), из локальных данных не проверено: регуляции ER/DS3 нет. Это в списке Unverified.
2. **ER-события умеют проверять и назначать цель.** Условие `IfCharacterTargetedBy` и инструкция `SetCharacterEventTarget` (раздел 2.5). [S]
3. **В ER есть мета-флаги поиска.** `include_in_ai_target_search` у типа персонажа (`FRS:character_type_properties.rs:9-11`). Фильтр команд `CSTeamTypeEnemy::validate` → `team_relation.oppose_target` (`FRS:crates/eldenring/src/cs/team_relation.rs:93-103, 128-133`). [S] Это фильтры кандидатов, к ранжированию они отношения не имеют [I].
4. **В Lua ER нет логики «жонглирования».** API выбора цели не существует (раздел 1.2). Есть `AddFriendAttackedTarget` («Notifies targeting system that someone in the team attacked target», `RDL-doc:…class_ai_func.html:1228`) и `TeamHelp_Reply` («Handles answering call logic, including setting as target?», `:10869`, вопрос в оригинале). [S]
5. **Наблюдения сообщества** [C]:
   - `ST2`: «if someone hits a boss it will focus its attention on the person that hit them, but if it is deciding who to target, for example wen you enter the boss fog right after the host, it will pick you instead».
   - `ST2` (OP): «a boss' attention goes to the player who hit it last».
   - `ST1`: «Bosses will spend most of their time on you (80%+) even with a gold summon or Mimic in play».

**[I] Итог раздела.** Нигде не найдено доказательств, что в ER есть отдельная «умная» система ненависти. Найденные механизмы (оценка с весом урона, `targetPriority`, события) есть и в Sekiro. Ощущение, что боссы в ER «жонглируют» целями, вероятнее всего объясняется комбинацией нескольких факторов:
- значениями `targetPriority` на эффектах призывов и фантомов;
- другими таймерами и порогами поиска (`searchThreshold`, короткие `SoundTargetForgetTime = 3`);
- тем, что в ER все игроки и призывы — полноценные участники сессии;
- изменениями в движке, которые в публичных источниках не описаны.

Это гипотеза, а не факт.

---

## 5. Почему в Sekiro Together враги ведут себя так, как описал юзер

Контекст мода. Враги хоста работают на Com-AI хоста. Реплика друга — нативный remote `PlayerIns` (команда 2), она входит в кандидаты (`RET:8, 48-52`). Удары друга по врагам хоста приходят пакетом 21 с полем `attacker chr+0x78` и применяются нативно через `0xA4F940 → 0x8D77B0 → 0xB680C0` (`IMPL:687`). [S/S-RE] Значит, целевая система хоста видит урон друга как урон от кандидата [I].

| Наблюдение | Механизм | Уровень |
|---|---|---|
| **«Прилипает» к одному** | (1) В Lua один слот `TARGET_ENE_0`, сменить цель скрипт может только через `ClearEnemyTarget`, а вызывает его лишь на особых эффектах (2.4). (2) У боссов `Sight/MemoryTargetForgetTime = 99999`, у 49 из 69 строк `aroundTargetIncrementPoint = 0` (3.1), поэтому естественного повода для нового поиска нет. (3) 78 вызовов `ForceCharacterTarget(…, 10000)` направляют засады на локального игрока, то есть хоста (2.5). (4) Быстрая квик-разворот реакция `QUICK_TURN_TO_PC` разворачивает врага к `TARGET_LOCALPLAYER` (`table_ai_common.lua:199-203`), а это тоже хост. | [S] факты, [I] связь |
| **Переключается на ближайшего, только если тот долго стоит перед ним** | Осведомлённость о втором игроке растёт очками в секунду от видимости, и смена происходит, только когда он побеждает по осведомлённости (`RET:26-27`, поля `aroundTargetIncrementPoint`/конусы зрения). Если игрок стоит перед врагом, он в «認識»-конусе, и очки идут. | [S-RE] + [I] |
| **Мгновенно переключается на того, кто нанёс хоть один удар** | `targetSys_DmgEffectRate = 100` у всех 769 строк. Урон входит в оценку целевой системы с полным весом. Судя по поведению, одного удара хватает, чтобы перевесить; вероятно, есть и отдельное правило «последний атакующий». Формула не проверена. | [S] поле, [I] величина |

**[I] Гипотеза юзера («в одиночной игре зашита примитивная логика») подтверждается частично.** Механизм оценки целей (урон, видимость, `targetPriority` «для мультиплеера») в движке Sekiro **есть**. Но:
- данные Sekiro его никак не настраивают (вес урона везде 100, `targetPriority` не используется, боссы не забывают цель);
- события и Lua завязаны на `10000`/`TARGET_LOCALPLAYER`, а в одиночной игре это единственный игрок.

Получается не «нет логики», а «логика не настроена под второго игрока, и местами жёстко привязана к хосту».

---

## 6. Варианты сделать смену цели умнее в Sekiro Together

Ниже варианты в порядке рекомендации.

### 6.1 (Рекомендуется первым) Гибрид: модель ненависти в DLL, а исполнение штатными рычагами движка

**Идея.** DLL на хосте ведёт свою таблицу ненависти по каждому врагу и игроку:
- доля урона в скользящем окне (например, 8 с);
- близость;
- время, проведённое целью;
- кулдаун смены и гистерезис;
- «липкость», настраиваемая по боссу.

Решение DLL выражается штатными средствами:

1. **Мягко: SpEffect `targetPriority`.** Завести 2–3 своих строки SpEffect (через `regulation`/`gameparam` override в me3 или патчем строки в памяти), например +2, +5 и −1, и вешать их на игрока, которого модель хочет сделать целью, или на текущую цель, чтобы её разгрузить. Поле штатно предназначено «для мультиплеера» (раздел 4). [S поле, I применимость]
2. **Жёстко: `ForceCharacterTarget(enemy, player)`.** Это EMEVD-инструкция 2004[65], которой сам Sekiro пользуется 94 раза (2.5). DLL вызывает её обработчик напрямую, так же как мод уже вызывает EMEVD-функции для гардероба. Нужен entity id или handle реплики (вопрос к RE). [S инструкция]
3. **Сброс: `ClearCharactersAITarget(enemy)`** (2004[16]) или наложение SpEffect `107710` на врага. Второй путь уже подключён в общем Lua: `ClearEnemyTarget` у всех 171 логик с `COMMON_HiPrioritySetup` (`table_ai_common.lua:204-210`). Минус 107710: у эффекта есть `behaviorId 2300`, `stateInfo 275`, `dmypolyId 260` (`CSV:SpEffectParam.csv:1467`), то есть, скорее всего, реакция или анимация «敵回され». Чистого сброса лучше добиваться через свою строку SpEffect плюс правку Lua (6.3) или через EMEVD-инструкцию. [S поля, I последствия]

**Цена.** Средняя: 2–4 дня. Логика в Rust, плюс поиск адресов обработчиков EMEVD 2004[65]/[16] и способа адресовать реплику.

**Риск.** Низкий или средний. Используются родные пути, движок сам доигрывает разворот, подход и ремапинг подцелей. Опасные места:
- `ForceCharacterTarget` может «залипнуть» (неизвестно, снимается ли он сам);
- скриптовые события боссов (фазы, кат-сцены) тоже зовут `ForceCharacterTarget(…, 10000)`, и мы будем с ними конкурировать. Нужен кулдаун и пауза модели на время событий.

**Антипинг-понг.** Смена цели разрешена, только если «ненависть» нового кандидата больше ненависти текущего × (1 + h), где h = 0,25–0,5, и с момента прошлой смены прошло не меньше T_min = 4–6 с (у боссов дольше). Одиночный удар добавляет не больше доли от порога. Добивание (deathblow) и захваты не прерываются.

### 6.2 Только параметры (без кода или с минимальным кодом)

| Правка | Ожидаемый эффект | Подтверждение | Цена / риск |
|---|---|---|---|
| `targetSys_DmgEffectRate` у боссов 100 → 20–40 | один удар уже не перетягивает цель | поле [S], величина эффекта не проверена [I] | низкая; через me3 `regulation`-override или запись в строку `SoloParamRepository` (индекс 9, `FRS:crates/sekiro/src/sprj/solo_param_repository.rs:328`). Риск: босс станет слишком «тупым» к атакующему |
| `SightTargetForgetTime` и `MemoryTargetForgetTime` у боссов 99999 → 5–10 | цель «забывается», если её не видно, и идёт повторный поиск | поля [S] | риск: босс уходит в небоевое состояние или домой (`maxBackhomeDist = 9999` у 59 из 69 строк, так что «домой» маловероятно) |
| `aroundTargetIncrementPoint` у боссов 0 → 25 | второй игрок в периферии быстрее набирает осведомлённость | поле [S] | эффект на уже идущий бой неизвестен |
| SpEffect `targetPriority` на постоянном эффекте реплики или хоста (±) | статический перекос в пользу одного из игроков | поле и описание [S] | дешево, но статично; чинит только «всегда хост» и не даёт «умности» |

Правка параметров хоста работает только на машине хоста. Враги считаются там (`RET:8-12`), поэтому этого достаточно [I]. Читает ли движок строку `NpcThinkParam` «живьём» или копирует её при создании AI, неизвестно. В ER `NpcThinkParamLookupResult` хранит указатель `param_row` (`FRS:crates/eldenring/src/cs/targeting.rs:11-16`), что скорее говорит о чтении живьём [I]. Это вопрос к RE.

### 6.3 Переопределение Lua-скриптов через me3

**Что возможно.** Заменить `script/aicommon.luabnd.dcx` или `script/mXX_….luabnd.dcx`. В каждом картовом luabnd свои копии логик боссов; например, логика 710000 лежит в `m11_01_00_00`. В Lua можно:
- сравнивать `GetDist(TARGET_ENE_0)` и `GetDist(TARGET_LOCALPLAYER)`, и при двух игроках понимать, «друг» ли сейчас цель [I];
- держать таймеры (`SetTimer`/`IsFinishTimer`) и числа (`SetNumber`);
- по своему правилу (время на цели, расстояния, SpEffect-сигнал от DLL через `AddObserveSpecialEffectAttribute` → `INTERUPT_ActivateSpecialEffect`) вызывать `ClearEnemyTarget()` + `Replanning()`.

**Чего нельзя.** Нельзя назначить конкретного игрока: API нет (1.2). После `Clear` выбор снова за движком. Нельзя видеть второго игрока, кроме как через ENE_0, LOCALPLAYER и `FRI_0` (союзник, не враг). [S/I]

**Формат.** AI-скрипты Sekiro — это **байткод Lua 5.0**, а не HavokScript. Заголовок файла `1B 4C 75 61 50` = `\x1bLuaP`, версия 0x50 (`E:\RE\work_ai\bnd\m11_01_00_00-luabnd\out\bin\710000_logic.lua`, первые 5 байт). [S] README декомпилятора: «decompiler for Lua 5.0.2 and HavokScript… [Lua] primarily used to implement AI logic», HavokScript нужен «to interface the game with the Havok behavior system» (https://github.com/nex3/DSLuaDecompiler). [S]

Рандомайзер для DS3 кладёт в `aicommon.luabnd` **plain-text** Lua без компиляции:
- `SR:RandomizerCommon/Core/GameData.cs:1920-1923` (`File.ReadAllText(path), "*.lua"`);
- `SR:RandomizerCommon/Misc/MiscSetup.cs:623-626` (`file.Bytes = Encoding.UTF8.GetBytes(contents)`);
- исходники лежат как `SR:dist/AI/314000_battle.lua`. [S]

Отсюда [I]: DS3 принимает текстовый Lua. Для Sekiro это не проверено. Если текст не примется, подойдёт стандартный `luac` 5.0.2. Это тоже [I]: формат `LuaP` стандартный, но проверка на совместимость сборки (размеры типов в заголовке `04 08 04 06 08 09 09 08`) ещё нужна.

**Цена.** Средняя или высокая. Нужно: проверить на копии, что текстовый Lua грузится в Sekiro; пересобрать BND4 через WitchyBND; тиражировать правки по всем картовым luabnd.

**Риск.** Средний: ошибка в общем скрипте ломает AI всех врагов. Сам по себе этот путь даёт «сброс → движок выбирает снова», то есть без 6.1 он недостаточен.

### 6.4 Арбитраж цели целиком в DLL (хук целевой системы)

**Идея.** Перехватить выбор в `SprjTargetingSystem`: функцию оценки `0x612400`/`0x626430` или запись результата в слот NormalEnemy (`RET:24-33`). Подставлять кандидата по своей модели ненависти (те же компоненты, что в 6.1).

**Цена.** Высокая: 1–2 недели RE и отладки.

**Риск.** Высокий. Внутренние связи движка неизвестны: память цели, подцели, звук, `LastTarget`. Если подменить только слот, AI может держать «старую» память. Плюс стоимость хука на каждый тик каждого врага. Оправдано, только если 6.1 не даёт нужной управляемости.

### 6.5 Вопросы к параллельному RE-агенту (бинарь `sekiro.exe` 1.06)

1. **Формула осведомлённости** в `0x612400`/`0x626430`. Из каких членов она складывается: очки зрения из `aroundTargetIncrementPoint` (`NpcThinkParam+0x68`), урон × `targetSys_DmgEffectRate` (`+0x4E`, u16), дистанция, SpEffect `targetPriority` (SpEffectParam `+0xF8`, f32)? Есть ли затухание и бонус удержания текущей цели?
2. **Откуда берётся событие урона.** Где урон от атакующего попадает в целевую систему: через `INTERUPT_Damaged` / `Damaged_Stranger` или через прямой вызов из `0xB680C0`? Есть ли отдельное правило «последний атакующий = цель» в обход 100-очкового порога?
3. **`targetPriority`.** Читает ли его код Sekiro вообще, и где? Есть ли проверка «в мультиплеере» (по счётчику членов сессии `0x8E3E60`, который у нас на хосте может быть ≤1, см. `RET` раздел c)? Если проверка есть, понадобится патч условия.
4. **EMEVD 2004[65] `ForceCharacterTarget` и 2004[16] `ClearCharactersAITarget`.** Нужны адреса обработчиков, как они резолвят entity id (`10000` = главный игрок) и можно ли передать handle реплики (`chr+0x8`) напрямую. Снимается ли «форс» сам, есть ли у него таймер.
5. **`NpcThinkParam`.** Читается ли строка живьём (указатель на row) или копируется при создании AI? Индекс 9 в `SoloParamRepository`.
6. **Строки Lua-биндингов** `ShiftCurrTargetBattleState`, `AddFriendAttackedTarget`, `MemoryRelativeTarget`, `HasLastTarget` (EXE-смещения `0x2951308`, `0x29511d0`, `0x29509a0`, `0x294f688`). Найти функции-реализации и понять, нельзя ли через них, или рядом с ними, адресовать слот `LastTarget`/`NormalEnemy`.
7. **`INTERUPT_Damaged_Stranger`.** Действительно ли это «урон не от текущей цели»?
8. **Включает ли `ClearEnemyTarget` немедленный новый поиск** или ждёт таймера поиска `0x625F30` (`RET:28-29`)?
9. **Таблица 34×34 команд** (`RET:58-66`). Совпадает ли индекс «кандидата-друга» с тем, что использует оценка, и не режет ли что-то вес реплики (например, флаги в `chr+0x1A15`)?

---

## Unverified / не найдено

- **Формула выбора цели** (вес урона, времени, дистанции, удержания) не описана ни в одном публичном источнике. Есть только имена полей и одна строка из `RET` («>= 100, наибольший побеждает»).
- **Как ER и DS3 используют `targetPriority`** (какие строки SpEffect, какие значения у призывов, фантомов и Мимика): регуляции ER/DS3 локально нет, публичных таблиц значений я не искал.
- **Что «умное» жонглирование в ER есть в движке**: не найдено ни в коде, ни в документации. Есть только [C] (последний ударивший, первый вошедший в туман).
- **Смысл `INTERUPT_Damaged_Stranger`, `ShiftCurrTargetBattleState`, `TeamHelp_Reply`**: не документирован (в ER-доке у `TeamHelp_Reply` стоит «?»).
- **Публичных декомпилированных AI-скриптов ER и Sekiro не найдено.** Скрипты Sekiro декомпилированы здесь локально и в репозиторий не выкладываются. Для DS3 есть только `RDL` (Soul of Cinder) и рукописный `NvD`.
- **Вики-страниц soulsmods/soulsmodding про AI scripting не найдено** ни локально (в клоне только `emedf/`), ни поиском.
- **Грузит ли Sekiro plain-text Lua из luabnd**: не проверено. Для DS3 это следует из кода рандомайзера ([I]), для Sekiro и ER источников нет. Совместим ли стандартный `luac` 5.0.2 с заголовком Sekiro (`1B 4C 75 61 50 01 04 08 04 06 08 09 09 08`), тоже не проверено. Публичного компилятора HavokScript нет (есть только дизассемблер hksdisasm), но для AI он и не нужен.
- **Страницы Sekiro-модов на Nexus** (например, mods/42) недоступны (403), их содержимое не читалось.
- **Entity id реплики друга** для `ForceCharacterTarget` неизвестен.
- **Смещения `0x4E`, `0x68`, `0xF8`** вычислены по Paramdex-дефам, а не прочитаны из бинаря.
- **В крейте Sekiro (`FRS:crates/sekiro`) AI-структур нет**, поэтому сравнение ER↔Sekiro на уровне структур опирается только на RTTI-строки `EXE` и `RET`.
