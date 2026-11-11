# Orchestrator prompt: MCP server for build creation

You are the ORCHESTRATOR of the implementation phase. Plan, delegate,
review and merge-ready PRs; write code yourself only where delegating costs
more than it saves. Read `AGENTS.md`, `docs/design.md` and
`docs/mcp/ANALYSIS.md` first: the analysis has the verified facts, numbers
and command shapes this prompt relies on.

## Goal

An MCP server through which Claude creates, modifies and evaluates PoE 1
builds autonomously ("build me a crit cold DoT Occultist with 6M DPS and 8k
EHP"): it calls tools, gets upstream-exact calculated stats back and
iterates. The final deliverable is a demo in which an agent does exactly
that from a text goal and reports stats and a build code that opens in
upstream Path of Building.

## Architecture (decided, see ANALYSIS.md section 5)

- New crate `crates/mcp`, binary `pob-mcp`: an MCP server over stdio
  (JSON-RPC 2.0, newline-delimited), hand-rolled on `serde_json`, no async
  runtime (`Session` is `!Send`). Tools are written once against a
  `Backend` trait whose only method is "send one engine API JSON request,
  get the JSON response":
  - `InProcess`: `session::Session` (upstream translated to Rust). Default.
  - `LuaJit`: a subprocess running upstream's `HeadlessWrapper.lua` (via
    `oracle/headless.lua`) plus the same `crates/session/lua/session.lua`
    in a stdin/stdout loop. Verified to answer identically and 3-7x faster;
    needs `oracle/setup.sh` (LuaJIT, luarocks). Its `Deflate`/`Inflate` are
    stubs, so the MCP layer encodes/decodes build codes itself (zlib +
    URL-safe base64 of `saveXml`) for both backends.
- The engine API (`crates/session/lua/session.lua`) is the only place that
  touches upstream state. Missing capabilities become new `api.*` commands
  there, written the way the existing ones are (calling upstream's own
  functions, as its UI does), with tests in `crates/session/tests`. Keep
  them plain Lua against upstream globals so both backends run them.
- Agent-facing conveniences (fuzzy search, name -> id resolution, compact
  stat summaries, diffs) live in `crates/mcp` in Rust over cached JSON from
  the session; they never compute game numbers.
- Why: the session already runs upstream's whole program with oracle-exact
  results, already speaks JSON (`pob serve`), and is what the web app uses,
  so the MCP server adds no second engine and no upstream edits, and follows
  the typed engine for free as it replaces the translation behind the same
  commands. The LuaJIT backend buys speed and a live cross-check with real
  upstream at the cost of an optional native dependency.

Measured (ANALYSIS.md section 2): in-process startup ~6.5 s, load 2.2 s,
~400 ms per tree edit, node power 18-33 s, ~1-1.5 GB RSS; LuaJIT startup
2.4 s, load 0.4 s, ~90 ms per edit, node power ~5 s. Cold
`cargo build --release -p cli` 8 min on 4 cores. The submodule must be
initialised (`git submodule update --init --depth 1`).

Key paths:

| Path | What |
| --- | --- |
| `crates/session/src/lib.rs` | `Session::new(data)`, `Session::request(json) -> json` |
| `crates/session/lua/session.lua` | every command (`api.<cmd>`), dispatch in `api.handle` |
| `crates/session/tests/session.rs` | how commands are tested on corpus builds |
| `crates/cli/src/main.rs` | `pob serve` (JSON lines), `pob bench`; copy its `src_dir()` |
| `upstream/PathOfBuilding/src` | data read at run time (`POB_UPSTREAM_SRC` overrides) |
| `corpus/generated/*.xml`, `tests/oracle/*.json` | golden builds and their upstream outputs |
| `web/src/engine/types.ts` | TypeScript shapes of the existing responses |
| `sync/manifest.toml` | update if a new file ports upstream behaviour |

## Constraints

