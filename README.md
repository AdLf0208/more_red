# More Red — a Horizon port

**More Red** is a Minecraft mod. A minecraft forge mod that adds redstone logic gate plates. This repository is a **port of it to [Horizon](https://github.com/horizon-rust/horizon)**, the Rust engine for Minecraft Java 26.3. It carries the complete source of the port plus the content files it ships, and installs as a single `.hmod` package.

Upstream: [Commoble/morered](https://github.com/Commoble/morered) — licence **MIT**.

| | |
|---|---|
| Upstream mod | More Red ([source](https://github.com/Commoble/morered)) |
| Ported to | Horizon 0b14f8a, Minecraft Java 26.3, mod API `0.2` |
| Package | `morered.hmod` (install it with the Horizon launcher) |
| Licence | **MIT** — the upstream mod's licence; see [Licence and credit](#licence-and-credit) |

## What this port implements

- **The relay is not upstream's channel bridge.** Upstream has no channel
- **A relay answers a weak signal only** (`direct = false`). Its first version
- **The cable uses `minecraft:basalt` + `axis`** for the same pin state-order
- **NBT shape**: `power` is a compound of six sixteen-int arrays
- **Relay reads the strongest attached subwire** of an adjacent cable (all six
- **Recipes/loot**: the cable's `#morered:network_cables` key became
- **Model**: the jar's multipart base part uses the `morered:wire_parts`
- **Solve timing**: the cable solve shares the wire's per-tick worklist and

## Not yet ported

- ~~Placement orientation is not ported (`state_for_placement`)~~: **resolved in
- The carried `end_rod` template gives the plates their 2-px shape and
- The pulse clear lives in an in-memory map: a gate whose chunk is unloaded
- Inputs are sampled once per tick (the dirty set is applied at
- The coloured network cables, the **bundled cable** posts/spools and soldering
- A comparator's strength collapses to a boolean (the gates are binary).
- **The wire state carries `axis`** (the template's shape selector) on top of
- **No `TRANSFORM`** state (`EightGroup`), so structure rotation/mirroring of

## Installing

1. Install the [Horizon launcher](https://github.com/horizon-rust/horizon/releases) (0.2.70 or newer).
2. Add `morered.hmod` to the instance's mods.
3. The package is pinned to a specific Horizon build; the launcher checks that pin.

## Licence and credit

More Red was created by its upstream authors; the code, models and assets in the original mod
remain theirs and are used here under **MIT**. The Rust in this repository is a clean-room
rewrite for Horizon, not a translation of the original source.

- `LICENSE` — the upstream licence text.
- `LICENSE-NOTICE.md` — what this port carries and where it came from.
- The upstream project: https://github.com/Commoble/morered

## Technical details

```
horizon-mod.toml   package manifest (id, version, api, [native] pin)
Cargo.toml         the crate; depends on `horizon-mod-api` from the fetched SDK
src/               lib.rs (the one `impl Hooks`), runtime.rs (pure logic + tests), glue
assets/ data/      the content the package ships
tools/             content generators and the e2e scenario
```

- Build: `cargo horizon-mod build` (packages; it does not compile against the game).
- Test: `cargo test`, `cargo clippy --all-targets -- -D warnings`.
- In-game: `ports/tools/launcher-game.sh run morered` in the campaign tree, or install the
  `.hmod` and play; the harness scenario is `tools/e2e.sh`.

This repository is generated from the campaign's port at `ports/morered/`; the port's own
`notes.md` is the full record (decisions, traps, evidence).
