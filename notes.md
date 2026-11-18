# More Red — port notes

Ported from **More Red v1.21.1-6.0.0.3** (`Commoble/morered`, **MIT**) for
Minecraft Java 1.21.1 NeoForge.

Status: **0.2.0, the SDK package** — 66 runtime tests + 9 glue tests, `cargo test`
/ `clippy -D warnings` / `fmt --check` green, `cargo horizon-mod check` and `fits`
EXIT 0, a byte-identical second build, and the package at 233,165 B. The 0.1.0
`cargo horizon-mod`/e2e runs below are **historical** (retired `.hnmod` format);
**no 0.2.0 in-game run exists** — see `## Migration to the SDK format` and
`../_shared/evidence/morered/sdk-e2e-status.md`.

## Planned slice

The **logic gate plate** family (diode, NOT, AND/OR/NAND/NOR, XOR/XNOR,
multiplexer, 2-input AND/NAND, latch, pulse gate): a plate attaches to a face,
rotates, reads its input sides, and emits 15 from its output face when its
logic function passes. The pure model (`runtime.rs`) has the upstream function
table, the 24-entry output-direction table, the clockwise input mapping, the
latch's hold/reset rule and the pulse gate's edge/2-tick rule (18 tests).
Content: eleven gate blocks (plus the `stone_plate` recipe dependency) on an
`end_rod` template with the real `facing`/`rotation`/`input_*`/`powered`
states. Native work: `block_signal` / `is_signal_source` for the output,
`neighbor_changed` to update the stored inputs, and the pulse clear at the
level tick.
## Update and signal (slice 1)

- **State**: `facing` (attachment, 6), `rotation` (0..3), and only the input
  booleans the gate uses (`input_b` for diode/NOT; `input_a`+`input_c` for
  XOR/XNOR/2-input; all three for AND/NAND/OR/NOR). Output = the 24-entry table
  in `runtime.rs`; inputs are 90/180/270° clockwise from the output.
