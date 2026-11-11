# MCP server for autonomous build creation: analysis

Goal: an MCP server through which Claude creates, edits and evaluates PoE 1
builds with upstream-exact numbers, and iterates toward a target ("crit cold
DoT Occultist, 6M DPS, 8k EHP").

Legend: **[V]** verified by running it in this repo (2026-11-11, 4 vCPU,
15 GB cloud container), **[R]** read from code/docs, **[A]** assumption.

## 1. Architecture

This repository is not a Lua fork of PoB. It is a rewrite (`docs/design.md`):

| Part | What | Notes |
| --- | --- | --- |
| `upstream/PathOfBuilding` | upstream PoB Community, git submodule, pinned to `b32759ab` on `dev` | never edited; data and code read at build/run time. Not initialised on clone: `git submodule update --init --depth 1` **[V]** |
| `crates/upstream` | upstream's whole Lua program translated to Rust by `luagen` in its build script | runs on `crates/lua` (LuaJIT semantics, refcount + cycle GC). Exact parity with upstream **[R, V for gen-0001]** |
| `crates/upstream/src/host.rs` | Rust port of `HeadlessWrapper.lua` | real `Inflate`/`Deflate` installed by `crates/session` |
| `crates/session` | **engine API**: `Session::new(DataSource)`, `Session::request(json) -> json` | commands in `lua/session.lua` (1235 lines, `api.<cmd>`), translated by luagen; calls upstream's own UI handlers so results stay upstream's |
| `crates/cli` | `pob serve` (JSON lines on stdin/stdout), `pob bench`, `pob bundle` | `POB_UPSTREAM_SRC` overrides data dir |
| `crates/wasm` | C ABI over `session` for the browser (78 MB wasm) | |
| `web/` | Preact frontend, talks to the same JSON commands via workers | `web/src/engine/types.ts` types the responses |
| `crates/engine` | typed Rust engine (phase 7), replacing the translation piece by piece | `initEnv` done; `perform` partly (no minions, triggers, mirages, breakdowns): **not usable alone yet** **[R `docs/progress.md`]** |
| `crates/importer`, `crates/equivalence` | build-time data/mod parser import; tests vs oracle | |
| `oracle/` | upstream on real LuaJIT via `HeadlessWrapper.lua`, dumps `tests/oracle/<id>.json` | `oracle/setup.sh` builds LuaJIT + `luautf8`, `luacov`; needs `luarocks` (not preinstalled; `apt-get install luarocks` worked) **[V]** |
| `corpus/` | 5 upstream 3.13 test builds + SampleCharacter + 56 generated builds | golden builds with oracle outputs |

- `.cargo/config.toml`: only a 16 MB wasm stack for the translated program's deep recursion **[R]**.
- `.claude/skills/sync`: procedure to bump the submodule and port upstream commits (regenerate oracle, corpus, portgen; fix luagen/runtime, never generated code) **[R]**.
- CI (`.github/workflows/ci.yml`): `sync/check.py`, fmt, clippy `-D warnings`, portgen/synthlines diff, `cargo test --release`; web build; wasm vs native (`web/tests`); oracle reproducibility (`oracle/run.py check`); coverage **[R]**.
- Build: `cargo build --release -p cli`, cold, 4 cores: **8 min 18 s**; binary 104 MB **[V]**. Editing `session.lua` only re-runs luagen for `session` (seconds); touching upstream sources rebuilds the translated crate.

## 2. Headless mode (key question): yes, two ways, both verified

**A. In-process Rust session (translated upstream).** `pob serve` takes
`{"cmd":...}` lines. Verified end to end **[V]**: load `gen-0001.xml` → sidebar
`Hit DPS 8,986.9`, EHP `2,149`, Life `2,362`, equal to the oracle
(`CombinedDPS 8986.879125`, `TotalEHP 2148.92…`). From scratch: `newBuild` →
Witch via `control build.classDrop` → Occultist via `build.ascendDrop` →
level 95 → socket group Vortex + 3 supports at 20/20 → rare wand via `addItem`
→ `Total DPS inc. DoT 13,700.9` with a requirement warning → `clickNode` on a
notable allocates its 7-node path → `saveCode` round trips. `cargo test
--release -p session` covers all commands on a corpus build.

**B. Upstream on LuaJIT running the same `session.lua`.** `session.lua` is
plain Lua against upstream's globals, so it runs unmodified on top of
`HeadlessWrapper.lua` (via `oracle/headless.lua`) with a 20-line stdin loop
that redirects upstream's stdout logging to stderr **[V]**. Same spike, same
outputs (`13,700.9`, same tree path, same sidebar). One gap: upstream's
headless `Deflate`/`Inflate` are stubs, so `saveCode` returns `""` and
`loadCode` fails; build codes must be encoded/decoded by the host
(zlib + URL-safe base64 of `saveXml`) **[V]**.

Busted (`spec/`) is upstream's test setup; this repo does not run it, it
ports specs to Rust and uses the oracle instead **[R]**.

### Measured cost (gen-0001, release, idle machine unless noted)

| | Rust session (A) | LuaJIT (B) |
| --- | --- | --- |
| startup (`newBuild` done) | 6.4–6.7 s | 2.3–2.5 s |
| load build XML | 2.2–2.3 s | 0.33–0.42 s |
| edit + recalc (node alloc/dealloc), per edit | ~400 ms (`pob bench`: 20 edits 7.96 s) | ~90 ms (20 edits 1.81 s CPU) |
| gem/item/config edit, small new build | 35–80 ms | 15–30 ms |
| `nodePower` (all ~2.7k nodes) | 18 s (new build), 33 s (gen-0001) | 5.2 s (new build) |
| `tree` (geometry, 2872 nodes) | 255 ms | 95 ms |
| memory | 0.83 GB at start, 0.94 GB loaded, 1.5 GB after node power | not measured **[A: well under 0.5 GB]** |

The translation is 3–7× slower than LuaJIT today; the typed engine (phase 7)
is meant to beat both but is not complete. For an LLM agent, per-call latency
of 0.1–0.5 s is negligible next to model turns; startup, node power and RAM
per open build matter more.

## 3. Data model

- **Build** = PoB XML (`<PathOfBuilding><Build><Tree><Skills><Items><Config><Notes>…`).
  Build code = zlib(XML) → base64 with `+/` → `-_` (`api.saveCode`/`loadCode`) **[R, V]**.
  `api.saveXml` returns `build:SaveDB("code")`.
- **State lives in upstream's build mode** (`main.modes.BUILD`): `spec`
  (PassiveSpec: class/ascendancy, `allocNodes`, `masterySelections`, jewels),
  `skillsTab.socketGroupList` (groups → `gemList` with `nameSpec`, level,
  quality, enabled, count), `itemsTab` (items by id, `slots`, item sets),
  `configTab` (config sets; `input[var]`), `calcsTab` (`mainEnv`, `mainOutput`,
  `calcsEnv`). Every command ends with one frame (`runCallback("OnFrame")`),
  which recalculates if `buildFlag` was set **[R]**.
- **Calc entry**: `Modules/Calcs.lua` `calcs.buildOutput(build, mode)` →
  `CalcSetup.initEnv` (collects tree, items, gems, config into mod DBs) →
  `CalcPerform.perform` (+ `CalcDefence`, `CalcOffence`, triggers, mirages)
  → `calcFullDPS`. Output: `build.calcsTab.mainOutput` (= `player.output`,
  735 keys on gen-0001), minion in `mainEnv.minion.output` **[R, V key count]**.
- **Stats an agent needs** (output keys): `CombinedDPS`, `TotalDPS`,
  `TotalDotDPS`, `FullDPS` (with Full DPS groups), `WithPoisonDPS`/ignite/bleed
  variants, `CritChance`, `CritMultiplier`, `Speed`, `HitChance`; `Life`,
  `EnergyShield`, `Mana`, `ManaUnreserved`, `TotalEHP`, `*MaximumHitTaken`,
  `FireResist`/`ColdResist`/`LightningResist`/`ChaosResist` (+ `OverCap`),
  `Armour`, `Evasion`, `SpellSuppressionChance`, `BlockChance`,
  `SpellBlockChance`, `Str`/`Dex`/`Int` and `Req*`, `LifeRegenRecovery`.
  **Gap**: the session only exposes the formatted sidebar (`state.stats`,
  colour-coded strings like `^7Hit DPS:`) and the calcs tab cells; there is
  no command returning `mainOutput` numerically. Needed: `api.output`.
- Warnings (`state.warnings`: unmet requirements, over-reserved mana, too
  many jewels, …) and popups (`state.popups`) are already exposed.

## 4. Data sources and search

All in `upstream/PathOfBuilding/src`, loaded by upstream at startup:

| Data | Files | Exposed by session today |
| --- | --- | --- |
| gems | `Data/Gems.lua`, `Data/Skills/*.lua` | `gemList` (name, support, color, tags); per-level stats not exposed |
| uniques | `Data/Uniques/*.lua`, `Data/Uniques/Special/*` → `main.uniqueDB` | `itemDb {kind:"unique"}` (name, type, base, raw text incl. variants) |
| rare templates | `Data/Rares.lua` → `main.rareDB` | `itemDb {kind:"rare"}` |
| bases | `Data/Bases/*.lua` → `data.itemBases` | none (need `itemBases`) |
| affixes | `Data/ModItem*.lua`, `ModExplicit.lua`, `ModJewel*.lua`, crafting tables, essences | only via the item editor controls (`displayItem`, `control items.displayItem*`) — awkward for agents; need `itemMods {base}` |
| tree | `TreeData/<ver>/tree.lua` (latest `3_29`, `newBuild` uses it **[V]**) | `tree` (2872 nodes: id, name, kind, stats, ascendancy; connectors), `nodePath`, `nodeTooltip`, `masteryOptions` |
| config options | `Modules/ConfigOptions.lua` | `config` (sections, vars, control type, list, value) |

Search should be done in the MCP server over JSON fetched once per session
(gem list, unique list, tree nodes): case-insensitive substring/token match on
name, stats text and tags; e.g. "cold" + "damage over time" over node `stats`.
No index is needed at this size (≈3k nodes, ≈1.2k uniques, ≈700 gems) **[A sizes]**.

## 5. Integration options

| | (a) Lua MCP server inside headless PoB | (b1) Rust MCP server, in-process `Session` | (b2) Rust MCP server → LuaJIT subprocess running `session.lua` | (c) TS/Python server → `pob serve` |
| --- | --- | --- | --- | --- |
| Speed | fastest (LuaJIT) | 3–7× slower today; improves as the typed engine lands | fastest, + JSON hop (negligible) | as (b1) + process hop |
| Exactness | upstream itself | oracle-tested translation | upstream itself | as (b1) |
| Deps to run | LuaJIT + luautf8 + a zlib for codes + MCP/JSON-RPC in Lua | one binary + upstream `src/` | LuaJIT build (`oracle/setup.sh`, luarocks) + binary | Node/Python + binary |
| Fit with repo | outside the repo's direction (Rust engine, web) | same engine as web app; CI already tests it | reuses oracle runner | extra language/toolchain |
| Upstream merges | `session.lua` breaks on control renames (already handled by `sync`) | same | same | same |
| Cloud sandbox | needs luarocks | cargo only, 8 min cold build | cargo + luarocks | cargo + node |

**Recommendation: (b) as one Rust crate, `crates/mcp` (binary `pob-mcp`),
with a backend trait over the engine API's JSON commands.**

- Default backend **in-process `session::Session`** (b1): zero extra runtime,
  same engine and commands as the web app and CI, automatically benefits
  from the typed engine.
- Optional backend **LuaJIT subprocess** (b2) speaking the identical
  commands (verified to work unmodified), selected by flag/env, for speed
  (node power 5 s vs 33 s) and as a live cross-check against real upstream.
  Host-side build code encoding makes the two behave the same.
- MCP protocol hand-rolled on `serde_json` (stdio, newline-delimited
  JSON-RPC: `initialize`, `tools/list`, `tools/call`, `ping`). `Session` is
  `!Send` (`Rc` everywhere), so an async SDK (`rmcp`/tokio) adds friction for
  no gain on stdio.
- All game logic stays in upstream code; the MCP layer only adds:
  name→id resolution, search, compact summaries, diffs, batching.
- New engine capabilities go into `session.lua` as additive commands, so the
  web app can use them too and the `sync` skill already covers them.
- Not recommended: (a) (would duplicate the session layer in a second place
  and the MCP protocol in Lua), (c) (extra toolchain, no benefit over b).

## 6. Tool surface (MCP)

Builds are addressed by `build` (slot name, default `"main"`). Every editing
tool returns a compact stat summary + delta vs before + new warnings, so
the agent rarely needs to call `get_stats` separately.

| Tool | Args | Backed by |
| --- | --- | --- |
| `new_build` | class, ascendancy?, level?, build? | `newBuild`, `control build.classDrop/ascendDrop/characterLevel` |
| `load_build` | code \| xml \| corpus id, build? | `loadCode`/`loadXml` |
| `export_build` | build?, format: code\|xml | `saveXml` (+ host zlib/base64) |
| `get_stats` | keys?, actor?, build? | **new** `output` |
| `get_build_summary` | build? | `state`, `skills`, `items`, `treeState` condensed: class, level, points used/max, main skill links, equipped items by slot, keystones, warnings |
| `search_nodes` | query, kind? (notable/keystone/mastery/jewel), max? | `tree` cached, search in Rust |
| `allocate_nodes` | nodes: names or ids (paths auto-allocated), build? | `clickNode` per node (or **new** `allocNodes` batch); masteries via `selectMastery` |
| `deallocate_nodes` | nodes | `clickNode` on allocated / `deallocNodes` |
| `reset_tree` | | `resetTree` |
| `suggest_nodes` | stat (dps\|ehp\|life\|…), top?, max_path? | `powerStats`, `nodePower` + `nodePath` |
| `search_gems` | query, support? | `gemList` cached |
| `set_skill_group` | index? (new if omitted), gems [{name, level, quality}], slot?, main?, full_dps? | `addSocketGroup`, `setGem`, `removeGem`, `setSocketGroup`, `setMainSocketGroup` |
| `remove_skill_group` | index | `removeSocketGroup` |
| `search_items` | query, kind unique\|base, slot? | `itemDb`, **new** `itemBases` |
| `equip_item` | slot, unique name (+variant) \| raw item text, build? | `addItem {text, slot}`; unique raw from `itemDb` |
| `craft_rare` | slot, base, mods [lines], influences?, quality? | builds item text in Rust, **new** `parseItem` to reject lines that parse to nothing, then `addItem` |
| `list_config` / `set_config` | var, value (bool/number/list label) | `config`; `control config.<var>` (**new** `setConfig` with validation) |
| `compare_builds` | a, b, keys? | `output` on two slots, diff |
| `what_if` | edits[], build? | snapshot `saveXml`, apply, `output`, restore |

Deliberately excluded for v1: party tab, item/skill/config sets, tree
geometry, tooltips (available behind `detail` flags if needed).

## 7. Risks and unknowns, with spikes

| Risk | Spike / verification |
| --- | --- |
| Agent cannot reach targets like 6M DPS without realistic gear knowledge | M5 demo; seed `search_items` with uniques; `craft_rare` needs affix tier data (`itemMods`) so crafted rares are legal, otherwise agents invent impossible mods. Decide policy: legal mods only (validate tier/base), reported in output |
| Free-text item lines that parse to nothing silently do nothing | **new** `parseItem` exposing upstream's `extra`/unparsed per line; reject in `craft_rare` |
| Class change with allocated nodes opens a confirm popup instead of changing | handle `state.popups` in `new_build`/`set_class` (`popupButton`) — test |
| Memory: ~1–1.5 GB per in-process session limits parallel build slots | measure 3 slots; fall back to one live session + XML snapshots |
| Startup 6.5 s (in-process) per MCP server start | acceptable once per Claude session; measure under `claude mcp` |
| Cold build 8+ min in a fresh cloud sandbox | SessionStart hook builds in background, or CI publishes a prebuilt Linux `pob-mcp` artifact; measure both |
| `nodePower` 18–33 s blocks the server (single thread) | call it only from `suggest_nodes`; consider LuaJIT backend or restricting to candidate nodes (power over a node subset needs a small new command) |
| Upstream renames UI controls → `session.lua` breaks | already part of the `sync` skill; MCP tests on corpus builds catch it |
| Typed engine swaps in under the session | intended; outputs stay oracle-equal by the project's tests |
| LuaJIT backend dependency setup (luarocks absent by default) | optional backend only; document `apt-get install luarocks && oracle/setup.sh` |
| JSON number fidelity: dkjson encodes numbers with `tostring` (`%.14g`), the oracle with `%.17g` **[R]**, so values through existing commands are not bitwise | `api.output` must emit numbers with `%.17g` (as `oracle/json.lua` does); M0 test asserts bitwise equality with `tests/oracle/*.json` |

## Appendix: reproducing the spikes

```sh
git submodule update --init --depth 1
cargo build --release -p cli                 # 8 min cold
echo '{"cmd":"state"}' | target/release/pob serve
target/release/pob bench corpus/generated/gen-0001.xml
apt-get install -y luarocks && oracle/setup.sh   # LuaJIT backend only
```

LuaJIT serve loop used for backend B (run with cwd `upstream/PathOfBuilding/src`
and the `LUA_PATH`/`LUA_CPATH` of `oracle/run.py`'s `lua_env`; args: `oracle/`
dir, path to `crates/session/lua/session.lua`):

```lua
-- Serves crates/session/lua/session.lua on upstream headless LuaJIT: JSON lines on stdin/stdout
local oracle, sessionLua = arg[1], arg[2]
package.path = oracle .. "/?.lua;" .. package.path
local headless = require("headless")
local stdin, stdout = io.stdin, io.stdout
-- upstream logs to stdout; keep stdout for responses
print = function(...)
	local t = { ... }
	for i = 1, select("#", ...) do t[i] = tostring(t[i]) end
	io.stderr:write(table.concat(t, "\t"), "\n")
end
io.write = function(...) io.stderr:write(...) end
headless.start()
ConPrintf = function(fmt, ...) io.stderr:write(string.format(fmt, ...), "\n") end
local api = dofile(sessionLua)
api.handle('{"cmd":"newBuild"}')
for line in stdin:lines() do
	if line:match("%S") then
		stdout:write(api.handle(line), "\n")
		stdout:flush()
	end
end
```