- Never edit `upstream/PathOfBuilding`, generated code (`crates/upstream`,
  `mod_parser/generated.rs`), `tests/oracle`, the corpus or
  `tests/tolerances.toml`. Expected numbers come from the oracle only.
- Minimal invasiveness: new commands are additive in `session.lua`; do not
  change existing command shapes (the web app depends on them, see
  `web/src/engine/types.ts`). If one must change, change the web client in
  the same PR and keep `web/tests` green.
- Do not break the web app or the GUI paths: `cargo test --release`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`,
  `sync/check.py`, and the `web`/`wasm` CI jobs stay green.
- One PR per milestone, squash merged by the owner; each PR reviewable on
  its own (target < ~800 changed lines excluding tests/fixtures). Brief
  commit messages. Feature branch per milestone.
- Editing `session.lua` re-runs luagen for the session crate only (seconds);
  never touch `upstream/PathOfBuilding/src` (rebuilds the translated crate,
  ~15+ minutes).
- Release builds only for anything timed or tested end to end; debug builds
  of the translated program are far slower.

## Milestones

Each milestone: branch, PR, acceptance criteria met, CI green.

### M0 Spike: one tool, real DPS (sequential, do it yourself or one agent)

`crates/mcp` with `initialize`, `tools/list`, `tools/call` and a single tool
`evaluate_build {xml | code}` returning `CombinedDPS`, `TotalEHP`, `Life`,
`EnergyShield`, resists, from a new `api.output` command that returns
`build.calcsTab.mainOutput` numerically (filtered to requested keys, or a
default summary set). dkjson writes numbers with `tostring` (`%.14g`), so
`api.output` must emit them with `%.17g` like `oracle/json.lua`, or exact
comparison with the oracle fails.

Acceptance:
- `crates/mcp/tests/` spawns `pob-mcp`, does the MCP handshake, calls
  `evaluate_build` on `corpus/generated/gen-0001.xml` and asserts the values
  equal `tests/oracle/gen-0001.json` `MAIN.player` exactly (bitwise, via
  `%.17g`/serde round trip).
- `claude mcp add pob -- target/release/pob-mcp` works locally; a Claude
  Code session lists the tool and gets the numbers.
- Startup and per-call latency measured and recorded in the PR.

### M1 Engine API additions (session.lua; parallelisable by area)

New commands, each with a test in `crates/session/tests` on corpus builds:
- `output {keys?, actor?: "player"|"minion"}`: numeric outputs (done in M0,
  extend with minion and Full DPS table `SkillDPS`).
- `setClass {className}`, `setAscendancy {name}` (wrap the class/ascend
  drop-down handlers; surface the "class change resets tree" popup as an
  error unless `force`).
- `setLevel {level}`, `setBandit`, `setPantheon` (via existing controls, or
  thin wrappers).
- `setConfig {var, value}` and `configOptions` (typed: check/count/list with
  allowed values), wrapping `configTab` controls' handlers.
- `itemBases {type?}` (name, type, implicit lines, requirements).
- `parseItem {text}`: base found, mod lines with `extra` (unparsed rest) so
  the agent can see which lines did nothing; and `itemMods {base}` listing
  craftable prefix/suffix lines with tiers for crafted rares.
- `allocNodes {ids}` / `deallocNodes {ids}` (batch, one recalc), and
  `nodeSearch` is NOT needed here (done in Rust over `tree`).
- `gemInfo {name}` (tags, levels' stats, supported types) if gemList is not
  enough for picking supports.

Parallel: config/class/level (agent A), items (agent B), tree batch +
output (agent C). They all touch `session.lua`: give each a disjoint
section, rebase sequentially before merge; or land as 2-3 PRs in sequence.

### M2 MCP tool surface (crates/mcp; parallelisable after M1 lands)

Implement the tools in ANALYSIS.md section 6, with JSON schemas, concise
outputs (no tree geometry, no tooltips unless asked), and errors that tell
the agent how to fix the call. Multi-build: named build slots
(`build_id`), each its own `Session` (or saved XML reloaded on demand if
memory is too high; measure). Splits:
- build lifecycle + stats + compare (agent A)
- tree tools: search, allocate by name/id with path, node power top-N
  (agent B)
- skills + gems search (agent C)
- items: search uniques/bases, equip unique by name, craft rare from mod
  lines, parse check (agent D)
- config tools (agent E)
Each agent adds integration tests in `crates/mcp/tests` against golden
builds.

Acceptance: every tool has a test; a scripted MCP session builds a
level-90 character from `newBuild` (class, ascendancy, ~100 points by
notable names, one 6-link, uniques and a crafted rare in every slot,
config for a boss), and the resulting build code loads in `pob serve`
(and in upstream via the oracle runner: `oracle/headless.lua` loading the
saved XML gives identical `CombinedDPS`).

### M3 Agent ergonomics and speed

- `suggest_nodes {stat, budget}`: node power (`nodePower`) top-N with path
  cost and per-point gain; batch apply.
- `what_if {edits}`: apply edits to a copy, return stat deltas, discard.
- Keep tool outputs small (summaries by default, `detail` flag for more).
- `LuaJit` backend (`--backend luajit`): a Lua serve loop under
  `crates/mcp/lua/` (reuses `oracle/headless.lua`; upstream logs to stdout,
  so redirect `print`/`ConPrintf` to stderr), plus a test (ignored without
  `.oracle/`) that both backends give identical `get_stats` for every
  golden build and for the M2 scripted build.
- Measure a 50-iteration optimisation loop; if per-edit recalc is the
  bottleneck, report numbers and propose next typed-engine targets (do not
  start that work here).

### M4 Packaging

- Claude Code cloud and local: a `.mcp.json` at repo root pointing at
  `target/release/pob-mcp`, a `SessionStart` hook (or setup script) that
  builds it, and/or a CI job uploading a prebuilt Linux binary as a release
  artifact the hook downloads (the build of the translated crate is long,
  see ANALYSIS.md).
- `docs/mcp/README.md`: install, tool list, examples.

### M5 Demo (final)

A fresh Claude session with only the MCP server and a short system prompt
receives: "Build me a crit cold DoT Occultist with 6M DPS and 8k EHP". It
must, without human help: create the build, iterate with tools, and report
final `CombinedDPS`/`TotalDotDPS`, `TotalEHP`, life/ES, resists, the main
skill setup, and the build code. Acceptance: the code, loaded by upstream
headless (`oracle/headless.lua`), reproduces the reported numbers exactly;
transcript and code committed under `docs/mcp/demo/`. If the targets are
unreachable with reasonable gear, the agent must say so and report the best
achieved; record that honestly.

## Delegation

- Sequential: M0 (sets the crate skeleton and test harness), then M1's
  first PR (`output`), then the rest.
- Parallel: M1 areas, M2 tool groups, M4 packaging alongside M3.
- Give each subagent: the milestone section, the file paths above, the
  command conventions of `session.lua`, and the acceptance tests it must
  add. Ask for PR-ready branches with test output in the summary.
- Review every subagent diff yourself against the constraints before
  opening the PR; run the tests yourself, do not trust summaries.

## Testing strategy

- Golden builds: `corpus/generated/gen-*.xml` and the 3.13 builds, with
  upstream's outputs in `tests/oracle/<id>.json`. Any number a tool reports
  for a loaded corpus build must equal the oracle's.
- Edits: compare to upstream by saving the edited build's XML and running
  it through `oracle/headless.lua` (LuaJIT, `oracle/setup.sh`; needs
  `luarocks`) — write a small helper test (`#[ignore]` unless the oracle is
  installed) that does this for the M2 scripted build and the M5 demo.
- Round trip: build code out → `loadCode` → identical outputs.
- Protocol: MCP handshake, tool schema validity, error paths (unknown node
  name, bad item text, unknown gem) return `isError` with a helpful message.
- Run `cargo test --release -p session -p mcp` per PR; the full
  `cargo test --release` before merge (equivalence tests take minutes each).
