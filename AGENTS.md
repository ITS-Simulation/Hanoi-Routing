# Repository Guidelines

## Project Structure & Module Organization
- `RoutingKit/`: C++ routing library (CH/CCH, OSM loaders, `bin/test_*` executables).
- `CCH-Generator/`: C++17 tools for generating and validating RoutingKit-format graphs (`build/cch_generator`, `build/validate_graph`).
- `CCH-Hanoi/`: Rust workspace hub (`hanoi-core`, `hanoi-cli`, `hanoi-tools`) for Hanoi-specific integration, CLI exposure, and turn-expanded graph tooling.
- `rust_road_router/`: Rust workspace (engine, server, conversion, research crates).
- `Maps/`: source `.osm.pbf` inputs and generated graph vectors under `Maps/data/*`.
- `scripts/`: helper scripts (`pipeline_test`, `graph_binary_viewer`).
- `docs/`: design notes and `CHANGELOGS.md` (update this for every code/doc change).

## Build, Test, and Development Commands
- Build CCH generator:
  - `cmake -S CCH-Generator -B CCH-Generator/build -DCMAKE_BUILD_TYPE=Release`
  - `cmake --build CCH-Generator/build -j"$(nproc)"`
- Build RoutingKit:
  - `cd RoutingKit && ./generate_make_file && make -j"$(nproc)"`
- Build line-graph generator:
  - `cargo build --release --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-tools --bin generate_line_graph`
- Build CCH-Hanoi workspace crates:
  - `cargo build --release --manifest-path CCH-Hanoi/Cargo.toml --workspace`
- Build Rust workspace:
  - `cargo build --release --manifest-path rust_road_router/Cargo.toml`
- Run end-to-end graph pipeline:
  - `CCH-Generator/scripts/run_pipeline Maps/hanoi.osm.pbf`

## Coding Style & Naming Conventions
- C++: C++17, existing files use tab-indented blocks, braces on same line, `snake_case` for functions/variables, `CamelCase` for types.
- Rust: follow `rustfmt` defaults (`rust_road_router/rustfmt.toml` sets `max_width = 160`); run `cargo fmt --all` in `rust_road_router`.
- Scripts: Bash with `set -euo pipefail`; Python/Rust helpers use descriptive `snake_case` file names.
- Keep generated binaries and temporary outputs out of source directories; write derived data to `Maps/data/`.

## Time Unit Conventions
- Persisted `travel_time` values in RoutingKit-format graph vectors are **milliseconds** (`u32`).
- Canonical OSM conversion used in this repo (`geo_distance[m] * 18000 / speed[km/h] / 5`) yields milliseconds.
- In `rust_road_router`, integer TD routing (`time_dependent` module) also uses millisecond timestamps/weights (for example, one-day period is `86_400_000`).
- `tt_units_per_s` is the authoritative metadata for integer datasets; current project pipelines/importers use `tt_units_per_s = 1000` (ms).
- `floating_time_dependent` / CATCHUp code uses **seconds internally** (`f64`) and performs explicit conversion from/to millisecond integer inputs.
- Some legacy comments/messages still mention seconds in RoutingKit; treat executable formulas and persisted data contracts as source of truth.

## Testing Guidelines
- Run Rust tests: `cargo test --workspace --manifest-path rust_road_router/Cargo.toml`.
- Run CCH-Hanoi tests/checks: `cargo test --workspace --manifest-path CCH-Hanoi/Cargo.toml`.
- Validate generated graphs:
  - `CCH-Generator/build/validate_graph <graph_dir>`
  - `CCH-Generator/build/validate_graph <graph_dir> --turn-expanded <graph_dir>/line_graph`
- RoutingKit regression checks are executable tests in `RoutingKit/bin/test_*` (run relevant binaries for touched components).
- No fixed coverage threshold is enforced; include at least one regression path for behavior changes.

## Commit & Pull Request Guidelines
- This root folder currently has no usable git history; follow the established changelog pattern in `docs/CHANGELOGS.md` (`YYYY-MM-DD — short title` + concise change bullets).
- Use imperative, scoped commit subjects (example: `CCH-Generator: validate conditional turn bounds`).
- PRs should include:
  - problem statement and scope,
  - affected paths/modules,
  - commands run for build/test/validation,
  - sample output or metrics when pipeline/data behavior changes.

## Planning Guidelines
- For Plan mode, all plans must be generated to `docs/planned/` folder

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **Hanoi-Routing** (18560 symbols, 45640 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## When Debugging

1. `gitnexus_query({query: "<error or symptom>"})` — find execution flows related to the issue
2. `gitnexus_context({name: "<suspect function>"})` — see all callers, callees, and process participation
3. `READ gitnexus://repo/Hanoi-Routing/process/{processName}` — trace the full execution flow step by step
4. For regressions: `gitnexus_detect_changes({scope: "compare", base_ref: "main"})` — see what your branch changed

## When Refactoring

- **Renaming**: MUST use `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` first. Review the preview — graph edits are safe, text_search edits need manual review. Then run with `dry_run: false`.
- **Extracting/Splitting**: MUST run `gitnexus_context({name: "target"})` to see all incoming/outgoing refs, then `gitnexus_impact({target: "target", direction: "upstream"})` to find all external callers before moving code.
- After any refactor: run `gitnexus_detect_changes({scope: "all"})` to verify only expected files changed.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Tools Quick Reference

| Tool | When to use | Command |
|------|-------------|---------|
| `query` | Find code by concept | `gitnexus_query({query: "auth validation"})` |
| `context` | 360-degree view of one symbol | `gitnexus_context({name: "validateUser"})` |
| `impact` | Blast radius before editing | `gitnexus_impact({target: "X", direction: "upstream"})` |
| `detect_changes` | Pre-commit scope check | `gitnexus_detect_changes({scope: "staged"})` |
| `rename` | Safe multi-file rename | `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` |
| `cypher` | Custom graph queries | `gitnexus_cypher({query: "MATCH ..."})` |

## Impact Risk Levels

| Depth | Meaning | Action |
|-------|---------|--------|
| d=1 | WILL BREAK — direct callers/importers | MUST update these |
| d=2 | LIKELY AFFECTED — indirect deps | Should test |
| d=3 | MAY NEED TESTING — transitive | Test if critical path |

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/Hanoi-Routing/context` | Codebase overview, check index freshness |
| `gitnexus://repo/Hanoi-Routing/clusters` | All functional areas |
| `gitnexus://repo/Hanoi-Routing/processes` | All execution flows |
| `gitnexus://repo/Hanoi-Routing/process/{name}` | Step-by-step execution trace |

## Self-Check Before Finishing

Before completing any code modification task, verify:
1. `gitnexus_impact` was run for all modified symbols
2. No HIGH/CRITICAL risk warnings were ignored
3. `gitnexus_detect_changes()` confirms changes match expected scope
4. All d=1 (WILL BREAK) dependents were updated

## Keeping the Index Fresh

After committing code changes, the GitNexus index becomes stale. Re-run analyze to update it:

```bash
npx gitnexus analyze
```

If the index previously included embeddings, preserve them by adding `--embeddings`:

```bash
npx gitnexus analyze --embeddings
```

To check whether embeddings exist, inspect `.gitnexus/meta.json` — the `stats.embeddings` field shows the count (0 means no embeddings). **Running analyze without `--embeddings` will delete any previously generated embeddings.**

> Claude Code users: A PostToolUse hook handles this automatically after `git commit` and `git merge`.

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