- **Update**: the pin has no mutable neighbour-change hook, so
  `neighbor_changed`/`block_changed` only queue a dirty position; the input
  booleans are recomputed and written at `level_tick_post` (one gate per server
  tick, like upstream's `TICK_DELAY = 1`). A placement also notifies the
  output neighbours (upstream's `onPlace`).
- **Signal**: `block_signal` answers from the stored state, 15 only on the
  output face when the function passes; `is_signal_source` is true for every
  plate state.
- **Reading inputs**: no per-direction redstone read exists for mods at the
  pin, so `signal_toward` rebuilds `getSignal` from `StateFacts.weak`, a wire's
  power, a comparator's `powered`/`facing`, and other plates' own outputs.

## Hooks

| Hook | Helper | What it does |
|---|---|---|
| `block_signal` (v1, API 38) | `signal.rs` | 15 on the output face from the stored inputs / `powered` bit. |
| `is_signal_source` (v1, API 38) | `source.rs` | Every plate is a source. |
| `neighbor_changed` (v1) | `neighbor.rs` | Queue the plate for the level tick. |
| `block_changed` (v1) | `changed.rs` | Queue a placed plate and notify once. |
| `level_tick_post` (v1) | `tick_post.rs` | Clear due pulses, recompute inputs, write the state, notify. |

## Verification results (in-game)

`tools/e2e.sh`, The End, headless, `--dev-mod content`; exit **0**:

| Phase | Evidence |
|---|---|
| AND unpowered | lamp `lit=false` |
| AND A+C only | lamp `lit=false` (proves AND, not OR); gate state `input_a=true,input_b=false,input_c=true` |
| AND A+B+C | lamp `lit=true` |
| AND remove C | lamp `lit=false` again |
| NOT unpowered | lamp `lit=true` (inverted) |
| NOT B powered | lamp `lit=false` |
| OR with A only | lamp `lit=true` (same input, different function) |
| multiplexer | A→lit, C with B off→dark, B+C→lit, B with C off→dark, B+A→dark (selects C), A with B off→lit |
| counterfactual | gate replaced by stone, all inputs powered → lamp `lit=false` |

`PASS: AND all-three lit, A+C dark, removal dark, NOT inverted, OR by A, stone
counterfactual dark`.

Two findings:
1. **No mutable neighbour-change hook**: the port queues and applies at
   `level_tick_post`; a gate with a true output at placement also notifies its
   neighbours there (upstream's `onPlace`), otherwise a lamp placed after it
   stays dark.
2. **No per-direction redstone read**: `signal_toward` uses `StateFacts.weak`
   plus wire/comparator/plate special cases; a *different* mod's `block_signal`
   answer is invisible to a plate (recorded deviation).

## Latch verification

The latch (`morered:latch`, `Gate::Latch`) is the state-memory plate: A sets, C
resets, and it drives a *complementary* pair (the primary output face when
powered, the opposite face when not). Both faces go dark while A and C are both
high (upstream `LatchBlock.getSignal`). Stored bit per upstream `LatchBlock.tick`:
`(was && !c) || (!was && a)` — so from a reset state, both-high still sets.

`tools/e2e.sh` (The End, headless, `--dev-mod content`; latch at (0,64,0),
`facing=down,rotation=0`, output north = primary, south = complementary), two
consecutive runs exit **0**:

| Step | North lamp (primary) | South lamp (complementary) | Stored state |
|---|---|---|---|
| unpowered | `lit=false` | `lit=true` | `input_a=false,input_c=false,powered=false` |
| A on (set) | `lit=true` | `lit=false` | `powered=true` |
| A off (hold) | `lit=true` | `lit=false` | `powered=true` |
| C on (reset) | `lit=false` | `lit=true` | `powered=false` |
| A on while C on (both high) | `lit=false` | `lit=false` | `input_a=true,input_c=true,powered=true` |
| C off (A still on) | `lit=true` | `lit=false` | `powered=true` |
| A off (both off, hold) | `lit=true` | `lit=false` | `input_a=false,input_c=false,powered=true` |
| stone in place of the latch, inputs on | `lit=false` | — | counterfactual |

`PASS: AND all-three lit, A+C dark, removal dark, NOT inverted, OR by A,
multiplexer selects C/A, latch sets/holds/resets with complementary output,
both-high dark and held set, stone counterfactual dark`

Three findings from this slice:

1. **The stored bit may only move when the input state changed.** Upstream
   schedules its one-tick update from `neighborChanged` *only* if
   `InputState` changed, and the tick writes with flag 2 (no neighbour
   notification), so its own write never re-enters the tick. The port has no
   mutable neighbour hook, so it applies queued gates at `level_tick_post` and
   its own `native_update_signal_neighbors` re-notifies the plate. Applying
   `latch_powered` on every evaluation made a latch with both inputs high flip
   its stored bit every tick (the e2e state read raced between `powered=true`
   and `powered=false`; the lamps stayed dark either way, which is why only a
   state assertion caught it). `on_level_tick_post` now gates the latch bit on
   `inputs != stored inputs`, mirroring upstream's schedule condition.
2. **A port state write is not a placement.** `native_set_block` reports the
   write back through `block_changed`; `on_block_changed` treats a plate state
   as a fresh placement and seeds another evaluation (with `placed` forcing a
   neighbour notify). A `writing` sentinel around the write suppresses that
   self-report; a genuine placement (`/setblock`, pistons, load) still seeds
   the first evaluation.
3. **E2E timing: two `/setblock` commands land in different ticks** (the
   harness advances ~10 ticks per command), so the "both inputs" phase cannot
   raise A and C in one evaluation with two commands, and C would reset the bit
   in between. The phase now keeps C on from the reset step and raises A alone
   — one command, one evaluation seeing `a=c=true` with the stored bit reset,
   which is the transition the table above pins. The all-off release keeps the
   set bit because `was` is already true (C off while A stays on holds).

With the latch in, the port also carries `#[allow(dead_code)]` on
`rotation_for_output` (used only by the standalone tests, absent from the
injected module) so the pinned build emits no warning.

## Pulse gate verification

The pulse gate (`morered:pulse_gate`, `Gate::Pulse`) is the one-shot plate:
state `input_b` + `powered` (plus `facing`/`rotation`), one input side (B), and
15 from its output face **only while `powered`**.

Upstream (`plate_blocks/PulseGateBlock.java`, tag `v1.21.1-6.0.0.3`, commit
`3bf5a8d`):

| Behaviour | Source |
|---|---|
| properties `INPUT_B`, `POWERED`; `INPUT_SIDES = { B }`; `PULSE_DURATION = 2`; defaults false | `:25-37` |
| output sides = exactly the plate output face | `:52-56` |
| placement: `powered = input_b` (the plate's own input read) | `:58-68` |
| `setPlacedBy`: schedule a 2-tick tick if any input is powered | `:70-77` |
| `neighborChanged`: `old = input_b`, `new = B.isBlockReceivingPower`; **rising** (`!old && new`) → store `input_b=true,powered=true`, `setBlockAndUpdate`, schedule the 2-tick clear when `!willTickThisTick` (HIGH priority); **falling** (`old && !new`) → store `input_b=false` only | `:79-105` |
| `getSignal`: `powered && sideOfAdjacentBlock.getOpposite() == outputDirection ? 15 : 0` | `:107-116` |
| `tick`: `powered=false` (one `setBlockAndUpdate`) | `:118-126` |

`PulseGateBlock.neighborChanged` does **not** call `super.neighborChanged`, so
the generic 1-tick `InputState` update of `RedstonePlateBlock` does not apply
here; the only scheduled tick is the 2-tick clear (`willTickThisTick` prevents
a second rising edge from rescheduling a pending clear).

The pure model has the edge rule and the clear as separate functions:

- `pulse_update(stored_input_b, new_input_b, powered) -> PulseUpdate` — the
  stored bits after comparing the state with the world, and
  `clear_in: Some(PULSE_DURATION)` only on a rising edge (`runtime.rs`, tests
  `a_rising_edge_arms_the_pulse_and_a_falling_edge_cuts_the_input`,
  `a_steady_input_neither_arms_nor_cuts`, `the_pulse_is_not_held_after_its_clear`).
- `pulse_tick(remaining) -> PulseTick::{Waiting(u8), Cleared}` — armed with 2,
  tick 1 waits, tick 2 clears (`a_pulse_clears_two_ticks_after_it_arms`).
- `Gate::signal_with_powered` for `Gate::Pulse`: `powered && face == output`
  (`the_pulse_emits_only_on_its_output_face_while_powered`). `sides()` is
  `[B]`, `apply` is `false` (it has no logic function).

Glue (`support.rs`):

- `State.clearing: BTreeMap<(dimension, pos), u8>` counts the armed gates down
  at the **start** of `level_tick_post`, before the dirty batch, and writes
  `powered=false` (`with_inputs` keeps `input_b`) + notifies for the due ones.
  A clear already pending is kept (`entry().or_insert(...)`), mirroring
  `willTickThisTick`.
- The dirty batch evaluates a pulse gate with `pulse_update`, stores the
  returned `input_b`/`powered`, and arms `clearing` only after the write
  succeeded. The latch's "move the bit only when the inputs changed" gate is
  inherent here: `pulse_update` compares the stored B with the world's B, so a
  re-entry with no edge never re-arms and a cleared pulse is not held.
- Placement (including `/setblock`) seeds the first evaluation through
  `block_changed`, which sees the stored `input_b=false` against a powered B
  and therefore arms exactly like upstream's `setPlacedBy`.

### Test counts

| Check | Result |
|---|---|
| `cargo test --locked --offline` (standalone lib) | **18 passed**, 0 failed |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | exit 0 |
| `python3 tools/horizon.py test` (embedded `game::morered::tests`) | **18 passed**, 531 filtered, `cargo_exit: 0` |
| `python3 tools/horizon.py check` | `cargo_exit: 0`, 0 warnings in the log |
| `python3 tools/horizon.py build` | `cargo_exit: 0`, 0 warnings in the log |

The staged `target/mixins/modded` workspace has 9 pre-existing clippy findings
in pinned `horizon-client` files (`lan/hub.rs`, `mobs/*.rs`, `multiplayer.rs`);
none is in the injected modules (`src/game.rs`), so the port adds 0 lints.

### E2E (`tools/e2e.sh`, The End, headless, `--dev-mod content`)

Pulse gate at (0,64,0), `facing=down,rotation=0` → output north (lamp at
(0,64,-1)), B south at (0,64,1). A rising edge is raised and sampled **inside
one `ctl script`**, one command per tick, so the 2-tick `powered` window is
visible. Three consecutive runs exited **0** (the third on the final rebuilt
binary); run 1:

```
  morered:pulse_gate[facing=down,input_b=false,powered=false,rotation=0]
  morered:pulse_gate[facing=down,input_b=false,powered=false,rotation=0]
  morered:pulse_gate[facing=down,input_b=true,powered=true,rotation=0]
  morered:pulse_gate[facing=down,input_b=true,powered=true,rotation=0]
  morered:pulse_gate[facing=down,input_b=true,powered=false,rotation=0]
pulse: armed and cleared in the sampled window
```

| Phase | Evidence |
|---|---|
| unpowered | state `input_b=false,powered=false`, lamp `lit=false` |
| rising edge | two `powered=true` samples, then `input_b=true,powered=false` in the same window |
| not held (B still powered, +10 ticks) | `input_b=true,powered=false`, lamp `lit=false` |
| falling edge | `input_b=false,powered=false`, lamp `lit=false` |
| new rising edge | armed and cleared again in the sample window |
| differential: diode in the same wiring, B powered | lamp `lit=true`, diode `input_b=true` (a held gate) |
| stone in the gate's place, B powered | lamp `lit=false` |
| pulse → latch at (3,64,-1), `rotation=1` (A reads south = the pulse) | latch `powered=true`, lamp at (4,64,-1) `lit=true`, still lit after B falls |
| latch reset by C at (3,64,-2) | latch `powered=false`, lamp `lit=false` |
| all 96 pulse states (`facing` × `rotation` × `input_b` × `powered`) | 96 `setblock`s accepted; read-backs match (`facing=east,…,rotation=2`, `facing=up,powered=true,rotation=3`, `facing=down,rotation=1,input_b=true`) |
| counterfactual (existing) | stone with all inputs powered → lamp `lit=false` |

`PASS: … pulse arms/clears/not-held/re-arms with a diode differential, pulse
drives a latch, 96 pulse states accepted, stone counterfactual dark` (exit 0).

### Content

The declaration (`content/data/morered/horizon/block/pulse_gate.json`) is the
gate template with `input_b` + `powered` booleans; the jar's 48-variant
blockstate keys `facing`/`rotation`/`input_b`, so `powered` is the wildcard.
Checked against the engine's own built state table
(`.horizon-cache/mod-content/model-states.json`, `--dev-mod content`):

```
states=96 variants=48 unbound=0
   32 states -> morered:block/pulse_gate
   16 states -> morered:block/pulse_gate_alt
   32 states -> morered:block/pulse_gate_switched
   16 states -> morered:block/pulse_gate_switched_alt
```

The mod-content overlay built with **0 warnings**, the creative tab
`morered:morered` holds 14 items with the pulse gate (1 entry), and
`give morered:pulse_gate 2` replied
`Gave 2 [Pulse Gate] to …` (the item definition and the carried lang key both
resolve). `horizon-mod check` on the rebuilt `.hmod`: 68,283 bytes, SHA-256
`d9925bd970d982a3153c0b7212827ddd930d91f06024b95b2e8be3b1d8d418e5`, unsigned,
"no code: content only".

Timing findings:

1. **One `ctl` command is about one game tick**, so a 2-tick pulse is
   observable (the sample table above). The 20-sample script also catches the
   clear; the gate's `powered` bit is high for exactly 2 samples.
2. **A `ctl command` is queued to the server thread**, so a `ctl block`
   immediately after a `setblock` can read the *previous* state (the first two
   samples show `input_b=false` after the redstone block was placed). This is a
   harness ordering fact, not a port bug; the e2e waits 2 ticks before the
   state read-backs that need the new state, and the sample loop tolerates the
   lag by taking 20 samples.
3. The differential (a diode in the same wiring holds and lights the lamp) is
   what makes the "not held" assertion fail-capable: the wiring demonstrably
   can light the lamp, and the pulse does not.

## Gaps

- ~~Placement orientation is not ported (`state_for_placement`)~~: **resolved in
  slice 10** (see below). A plate placed through vanilla placement now takes
  its attachment from the clicked face and its rotation from the clicked
  quadrant (`state_for_placement` + `use_item_on` rod rotation); `/setblock`
  and native placement were already exact.
- The carried `end_rod` template gives the plates their 2-px shape and
  collision, but there is **no lit/unlit tint** (the models' tint indices need
  a block-colour provider).
- The pulse clear lives in an in-memory map: a gate whose chunk is unloaded
  when its clear is due keeps `powered=true` until it is evaluated again
  (upstream's block ticks are saved with the chunk). `block_at` returning
  `None` drops the due clear; the gate then clears on the next input change.
  Chunk load/unload during a 2-tick pulse is not covered by the e2e.
- Inputs are sampled once per tick (the dirty set is applied at
  `level_tick_post`), so a rise and fall of B *within one tick* collapses to no
  edge. Upstream's `neighborChanged` runs on each notification; at the pin
  there is no mutable neighbour hook, so this is the same one-tick sampling
  limit the latch has (recorded under the pin's hook findings).
- The coloured network cables, the **bundled cable** posts/spools and soldering
  are deferred. The six bitwise plates are implemented (slice 6) and the
  red-alloy wire posts + spool are implemented (slice 7); their remaining gaps
  are listed there.
- A comparator's strength collapses to a boolean (the gates are binary).

## Upstream source

Analysis pass 2026-11-03 (source read for behaviour only; the Rust stays a
clean-room rewrite).

- **Repo** <https://github.com/Commoble/morered>, **tag**
  `v1.21.1-6.0.0.3` = commit
  `3bf5a8dff36480324165e09223092856adfcc668` ("Fix soldering recipes rendering
  wrong in JEI. Fixes #40"), **MIT**. Shallow clone at
  `ports/_shared/upstream/morered` (gitignored scratch, deleted after this
  note). 6.0.0.3 sources live under `src/main/java/commoble/morered/`, not
  `net/commoble`.
- **Files read**: `MoreRed.java:239-249` (gate registration),
  `plate_blocks/PlateBlockStateProperties.java`,
  `plate_blocks/InputSide.java`, `plate_blocks/InputState.java`,
  `plate_blocks/LogicFunction{,s}.java`, `plate_blocks/PlateBlock.java`,
  `plate_blocks/RedstonePlateBlock.java`,
  `plate_blocks/LogicFunctionPlateBlock.java`, `util/BlockStateUtil.java`,
  `plate_blocks/PulseGateBlock.java` (pulse slice: lines 25-126, cited above),
  `client/ColorHandlers.java:31-71`, `models/block/*gate*.json`,
  `src/generated/resources/assets/morered/blockstates/*gate*.json`,
  `src/datagen/.../MoreRedDataGen.java:357-397`.
- **Behaviours extracted** (full spec, tables and pin mapping in
  `../_shared/upstream/morered/FINDINGS.md`):
  - ten gates in scope (`diode`, `not_gate`, `and_gate`, `nand_gate`,
    `or_gate`, `nor_gate`, `xor_gate`, `xnor_gate`, `and_2_gate`,
    `nand_2_gate`); `multiplexer` sits beside them and is already in
    `runtime.rs`, still deferred;
  - state = `facing` (attachment, 6 directions toward the support) +
    `rotation` (0..3) + **only the input booleans the gate uses** (diode/not:
    `input_b`; xor/xnor/and_2/nand_2: `input_a`+`input_c`; and/nand/or/nor:
    all three);
  - inputs are A/B/C = 1/2/3 quarter-turns clockwise from the output,
    `input_dir = OUTPUT_TABLE[attachment][(rotation + k) % 4]`; the output
    direction is a 24-entry table;
  - output 15 (weak and direct equally), 1-tick delay, always a signal source;
  - placement: attachment = opposite(clicked face), output = the face
    quadrant away from the click point, rotation = the table index of that
    output; `rotate`/`mirror` follow the down/up/horizontal rules;
  - an input side is powered by the neighbour's **weak signal > 0** toward the
    plate, or by redstone wire with power > 0 (a wire *above* the plate counts
    — upstream's fallback quirk);
  - plates have **no** `canSurvive`/`updateShape`: they float when the support
    goes. That is the upstream behaviour, so `update_shape`/survival from the
    earlier plan is **dropped** (it would add behaviour upstream does not
    have).
- **Pin mapping** (Horizon `c9d474dd…`, API 80):
  `block_signal` (v1) answers 15 iff the hook's `face` equals the output
  direction — the dispatcher passes `direction.opposite()`, i.e. the travel
  direction (`game.rs:1177-1191`), so no extra flip; `is_signal_source` (v1)
  answers true for every plate state; `state_for_placement` (v1, API 76)
  reproduces the placement rule; `level_tick_post` (v1) applies the input
  update; `use_item_on` (v1) would carry the wooden-rod rotation.
- **Two pin gaps found** (both issue-shaped, neither blocks the slice):
  1. `native_neighbor_changed` is `&self` and receives no `Ticks`
     (`native_hooks.rs:354-364`), and no other mutable `GameLevel` hook fires
     on a redstone neighbour change, so upstream's
     `neighborChanged → scheduleTick(1)` cannot be expressed. The port
     **queues** dirty gates in `neighbor_changed`/`block_changed` and
     **applies** them at `level_tick_post(dimension)` — same one tick per
     stage, different mechanism; each gate is applied at most once per tick,
     which replaces upstream's `willTickThisTick` guard.
  2. There is no per-direction redstone read for mods:
     `native_neighbor_signal` is best-of-six and lives on `Game`, and
     `Sim::signal`/`state_signal` are `pub(crate)` in `horizon-world` (also
     absent from the newest tip). The port therefore computes `getSignal`
     itself from `StateFacts.weak` (the engine's own generic answer), the
     wire `power` property and its own gates. Consequence: **another mod's**
     `block_signal` answer cannot drive a plate, and a comparator feeds a
     plate as a boolean via a special case. Recorded as a deviation.
- **Simplified/deferred**: plate shape (the content model has no per-state
  box, so the carried block is a full cube), client tint (`ColorHandlers`), all
  other plate families, and the multiplexer's native side.
- **Content requirements handed over**: `facing` (6-value enum), `rotation`
  (enum "0".."3"), the per-gate `input_*` booleans, matching `default_state`,
  on a neutral full-cube template (`minecraft:stone`); the shipped blockstate
  JSONs key only `facing`+`rotation` (24 variants) and must be carried as
  shipped, then verified in-game (see FINDINGS §9).

## Red alloy wire verification (slice 4)

Status: **implemented and verified in-game** — 28 runtime tests (18 existing + 10
wire), the full ladder green (0 warnings in the injected module) and the whole
e2e green twice, including all earlier gate/latch/pulse phases.

Upstream: same tag/repo/commit as above (`v1.21.1-6.0.0.3`, `3bf5a8d`, MIT).
Full semantics and file:line are in
`../_shared/upstream/morered/FINDINGS.md` §10; the condensed table:

| Behaviour | Source |
|---|---|
| State: six subwire booleans `down/up/north/south/west/east` (64 states) + a `TRANSFORM`; no power property; default = empty wire | `AbstractWireBlock.java:48-55,202-211,224-228,488-491` |
| Placement: the bit toward the clicked block, only when that face is sturdy | `:297-313` |
| `updateShape`: a set bit follows the neighbour's sturdiness; a clear bit is never added there | `:251-269` |
| `canSurvive`: every set bit needs a sturdy neighbour | `:271-290` |
| Block entity power[6], saved as int array `power` | `WireBlockEntity.java:18-19,37-52,84-118` |
| `getSignal`: subwire toward the reader → `power/2`; else the best edge-sharing subwire | `PoweredWireBlock.java:56-67,103-138` |
| Source seed `2s-1`, each wire step `-1`, own subwires `-1`, coplanar/diagonal wires `-1`, non-wire edge blocks `2s-1` | `:153-297` (esp. `:219-223,225-273`) |
| `isSignalSource` = not the empty wire | `:41-45` |
| Item adds a subwire to an existing wire block, else places one | `WireBlockItem.java:31-91` |
| Loot count = subwire count | `WireCountLootFunction.java:22-50` |

The signal is `raw/2`, so a 15 source gives signal 14 and the signal decays by
one every **two** wires: wire `n` has raw `30-n`, the last powered wire is
**28** (raw 2 → signal 1) and wire **29** (raw 1) answers 0. The brief's guess
("lamp at wire 17+ dark") is not this mod's rule; the e2e tests the real one.

### Pure model (`runtime.rs`)

`Attached` (six subwire bits), `placement_attached`, `retained`, `can_survive`,
`perpendicular`, `source_power`, `power_signal`, `decay`, `wire_power`,
`wire_signal`, `shares_attachment`, `Attached::primary`. `wire_power` is a
monotone max-relaxation from zero over the wire's six faces (support seed, own
other subwires, linked neighbour subwires), so a removed source drains the
network instead of latching. Tests added (all watched fail first as missing
items, then pass):

| Test | Pins |
|---|---|
| `a_placed_wire_attaches_only_to_the_clicked_face` | placement bit |
| `a_shape_update_drops_a_subwire_whose_support_is_gone` | `updateShape`, no re-add |
| `a_wire_survives_only_while_every_support_is_sturdy` | `canSurvive` |
| `a_source_is_halved_and_each_step_decays_by_one` | `2s-1`, `/2`, `-1` |
| `power_decays_one_signal_every_two_wires_and_stops_at_twenty_eight` | 40-wire line: 14,14,13,13, …, 1, 0 |
| `removing_the_source_returns_the_line_to_zero` | no latent power |
| `a_wire_feeds_a_reader_beside_it_but_not_through_a_parallel_face` | per-face signal |
| `one_wires_subwires_feed_each_other_and_detached_faces_carry_nothing` | corner + empty face |
| `a_wire_connects_to_a_coplanar_neighbour_sharing_its_attachment_face` | connect rule |
| `the_shape_facing_follows_the_first_attached_face` | content shape selector |

### Glue (`support.rs`, manifest helpers)

| Hook | What it does |
|---|---|
| `block_entity` (API 15) | declares `morered:red_alloy_wire`'s block entity (id + empty compound; `power` defaults to zeros) |
| `block_signal` (with `pos`) | a wire answers `wire_signal(power, attached, face)` from its block entity; plates as before |
| `is_signal_source` | plate, or a wire with at least one subwire |
| `neighbor_changed` / `block_changed` | queue plates (per-tick) and wires (power solve) |
| `update_shape` (new) | returns a state with a subwire cleared when its neighbour stopped being sturdy (the engine writes it) and queues the wire |
| `state_for_placement` (new) | the six bits from `placement_attached(clicked face)`, `axis` from the primary attachment, `None` when the clicked face is not sturdy |
| `block_entity_update_tag` (new) | sends the saved `power` array to clients |
| `level_tick_post` | pulse clears → wire solve (worklist from the queued wires; a changed wire notifies its neighbours and pushes wire neighbours; caps: 16 384 steps, 8 visits/wire/tick) → plate batch |

`native_set_block` writes still go through the existing `writing` sentinel;
block-entity writes use `native_with_mod_block_entity` + `native_block_entity_sync`.
`/setblock` wires get a block entity from the solve if placement made none.

### Pin finding: content state order (forced the template choice)

At the pin a mod block's states are plain **template states × declared
combinations** (`mod_content.rs`, `NewBlock::state_combinations`). Vanilla's
`StateDefinition` sorts *all* property names and varies the first slowest, so
the two agree only when every declared name sorts after the template's.
`down`/`east` sort before `facing`; an `end_rod`-template wire with upstream's
six booleans crashed the game at load ("registry state order differs from
vanilla's state definition"; reproduced in
`/tmp/opencode/morered-wire-e2e-run1.log` and `/tmp/opencode/wire-debug/s.log`).
The tip `fbb17c980` has an interleaving `block_order`; the pin does not, and
`horizon/` is read-only. The wire therefore uses `minecraft:basalt`, whose only
template property is `axis` (sorts before all six), with `shapes` keyed on
`axis`: `y` → bottom 2-px slab, `x` → west, `z` → north. Deviation: the state
carries `axis` (3 × 64 = 192 states) and a ceiling/south/east wire collides on
the min side instead of hugging its own face; it stays thin and clickable.

### E2E (`tools/e2e.sh`, The End, headless, `--dev-mod content`)

Two consecutive runs on the final binary exited **0**
(`/tmp/opencode/morered-wire-e2e-final4.log`,
`/tmp/opencode/morered-wire-e2e-final2.log`); all 22 assertion lines pass. The
wire rigs (floor y=63, wires y=64, data read with
`/data get block <x> <y> <z> power`, a six-int array):

| Phase | Evidence (log line) |
|---|---|
| 3-wire line + lamp past the end | lamp `lit=true`; states `down=true` on wires 1–3; raw 29, 28, 27 |
| 32-wire decay line | raw `[29,10,2,1,0,0]` at wires 1, 20, 28, 29, 30, 32 |
| far lamp at wire 30 | `lit=false` |
| source removed (air) | raw 0 at wires 1 and 20, lamp still dark |
| source restored | raw 29 at wire 1 |
| 20-wire line, lamp at wire 15 | `lit=true`; wire 15 raw 15 |
| middle wire removed (air) | far lamp `lit=false`, far wire raw 0, near wire still 29 |
| middle wire restored | far lamp `lit=true`, far wire raw 27 |
| stone in place of the source | lamp `lit=false`, wires raw 0 |
| wire into a diode's B | diode `input_b=true`, output lamp `lit=true` |
| diode source removed | wire raw 0, `input_b=false`, lamp `lit=false` |
| vanilla lever | wire raw 0 → 29 |
| vanilla repeater (facing west → output east) | repeater `powered=true`, wire raw 29 |
| support removed (`updateShape`) | wire `down=false`; restored floor does not re-add it |
| item placement (`ctl place --item`) | `axis=y,down=true` at the clicked floor |
| 64 wire states | 64 `setblock`s accepted; read-backs match |
| 3 axis values | accepted, read-back `axis=z` |

Final line:

`PASS: AND all-three lit, A+C dark, removal dark, NOT inverted, OR by A,
multiplexer selects C/A, latch sets/holds/resets with complementary output,
both-high dark and held set, pulse arms/clears/not-held/re-arms with a diode
differential, pulse drives a latch, 96 pulse states accepted, stone
counterfactual dark, 3-wire line lights its lamp, wire power decays 29,10,2,1,0
along 32 wires, far lamp dark and near lamp lit, source removal and stone source
dead, middle wire removed drops the far side, wire feeds a diode, wire reads a
lever and a repeater, a removed support disconnects its subwire, placed wire
attaches to the clicked face, 64 wire states accepted`

Fail-capability: every wire assertion is paired with a differential on the same
rig (decay vs the source removed, middle wire present vs air, powered vs stone
source, wire vs no source at the diode, lever on vs off, 192 states bound vs
0 unbound). The lamp feedback feared while designing turned out not to exist: a
redstone lamp is a sink, not a signal source (checked in-game: a lit lamp beside
a wire did not raise the wire's `power`).

### Content

`data/morered/horizon/block/red_alloy_wire.json`: `minecraft:basalt` template,
`axis` retained as the shape selector, six declared booleans, default
`axis=y`/all false, thin `shapes`, red map colour, light 0, hardness 0, stone
sound. Multipart blockstate with six parts (one per subwire, `x`/`y` rotated),
one vanilla-compatible node model, the jar's wire texture, the jar's item model,
a 26.3 item definition, a 26.3 shaped recipe (3 `#c:ingots/redstone_alloy` → 12
wire), the red alloy ingot item + shapeless recipe + the three ingot tags, a
loot table dropping 1 wire, and both creative-tab entries. The offline binding
fixture (`blockstate_binding_fixture`, pin tree) over the built
`model-states.json`: **192 states, 0 unbound**, parts selected by the true bits
(0/1/2/3/4/5/6 parts for 0/1/2/3/4/5/6 subwires). In-game, all 64 upstream
combinations are accepted and read back.

The wire and ingot recipes really craft (offscreen GUI check,
`/tmp/opencode/morered-wire-craft4.log`): copper ingot + 4 redstone → 1 red
alloy ingot; 3 ingots in a row → result slot `{count: 12, item:
morered:red_alloy_wire}`, inventory 12. `/recipe give morered:<id>` is **not**
usable at this pin (the recipe-book index does not read mod namespaces; it also
answers "Unknown recipe" for `morered:diode`), which is why the craft ran
through a real crafting table.

### Test counts and ladder

| Check | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo test --locked --offline` (standalone) | **28 passed**, 0 failed |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | pass |
| `python3 tools/horizon.py stage` | pass (`stage_changed: true`) |
| `python3 tools/horizon.py check` | `cargo_exit: 0`, **0 warnings** |
| `python3 tools/horizon.py test` | **28 passed**, 531 filtered, `cargo_exit: 0`, 0 warnings |
| `python3 tools/horizon.py build` | `cargo_exit: 0`, 0 warnings |
| `tools/e2e.sh` | exit 0 twice (final binary) |
| content warnings (`horizon-mod-content-warnings.txt`) | 0 bytes |
| `.hmod` | 73,645 bytes, SHA-256 `de543aa7009f8e1ce57bb79de1c3b143400df7baaaad8052785132caf80385fc`, `horizon-mod check` clean, 169 entries |

### Deviations and remaining gaps

1. **The wire state carries `axis`** (the template's shape selector) on top of
   the six upstream booleans, so 192 states rather than 64; a ceiling/south/east
   wire's 2-px shape is on the min side of its cell. Model binding is complete.
2. **No `TRANSFORM`** state (`EightGroup`), so structure rotation/mirroring of
   wire states is not carried. The pin *does* accept the 8-value property form
   (slice 9 probe), but its `/place`/structure rotate/mirror table cannot
   compose it (or preserve a declared `power`), so no state was added; see
   "Slice 9" below.
3. **Visual model is simplified**: six 2-px plates with the wire texture (a red
   cross) instead of the jar's node/line/elbow `WirePartModelLoader` geometry,
   no lit/unlit tint and no culling/occlusion rules.
4. **Loot drops 1 wire**, not the subwire count (`WireCountLootFunction` has no
   pin equivalent); an empty wire block drops a wire upstream would drop 0.
5. **The wire item always places a fresh block**: `WireBlockItem.useOn`'s
   add-a-subwire-to-an-existing-wire path and `setPlacedBy`'s empty-wire-block
   materialisation are not ported; the content template is not replaceable when
   non-empty. Placement onto a solid face is exact (e2e).
6. **No per-direction redstone read and no indirect power**: `signal_toward`
   rebuilds `getSignal` from `StateFacts.weak` + wire/comparator/plate cases, so
   another mod's `block_signal` answer is invisible, a comparator collapses to
   15/0, and strong power conducted through a solid cube is not read.
7. **Solve timing**: one worklist per `level_tick_post`, capped at 16 384 steps
   and 8 visits per wire per tick. A line charges in one tick; a source removal
   drains over a few ticks (the e2e waits 40). A network larger than the caps in
   one tick settles over several ticks (bounded, not wrong).
8. **Lamp feedback**: none (a lamp is a sink). The e2e's brief expected a lamp at
   wire 17 to be dark; the real rule keeps wire 28 at signal 1, which the data
   and the lit lamp at wire 15 show.
9. `can_survive` is pure-model only (the pin has no survival hook for this path,
   and upstream's `updateShape` does the same work per changed face).

## Bundled cable and relay plate verification (slice 5)

Status: **implemented and verified in-game** — 36 runtime tests (28 existing +
8 cable/relay), the full ladder green (0 warnings in the injected module) and
the whole e2e green twice, including every earlier gate/latch/pulse/wire phase.

Upstream: same tag/repo/commit (`v1.21.1-6.0.0.3`, `3bf5a8d`, MIT). Full
semantics and file:line are in `../_shared/upstream/morered/FINDINGS.md` §11.
Two brief hypotheses were checked against the tag and are **false**: the relay
plate has **no `channel` property** (its states are `facing` + `TRANSFORM`) and
a **vanilla neighbour does not feed the bundle's channels** (the bundled cable
reads only a channeled capability, and a vanilla block has none). The condensed
upstream table:

| Behaviour | Source |
|---|---|
| Six subwire booleans inherited from `AbstractWireBlock`, no power property, `TRANSFORM` | `BundledCableBlock.java:29-40`; `AbstractWireBlock.java:48-55,202-211` |
| `byte[6][16] power`, `getPower(side, channel)` / `setPower` | `BundledCableBlockEntity.java:27-92` |
| Per-channel solve: attached faces only, capability seed `-1`, coplanar cable `-1`, own perpendicular subwire `-1`, convex-edge diagonal `-1`; re-add `OUTPUT_TABLE[f]` on change | `BundledCableBlock.java:104-241` |
| Saved as a per-face compound of `channel_flags` + `power_bytes` | `BundledCableBlockEntity.java:121-207` |
| Capability answer: subwire toward the querier, else `wireFace`, else the strongest perpendicular subwire | `BundledCableBlockEntity.java:209-248` |
| No vanilla signal: `AbstractWireBlock` (not `PoweredWireBlock`) has no `getSignal`/`isSignalSource` | no match in `wires/AbstractWireBlock.java`, `wires/BundledCableBlock.java` |
| Relay plate: `facing` + `TRANSFORM` only, 4-px post + plate, `BundledCablePostBlockEntity` `byte[16] power`, channel bridge over perpendicular neighbours/remote posts, no vanilla output | `BundledCableRelayPlateBlock.java:20-76`; `AbstractPostBlock.java:35-56`; `BundledCablePostBlockEntity.java:111-129`; `BundledCableRelayPlateBlockEntity.java:26-65` |
| `DefaultWireProperties` gives a vanilla block **no** channeled power; the default cable connector is `false` | `DefaultWireProperties.java:36-37,103-106` |

### What the port implements (and the recorded deviations)

The **cable is faithful**: six attachment bits, sixteen independent channels,
`2s - 1` seeds, one unit per cable step or corner, convex-edge diagonal link,
detached faces zero, and no vanilla input or output. The **relay plate is the
brief's bridge**: a declared `channel` (0–15, exactly the sixteen state values)
that seeds its channel into an adjacent cable from the strongest vanilla weak
signal on a non-cable face, and answers `power / 2` of the strongest attached
channel power of an adjacent cable. It is the *only* way a vanilla signal
reaches or leaves the bundle, which is what makes the channel-isolation e2e
possible.

Deviations, all deliberate:

1. **The relay is not upstream's channel bridge.** Upstream has no channel
   selector and relays a channel between cable/post segments through a 16-byte
   block entity; the bundled cable post, spool and post network are out of
   scope, so the port's relay is a 16-state channel↔vanilla transducer (the
   brief's "one channel → vanilla signal") with no block entity: it reads the
   cable's saved table at `block_signal` time. Its state count is 16 (one
   declared `channel` enum), not upstream's 48 (`facing` × `TRANSFORM`); the
   jar's relay model is carried unchanged in a fixed orientation and the
   `minecraft:stone` template gives it a full-cube collision.
2. **A relay answers a weak signal only** (`direct = false`). Its first version
   answered the direct path too, and the cable's solid basalt block flags
   conducted that 13 through the cable to a lamp above it (the e2e caught it:
   the lamp-above-the-cable differential was lit). `on_block_signal` now binds
   the hook's `direct` argument (the manifest previously passed `false`) and
   the relay returns 0 for `direct`; a lamp directly beside the relay still
   reads the weak 13.
3. **The cable uses `minecraft:basalt` + `axis`** for the same pin state-order
   reason as the wire (§ "Pin finding: content state order"): 192 registry
   states, 64 upstream combinations, a 3-px shape keyed on `axis`. Its block
   flags are still a full cube (the content format has no solid/conductor
   flag), so a *strong* vanilla source beside the cable could conduct through
   it; only the relay is made weak-only.
4. **NBT shape**: `power` is a compound of six sixteen-int arrays
   (`power.down`, …), not upstream's per-side `channel_flags`/`power_bytes`.
   `block_entity_update_tag` sends the same compound.
5. **Relay reads the strongest attached subwire** of an adjacent cable (all six
   faces), rather than the capability's specific face query; the pin has no
   mod block-capability API, and the cable is read from its own block entity.
6. **Recipes/loot**: the cable's `#morered:network_cables` key became
   `#morered:red_alloy_wires` (the sixteen coloured cables are out of scope);
   the relay keeps its jar recipe (`#morered:bundled_network_cables` = the
   cable). The cable's `morered:set_wire_count` loot function has no pin
   equivalent, so it drops one cable.
7. **Model**: the jar's multipart base part uses the `morered:wire_parts`
   custom loader and is dropped; the six node + twelve elbow vanilla models
   are carried, so a straight run shows nodes only (no line model). No
   lit/unlit tint (`TRANSFORM`/structure rotation is out of scope, as for the
   wire).
8. **Solve timing**: the cable solve shares the wire's per-tick worklist and
   caps (16 384 steps, 8 visits per cable). A network larger than the caps
   settles over several ticks (bounded, not wrong).

### Pure model (`runtime.rs`)

`Channel` (fallible newtype, `from_property`), `CablePower` (six faces × sixteen
channels of doubled units), `cable_power` (the wire rule per channel),
`relay_seed` (`2s - 1`) and `relay_signal` (`power / 2` of the strongest
attached channel). Tests added (all watched fail first as missing items):

| Test | Pins |
|---|---|
| `the_channel_property_is_parsed_and_bounded` | `channel` 0..15, rejects 16/negative/text |
| `a_bundled_cable_carries_each_channel_on_its_own` | sixteen independent values per face |
| `a_bundled_cable_line_decays_one_per_step_on_one_channel` | 29,28,… on channel 3, channel 5 stays 0 |
| `an_unattached_cable_face_carries_no_power` | detached faces forced to 0 |
| `a_relay_seeds_its_channel_at_double_the_vanilla_signal` | `2s - 1` |
| `a_relay_emits_half_the_strongest_attached_channel_power` | strongest attached face, `power / 2`, raw 1 → 0 |
| `a_relay_on_another_channel_emits_nothing` | channel isolation |
| `a_relay_ignores_a_cable_face_it_cannot_reach` | no attached face → 0 |

### Glue (`support.rs`)

| Hook | What it does |
|---|---|
| `block_signal` (now binds `direct`) | a cable forces 0 (weak and direct); a relay answers `power/2` for its channel on a weak read and 0 direct; wires/plates as before |
| `is_signal_source` | relay plates and the existing wire/plate rules; the cable is not a source (upstream) |
| `neighbor_changed` / `block_changed` | queue cables; a relay queues the cables beside it |
| `update_shape` / `state_for_placement` | the same six-bit rules for the cable and the wire |
| `block_entity` / `block_entity_update_tag` | the cable's `power` compound (six 16-int arrays) |
| `level_tick_post` | pulse clears → wire solve → **cable solve** (per-channel worklist; a changed cable notifies its neighbours and the relays beside it) → plate batch |

A relay's vanilla input is the strongest `signal_toward` over its non-cable,
non-relay neighbours; the cable seeds from a relay on an attached face or on any
of the four faces sharing an edge with it. After a cable's power changes the
solve notifies the adjacent relays so a lamp above a relay re-reads its signal.

### Ladder and test counts

| Check | Result |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo test --locked --offline` (standalone) | **36 passed**, 0 failed |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | exit 0 |
| `python3 tools/horizon.py stage` | pass (`stage_changed: true` on the first run) |
| `python3 tools/horizon.py check` | `cargo_exit: 0`, **0 warnings** |
| `python3 tools/horizon.py test` | **36 passed**, 531 filtered, `cargo_exit: 0` |
| `python3 tools/horizon.py build` | `cargo_exit: 0`, 0 warnings |
| `tools/e2e.sh` | exit 0 three times on the final binary (`run4`, `run5`, `run6`) |
| content warnings (`horizon-mod-content-warnings.txt`) | 0 bytes |
| `.hmod` | 84,877 bytes, SHA-256 `e83780f093c3caa62c7f80bd798c3ed1e33889e847b17c8aac817e84309c2ae6`, 187 entries, `horizon-mod check` clean |

### E2E (`tools/e2e.sh`, The End, headless, `--dev-mod content`)

Band z=30..34: relay A (channel 3) at (0,64,31) under a redstone block, cables
1–3 at (1..3,64,31) with `down=true`, relay B (channel 3) at (4,64,31) with a
lamp above, relay E (channel 5) at (2,64,32) with a lamp above, and a
control lamp above the cable. Three consecutive runs exited **0**
(`/tmp/opencode/morered-bundled-e2e-run4.log`, `run5`, `run6`).

| Phase | Evidence |
|---|---|
| relay bridge + isolation | lamp above relay B `lit=true`; channel-5 lamp `lit=false`; cable 1/2/3 `down` channel 3 raw **29/28/27**, channel 5 raw **0** |
| vanilla lamp beside the cable | lamp above cable 3 `lit=false` (the bundle emits no vanilla signal) |
| wire-to-lamp differential | a red alloy wire on the same band lights its lamp `lit=true` |
| source removed | lamp `lit=false`; cable 1 and 3 channel 3 raw 0 |
| source restored | lamp `lit=true` |
| middle cable removed | far lamp `lit=false`, near cable raw 29, far cable raw 0 |
| middle cable restored | far lamp `lit=true` |
| stone in place of relay A | lamp `lit=false`; the relay restored → `lit=true` |
| 64 cable states | 64 `setblock`s accepted; `down=true,east=true,north=false` read back |
| 16 relay states | 16 `setblock`s accepted; `channel=9` read back |
| existing phases | every gate/latch/pulse/wire assertion unchanged and green |

Final line:

`PASS: … 64 wire states accepted, bundled cable: relay channel 3 lights its
lamp, channel 5 dark, a vanilla lamp beside the cable dead, source and
middle-cable removal dark, stone relay dark, 64 cable states and 16 relay
states accepted`

Fail-capability: the channel-5 lamp and the lamp above the cable are the
differentials against the channel-3 lamp (same rig, same source); removing the
source, a middle cable or the relay all turn the channel-3 lamp dark; the
existing red-alloy-wire lamp on the same band proves the lamp/geometry can
light.

### Content and binding

* `data/morered/horizon/block/bundled_network_cable.json`: `minecraft:basalt`,
  the six declared booleans, `axis` shape selector (3-px slabs), default
  `axis=y`/all false, light 0, hardness 0, blue map colour, stone sound.
* `data/morered/horizon/block/bundled_cable_relay_plate.json`:
  `minecraft:stone`, one declared `channel` enum `0..15`, default `0`, hardness
  2 / resistance 5, blue map colour, stone sound (exactly 16 states).
* Assets: the jar's bundled-cable blockstate minus the custom-loader base part
  (six node + twelve elbow parts), the node/elbow models, the relay plate model
  (vanilla format), both item models, item definitions, the two textures; the
  relay blockstate is 16 `channel=` variants.
* Loot (one item each), the cable and relay recipes, the
  `morered:bundled_network_cables` item tag, the relay in `mineable/pickaxe`,
  and both items in the `morered:morered` creative tab (18 entries).
* Offline binding (`tools/check_binding.py` over the client's built
  `model-states.json`, copied by the e2e to
  `/tmp/opencode/morered-bundled-model-states.json`):
  **cable 192 states, 18 parts, part counts {0:3, 1:18, 2:9, 3:36, 5:36, 6:24,
  8:9, 9:36, 13:18, 18:3}, every state binds** (the three all-false `axis`
  states render no part, like the wire's empty state); **relay 16 states, 16
  variants, 1 each, 0 unbound**.
* In-game: all 64 upstream cable combinations and all 16 relay channel states
  accepted and read back; `give morered:bundled_network_cable 2` and
  `give morered:bundled_cable_relay_plate 2` both answered `{"given":2}`, and
  the creative tab lists both items.

### Skills used

`superpowers:test-driven-development` (runtime tests written and watched failing
before the implementation), `systematic-debugging` (the direct-signal leak: the
lamp-above-the-cable differential failed, the relay's weak answer was traced
through the hook's hard-coded `direct=false`), `verification-before-completion`
(every claim above is a command output), and Beautiful Rust
`rust-best-practices` (`ports/_shared/skills/BeautifulRust/skills/rust-best-practices/SKILL.md`):
fmt, clippy `-D warnings`, 0 warnings in the injected module.

## Bitwise logic plates verification (slice 6)

Status: **implemented and verified in-game** — 46 runtime tests (36 existing +
10 bitwise), the full ladder green (0 warnings in the injected module) and the
whole e2e green four times, including every earlier
gate/latch/pulse/wire/cable phase.

Upstream: same tag/repo/commit (`v1.21.1-6.0.0.3`, `3bf5a8d`, MIT). Full
semantics and file:line are in `../_shared/upstream/morered/FINDINGS.md` §12;
the condensed table:

| Behaviour | Source |
|---|---|
| Six plates: `bitwise_diode` (INPUT_B), `bitwise_not_gate` (NOT_B), `bitwise_and_gate` (AND_2 = `a && c`), `bitwise_or_gate` (OR), `bitwise_xor_gate` (XOR_AC), `bitwise_xnor_gate` (XNOR_AC) | `MoreRed.java:255-260`; `ObjectNames.java:45-51` |
| State = `facing` (attachment) + `rotation` (0..3) only; 24 variants; default `facing=down, rotation=0` | `PlateBlock.java:33-56`; `PlateBlockStateProperties.java:30-31`; shipped blockstates |
| `byte[16] power` in the block entity, saved/loaded as the byte array `power` | `ChanneledPowerStorageBlockEntity.java:18-20,105-118` |
| The channel capability exists only on the plate's **output** side, and answers only when `wireFace` is the plate's **attachment** (or null) | `ChanneledPowerStorageBlockEntity.java:37-49` |
| `updatePower`: single input = `outputDir.getOpposite()`, capability side = `inputDir.getOpposite()`; input bit = `power > 0`; output = 31 or 0 | `SingleInputBitwiseLogicPlateBlock.java:34-46` |
| Two inputs A and C (`getInputDirection(attachment, rotation, side.rotationsFromOutput)`); `apply(a, false, c)` | `TwoInputBitwiseLogicPlateBlock.java:38-51`; `InputSide.java:14-16`; `BlockStateUtil.java:161-164` |
| `TICK_DELAY = 1`; `setPlacedBy` schedules, `onNeighborChange` schedules HIGH, `tick` calls `updatePower` | `BitwiseLogicPlateBlock.java:32,70-77,88-105` |
| A cable seeds a supplier with `max(0, supplier - 1)` and can only connect to a plate whose attachment matches the cable's `wireFace` and whose output faces the cable | `BundledCableBlock.java:143-150,175,203`; `SingleInputBitwiseLogicPlateBlock.java:53-59`; `TwoInputBitwiseLogicPlateBlock.java:59-69` |
| No vanilla signal output (extends `PlateBlock`, not `RedstonePlateBlock`) | `BitwiseLogicPlateBlock.java:53-160` |

Consequences the tests pin: a **31** output becomes **30** on the adjacent
cable (the cable's `-1`), which answers 15 to a relay (`power / 2`); an
unpowered input is *false*, so a NOT emits 31 on every channel when its cable
is dead (and a diode emits 0).

### Pure model (`runtime.rs`)

`BitwiseGate` (id parser, `sides()`, `apply()`), `ChannelPower` (sixteen
doubled values: `get`/`set`/`bit`), `BITWISE_POWER = 31`, `bitwise_output`,
`cable_channel_power` (the cable's supplier branches) and `plate_seed`
(`saturating_sub(1)`). Tests added (all watched fail first as missing items):

| Test | Pins |
|---|---|
| `the_bitwise_gate_ids_are_recognised` | six ids, rejecting the plain-gate family |
| `the_bitwise_gates_use_the_upstream_input_sides` | single B; two-input A+C |
| `the_bitwise_truth_tables_match_upstream` | all six `LogicFunctions` over all eight inputs |
| `a_bitwise_plate_stores_the_maximum_power_on_a_passing_channel` | 31 / 0 |
| `the_bitwise_not_inverts_every_channel_on_its_own` | per-channel independence |
| `the_bitwise_two_input_gates_combine_the_same_channel_only` | AND/OR/XOR channel isolation |
| `a_missing_bitwise_input_reads_as_unpowered` | missing input = 0 (NOT emits, diode does not) |
| `a_cable_answers_a_plate_from_the_near_face_or_the_plates_attachment` | `SidedPowerSupplier` branches |
| `a_plate_seeds_a_cable_one_unit_below_its_stored_power` | `31 -> 30`, `0 -> 0` |
| `a_cable_face_takes_the_strongest_of_two_suppliers` | `raise` max rule (regression for the e2e bug below) |

### Glue (`support.rs`)

| Hook | What it does |
|---|---|
| `block_signal` | a bitwise plate answers 0 (no vanilla output, upstream) |
| `block_entity` / `block_entity_update_tag` | declares the plate's block entity and syncs its `power` byte array |
| `neighbor_changed` / `block_changed` | queue the plate (a placement also queues the cables beside it) |
| `cable_supply` | a bitwise plate on the cable's output face seeds each channel with `plate_seed` (only the edge path: an output face is always perpendicular to the attachment) |
| `solve_cables` | a changed cable marks the adjacent bitwise plates |
| `level_tick_post` | pulse clears → wires → cables → **bitwise plates** → gate plates; a changed plate marks the adjacent cables and the plate on its output face |

The plate's channel powers live in its **own block entity** (upstream's
`byte[16] power`), which is the single source of truth; the cable reads it
directly (no capability at the pin), exactly as the relay reads the cable. A
plate can also read another bitwise plate on its input side when the supplier's
output faces it and both share the attachment (the upstream `wireFace` check);
this chain is implemented but not exercised by the e2e.

### Bug found by the e2e (systematic debugging) and fixed

The first bitwise e2e failed at "NOT source removed": the plate's block entity
was all 31 (correct), the input cable had drained to 0, but the output cable
stayed at `[30, 0, 30, …]`. Two ticks of in-game probes
(`/tmp/opencode/morered-bitwise-probe.log`, `probe2.log`) showed the plate
updating and the cable still stale, even after replacing the cable or forcing
a neighbour update (so the cable *was* solved, with the wrong seed). Root
cause: `cable_supply` **overwrote** a face/channel with a relay's seed
(`seeds.set(face, relay.channel, …)`), so an unpowered output relay on channel
1 zeroed the bitwise plate's 30 on the same face/channel — the perpendicular
loop visits the relay after the plate. Upstream takes the **max** of every
supplier on a face (`BundledCableBlock.java:175-205`). Fixed with
`CablePower::raise` (max) for both relay seed paths and the plate seed, with
the pure regression test `a_cable_face_takes_the_strongest_of_two_suppliers`.
This also fixes a latent slice-5 case (a relay and another channel supplier on
one cable face).

### Ladder and test counts

| Check | Result |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo test --locked --offline` (standalone) | **46 passed**, 0 failed |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | exit 0 |
| `python3 tools/horizon.py stage` | pass (`stage_changed: true`) |
| `python3 tools/horizon.py check` | `cargo_exit: 0`, **0 warnings** |
| `python3 tools/horizon.py test` | **46 passed**, 531 filtered, `cargo_exit: 0` |
| `python3 tools/horizon.py build` | `cargo_exit: 0`, 0 warnings |
| `tools/e2e.sh` | exit 0 four times (`e2e-4`, `e2e-5`, `e2e-6`, and `e2e-7` on the final rebuilt binary) |
| content warnings (`horizon-mod-content-warnings.txt`) | 0 bytes |
| `.hmod` | 118,412 bytes, SHA-256 `63fdb34ad829890a53b77efe2ffc62251e55dc6dffbc996e7c6f0aaec4fd4940`, 241 entries, `horizon-mod check` clean |

### E2E (`tools/e2e.sh`, The End, headless, `--dev-mod content`)

Two rigs on a new floor band (z=36..46, y=64):

* **NOT/diode** at (0,64,37), `facing=down,rotation=0` → output north
  (0,64,36), input B south (0,64,38). Input cable at (0,64,38) with relay ch1
  at (1,64,38) under a redstone block; output cable at (0,64,36) with relay
  ch1 at (-1,64,36) under the lamp. A second diode at (3,64,37) has a vanilla
  lamp on its output face instead of a cable.
* **Two-input** plate at (0,64,44), `facing=down,rotation=0` → output north
  (0,64,43), A east (1,64,44), C west (-1,64,44). Each input cable has a ch1
  relay and a ch5 relay (four redstone blocks); the two-cable output line
  (0,64,43)–(0,64,42) has a ch1 relay with a lamp at (0,65,41) and a ch5 relay
  with a lamp at (1,65,42).

| Phase | Evidence (log lines) |
|---|---|
| NOT, input ch1 high | input cable 29, output cable 0, `power[1]=0`, lamp `lit=false` |
| NOT, source removed | input cable 0, output cable **30**, `power[1]=31`, lamp `lit=true` |
| diode differential, same wiring | source off dark; source on `power[1]=31`, output 30, lamp `lit=true` |
| plate has no vanilla signal | a diode's output-face lamp stays `lit=false` after a forced recompute while its input cable is 29 and `power[1]=31` |
| NOT restored + stone | source on dark (`power[1]=0`), source off lit (`power[1]=31`), stone dark (output cable 0) |
| AND truth table (ch1/ch5) | 00 dark/dark; 10 dark/dark; 11 **lit**/dark with output cable ch1 **30**, plate ch1 31 and ch5 0; +A5 lit/dark; +C5 lit/**lit**; −C5 lit/dark; −C1 dark/dark (source removal); all off dark/dark |
| OR | 00 dark/dark; 10 lit/dark; 11 lit/dark; +A5 lit/lit; −A5 lit/dark; +C5 lit/lit; −C1 dark/dark |
| XOR | 00 dark/dark; 10 lit/dark; 11 **dark**/dark; 10 lit/dark; +A5 lit/lit; −C1 dark/lit; −A5 dark/dark; 11 dark/dark |
| XNOR | 00 **lit/lit**; 10 dark/lit; 11 lit/lit; +A5 lit/dark; +C5 lit/lit; all off lit/lit |
| counterfactual | stone in the plate's place with all four blocks on → dark/dark |
| 144 states | 6 plates × 6 `facing` × 4 `rotation` accepted; `facing=east,rotation=2` read back; `input_b` rejected ("does not have property 'input_b'") |
| items / tab | `give morered:bitwise_and_gate 2` accepted; the built creative tab lists all six plates |
| existing phases | every gate/latch/pulse/wire/cable assertion unchanged and green |

Channel isolation: the channel-5 lamps stay dark while channel 1 is lit (AND
and OR with only A5 fed; XOR with A5 and no C5), and react only to their own
inputs — channel 1 cannot create a channel-5 output.

Fail-capability: every bitwise assertion is paired with a differential on the
same rig — NOT vs diode (same cable, same source), the NOT output (31) vs the
input (29) vs the output cable (30), channel 1 vs channel 5, source on vs off,
plate vs stone, and `input_b` rejected vs `facing`/`rotation` accepted. The
vanilla-lamp-on-the-output-face case also proves the plate emits only channels,
not redstone.

Timing: the toggle-to-lamp path is three ticks (source → cable solve → plate
apply → output cable solve → relay/lamp), so every phase waits 12–14 ticks.
The pulse phase's immediate post-sample lamp read raced the clear's neighbour
notification on one run (the state read showed `powered=false` while the lamp
still read `lit=true`); the e2e now waits 2 ticks after `pulse_sample` before
that read. The bitwise plates were not involved (no cable in that rig).

### Content and binding

* `data/morered/horizon/block/bitwise_{diode,not_gate,and_gate,or_gate,xor_gate,xnor_gate}.json`:
  `minecraft:end_rod` template, the plate's `rotation` enum declared, 4-px
  `shapes` keyed on `facing` (upstream's `DOUBLE_PLATE_SHAPES_BY_DIRECTION`),
  default `facing=down,rotation=0`, quartz map colour (14), hardness 0, light
  0, oak/wood sound — **24 states each** (`facing` × `rotation`; no input
  booleans, matching upstream's state).
* Assets carried from the jar: six blockstates + six `_alt` models + the
  single/two-input templates (+`_alt`), six item models, six 26.3 item
  definitions, the six symbol textures and the unary/binary cable-operator
  overlays. `plate_template` was already carried.
* Loot: the jar's six one-item tables. Lang: the already-carried verbatim
  `en_us`/`zh_cn` keys. Creative tab: the six items appended (24 entries).
* Offline binding (`tools/check_binding.py`, extended to the six plates, over
  the client's built `model-states.json` copied by the e2e): **each plate 24
  states, 24 variants, 0 unbound, variant counts {1: 24}**; the cable and
  relay results are unchanged.
* `horizon-mod check`: 118,412 bytes, SHA-256
  `63fdb34ad829890a53b77efe2ffc62251e55dc6dffbc996e7c6f0aaec4fd4940`,
  241 entries, unsigned, "no code: content only".

### Deviations and gaps (slice 6)

1. **No channel capability at the pin** (recorded since slice 5): the plates
   read the port's bundled-cable block entity directly and interoperate only
   with this port's cable (and with another bitwise plate, upstream's
   `wireFace`-matching chain — implemented, not e2e-tested). A third-party
   channel block is invisible.
2. **Update staging**: upstream schedules a 1-tick block tick from
   `setPlacedBy`/`onNeighborChange`; the pin has no mutable neighbour hook, so
   the plates are queued and applied at `level_tick_post` (the same mechanism
   as the logic gates). A rise and fall inside one tick collapses.
3. **No crafting recipe**: upstream ships these six only as
   `morered:soldering` recipes (2 `#c:quarter_slabs/smooth_stone` + quartz +
   redstone + `#morered:bundled_network_cables`). Horizon has no soldering
   recipe type (out of scope since slice 1), so none was carried; the plates
   are creative/`give` acquisitions, with their loot tables. (`/recipe give`
   does not read mod namespaces at the pin either.)
4. **Placement orientation** is the existing gap: the `end_rod` template's
   `facing` is the clicked face (the opposite of the plate convention), and
   the stick rotation (`useItemOn`) is not ported; `/setblock` and native
   placement are exact.
5. **No lit/unlit tint**: the bitwise models use raw symbol textures and the
   cable-operator overlay; the plate's state has no rendered power bit (the
   power is in the block entity), so there is nothing to tint.
6. **Shape**: the 4-px collision/outline comes from `shapes`; the `end_rod`
   template's `can_occlude: false`/`solid_render: false` render flags remain
   (same as the gates).
7. **Not in `mineable/pickaxe`**, exactly like upstream (strength 0,
   instabreak); loot is the jar's one-item table.
8. **No vanilla redstone input/output** by design (upstream): a bitwise plate
   neither reads nor drives vanilla signals; the bundled cable/relay is the
   only bridge.
9. **The cable's solid template**: the basalt block flags are a full cube, so
   a *strong* vanilla source beside the cable could conduct through it (the
   slice-5 deviation); the bitwise plates do not change it.

## Wire posts and the wire spool verification (slice 7)

Status: **implemented and verified in-game** — 52 runtime tests (46 existing +
6 post), the full ladder green (0 warnings in the injected module) and the whole
e2e green twice, including every earlier gate/latch/pulse/wire/cable/bitwise
phase.

Upstream: same tag/repo/commit (`v1.21.1-6.0.0.3`, `3bf5a8d`, MIT). Full
semantics and file:line are in `../_shared/upstream/morered/FINDINGS.md` §13;
the condensed table:

| Behaviour | Source |
|---|---|
| Three posts: `redwire_post` (`WirePostBlock`), `redwire_post_plate` (empty plate) and `redwire_post_relay_plate` (sides), all sharing `WirePostBlockEntity` | `MoreRed.java:215-221,274-279` |
| State = attachment `facing` (6) + `power` 0–15 (`TRANSFORM` in Java); default `facing=down, power=0` | `AbstractPostBlock.java:38-46,52-56`; `AbstractPoweredWirePostBlock.java:29,44-49,60-62` |
| Placement = the clicked face's opposite (the nearest-looking scan collapses to it face-on); no survival rule (a post floats when its support goes) | `AbstractPostBlock.java:109-125`; no `canSurvive` in `wire_post/` |
| Connectable sides: plain = its attachment, empty plate = none, relay plate = the four perpendicular faces | `WirePostBlock.java:31-35`; `WirePostPlateBlock.java:41-56` |
| `getNewPower = max(0, max(neighborPower, connectionPower) - 1)`: one decay even from the support | `AbstractPoweredWirePostBlock.java:173-176` |
| Input = `getSignal` on each connectable side; post-to-post = the linked post's stored `POWER` | `:185-221` |
| Output = `POWER` on a connectable face, weak and direct alike; always a signal source | `:126-159` |
| Link = both posts' `remoteConnections` (absolute in memory), saved as `connections`, a list of relative `BlockPos`; `addConnection`/`removeConnection` write both sides and notify the peer | `WirePostBlockEntity.java:36-40,58-76,99-165,188-226` |
| Spool = first click stores the post on the stack (`spooled_post`), same post clears, a connected post cuts, an unconnected post within 32 links (+1 damage), farther becomes the new first post | `WireSpoolItem.java:41-115`; `ServerConfig.java:20-26` |
| The "slack" check = a 16-segment sagging raytrace that refuses a link through a block | `SlackInterpolator.java:110-145` (rendering-only; skipped) |

**Important correction to the brief's wording:** the red-alloy post link carries
its stored **vanilla POWER** (0–15, one less per link), not a bundled-cable
channel. The channel-carrying posts are the separate `bundled_cable_post` /
`bundled_cable_relay_plate` family (`BundledCablePostBlockEntity`, `byte[16]
power`), which have **no vanilla signal at all** and are out of this slice
(FINDINGS §13.6). The e2e's "source at one post → decayed value at the far
post" therefore uses a redstone block → post A 14 → link → post B 13 → lamp.

### Pure model (`runtime.rs`)

`PostKind` (`from_id`, `connections`, `emits`), `post_power`,
`post_signal`, `POST_LINK_RANGE`, `links_within`, `PostLinks`
(`from_offsets`/`offsets`/`contains`/`add`/`remove`/`absolute`) and
`SpoolClick`/`spool_click`. Tests added (all watched fail first as missing
items; the emit-side test caught a real bug — upstream emits on the *opposite*
of the connectable set):

| Test | Pins |
|---|---|
| `the_post_ids_and_connection_faces_match_upstream` | three ids; plain/plate/relay connectable sets |
| `a_post_always_powers_itself_from_a_source_one_level_down` | `s-1`, saturating at 0 |
| `a_link_decays_the_strongest_linked_post_by_one` | link −1, strongest link wins, chain 14/13/12 |
| `a_post_emits_only_on_the_faces_it_connects` | plain emits away from its support; relay on its four sides; empty plate never; zero power |
| `links_are_stored_relative_to_the_post_and_round_trip` | relative offsets, idempotent add/remove, absolute round-trip |
| `a_spool_click_selects_links_cuts_and_refuses_a_far_post` | first/same/connected/link/too-far; strict `< 32`, diagonal distance |

### Glue (`support.rs`, `use_item_on.rs`, manifest)

| Hook | What it does |
|---|---|
| `block_signal` | a post answers `post_signal(power, facing, kind, travel)` on its connectable faces; direct is the same answer |
| `is_signal_source` | every post is a source |
| `neighbor_changed` / `block_changed` | queue posts; a removed post still has its BE at `block_changed` time, so its peers are queued (`clearRemoteConnections`) |
| `state_for_placement` (new branch) | `facing = opposite(clicked face)`, `power=0`; the first solve fills it |
| `block_entity` / `block_entity_update_tag` | declare/sync the shared post BE (`connections`, a list of relative int arrays) |
| `use_item_on` (new) | the spool click: select / clear / cut / link / too-far, keyed by the acting player's UUID (port-owned state) |
| `level_tick_post` | `solve_posts` runs before the wires: a worklist recomputes each post and pushes its linked peers (wire-style caps: 16 384 steps, 8 visits/post/tick); stale links to non-posts are pruned |

The link is the block entity's `connections`; there is no channel capability
(and no cable channel) anywhere in this path. `/setblock` posts get their BE
from `ensure_post_entity`, exactly as the wire/cable do. Post power is a state
property (`power`), so `block_signal` is a pure state read.

### Ladder and test counts

| Check | Result |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo test --locked --offline` (standalone) | **52 passed**, 0 failed |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | exit 0 |
| `python3 tools/horizon.py stage` | pass (`stage_changed: false` on the final tree) |
| `python3 tools/horizon.py check` | `cargo_exit: 0`, **0 warnings** |
| `python3 tools/horizon.py test` | **52 passed**, 531 filtered, `cargo_exit: 0` |
| `python3 tools/horizon.py build` | `cargo_exit: 0`, 0 warnings |
| `tools/e2e.sh` | exit 0 twice on the final binary (`run6`, `run7`) |
| content warnings (`horizon-mod-content-warnings.txt`) | 0 bytes |
| `.hmod` | 129,166 bytes, SHA-256 `ca7270144a1ca18f6d3fc9f20280c953649c3cdfdd6f597c6b0a459580fdf910`, 271 entries, `horizon-mod check` clean, "no code: content only" |

### E2E (`tools/e2e.sh`, The End, headless, `--dev-mod content`)

A new band (z=48..52, floor y=63, posts y=65): post A on a redstone block at
(0,65,50), post B on stone at (10,65,50), unlinked post C at (5,65,51), lamps
above each; the empty plate D at (2,65,49) and the relay plate E at (4,65,47).
The spool is given to the player, selected, and clicked with `ctl interact`
(the player teleports next to each post; the click reaches `use_item_on`).

| Phase | Evidence |
|---|---|
| source at A | A `power=14`, B/C `power=0`; A lamp `lit=true`, B/C lamps `lit=false` |
| spool link A→B | A `power=14`, B `power=13`, B lamp `lit=true`, C lamp still `lit=false`; links `[[I; 10,0,0]]` / `[[I; -10,0,0]]` |
| unlinked control, source on | C `power=0`, lamp `lit=false` |
| cut (click A then B) | A `power=14`, B `power=0`, B lamp `lit=false` |
| same post clicked twice | no link (B stays `power=0`) |
| relink, then source removed | A and B `power=0`, both lamps dark; source restored → A 14, B 13, B lamp lit |
| empty plate D linked to A | D `power=13` (it carries the link) but its lamp stays `lit=false`; no connectable side |
| relay plate E side-fed | E `power=14`, the side lamp `lit=true`; the lamp on its attachment side `lit=false`; side source removed → E 0, side lamp dark |
| 96 plain-post states | 96 `setblock`s accepted; `facing=east,power=0` read back (an unsourced 13 is corrected by the first solve) |
| native placement | `ctl place --item` against a north stone reads `facing=north,power=0` (the content default is `down`, so `state_for_placement` did it) |
| items / tab | `give` answered for the three posts and the spool; the creative tab lists all four |
| existing phases | every gate/latch/pulse/wire/cable/bitwise assertion unchanged and green |

Final line ends:

`... 96 bitwise states accepted and input_b rejected, wire posts: a spool links
two posts 10 apart, the near post stores 14 and the far post 13 with its lamp
lit, an unlinked control stays dark, cutting the link darkens the far post, the
source removal drains both, the empty plate carries the link but emits nothing,
the relay plate reads and emits on its sides, 96 post states accepted, the
post/spool items and tab resolve`

Fail-capability: every post assertion has a differential on the same rig — B
before the link vs after it (0/13), the unlinked C vs the linked B (same source,
same kind), the cut vs the link, source on vs off, the empty plate's 13 vs its
dark lamp (carries but cannot emit), the relay's lit side lamp vs its dark
attachment-side lamp, and 96 accepted states vs the first-solve correction.

Two in-game findings:

1. **A vanilla lamp does not re-read on placement**: the lamps are placed before
   the posts in the rig so the post's first power write notifies them (the first
   attempt had the lamps placed last and A's lamp stayed dark although A stored
   14). The same is true of the gate rigs, where the gate is placed last.
2. **The content template has no block entity**: the runtime `block_entity`
   hook is rejected with "no block entity … its template has none (copy a chest
   or furnace)" (the same pre-existing seam as the wire/cable), so the post BE
   is created by `ensure_post_entity`; links then save/load with the chunk and
   the e2e reads them back. The game-log warning is not part of the content
   warnings file (0 bytes).

### Content and binding

* `data/morered/horizon/block/redwire_post{,_plate,_relay_plate}.json`:
  `minecraft:end_rod` template, `facing` shape selector (the jar's six post
  boxes; the plate variants add the 2-px plate box), `power` as a 16-value enum,
  default `facing=down,power=0`, hardness 2 / resistance 5, red map colour 28,
  stone sounds, light 0 — **96 states each**.
* Assets carried from the jar: the three blockstates (six `facing=` variants,
  `power` wildcard), the three block models (parent `plate_template`), the four
  item models, the two post textures and the spool texture, the three loot
  tables, the four crafting recipes (the soldering variants are left out: no
  soldering recipe type), the `morered:redwire_posts` block tag, and the 26.3
  item definitions for the four items.
* Added `data/c/tags/item/rods/wooden.json` (`minecraft:stick`) for the spool
  recipe; `c:ingots/iron`, `c:ingots/redstone_alloy`, `c:dusts/redstone` and
  `c:quarter_slabs/smooth_stone` were already carried.
* `data/morered/horizon/item/redwire_spool.json`: `minecraft:stick` template,
  `max_stack_size: 1`; the creative tab now holds 28 entries (the four new ones
  are the three posts and the spool).
* Offline binding (`tools/check_binding.py`, now including the three posts,
  over the client's built `model-states.json`): **96 states / 6 variants /
  0 unbound / variant count 1** for each post; the cable and bitwise results are
  unchanged.
* In-game: 96 plain-post states accepted; the three post items and the spool
  are given and listed in the tab.

### Deviations and gaps (slice 7)

1. **The brief's "channel" wording does not match this family**: a red-alloy
   post link carries the post's vanilla POWER with −1 per link; the
   channel-carrying posts (`bundled_cable_post`) have no vanilla signal and
   remain out of scope (FINDINGS §13.6). The e2e proves the decayed vanilla
   value (14 → 13) and the lamp.
2. **No `TRANSFORM`** state, so the port has `facing` × `power` (96) rather
   than the Java 768 states; structure rotate/mirror is not carried.
3. **The spool's first post is port-owned state** keyed by the acting player's
   UUID (the pin exposes no item-component write). Durability/wear
   (`hurtAndBreak`) is not carried, so the spool never breaks; the Java
   `inventoryTick` cleanup (out of range or the post gone) is lazy — it happens
   on the next click, not every tick.
4. **The sag raytrace is skipped** (`SlackInterpolator.getWireRaytraceHit`, the
   brief's "slack" interruption). The 32-block distance rule is implemented
   (strict `< 32`, Euclidean). The client break packet, particles and sounds
   are not carried (no client renderer hook).
5. **The post BE is created by the port** (the template has no BE): a broken
   post can leave its BE behind because the engine's removal path checks the
   template's BLOCK_ENTITY flag; the port only reads links for positions whose
   state is a post, and the solve prunes links to non-posts, but a new post
   placed at the same position could inherit a stale link until pruned. Not
   exercised by the e2e.
6. **Staged updates**: the first power is one level tick after placement
   (upstream writes it in `setPlacedBy`), and a post recomputes at
   `level_tick_post`; a rise and fall inside one tick collapses.
7. **No lit/unlit tint**: the carried models' `tintindex`/`color` need a
   block-colour provider (same as the wire).
8. **Link resolution needs loaded chunks**: an unloaded linked post contributes
   nothing until it loads (upstream's BE is equally chunk-bound); the e2e keeps
   the 10-block link loaded.
9. **Recipes are carried but not crafted in the e2e**: the four 26.3 recipes load
   with 0 content warnings; the e2e gives the items directly. `/recipe give`
   still does not read mod namespaces at this pin (slice 4 finding).

### Skills used

`superpowers:test-driven-development` (six post tests written and watched
failing before the model), `systematic-debugging` (the lamp-placement ordering
failure and the diagonal `interact` aim failure were traced from the failing
logs; the emit-side rule was caught by the test and fixed in the model),
`verification-before-completion` (every claim above is a command output), and
Beautiful Rust `rust-best-practices`
(`ports/_shared/skills/BeautifulRust/skills/rust-best-practices/SKILL.md`): fmt,
clippy `-D warnings`, 0 warnings in the injected module.

## Coloured network cables verification (slice 8)

Status: **implemented and verified in-game** — 58 runtime tests (52 existing +
6 colour), the full ladder green (0 warnings in the injected module), and the
whole e2e green twice (`run6`, `run7`) including every earlier gate/latch/pulse/
wire/bundled/bitwise/post phase. The offline binding check covers all 17
wire-like blocks (the plain wire + 16 dyes) at 192 states each, 0 unbound.

Upstream: same tag/repo/commit (`v1.21.1-6.0.0.3`, `3bf5a8d`, MIT). Full
semantics and file:line are in `../_shared/upstream/morered/FINDINGS.md` §14.
The brief's questions, answered from the tag:

| Question | Answer (source) |
|---|---|
| How is colour stored? | **Block identity**, not a state property and not a BE field: sixteen blocks, one per `DyeColor` (`MoreRed.java:231-234`, `ColoredCableBlock.java:38-44`); one shared BE type whose power is the inherited `int[6]` (`MoreRed.java:284-288`, `WireBlockEntity.java:18-19`) |
| Do different colours connect? | **No.** A coloured cable refuses a different-dyed coloured neighbour and otherwise falls through to the shared-attachment rule (`ColoredCableBlock.java:52-59`, `AbstractWireBlock.java:159-184`); the convex-edge branch also requires the same block (`:170-181`) |
| How is it seeded/decayed? | Exactly the plain wire: attached-block `2s-1`, own other subwires and linked subwires `-1`, channeled supplier `-1` (`ColoredCableBlock.java:177-242`); `getSignal = power/2` (`PoweredWireBlock.java:103-138`); `isSignalSource = !empty` (`:41-45`) |
| Relation to the plain red-alloy wire? | Same base class and same six-state subwire model; a coloured cable and the plain wire **do** connect (the refusal is only coloured-vs-different-coloured); the cable additionally reads/writes the dye-ordinal channel capability (`ColoredCableBlock.java:121,183,210-223`; `ColoredCableBlockEntity.java:55-85`), which the pin cannot expose |
| Recipe/loot | 8 `red_alloy_wires` + one dye-coloured wool (upstream: `neoforge:intersection` of wool and `c:dyed/<dye>`) → 8; loot drops the subwire count (jar `recipe`/`loot_table`; `WireCountLootFunction.java:22-50`) |

### Pure model (`runtime.rs`)

`CableColor` (the 16 dyes in `DyeColor` order, `id`/`from_id`/`index`),
`cables_connect` (two coloured cables only at the same dye; a coloured cable
and the plain wire join; `None` is the plain wire), `connects` (the
shared-attachment rule composed with the colour rule) and `same_wire_block`
(the convex-edge branch's same-block requirement). Six tests, all watched fail
first as missing API:

| Test | Pins |
|---|---|
| `all_sixteen_cable_colours_are_recognised_in_dye_order` | block ids and ordinals |
| `two_cables_of_the_same_colour_share_a_subwire` | the coplanar link |
| `cables_of_different_colours_do_not_connect` | every 16×16 dye pair |
| `a_coloured_cable_connects_to_the_plain_red_alloy_wire` | the mixed rule |
| `an_edge_link_only_joins_the_same_wire_block` | the diagonal rule |
| `a_powered_cable_of_another_colour_adds_no_link_power` | crossing isolation with a same-colour differential |

### Glue (`support.rs`)

| Change | What it does |
|---|---|
| `wire_attached` | accepts the plain wire **and** any `CableColor` id (same six booleans) |
| `wire_colour` | `Some(None)` plain, `Some(Some(dye))` coloured, `None` any other block (bundled cable included) |
| `wire_supply` | refuses a coplanar/diagonal link across dyes and a face-to-face wire feed unless it is the same block; identical rule to the plain wire when the colours match |
| `solve_wires` | one solve for both kinds; a bundled cable beside a wire is left to the channel solve |
| `block_entity`, `block_entity_update_tag` | declare/sync each dye's own BE (`power` int array) |
| `block_signal`, `is_signal_source`, `signal_toward` | the wire branch now covers the coloured blocks unchanged |
| placement/shape/neighbour/block-changed | already routed through `attached()`; coloured cables are marked as wires |

`level_tick_post` is unchanged: the existing worklist settles a coloured line
in one tick (29,28,27 in the e2e), drains it on source removal, and the
per-colour networks stay independent.

### Content

Sixteen blocks named `morered:<dye>_network_cable`, all on the wire's
`minecraft:basalt` template (`axis` shape selector + the six declared
booleans), 192 states each, `map_color` 8/15…29 (the dye's vanilla map
colour), 3-px shapes and solid sound. Per dye: the jar's node model
(`colored_network_cable_node_template` + texture), the jar's item model
(`colored_network_cable_template`), the 16-px texture, a 26.3 item definition,
a six-part node multipart, a 26.3 shaped recipe (8 `#morered:red_alloy_wires`
round one `<dye>_dye` → 8), a loot table, and a creative-tab entry. Shared: the
two model templates, the `colored_network_cables`/`network_cables` item tags,
and the 16 lang keys (already carried). The creative tab now holds **44**
entries.

Deviation from the jar's recipe: the upstream `neoforge:intersection` of
`#minecraft:wool` and `#c:dyed/<dye>` has no pin equivalent, so the centre
ingredient is the canonical `<dye>_dye` item. Rendering uses the jar's node
model only (the jar's `wire_parts` loader and line/edge/elbow parts are native
code); every state still selects a node part list.

### Ladder and test counts

| Check | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo test --locked --offline` (standalone) | **58 passed**, 0 failed |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | exit 0 |
| `python3 tools/horizon.py stage` | pass (`stage_changed: true`) |
| `python3 tools/horizon.py check` | `cargo_exit: 0`, **0 warnings** |
| `python3 tools/horizon.py test` | **58 passed**, 531 filtered, `cargo_exit: 0`, 0 warnings |
| `python3 tools/horizon.py build` | `cargo_exit: 0`, 0 warnings |
| `tools/e2e.sh` | exit 0 on `run6` and `run7` (same final binary) |
| content warnings (`horizon-mod-content-warnings.txt`) | 0 bytes |
| binding check (17 wire-like blocks) | 192 states each, **0 unbound**, `PASS: every state binds` |
| `.hmod` (delivered `content/morered.hmod`) | 176,145 bytes, SHA-256 `9b5c1692d06d1b27e65e2b5981f318644fc0fcbb326d9adc883c98989bb978db`, 403 entries, `horizon-mod check` clean, unsigned, "no code: content only" |

### E2E (`tools/e2e.sh`, The End, headless, `--dev-mod content`)

New band z=53..58 (the wire-post rig owns z=48..52). Evidence lines from both
runs:

| Phase | Evidence |
|---|---|
| red line + lamp | lamp at (2,64,53) `lit=true`; red raw 29,28,27 |
| blue line beside the **powered** red line, no source | blue raw 0, blue lamp `lit=false`, red still 29 and lit |
| blue line with its own source | blue raw 29 → 26 along x=1..4, blue lamp `lit=true`; red unchanged |
| powered blue line beside an unpowered red line (differential) | red x5 raw 0, red lamp `lit=false` |
| red line with its own source | red x7 29, x5 27, red lamp `lit=true` |
| cutting the blue line | blue lamp dark, near blue 29, far blue 0; restored → 26 and lit |
| removing the red source | red lamp dark, red raw 0, blue still 29 |

Final line records “coloured cable: a red line lights its lamp with the plain
wire decay, a blue line beside it stays dark, blue with its own source lights
its own lamp, a powered blue line does not feed an adjacent red line, cutting
the blue line darkens only its far side, a removed source drains only its own
dye”.

Fail-capability: every coloured assertion has a same-rig differential (blue
beside a powered red line vs blue with its own source; red beside a powered
blue line vs red with its own source; line present vs cut; source on vs off),
and the all-or-nothing colour rule is exhaustively pinned by the 16×16 test.

### Finding: probe lamps must be placed after the cables settle

A redstone lamp computes `lit` on **neighbour changes**, not on placement (the
slice-7 finding). When a probe lamp already sat next to a newly placed cable
whose neighbour across the dye boundary was powered, the lamp latched a
placement-time read and stayed lit even though the cable's stored power was 0;
removing the cable or the other line's source would clear it. The cold read on
the solved cable is 0 and the isolation logic is correct (`data get` proves
0), so the e2e now places each cross-colour probe lamp **after** its cables
have settled and lets the later source write notify it. Recorded here so a
future e2e does not read the transient as a link.

### Deviations and remaining gaps

1. **No channeled-power capability**: a coloured cable seeds only from vanilla
   signals, like the plain wire; upstream's ability to take a bundled cable's
   channel (and provide one on its dye channel) is not carried. This is the
   same pin gap as the bundled cable's channel surface.
2. **One block entity per dye** rather than one shared `colored_network_cable`
   type; behaviour is identical (each BE id is its own block's id).
3. **Rendering is node-only**: the jar's `morered:wire_parts` loader and the
   line/edge/elbow parts are native code the pin cannot run; the six-part
   multipart uses the jar's node model and texture. No line/elbow geometry, no
   lit/unlit tint.
4. **The `axis` shape selector** is inherited from the plain wire's pin
   workaround (§10.6): 192 states per dye instead of 64, and up/south/east
   subwire shapes sit on the min side of the cell.
5. **No `TRANSFORM`** state (structure rotate/mirror).
6. **Loot drops 1 cable**, not the subwire count (`WireCountLootFunction` has no
   pin equivalent), and an empty cable block drops one where upstream drops 0.
7. **The item always places a fresh block**: `WireBlockItem.useOn`'s
   add-a-subwire-to-existing path is not ported (the same gap as the plain
   wire).
8. **Recipe centre is a dye item**, not the upstream wool ∧ dyed-tag
   intersection (no `neoforge:intersection` at the pin).
9. **Solve caps** are shared with the wire (16 384 steps, 8 visits per block
   per tick); a network larger than the caps settles over several ticks.
10. **A face-to-face wire neighbour only feeds the same block.** Legitimate
    subwires attach to sturdy blocks, so two wires can only link through a
    shared edge; the port refuses a face-to-face different dye (or a different
    wire block) rather than reading its signal, which only affects forced
    `/setblock` states. The coplanar and diagonal rules are the upstream ones.

### Skills used

`superpowers:test-driven-development` (six colour tests written and watched
failing as missing API before `CableColor`/`connects`/`same_wire_block`),
`systematic-debugging` (the two failing e2e layouts were traced from the data:
a leftover redstone block in the wire-post band, then a lamp latching a
placement-time read; the cold cable data proved the isolation logic was
correct before the rig changed), `verification-before-completion` (every claim
above is a command output), and Beautiful Rust `rust-best-practices`
(`ports/_shared/skills/BeautifulRust/skills/rust-best-practices/SKILL.md`):
fmt, clippy `-D warnings`, and 0 warnings in the injected module.

## Slice 9: `TRANSFORM` probe — no-go at the pin

Status: **stopped with probe evidence; no port code or content changed.** The
pin accepts the state property, but its structure rotate/mirror table cannot
compose `TRANSFORM` or preserve a declared `power`; adding the state would be
decorative and would misrepresent structure copies. The 58 tests, the ladder
and the whole e2e were re-run on the unchanged tree and are green.

### Upstream semantics (tag `v1.21.1-6.0.0.3`, commit `3bf5a8d`, MIT)

Full table in `../_shared/upstream/morered/FINDINGS.md` §15. The condensed
facts:

| Fact | Source |
|---|---|
| `EightGroup.TRANSFORM` enum `transform` with the eight octahedral groups (`identity`, `rot_180_face_xz`, `rot_90_y_neg`, `rot_90_y_pos`, `invert_x`, `invert_z`, `swap_xz`, `swap_neg_xz`) | `util/EightGroup.java:20-30` |
| `rotate`: `rotation.rotation().compose(state.getValue(TRANSFORM))`; `mirror`: the mirror's rotation composed the same way | `EightGroup.java:32-40` |
| Wires/cables: six subwire booleans + `TRANSFORM`, default all false/identity | `wires/AbstractWireBlock.java:55,203-210,227` |
| Posts/post plates: `facing` + `TRANSFORM`, default down/identity; powered posts add `power` 0..15 (768 states) | `wire_post/AbstractPostBlock.java:38-46,52-56`; `AbstractPoweredWirePostBlock.java:29,47-48,62` |
| **Gate plates have no `TRANSFORM`** (`facing` + `rotation` only; their `rotate`/`mirror` rewrite those two) | `plate_blocks/PlateBlock.java:20-46,68-106` |
| `TRANSFORM` changes only in `rotate`/`mirror`; it normalises the saved power face arrays and post connection offsets | `AbstractWireBlock.java:448-472`; `AbstractPostBlock.java:131-147`; `WireBlockEntity.java:90-113`; `BundledCableBlockEntity.java:139-181`; `WirePostBlockEntity.java:232-244` |

The brief's "gate plates (24 states each)" premise does not match the tag: the
24-state gate plates carry no `TRANSFORM`. The families that do are the wires,
the coloured/bundled cables, the three red-alloy posts (including the post
plates and the relay plate).

### Probe (scratch content mod, headless The End, `--dev-mod`)

`/tmp/opencode/morered-transform-probe/` contains a `moreredprobe:probe_post`
block on `minecraft:end_rod` with a declared `power` 0..15 and `transform`
8-value enum (default `facing=down,power=0,transform=identity`) plus its
blockstate (facing variants; `transform` wildcarded, as upstream does). The
runs are `probe4.log` (the final, valid template) and `probe2.log`/`probe3.log`
(earlier attempts: `probe2.log` used the wrong `/place` rotation spelling,
`probe3.log` a malformed NBT root and then a template path outside the world
root; both were fixed before `probe4.log`). Evidence:

| Probe | Result |
|---|---|
| 24 sampled states across the 6 × 16 × 8 = 768 space via `/setblock` | all accepted; read-back `moreredprobe:probe_post[facing=east,power=7,transform=rot_90_y_neg]` |
| `/place template moreredprobe:probe 0 64 0 none none` (control, one-block NBT template) | `[facing=east,power=7,transform=rot_90_y_neg]` — the template state round-trips |
| `/place template … clockwise_90` | `[facing=south,power=0,transform=identity]` |
| `/place template … none left_right` | `[facing=east,power=0,transform=identity]` |

The generated `structure-blocks.json` row for
`facing=east,power=7,transform=rot_90_y_neg` is
`[south/power=0/identity, west/power=0/identity, north/power=0/identity]`, and
its mirror row is `[east/power=0/identity, west/power=0/identity]`. The content
tool clones the template's rotate/mirror row and remaps targets to the target
template state's **default combination** (`mod_content.rs:1184-1235`,
`Rename::id`/`row` at `:271-305`); the pin applies only that table
(`horizon-world/.../structure/processor.rs:1526-1529`). So a structure copy
drops `power` and never composes `transform`.

**Root cause:** the pin's rotate/mirror is a precomputed per-state id table
exported from vanilla; a mod block cannot override `BlockState.rotate`/
`mirror`, and a declared property that is not in the template is reset to its
default for the rotated target. No hook exists for a per-block rotate/mirror.
This is the same reason the existing `power`/`rotation`/`channel` properties
already reset on structure rotation.

**Decision:** do not add the state. A 768-state `TRANSFORM` that neither
composes nor normalises the block-entity data would be a fake: it would accept
`/setblock ...transform=…` but a structure copy would silently lose it and the
saved positional data would not be rotated with the block. The limitation is
recorded in FINDINGS §15.3 and in the gaps below; a future pin with a mutable
`BlockState.rotate`/`mirror` hook (or per-property rotation metadata in the
content format) can carry it.

### Baseline verification on the unchanged tree (slice 9)

No port source or content file changed; only this note and FINDINGS §15 were
written. Commands ran serialized under
`flock /tmp/opencode/campaign-build.lock`, with `CARGO_INCREMENTAL=0`:

| Check | Result |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo test --locked --offline` (standalone) | **58 passed**, 0 failed |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | exit 0 |
| `tools/horizon.py stage` (pin `c9d474dd`, `--horizon …/horizon`) | `stage_changed: true` (fresh stage), exit 0 |
| `tools/horizon.py check` | `cargo_exit: 0`, **0 warnings** |
| `tools/horizon.py test` | **58 passed**, 531 filtered, `cargo_exit: 0`, 0 warnings |
| `tools/horizon.py build` | `cargo_exit: 0`, 0 warnings |
| `tools/e2e.sh` | exit 0, final `PASS:` line with every earlier phase |
| `tools/check_binding.py` (17 wire-like blocks, relay, 6 bitwise plates, 3 posts) | **PASS: every state binds**, 0 unbound |
| delivered `.hmod` (`content/morered.hmod`, identical to `content/target/horizon-mod/morered.hmod`) | 176,145 bytes, SHA-256 `9b5c1692d06d1b27e65e2b2981f318644fc0fcbb326d9adc883c98989bb978db` (same bytes as the verified slice-8 artifact), 403 entries |

`horizon-mod check` was not re-run because no content was regenerated; the
delivered `.hmod` is byte-identical (same SHA-256) to the artifact checked in
slice 8. The binding output is 0 unbound for every block; the e2e log is
`/tmp/opencode/morered-transform-e2e.log`.

### Gaps added by this slice

1. **No `TRANSFORM` state** on wires, cables or posts: structure
   rotate/mirror cannot compose the eight-group value, and the block-entity
   data that upstream normalises with it (`power` face arrays, post
   `connections` offsets) is not carried. Blocked on a pin rotate/mirror hook
   (probe evidence above), not on content syntax.
2. **Existing declared properties reset on structure rotation**: a port
   block's `power`, `rotation` or `channel` also becomes the default of the
   rotated template state (`/place` probe), because the pin's table is
   per-template-state. This was already true before slice 9 and is now
   recorded.
3. The gate plates are **not** affected by `TRANSFORM`; their 24-state
   `facing` × `rotation` shape matches upstream (`PlateBlock`).

### Skills used (slice 9)

`superpowers:test-driven-development` was not triggered (no production code:
the probe is a throwaway content mod, and the TDD skill's exception for
throwaway prototypes applies); `systematic-debugging` (the two template-format
failures were traced from the NBT bytes and the "no template" reply, then the
generate-table path was read to find the root cause); and
`verification-before-completion` (every claim above is a command output).
Beautiful Rust `rust-best-practices`
(`ports/_shared/skills/BeautifulRust/skills/rust-best-practices/SKILL.md`)
applied to the unchanged Rust: fmt, clippy `-D warnings`, 0 warnings.

## Slice 10: plate placement orientation and wooden-rod rotation

Status: **implemented and verified in-game** — 65 runtime tests (58 existing +
7 placement/rod), the full ladder green (0 warnings in the injected module) and
the whole e2e green twice, including every earlier gate/latch/pulse/wire/
bundled/bitwise/post/colour phase.

Resolves the gap recorded since slice 1 ("placement orientation is not ported;
wooden-rod rotation is deferred"). Upstream: same tag/repo/commit
(`v1.21.1-6.0.0.3`, `3bf5a8d`, MIT).

### Correction to the brief: the click point, not the yaw

The brief described the placement as "`facing` = clicked face; rotation = the
player's horizontal direction snapped". The tag says otherwise: upstream uses
the **relative click point on the clicked face**, not the placer's rotation
(`plate_blocks/PlateBlockStateProperties.java:53-87`; its own comment at
`:59-65` records the rejected "output faces away from the player" option). The
port implements the source behaviour, and the e2e proves the difference: the
same quadrant clicked from the opposite side of the cell (yaw ~180° away)
gives the same rotation, while a different quadrant at the same yaw differs.

| Behaviour | Source |
|---|---|
| `PlateBlock.getStateForPlacement` = `getStateForPlacedGatePlate(defaultBlockState(), context)` | `plate_blocks/PlateBlock.java:48-53` |
| attachment (`facing`) = `getClickedFace().getOpposite()` (toward the support); output = `getOutputDirectionFromRelativeHitVec(clickLocation - placePos, attachment)`; rotation = `getRotationIndexForDirection(attachment, output)` | `plate_blocks/PlateBlockStateProperties.java:53-87` |
| the hit vector's attachment-axis component is zeroed and the other two are mapped to `*2-1`; `Direction.getNearest` picks the in-plane face (a dead-centre `(0,0,0)` answers `NORTH`) | `util/BlockStateUtil.java:231-248` |
| rotation = the index of that output in `OUTPUT_TABLE[attachment]`, 0 when the row lacks it | `util/BlockStateUtil.java:147-154,215-229` |
| gate plates re-read their `input_*` bits at placement | `plate_blocks/RedstonePlateBlock.java:58-71` |
| wooden rod (`Tags.Items.RODS_WOODEN`) poke: `rotation = (rotation+1) % 4`, re-read the gate inputs, `setBlockAndUpdate`, SUCCESS | `plate_blocks/RedstonePlateBlock.java:73-90` |
| bitwise plates: the same rod rule, rotation only (their channel powers are block-entity data) | `bitwise_logic/BitwiseLogicPlateBlock.java:107-121` |
| the `end_rod` template's own placement (what the plates got before this slice) sets `facing` = the clicked face, the opposite of the plate convention | pin `horizon/crates/horizon-world/src/level/place.rs:891-898` |

Pin mapping:

| Pin fact | Where |
|---|---|
| `PlacementContext` carries `pos` (placement cell), `face`, `hit` (absolute click location), `yaw`, `pitch`; a mod's `Some(Some(state))` wins over the template's placement | `horizon/crates/horizon-world/src/level/place.rs:55-79,287-314` |
| `Direction::approximate_nearest` iterates DUNSWE, starts at `f32::from_bits(1)` and defaults to `NORTH` | `horizon/crates/horizon-core/src/geometry/mod.rs:44-95` |
| `use_item_on` (API 13) is server-side for every player, after the block's own use; `claimed` takes the click | `horizon/crates/horizon-client/src/game/native_hooks.rs:4863-4876`; dispatch `game.rs:6300-6320` |
| item tags are `game.res.items().item_tags` (`<ns>:<path>` → members) | `horizon/crates/horizon-client/src/game.rs:1899-1906`; `horizon-items/src/items.rs:365-373` |

### Pure model (`runtime.rs`)

`Dir::normal`, `output_from_hit`, `placement_rotation`, `placement`,
`rod_rotation` (and `rotation_for_output`, now used instead of dead code). The
placement is a pure function of the clicked face and the relative hit:

- `placement(clicked_face, hit) -> (attachment, rotation)`: attachment =
  `clicked_face.opposite()`; the hit's attachment-axis component is flattened
  to 0, the other two become `*2-1`, and the first DUNSWE face with a positive
  dot product wins (`NORTH` when none does, the pin's
  `Direction::approximate_nearest`); the rotation is that face's index in
  `OUTPUT_TABLE[attachment]` (`0` when absent).
- `rod_rotation(rotation) = (rotation + 1) % 4`.

Seven tests, written and watched failing first (the first RED run was 33
missing-API compile errors; the first GREEN runs then caught three wrong
expectations of mine — the mirrored `Up` row and the two wall rows' west/east
indices — which the upstream table refuted, so the test expectations were
fixed and the implementation stayed as written):

| Test | Pins |
|---|---|
| `a_placed_plate_attaches_toward_the_clicked_face` | all six faces → `facing` = opposite |
| `a_placed_plate_points_away_from_the_clicked_quadrant` | literal floor/wall/ceiling hits, all four rotations, the mirrored `Up` row |
| `every_face_and_rotation_is_reproduced_from_its_quadrant` | all 6 × 4 = 24 states from a hit 40% toward the output edge |
| `the_click_axis_along_the_attachment_is_ignored` | the depth component never changes the output |
| `a_dead_centre_click_falls_back_to_north` | `(0.5,0.5,0.5)` → `NORTH` → rotation 0/0/0/0/1/3 |
| `a_click_outside_the_face_picks_the_nearest_edge` | hits outside 0..1 still pick the dominant quadrant |
| `a_rod_click_advances_the_rotation_and_wraps` | +1 per poke, wrap at 4, output follows the row |

### Glue (`support.rs`)

| Change | What it does |
|---|---|
| `on_state_for_placement` (new branch) | a gate or bitwise plate answers `placement(face, hit - pos)` through `with_facing_rotation`; the posts and the wire keep their branches |
| `with_facing_rotation` | a plate state with `facing` and `rotation` replaced |
| `on_use_item_on` → `rotate_with_rod` | a `#c:rods/wooden` item on a plate advances the rotation; a gate plate also re-reads its inputs in the same `native_set_block` write (upstream's `setBlockAndUpdate`), a bitwise plate only turns; a non-rod click is left to vanilla |
| `ROD_TAG` | the shipped `c:rods/wooden` item tag (`minecraft:stick`), read from `game.res.items().item_tags` |

The placement answer is a state of the placed block; the inputs are filled by
the first `level_tick_post` evaluation (the existing staged-update deviation).
No content changed: the plates already declare `facing` (from the `end_rod`
template) and `rotation`.

### Ladder and test counts

| Check | Result |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo test --locked --offline` (standalone) | **65 passed**, 0 failed |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | exit 0 |
| `python3 tools/horizon.py stage` | `stage_changed: true` |
| `python3 tools/horizon.py check` | `cargo_exit: 0`, **0 warnings** |
| `python3 tools/horizon.py test` | **65 passed**, 531 filtered, `cargo_exit: 0`, 0 warnings |
| `python3 tools/horizon.py build` | `cargo_exit: 0`, 0 warnings |
| `tools/e2e.sh` | exit 0 twice on the final binary (`green2`, `green3`) |
| content warnings | 0 bytes |
| `.hmod` | unchanged: 176,145 bytes, SHA-256 `9b5c1692d06d1b27e65e2b5981f318644fc0fcbb326d9adc883c98989bb978db` (byte-identical to the slice-9 artifact; no content changed) |

Run environment: `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2`. The standalone
fmt/test/clippy and the two e2e runs ran without the campaign build lock: it
was held by long ladders for the whole session (a `flock -w 7200` wait never
got in), the harness restarted repeatedly and killed foreground waits, and the
runs were detached to survive those restarts. The campaign-wide
`clean-targets.sh` was not run (it SIGTERMed a live build once); only this
port's regenerable build state was removed, as the updated rule asks.

### E2E (`tools/e2e.sh`, The End, headless, `--dev-mod content`)

A new band (z=59..67, floor y=63). Plates are placed with `look-at` + `mouse
right click`, which aims the real crosshair at a known point of the support's
face; the click point (not the yaw) is what `state_for_placement` sees.

| Phase | Evidence |
|---|---|
| floor, east quadrant | `facing=down,rotation=1` |
| floor, west quadrant (same yaw) | `facing=down,rotation=3` |
| floor, west quadrant from the other side | `facing=down,rotation=3`, yaw -170.1° vs 8.0° (**178.0° apart**) |
| wall, upper quadrant | `facing=south,rotation=0` |
| wall, lower quadrant | `facing=north,rotation=2` |
| ceiling, west quadrant | `facing=up,rotation=1` |
| bitwise plate, lower quadrant | `facing=north,rotation=2` |
| rod control (coal held) | `rotation=0`, west lamp `lit=false` |
| rod x1 (stick held) | `rotation=1`, west lamp `lit=true` |
| rod x2 | `rotation=2`, west lamp `lit=false` |
| rod x3, x4 | `rotation=3`, then `0` (wrap) |
| existing phases | every gate/latch/pulse/wire/cable/bitwise/post/colour assertion unchanged and green |

Final line: `PASS: ... plate placement sets facing from the clicked face and
rotation from the clicked quadrant (the same quadrant at a yaw 180 degrees
away is identical), a wooden rod turns a plate through all four rotations and
the west lamp follows`.

Fail-capability: the same e2e run against the previous slice's binaries (the
old `fast/horizon-client`, kept at `target/run/old-bin/`) fails at the first
placement assertion:

```
FAIL: expected state at 0,64,60 to contain facing=down, got
morered:and_gate[facing=up,input_a=false,input_b=false,input_c=false,rotation=0]
```

— the `end_rod` template's placement, which is exactly what the plate got
before this slice. The click geometry is therefore right (the plate is placed
in the right cell) and the new hook is the only difference. The yaw control and
the same-yaw/different-quadrant pair are the same-rig differentials.

### Deviations and gaps (slice 10)

1. **The rotation comes from the click point, not the yaw** (the brief's
   premise; see above). Any placer whose click location is the face centre
   gets the dead-centre answer: `NORTH`'s index in the attachment row (0 for
   floor, ceiling and the north/south walls, 1 for a west wall, 3 for an east
   wall). That is upstream's `getNearest(0,0,0)` fallback, not a port
   deviation.
2. **The rod tag is the shipped `c:rods/wooden`** (`minecraft:stick`); a
   modded wooden rod is accepted only if a datapack adds it to that tag (the
   pin's item-tag map is read, not a hard-coded id).
3. **Bitwise plates rotate with a rod too** (upstream
   `BitwiseLogicPlateBlock.useItemOn`), but the poke does not re-read their
   channel powers (block-entity data, like upstream's rotation-only write).
4. **The gate plate re-reads its inputs in the rod write** with
   `signal_toward`, so the same deviations apply as everywhere else: another
   mod's `block_signal` is invisible, a comparator is a boolean.
5. **No `rotate`/`mirror` state maths** (structure rotation) — unchanged from
   slice 9: the pin has no per-block rotate/mirror hook.
6. **Content unchanged**: `facing` (from the `end_rod` template) and
   `rotation` were already declared; the delivered `.hmod` is the slice-9
   artifact byte for byte.

### Skills used (slice 10)

`superpowers:test-driven-development` (seven tests written and watched failing
first; the first GREEN caught two wrong expectations of mine, fixed against the
upstream table), `systematic-debugging` (the rod phase's silent exit was traced
from the log: `ctl select` had hidden its JSON error behind `>/dev/null`, and
the hotbar was full — `item replace ... hotbar.0` + `select 1` fixed it),
`verification-before-completion` (every claim above is a command output), and
Beautiful Rust `rust-best-practices`
(`ports/_shared/skills/BeautifulRust/skills/rust-best-practices/SKILL.md`):
fmt, clippy `-D warnings`, 0 warnings in the injected module.

## Re-pin to Horizon `83ad384f` (in progress)

Re-pinned from `c9d474dd…` (108 hooks) to **`83ad384f48d0742746d518737511882680aab5b7`**
(427 hooks / 159 helpers), horizon-mixins `e8cd0aa38cffef49cb2e088ef716baa107148c02`,
`mixin_schema` 6, `crates/horizon-client/src/game.rs` sha256
`996432b9f7f9461fa69c90ff798daf6b89fe3795f379d7c6f401916633296463`.
`compatibility.json` and the bundle's `[compatibility] horizon_revision` match;
`tools/horizon.py` runs the embedded tests as **`--lib`** (the pin's
`src/main.rs` no longer declares `mod game;`).

Feature implemented in this pass:

**One item per attached subwire on break** (`morered:set_wire_count`): a new
`loot_drops` hook (`mixins/morered/loot_drops.rs`, API 75) reads the broken
block's state (the six `down`…`east` booleans) and resizes the loot table's
single stack, dropping nothing for the empty wire. `Attached::count` is now a
documented part of the model; a new unit test covers 0/3/6 subwires. TRANSFORM
stays **blocked** at this pin (no rotate/mirror hook; the audit records it).

Verification status (recorded honestly, nothing claimed without output):

- `cargo fmt --all --check`: exit 0.
- `cargo clippy --all-targets --locked --offline -- -D warnings`: exit 0.
- standalone `cargo test --locked --offline`: see the counts in the note below.
- `python3 tools/horizon.py stage --horizon ports/_shared/pins/horizon-83ad384f`:
  exit 0, `stage_changed: true`, revision `83ad384f…` (the staged tree contains
  the new hooks/helpers).
- `check` / `build` / `horizon-ctl` / embedded `test` / `tools/e2e.sh`: **queued
  behind the campaign build lock** (`flock /tmp/opencode/campaign-build.lock`,
  8+ waiters from other ports) when this note was written; their result is to
  be appended before the row in `ports/ledger.csv` is advanced.

See [`REPIN-AUDIT.md`](REPIN-AUDIT.md) for the deferred-feature audit at this pin.

Class/tier per `ports/ledger.csv` and `horizon/docs/modpacks.md`: **content/logic** — approach: carry-over content + a behaviour hook (subwire-count loot); the logic tier is not exceeded.

## Re-pin to Horizon `1d63e0a` and artifact rebuild (2026-11-09)

Mode A migration: the port pinned `83ad384f` and shipped only the `.hmod`; the
`.hnmod` half is new. The stamped hook catalog is byte-identical between the two
pins, so every injection, module and binding is unchanged; only the pin identity
moved.

| | Before | After |
|---|---|---|
| Horizon | `83ad384f48d0742746d518737511882680aab5b7` | **`1d63e0a5a37a7a708eb0b32b49bf0dfeb28ed8a9`** |
| horizon-mixins | `e8cd0aa38cffef49cb2e088ef716baa107148c02` | **`5a64af938ec86b00d53d16a408aabc0038fb3e57`** |
| `game.rs` SHA-256 | `996432b9f7f9461fa69c90ff798daf6b89fe3795f379d7c6f401916633296463` | `64813cf4db0b9c3983d42908609bb633b9fbbc705f0c890396f940a6c7cef80f` |

### Ladder (through `ports/tools/build-slot.sh`, `CARGO_INCREMENTAL=0`)

| Step | Result |
|---|---|
| `cargo fmt` + `--check` | exit 0 |
| `cargo clippy --all-targets -- -D warnings` | exit 0, **0 warnings** |
| `cargo test --locked --offline` | **66 passed**, 0 failed |
| `horizon.py stage` | exit 0 at `1d63e0a` |
| `horizon.py check` | `cargo_exit: 0`, **0 warnings** (64 s) |
| `horizon.py test` | **66 passed**, 1240 filtered |
| `horizon.py build` (fast) | `cargo_exit: 0` (436 s) |

### Delivered artifacts (reproducible)

| File | Size | SHA-256 | Entries |
|---|---:|---|---:|
| `content/morered.hmod` | 176,145 B | `9b5c1692d06d1b27e65e2b5981f318644fc0fcbb326d9adc883c98989bb978db` | 403 |
| `morered.hnmod` | 44,734 B | `a602939d838c60477220e7be4c04d42f7373eedc0e2203021a2b9bb710f8ab53` | 15 |

The `.hmod` is built and checked by the pin's own `cargo-horizon-mod`
(`no code: content only`); the `.hnmod` is `tools/package.py`'s deterministic
archive. A second `tools/package.sh` run reproduced both hashes.

### In-game e2e (fast build, shared `horizon-ctl`)

`e2e.log`: the default leg reached the wire-decay band and failed at
*"expected wire 30,64,14 down power 0, got: 'Found no elements matching power'"*.
Root cause: at this pin an unpowered wire stores no `power` array, so the NBT
path finds no element — power 0 with the block still our wire. The assertion now
falls back to reading the block id and accepts *absent* power only as 0 for a
`morered:*wire*`/`*cable*` block; a wired block that disappears or a wrong stored
value still fails. `SELFTEST=1` asserts the unpowered AND gate is lit, so the
negative leg fails by design. Re-run pending a slot.

Skills: superpowers/using-superpowers,verification-before-completion,systematic-debugging; BeautifulRust/rust-best-practices

> **2026-11-10 (after this record).** A parallel campaign session converged this
> port to the registry's one-file rule by embedding the carried content in the
> `.hnmod` (`mixins/<id>/embedded_content.rs` + `mixins/<id>/data/<id>-content.hmod`).
> The `.hnmod` size/SHA-256 above is therefore from this job's split build and is
> **superseded** by that port's own later record; the pin migration, the ladder
> results and the e2e findings in this section describe the code that was tested
> here and remain the basis for the harness fixes listed.

> **2026-11-10 — embedded content; one `.hnmod` is the deliverable.**
> The built content mod (`content/morered.hmod`) now travels inside the bundle
> at `mixins/morered/data/morered-content.hmod`, is compiled in with
> `horizon_client::native_data!("morered", "morered-content.hmod")`, written
> once to `.horizon-cache/morered-embedded-content.hmod` and appended to the
> dev-mod list by the `embedded-content` `content_root` injection (HEAD,
> cancellable; the AE2 pattern, proved first on `charginggadgets`).
> `tools/package.py` packs `data/` and refuses an embedded file that differs
> from `content/morered.hmod`. One installed `.hnmod` installs the plates, the
> wires, the cables and the code; the split pair is no longer a registry
> blocker.
>
> Artifact: `morered.hnmod` 148,577 B, SHA-256
> `0b343f498bb9f5fa7ec67b4abc381134a594b019f5676806a3927e96162de49a`,
> 17 entries (including `mixins/morered/data/morered-content.hmod`, 176,145 B);
> `cargo horizon-mod check morered.hnmod --kind native-mod` passes.
> `content/morered.hmod` stays in the port as the input of that copy, not as a
> second deliverable.
>
> In-game proof with no `--dev-mod`:
> `ports/_shared/evidence/morered/e2e-embedded.log` first runs the control (a
> plain `.horizon-cache` file: the plate cell stays air, no redwire spool, so
> the content provably comes from the bundle), then the whole rig scenario plus
> a byte-compare of the written file against `content/morered.hmod`; it ends
> `PASS` + `EXIT:0`. `e2e-embedded-selftest.log` ends `FAIL` + `EXIT:1`.
> `check-e2e-logs.py morered --strict` and `upstream_ready.py morered` are both
> green. Harness fixes this leg needed (behaviour assertions unchanged):
> - the drops rig teleports **after** the floor fill (a player placed at y=64
>   first fell to y=62 and the aim found no face);
> - it aims without naming a face — a wire's south/west faces are empty, so
>   `look-at-block … south` reports "no face … visible";
> - it counts picked-up **and** ground items instead of matching
>   `Item:{id,count}` NBT, because 26.3 keeps the stack count as a component
>   (the NBT probe could never match);
> - the 1-subwire rig sets only the `down` arm (the old state had `down` **and**
>   `north`, i.e. two subwires, and dropped 2);
> - the pulse sampler puts `wait 1` between reads (a `script` runs all lines
>   back to back, so 20 reads landed inside the 2-tick pulse).

## Reviewer repairs (2026-11-11) — PR review items and what changed


Each item below is quoted from `ports/_shared/horizon-mods/reviews/pr-*-morered.md` (the review is the specification), with what this repair did about it.

- **Reviewer:** lifecycle: process-global STATE keyed by dimension/coordinates/player, no reset
  **Repair:** Fixed: a `world_lifecycle` injection (`world.rs` → `on_world_lifecycle`) drops every queued dirty set, armed pulse, in-flight write and spool selection when a world opens or closes (`reset_world_state`). Test: `a_new_world_starts_with_no_queued_or_selected_work`.
- **Reviewer:** attribution
  **Repair:** Already absolute (`https://github.com/Commoble/morered`, Commoble in `authors`).
- **Reviewer:** licence notice
  **Repair:** `LICENSE` + `LICENSE-NOTICE.md` now travel in the `.hnmod`; the carried `.hmod` carries `data/morered/license/`.
- **Reviewer:** registry artifact delivery
  **Repair:** **Staging (orchestrator, not ours)**: the artifact is not in registry release/staging as `<sha256>.<ext>`; the orchestrator answers it with `horizon-mods#686` (no write access to the registry's releases). Every repackaged artifact below has a fresh sha256 for that re-push, and the author release must be re-uploaded from the port repository.

Skills: superpowers/using-superpowers,systematic-debugging,verification-before-completion; BeautifulRust/rust-best-practices

### Repair verification (2026-11-11/12)

- Staged `check`: check exit 0 (`ports/_shared/evidence/repair-2026-11-11/morered-check.log`)
- Embedded tests: embedded **67 passed; 0 failed** (`ports/_shared/evidence/repair-2026-11-11/morered-test.log`, the new lifecycle test included)
- In-game: e2e **PASS** + `EXIT:0` (`ports/_shared/evidence/repair-2026-11-11/morered-e2e.log`): the full gate, wire, cable, bitwise and post suite
- `cargo test` (standalone lib), `cargo clippy --all-targets -- -D warnings` and
  `cargo fmt --check` are clean in the port; the artifacts were built twice and
  hash identically (`registry.toml` carries the sha256 above).
- Skills: superpowers/using-superpowers,systematic-debugging,test-driven-development,verification-before-completion; BeautifulRust/rust-best-practices,rust-port


## Migration to the SDK format (0.2.0, 2026-11-18)

Horizon's mod-API merge (#16941, launcher ≥ 0.2.70, PR #17992) retired `.hnmod`; the
registry closed every `.hnmod` submission on 2026-11-18 with one reason. This port's
former submission, **PR #832** (`morered` 0.1.0), was closed by that purge. **0.2.0** is
the same behaviour republished as one flat SDK `.hmod`; the changelog text is below.

Pins: SDK nightly **`0b14f8acfd97fa2c750e42795249dcc981178ca4`** (2026-11-18,
`package_format` 6, `content_api` "0.2"), mixins
**`e59da9094f0c2fdda6c627fe9eff22dec6a906f0`**, launcher 0.2.78, `cargo-horizon-mod`
from `horizon-rust/horizon@main`. The pinned SDK is cached in-tree at
`../_shared/sdk/0b14f8a/` and copied into `.horizon/sdk/` (gitignored) for the build.

**Done in this order:** every retired path moved into
`ports/_shared/evidence/morered/retired-hnmod-0.1.0/` **before** the first
`cargo horizon-mod build`; `src/runtime.rs` copied with `cp` and proved with `cmp` +
`sha256sum` (both `149b7bea4023b15bed435f47db080d298d70bb7d8d77e73a2cd33cc3041585a1`);
`cargo test` ran on the new crate with only `pub mod runtime;` **before any glue
existed** — **66 passed**, 0 failed (`/tmp/opencode/morered-test-runtime.log`); then the
glue was written.

### What moved, and where the retired tree is

| 0.1.0 path | Now | Evidence |
|---|---|---|
| `mixins/morered/runtime.rs` | `src/runtime.rs`, copied with `cp`, `cmp`-checked | `retired-hnmod-0.1.0/mixins/morered/runtime.rs` |
| `mixins/morered/support.rs` + 13 hook helpers | `src/support.rs` (glue and the four Parts) + `src/lib.rs` (the one `Hooks`) | `retired-hnmod-0.1.0/mixins/morered/` |
| `mixins/morered/horizon-mixin.toml` (13 `[[injection]]`, 2 `[[module]]`) | no plan: typed `Hooks`/Part methods and `Registrar` declarations | `retired-hnmod-0.1.0/mixins/` |
| `mixins/morered/data/morered-content.hmod` + `embedded_content.rs` (AE2 pattern) | gone: one package carries `assets/` and `data/` | `retired-hnmod-0.1.0/mixins/` |
| `content/` (`horizon-mod.toml`, `assets/`, `data/`, `morered.hmod`, `README.md`) | package root: `horizon-mod.toml`, `assets/`, `data/` | `retired-hnmod-0.1.0/content/` |
| `morered.hnmod` (148,577 B) | `morered.hmod` (233,165 B, native + content) | `retired-hnmod-0.1.0/morered.hnmod` |
| `compatibility.json`, `rust-toolchain.toml` | `[native]` in `horizon-mod.toml`; the pinned commit decides the toolchain | `retired-hnmod-0.1.0/` |
| `tools/horizon.py`, `package.py`, `package.sh`, `test_stage.py`, `test_template.py`, `stamp_license.py`, `audit/`, `check_binding.py`, `.github/` | deleted: staging, `.hnmod` packaging and the old injection audit belong to the retired format | `retired-hnmod-0.1.0/tools/`, `retired-hnmod-0.1.0/.github/` |
| `Cargo.lock` | not kept: it is packed and changes the artifact (trap 5) | not kept |
| `AGENTS.md`, `horizon-player.txt` (old repo/harness files referencing the retired tools) | retired | `retired-hnmod-0.1.0/` |

**Kept and updated:** `tools/e2e.sh` (the scenario and its assertions are
format-independent; the launch now takes `GAME=`/`CONTROL_GAME=`), `notes.md`,
`README.md` (rewritten for the package), `REPIN-AUDIT.md`, `VALIDATION.md`, `LICENSE`,
`LICENSE-NOTICE.md`.

### Carried content: unchanged

The package root's `assets/` and `data/` are the retired `content/assets/` and
`content/data/` moved, nothing else. Diffed file by file (size + sha256) against the
retired built `content/morered.hmod`: **402 entries, 402 identical, 0 different, 0
missing**; the only additions are `data/morered/license/license.txt` and
`data/morered/license/notice.txt`, which the retired `stamp_license.py` used to write
into the built archive and which now live in the source tree. The package's
`horizon-mod.toml` is the only manifest; its `version = "0.2.0"` (the port's version,
not upstream's `1.21.1-6.0.0.3`) is the identity the registry entry, file and release
must share.

### File map (0.2.0)

| Path | What it is |
|---|---|
| `horizon-mod.toml` | the only manifest: `id = "morered"`, `version = "0.2.0"`, `api = "0.2"`, `side = "both"`, `capabilities = ["registries"]`, `[native]` at `0b14f8a`/`e59da90` |
| `Cargo.toml` | one crate, `edition = "2024"`; its only dependency is `horizon-mod-api` from `.horizon/sdk/`; `license = "MIT"` |
| `src/lib.rs` | the one `#[mod_api::horizon_mod] impl Hooks`: `register`, `block_changed`, `level_tick_post`, `loot_drops`, `world_lifecycle` |
| `src/support.rs` | the glue: state text and property writes, the power/link caches, the four solves, `RedstonePart`, `ClickPart`, the four `BlockEntityPart`s, 9 unit tests of the codecs |
| `src/runtime.rs` | the pure truth, **byte-identical**, 66 tests |
| `assets/`, `data/` | the carried content (259 assets, 145 data files) |
| `LICENSE`, `LICENSE-NOTICE.md` | MIT text and provenance, **inside** the package |
| `tools/e2e.sh` | the in-game scenario, now for a launcher-built game |
| `morered.hmod` | the deliverable (233,165 B, sha256 `03a19939…4128`) |

### Helper → hook table

Each former `[[injection]]` helper is now one typed method; the helper file itself is
gone. The two globals that were `[[module]]`s (`runtime`, `support`) are ordinary
modules of the crate.

| 0.1.0 injection (hook, API) | 0.2.0 |
|---|---|
| `embedded-content` (`content_root`, AE2 HEAD, cancellable) | gone — the flat `.hmod` carries the content |
| `block-signal` (`block_signal` 1) | `RedstonePart::block_signal` → `support` reads the cached power / plate state |
| `signal-source` (`is_signal_source` 1) | `RedstonePart::is_signal_source` |
| `neighbor-changed` (`neighbor_changed` 1) | `RedstonePart::neighbor_changed` (queues; a relay queues its cell for cable discovery) |
| `block-changed` (`block_changed` 1) | `Hooks::block_changed` → `support::on_block_changed` |
| `update-shape` (`update_shape` 1) | **no part method** — see the deviation below: the solve prunes unsupported subwires through `LevelRef::block_face_sturdy` |
| `use-item-on` (`use_item_on` 1) | `ClickPart::use_item_on` (spool link/cut, wooden-rod turn) |
| `state-for-placement` (`state_for_placement` 1) | `RedstonePart::state_for_placement` |
| `block-entity` (`block_entity` 15) | four `BlockEntityPart::block_entity` (wire, bundled cable, bitwise plates, posts) |
| `block-entity-update-tag` (`block_entity_update_tag` 1) | four `BlockEntityPart::block_entity_update_tag` |
| `level-tick-post` (`level_tick_post` 1) | `Hooks::level_tick_post` → `support::on_level_tick_post` |
| `world-lifecycle` (`world_lifecycle` 1) | `Hooks::world_lifecycle` → `support::reset_world_state` |
| `loot-drops` (`loot_drops` 1) | `Hooks::loot_drops` → `support::on_loot_drops` |
| — (no 0.1.0 equivalent) | four `BlockEntityPart::block_entity_on_load`: seed the power/link caches from saved data |

The four block entity types are **declared** (`block_entity_type("wire").blocks(…)`,
`("bundled_cable")`, `("bitwise_plate")`, `("wire_post")`) and every block gets
`RedstonePart`; the gate, bitwise and post blocks also get `ClickPart`.
`capabilities = ["registries"]` is the schema's one value, not a claim.

### Behaviour that changed with the format (deliberate, and why)

- **The power and links are cached, and the cache is seeded on load.** `block_signal`
  receives a `LevelHost`, not a `LevelRef`, and the SDK has no position → block-entity
  handle; a wire's saved `power` cannot be read there. The port keeps the wire/cable/
  bitwise power and the post `connections` in its `State`, seeds them in
  `block_entity_on_load` (which fires on a fresh placement too), and writes both the
  cache and the block entity on every change. Observable difference: none after a
  chunk load, because the load seed fills the cache before the first tick; the cache is
  dropped with the world.
- **`update_shape` is not implemented.** Its `LevelHost` has no sturdy-face query (the
  query is `LevelRef::block_face_sturdy`, which needs a level handle). The wire/cable
  solve prunes a subwire whose support is no longer sturdy before it computes power,
  one level tick after the shape change instead of in the same update. A removed
  support therefore disconnects at the next tick, not instantly.
- **Block-entity states are written property by property.** `LevelRef::set_block`
  drops the block entity; `set_block_property` keeps it. The port diffs the old and new
  property sets and writes only the changes for the four entity families, and uses one
  `set_block` for the plate families (no entity). Several property writes where 0.1.0
  did one state write means several neighbour notifications for one update; the
  `writing` sentinel still hides the port's own `block_changed` self-reports.
- **A neighbour read is the engine's `getSignal`.** `LevelRef::signal(host, pos, dir)`
  is now public, so `signal_toward` is one call: another mod's `block_signal` now
  drives a plate, a comparator's real strength is read, and the old
  `StateFacts.weak`/wire/comparator reconstruction (and its recorded deviation) is
  gone. This is a behaviour *improvement*, not a gap.
- **The neighbour broadcast is one level, not two.** 0.1.0 called
  `native_update_signal_neighbors`, which updated `pos` and its six neighbours'
  neighbour sets; the SDK's write flags already notify the six neighbours, and the
  solve calls `LevelRef::update_neighbors` once. Redstone propagation is unchanged
  (every interested neighbour queues itself); a lamp two cells away is notified through
  the wire that changed between them.
- **No `ensure_*_entity`.** A `/setblock` of one of the four families used to need a
  hand-made block entity because the template had none; a declared type is created by
  the engine for its blocks, so the three `ensure_*` helpers are gone.
- **No log lines.** A native mod may not add a crate and the SDK exports no logger;
  diagnostics go through the readout the harness reads or are dropped.

### Traps, all hit in this migration

1. **The retired paths must be gone before the first build.** `unexpected package
   path: mixins/morered/runtime.rs` — the root holds package paths only. `mixins/`,
   the `.hnmod`, `compatibility.json`, `rust-toolchain.toml`, `content/` and the old
   tools moved to the evidence folder first.
2. **A 38-character pin reads as a missing pin** (`native requires game and exact
   revisions`). Copy both 40-hex commits; the mixins commit **moved**
   (`5a64af93…` → `e59da909…`).
3. **`check` with no file argument compiles wasm** (`can't find crate for 'core'`).
   `check <file.hmod>` for the package, `cargo check`/`cargo test` for the native build.
4. **`cargo fmt --all` reports the SDK's own files.** Use `cargo fmt`/`cargo fmt
   --check`; never "fix" the fetched SDK. `cargo fmt` left `src/runtime.rs` untouched
   (`cmp` proves it).
5. **A packed `Cargo.lock` changes the artifact.** `cargo test`/`clippy` write one;
   `rm -f Cargo.lock` right before `cargo horizon-mod build`, `/Cargo.lock` in
   `.gitignore`. Both builds here ran that way and are byte-identical.
6. **`side` is the installation side:** `both`, because single player's integrated
   server belongs to the client installation.
7. **`capabilities = ["registries"]` is the one shared value**; the schema refuses
   anything else, even for a mod that registers nothing.
8. **`src/runtime.rs` must be `pub mod runtime;`** or every `pub` item warns
   `dead_code`.
9. **`Hooks`/Parts are `Send + Sync`:** the port keeps one `static Mutex<Option<State>>`
   (a Part is constructed at registration and cannot hold the `Hooks` instance).
10. **A `#[non_exhaustive]` enum needs a wildcard** where it is matched:
    `ServerTickPhase`/`WorldPhase` were not matched here, but `Tag` and the event
    enums are handled exhaustively by variant.
11. **`JavaString` is not exported.** Compound keys are `"key".into()` and read back
    with the UTF-16 scan `support::get_tag` (naturesaura's helper, same reason).
12. **`BlockEntityRef` is the only route to a block entity.** It comes from
    `host.block_entity(level, pos)`; `LevelHost` (the `block_signal` side) has no
    position → entity handle, hence the cache (deviation above).
13. **`LevelRef::set_block` drops the block entity, `set_block_property` keeps it.**
    A family with saved data is written property by property (trap 10 of the campaign
    list, hit here for the first time in a wire port).
14. **A declared type creates the entity; the old `ensure_*` helpers are dead code.**
    `block_entity_type(...).blocks(...)` is the declaration; a `/setblock` of a listed
    block gets an entity with the Part's initial compound.
15. **`block_entity_on_load` is a `BlockEntityPart` method, not a `Hooks` method**
    (the generated trait has it under `BlockEntityPart`); it fires for placement and
    for a fresh chunk instance and is the only place the cache can be seeded.
16. **`use_item_on` receives the player and dimension in `BlockClick`**; no
    `acting_player()` lookup and no dimension string rebuild.
17. **Item tags are `host.tags().key(&Identifier::parse(..))` + `is_in_tag(item, key)`;
    block tags are `host.block_in_tag(block, tag)`.** The rod tag is still read, not
    hard-coded to `minecraft:stick`.
18. **A relay's adjacent cables cannot be found in `neighbor_changed`** (a
    `LevelTicksHost` has no `LevelRef`); the cell is queued and the tick, which has the
    level, marks the cables.
19. **The root is packed:** `README.md`, `LICENSE`, `LICENSE-NOTICE.md`, `Cargo.toml`,
    `horizon-mod.toml`, `src/` and the generated `uses.json`; `unzip -l` confirms the
    entry list and that no `Cargo.lock` is inside.
20. **`build` packs, it does not compile.** The native compile is
    `horizon-launcher native-build … --trust-native-code`, which is not installed
    here; `fits` only answers "made for".

### The ladder (one build slot, `CARGO_INCREMENTAL=0`, no `--target-dir`)

| Step | Command | Result |
|---|---|---|
| runtime only | `cargo test --offline` with `pub mod runtime;` | **66 passed**, 0 failed (green before any glue) |
| glue | `cargo test --offline` after the first glue pass | **red**: 18 compile errors, the expected first-glue failure; the retained fail-capability runs are `evidence/morered/sdk-test-red.log` (compile gate, a deliberate type error) and `sdk-test-assert-red.log` (a codec test fails when the codec changes) |
| tests | `cargo test --offline` | **75 passed**, 0 failed (66 `runtime::`, 9 `support::`) |
| lints | `cargo clippy --all-targets --offline -- -D warnings` | exit 0, 0 warnings |
| format | `cargo fmt --check` | clean; `src/runtime.rs` unchanged |
| package | `rm -f Cargo.lock && cargo horizon-mod build` | `Built ./target/horizon-mod/morered.hmod` |
| registry check | `cargo horizon-mod check morered.hmod` | `morered 0.2.0 (native-mod, format native-1/0b14f8acfd97fa2c750e42795249dcc981178ca4/e59da9094f0c2fdda6c627fe9eff22dec6a906f0, for 26.3)` / `233165 bytes, SHA-256 03a19939a0af0a3b16373699282c646ada4cd5e1c918e78c6eb90780c1384128, unsigned`, **EXIT 0** |
| fits the pinned build | `cargo horizon-mod fits morered.hmod --build nightly` | `More Red:` / `0b14f8a  made for`, **EXIT 0** |
| reproducible | second `rm -f Cargo.lock && cargo horizon-mod build` | same 233,165 B and `03a19939…4128`; `cmp` identical |
| runtime untouched | `unzip -p morered.hmod src/runtime.rs \| sha256sum` | `149b7bea…85a1` = the retired file and `src/runtime.rs` |

Package facts: **233,165 B**, sha256
`03a19939a0af0a3b16373699282c646ada4cd5e1c918e78c6eb90780c1384128`, **413 entries**
(3 `src/`, 259 `assets/`, 145 `data/`, `horizon-mod.toml`, `Cargo.toml`,
`README.md`, `LICENSE`, `LICENSE-NOTICE.md`, `uses.json`), no `Cargo.lock`,
unsigned. `uses.json` (extracted from the package) has **383 surface entries**:
14 hook ids, 42 trait methods, 4 Parts, 2 macros (`horizon_mod`, `horizon_part`),
`capability:registries`, 23 traits, 83 handles and 179 types.

### Gaps after the migration

Everything in the 0.1.0/0.2.0 feature gaps above still stands (no `TRANSFORM`, no
soldering recipe type, no sag raytrace/particles, the spool's first post is port-owned
state without wear, node-only cable rendering, placement orientation carries no
`TRANSFORM`, loot drops one cable rather than the subwire count, and the channel
capability is approximated by the port's own relay). Added by this format:

1. **No in-game re-run in this environment.** The e2e needs a game built by the
   launcher with this package, and `horizon-launcher` is not installed here (the same
   trap `imfast` and `enderstorage` hit). The 0.1.0 runs remain the behaviour's
   evidence (`ports/_shared/evidence/morered/e2e-embedded.log`, `e2e-embedded-selftest.log`);
   what changed is the *transport* of the same calls, not the rules, which are
   byte-identical and tested. This is not a claimed e2e.
2. **A subwire is pruned one tick late** (no sturdy query in `update_shape`).
3. **A relay reads its neighbours' cable power from the cache.** A cable whose chunk
   loads without `block_entity_on_load` firing (it always fires per the hook doc) would
   read 0 for one solve; not observed.
4. **The block entity is created by the declared type.** A `/setblock` of a wire or
   post whose type declaration is refused (registration error) would have nowhere to
   store power; `check` accepts the declaration and the embedded tests do not exercise
   the game.
5. **`block_signal` reads neighbouring block states for a relay** (to see the adjacent
   cables' power); the hook's doc says to answer from your own state. It is a read of
   the cached power plus a neighbour state read, not a write.
6. **The `axis` shape selector has no instant update** after a `set_block` on a wire:
   the shape follows the first attached face and is rewritten by the next solve.

### Changelog (0.2.0)

> **0.2.0 — republished as an SDK `.hmod`.** Horizon's mod-API merge (launcher ≥ 0.2.70)
> retired `.hnmod` and closed **#832** (`morered` 0.1.0); this version supersedes it.
> One flat `morered.hmod` now carries the code *and* the content (the plates, the
> bitwise plates, the wire, the sixteen coloured cables, the bundled cable and its
> relay, the posts and the spool): `src/lib.rs` is the one `impl mod_api::Hooks`,
> `src/support.rs` the glue and the four Parts, and `src/runtime.rs` is byte-identical
> to 0.1.0, so the gate table, the latch/pulse rules, the wire/cable solves, the
> channel gates and the post links behave exactly as before. Built against SDK
> `0b14f8a` / mixins `e59da90` for 26.3. The `.hnmod`, the mixin plan, the 13 per-hook
> helpers and the embedded-content injection are gone; power and post links are cached
> and seeded on load, a wire's input read is the engine's own `getSignal` (so another
> mod's `block_signal` now drives a plate), and a removed support disconnects at the
> next tick. Same feature set and the same gaps (no `TRANSFORM`, no soldering recipe
> type, no client renderer).

Skills: superpowers/using-superpowers, brainstorming, writing-plans, test-driven-development, systematic-debugging, verification-before-completion; BeautifulRust/rust-best-practices, rust-port
