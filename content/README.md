# More Red — carried content mod (logic gate plates)

More Red **1.21.1-6.0.0.3** (MC 1.21.1, NeoForge) carried from its jar into a Horizon
content mod (`.hmod`), scoped to the **logic gate plate** family: the diode, the NOT
gate, the three-input AND/NAND/OR/NOR gates, the two-input XOR/XNOR/AND/NAND gates,
and the `stone_plate` they are crafted from. This folder **is** the mod root
(`horizon-mod.toml` + `assets/` + `data/`); the carry output, the check scripts and
their logs live in `ports/_shared/carry-work/morered/`. Content only: there is no
code here, so the gate behaviour (output signal, input updates, placement, the
stick rotation) belongs to the native port in `ports/morered/`.

## Provenance

| | |
|---|---|
| Modrinth project | `UxYNfnfx` (slug `more-red`) — **no 1.21.1 file**; its newest files are 4.0.0.4 for 1.20.1, so the jar came from CurseForge |
| CurseForge | project `387638`, file `5763286`, `morered-1.21.1-6.0.0.3.jar`, 535 669 bytes, uploaded 2024-09-28 |
| Download | <https://mediafilez.forgecdn.net/files/5763/286/morered-1.21.1-6.0.0.3.jar> (and `edge.forgecdn.net`, same bytes) |
| SHA-1 | `a69f18365301470c0fc2b07125c9ba3c10f270f6` |
| SHA-256 | `8075126184f540c6b35b92127088f6cc4c9544627acac9f2287c62a0dfbde74e` |
| Verification | the official CDN download and the copy the modpack ships (`PrismLauncher/…/FTB Evolution/.minecraft/mods/morered-1.21.1-6.0.0.3.jar`) have the same SHA-1; the size is the one cfwidget's API records for file 5763286 (<https://api.cfwidget.com/minecraft/mc-mods/more-red>). No file-hash API was reachable: Modrinth has no 1.21.1 version and CurseForge's v1 API needs a paid key. |
| Licence | MIT (`META-INF/neoforge.mods.toml`: `license="MIT License"`); local-only until the maintainer approves publishing (ARCHITECTURE.md §8) |
| Upstream read | `https://github.com/Commoble/morered`, tag `v1.21.1-6.0.0.3`, commit `3bf5a8d` (shallow clone in `ports/_shared/upstream/morered/`) |

Files read upstream (facts taken from them, no Java copied):

- `plate_blocks/PlateBlockStateProperties.java` — `ATTACHMENT_DIRECTION = BlockStateProperties.FACING`
  (so the serialized property is **`facing`**, not `attachment_direction`), `ROTATION` 0..3,
  `INPUT_A/B/C`, and `getStateForPlacedGatePlate` (attachment = the clicked face's opposite,
  rotation from the click position).
- `plate_blocks/PlateBlock.java` — the six 2-px shapes `SHAPES_BY_DIRECTION`, default
  `facing=down, rotation=0`.
- `plate_blocks/RedstonePlateBlock.java` / `LogicFunctionPlateBlock.java` — `OUTPUT_STRENGTH 15`,
  `TICK_DELAY 1`, `isSignalSource`, `getSignal` for the output face only, the stick rotation.
- `plate_blocks/{InputSide,InputState,LogicFunctions}.java` — which input sides each gate has
  (`A` = 90° clockwise from the output, `B` = 180°, `C` = 270°) and the tint indices.
- `MoreRed.java` (lines 207–272) — `strength(0F)`, `SoundType.WOOD`, `MapColor.STONE`,
  `NoteBlockInstrument.BASEDRUM`, the gate-to-input-side table, the creative tab
  (`itemGroup.morered`, NOR gate icon).
- `client/ColorHandlers.java` — `LIT 0xFFFFFF` / `UNLIT 0x560000` per tint index (a gap here, §Gaps).
- `util/BlockStateUtil.java` — the output-direction table (the native port's lookup table).

## Carry

```bash
cd ports/_shared/carry-work/morered
/tmp/opencode/atm10-carry-target/debug/horizon-port-pack check morered-1.21.1-6.0.0.3.jar ./carry-out
```

```
morered: Partly. 6 blocks, 4 items port; not 39 blocks with no vanilla block to copy (left out), 54 recipes for left-out blocks.
  ports: 6 blocks
  ports: 4 items
  ports: 45 loot tables
  ports: 83 textures
  ports: blocks take the closest vanilla block's shape, hardness and sounds
  skipped: 39 blocks with no vanilla block to copy (left out)
  skipped: 54 recipes for left-out blocks
  skipped: 43 recipes of types Horizon doesn't have
  skipped: 22 blocks with behaviour of their own (use, ticks, collisions)
  skipped: 1 block with a shape of its own: it ports with its template's
  skipped: 2 items with behaviour of their own (use, on hit)
  skipped: 7 block entities (machines, containers, animated blocks)
  skipped: 1 menu (a container screen)
  skipped: 1 screen of its own
  skipped: 2 renderers or models in code
  skipped: 4 network messages
  skipped: 6 game events it reacts to (BlockEvent, ChunkWatchEvent, EventHooks, ...)
  skipped: 2 Mixins (patches to the game's code)
```

The six blocks the carry declares are the posts/cable plates/soldering table — **none
of the gate plates**: `carry-report.json`'s `blocks_unknown` gives each gate the reason
`no template for properties [facing, rotation]` (no vanilla block has both a six-value
`facing` and a `rotation` 0..3 property), and the 39 left-out blocks are counted as
"39 blocks with no vanilla block to copy". So for this slice the carry supplied the
*assets* (blockstates, models, textures, lang) and the *loot tables*, and everything
else — the 11 block declarations, the 10 gate recipes, the item model definitions, the
tags, the creative tab — is hand-written here. `carry-report.json`: 436 files copied,
namespaces `[]`, outside `[]`, 27 recipes upgraded, 45 loot tables upgraded,
oracle `false`, the six carried blocks guessed `minecraft:carved_pumpkin`/`minecraft:stone`.

`stone_plate` is carried as well: it is not a gate, but every gate recipe's `#` row is
`#c:quarter_slabs/smooth_stone`, whose only member is `morered:stone_plate` — without
it the ten recipes would load and never match.

## Counts

151 content files; the `.hmod` holds 152 entries (the 151 + `horizon-mod.toml`).

| Path | Files | |
|---|---|---|
| `assets/morered/blockstates` | 14 | carried: the 10 logic gates + `stone_plate` + `latch` (24 variants each), `multiplexer` and `pulse_gate` (48 each: they key `input_b`) |
| `assets/morered/models/block` | 33 | carried: the 10 gates' 20 models (`<gate>` + `<gate>_alt`), the multiplexer's 4, the latch's 2, the pulse gate's 4, `plate_template`, `stone_plate`, `stone_plate_alt` |
| `assets/morered/models/item` | 14 | carried: `<block>` parents `morered:block/<block>` |
| `assets/morered/items` | 14 | added here: the 26.3 item model definitions (`minecraft:model` → `morered:item/<block>`) |
| `assets/morered/textures/block` | 22 | carried: `plate_overlay`, `plate_torch`, the gates' input/middle/output overlays |
| `assets/morered/lang` | 2 | carried verbatim: `en_us`, `zh_cn` (vanilla-style `block.morered.*` keys) |
| `data/morered/horizon/block` | 14 | added here: the declarations (§Blocks) |
| `data/morered/horizon/creative_tab` | 1 | added here |
| `data/morered/loot_table/blocks` | 14 | carried, already 26.3 format |
| `data/morered/recipe` | 17 | added here: the 13 gate recipes + 4 for `stone_plate`, upgraded to 26.3 |
| `data/c/tags/item` | 5 | `dusts/redstone` and `ingots/iron` (added), `quarter_slabs/smooth_stone`, `slabs/smooth_stone`, `smooth_stone` (carried) |
| `data/minecraft/tags/block/mineable` | 1 | `pickaxe`, `stone_plate` only — the jar does not list the gates (§Gaps) |
| `horizon-mod.toml` | 1 | added here |

## Blocks

All fourteen keep the jar's ids unchanged. `facing` is Java's
`PlateBlockStateProperties.ATTACHMENT_DIRECTION`, i.e. `BlockStateProperties.FACING`
(down/up/north/south/west/east, pointing **toward** the supporting block); `rotation`
is the jar's `IntegerProperty("rotation", 0, 3)`; `input_a/b/c` are the jar's
`BooleanProperty`s, `A` = 90° clockwise from the output, `B` = 180°, `C` = 270°.
`powered` is the state-memory bit the latch and the pulse gate add.

| Block id | input properties | logic function | states |
|---|---|---|---|
| `morered:diode` | `input_b` | `INPUT_B` | 6 × 4 × 2 = 48 |
| `morered:not_gate` | `input_b` | `NOT_B` | 48 |
| `morered:or_gate` | `input_a`, `input_b`, `input_c` | `OR` | 6 × 4 × 8 = 192 |
| `morered:nor_gate` | `input_a`, `input_b`, `input_c` | `NOR` | 192 |
| `morered:and_gate` | `input_a`, `input_b`, `input_c` | `AND` | 192 |
| `morered:nand_gate` | `input_a`, `input_b`, `input_c` | `NAND` | 192 |
| `morered:xor_gate` | `input_a`, `input_c` | `XOR_AC` | 6 × 4 × 4 = 96 |
| `morered:xnor_gate` | `input_a`, `input_c` | `XNOR_AC` | 96 |
| `morered:and_2_gate` | `input_a`, `input_c` | `AND_2` | 96 |
| `morered:nand_2_gate` | `input_a`, `input_c` | `NAND_2` | 96 |
| `morered:multiplexer` | `input_a`, `input_b`, `input_c` | `MULTIPLEX` | 192 |
| `morered:latch` | `input_a`, `input_c`, `powered` | hold/reset | 6 × 4 × 4 × 2 = 192 |
| `morered:pulse_gate` | `input_b`, `powered` | one-shot (2 ticks) | 6 × 4 × 2 × 2 = 96 |
| `morered:stone_plate` | — | — (`PlateBlock`) | 6 × 4 = 24 |

1752 new block states in total. Every block's default state is the jar's:
`facing=down, rotation=0`, inputs and `powered` `false`.

The native port in `ports/morered/mixins/morered/support.rs` reads exactly these
names — `state.properties.get("facing")`, `get("rotation")`,
`input_a`/`input_b`/`input_c`, `powered` — so its plate reader works against these
declarations unchanged.

`data/morered/horizon/block/and_gate.json`, verbatim:

```json
{
  "template": "minecraft:end_rod",
  "properties": {
    "hardness": 0.0,
    "explosion_resistance": 0.0,
    "light": 0,
    "requires_tool": false,
    "map_color": 11,
    "sounds": "minecraft:oak_planks"
  },
  "shapes": {
    "property": "facing",
    "values": {
      "down": [[0, 0, 0, 1, 0.125, 1]],
      "up": [[0, 0.875, 0, 1, 1, 1]],
      "north": [[0, 0, 0, 1, 1, 0.125]],
      "south": [[0, 0, 0.875, 1, 1, 1]],
      "west": [[0, 0, 0, 0.125, 1, 1]],
      "east": [[0.875, 0, 0, 1, 1, 1]]
    }
  },
  "state_properties": {
    "input_a": { "type": "boolean" },
    "input_b": { "type": "boolean" },
    "input_c": { "type": "boolean" },
    "rotation": { "type": "enum", "values": ["0", "1", "2", "3"] }
  },
  "default_state": {
    "facing": "down",
    "rotation": "0",
    "input_a": "false",
    "input_b": "false",
    "input_c": "false"
  }
}
```

The other ten differ only in which `input_*` entries exist (the table above) and, for
`stone_plate`, in `hardness`/`explosion_resistance` 1.5 and `requires_tool: true`
(Java's `strength(1.5F).requiresCorrectToolForDrops()`).

### Template and models

`shapes` selects its boxes by a **template** state property
(`horizon-client/src/mod_content.rs`: the shape table is built from the template's
states, then shared by every combination of the block's own properties), so the thin
plate had to come from the template, not from a declared property. `minecraft:end_rod`
is the only vanilla block with a bare six-value `facing` and no other state: its
`facing` becomes the gates' `facing` (six states), and the plate is declared as the
six Java boxes above. `shapes` replaces the outline *and* the collision box, so the
gates pick and collide as 2-px plates.

The carried blockstates keep working unchanged: each keys `facing=…,rotation=…` and
lets `input_*` be a wildcard, which binds each of the 24 variants to the
2ⁿ input combinations of that face/rotation (`bind` treats a selector's absent
properties as wildcards, and the 24 selector sets are disjoint). Verified offline with
`horizon-assets`' `blockstate_binding_fixture` against the built `model-states.json`:
**0 unbound states**, 24 variants used per block, 8/4/2/1 states per variant exactly
(`ports/_shared/carry-work/morered/bind-output.jsonl`).

Template numbers: `end_rod` is already hardness 0, explosion resistance 0, no tool,
`SoundType.WOOD`; `light: 0` overrides its 14; `map_color: 11` is `MapColor.STONE`
(`7368816`); `sounds: minecraft:oak_planks` is a vanilla block with `SoundType.WOOD`.
`stone_plate` overrides hardness/resistance to 1.5 and `requires_tool: true`.

## Hand fixes (what the carry got wrong or left out)

1. **The eleven block declarations** — the carry ports no gate (neither `stone_plate`),
   so each is written here; without them the blockstates and loot tables would be inert.
2. **`facing`, not `attachment_direction`.** The Java constant is
   `PlateBlockStateProperties.ATTACHMENT_DIRECTION`, but its value is
   `BlockStateProperties.FACING`: the serialized property is `facing`, which is what the
   jar's blockstates, the native port (`support.rs`) and the engine all use. Declared
   through the template (see above) instead of as a `state_properties` entry — declaring
   a property the template already has is an error
   (`state property facing is already on minecraft:end_rod`). The 152-case check also
   proves `attachment_direction` is *not* a property of these blocks.
3. **`light: 0`** — `end_rod` emits 14; a gate must not light the room.
4. **`stone_plate` carried** with its four recipes, or the gates have no crafting
   ingredient (`#c:quarter_slabs/smooth_stone` is `morered:stone_plate`).
5. **The ten gate recipes** — the carry skipped them ("54 recipes for left-out
   blocks"). Recovered from the jar and upgraded to 26.3 by hand: `{"item": X}` →
   `"X"`, `{"tag": T}` → `"#T"`, `result: {id, count}`, `type` last. Their soldering
   variants (`morered:soldering`, 10 files) stay out: Horizon has no such recipe type.
6. **Convention tags** — `data/c/tags/item/dusts/redstone.json` (`minecraft:redstone`)
   is new; the jar's `quarter_slabs/smooth_stone` → `stone_plate`, `slabs/smooth_stone`
   → `minecraft:smooth_stone_slab` and `smooth_stone` → `minecraft:smooth_stone` were
   carried. An unknown tag expands to an empty ingredient, so a recipe with one loads
   and never matches (proved by the counterfactuals in §Evidence).
7. **Item model definitions** (`assets/morered/items/<id>.json`) — the carry adds
   these for the blocks it ports; the eleven here get the same
   `{"model":{"model":"morered:item/<id>","type":"minecraft:model"}}`.
8. **Creative tab** — `data/morered/horizon/creative_tab/morered.json`, title
   `itemGroup.morered` (the jar's lang key), icon `morered:nor_gate` (Java's icon), the
   eleven items in the Java tab's order.
9. **Block tag** — `data/minecraft/tags/block/mineable/pickaxe.json` with
   `morered:stone_plate`, exactly the jar's membership among the carried blocks.
10. **Lang** — nothing to rewrite: the jar already uses `block.morered.<path>` and
    `itemGroup.morered`, and Horizon excepts `block.<ns>.<path>`. `give` prints
    "NOR Gate", "Diode", "Stone Plate".

## Evidence

Build and self-check (from this folder):

```bash
CARGO_TARGET_DIR=/tmp/opencode/atm10-carry-target cargo run --locked --offline --quiet \
  --manifest-path ../../../horizon/crates/cargo-horizon-mod/Cargo.toml -- horizon-mod build
# Built ./target/horizon-mod/morered.hmod
CARGO_TARGET_DIR=/tmp/opencode/atm10-carry-target cargo run --locked --offline --quiet \
  --manifest-path ../../../horizon/crates/cargo-horizon-mod/Cargo.toml -- horizon-mod check morered.hmod
# morered 1.21.1-6.0.0.3 (mod, API 0.2, both)
#   52206 bytes, SHA-256 66ceb509afc5c55bac87f67e73d13e0fb9ae2204154381db58533603ca6d8d70, unsigned
#   may add blocks, items, mob effects and entity types to your worlds
#   no code: content only, nothing to run
```

The delivered `content/morered.hmod` is that file.

**In-game state check** — `ports/_shared/carry-work/morered/check.sh` (headless, the
End, a scratch `HORIZON_HOME`): it sets 152 states with `setblock` (every `facing`,
every `rotation`, and every input combination of every gate) and reads each back with
`ctl block`; a wrong expectation fails the run. Output (`check-output.txt`):

```
152 cases
== setblock + read back ==
read back 152 states
== counterfactuals: undeclared property and out-of-range value must fail ==
rejected as expected: morered:diode[attachment_direction=down]
    {"error":"unknown block property"}
rejected as expected: morered:diode[rotation=4]
    {"error":"invalid block property value"}
rejected as expected: morered:and_gate[facing=sideways]
    {"error":"invalid block property value"}
rejected as expected: morered:xor_gate[input_b=true]
    {"error":"unknown block property"}
== loot table: break a diode in survival ==
{"block":"minecraft:air","block_before":"morered:diode[facing=down,input_b=false,rotation=0]","drops":{"morered:diode":1},...}
...
PASS: all declared states set and read back; undeclared properties/values rejected
```

`SELFTEST=1 check.sh` corrupts one expectation on purpose and exits 1 — the check can
fail (`check-selftest.txt`).

**Model binding** — `blockstate_binding_fixture` over the built `model-states.json` and
the carried blockstates: `0 unbound states`, 24 variants per block
(`bind-output.jsonl`).

**Recipes and tags** — `ports/_shared/carry-work/morered/craft.sh` (a Vulkan
`--offscreen` run, survival, a crafting table): fills `"trt"` / `"###"` with two
redstone torches, one redstone and three stone plates and reads the result slot:

```
result slot 200: {"count": 1, "item": "morered:diode", "slot": 200}
inventory counts: {..., "morered:diode": 1, ...}
PASS: trt/### crafted a morered:diode
```

Counterfactuals, same scenario on a copy of the content with one hand-written tag
removed — the recipe still loads (`/recipe give` accepts it) but never matches:

| removed | result slot | exit |
|---|---|---|
| `data/c/tags/item/quarter_slabs/smooth_stone.json` | `{"item": null}` | 1 |
| `data/c/tags/item/dusts/redstone.json` | `{"item": null}` | 1 |

`/recipe give @s morered:<gate>` succeeds for all eleven, so the recipes load.

**Creative tab**: the built `creative-inventory.json` has
`{"id":"morered:morered","type":"CATEGORY","title":"itemGroup.morered","icon":"morered:nor_gate", …}`
with the eleven items.

**Rendering**: `screenshot-gates.png` (a four-rotation row of AND gates, two diodes,
an XNOR plate facing up, a stone plate and a redstone wire, all in the End) shows the
thin plates with the carried smooth-stone texture, the diode silhouettes and the torch
overlays drawn at the right rotation.

**Warnings**: the build's `horizon-mod-content-warnings.txt` is empty (0 bytes).

**Known harness limit** (not a content bug): `#tag` item predicates in commands
(`clear @s #c:…`) validate against the prepared registry table, which lists vanilla
tags only, so every mod tag answers "Unknown item tag" there whatever the content
holds. That is why the tags are proved with a real craft instead.

## Gaps (remaining, and why)

1. **No behaviour.** `input_a/b/c` are never updated and `block_signal`/`is_signal_source`
   are not declared: the gates are inert plates until the native port adds
   `neighbor_changed` (update the stored inputs), `block_signal` (15 on the output face
   when the logic function passes), `is_signal_source` and survival. The content side is
   ready for it: the state names are the jar's, and
   `BlockStateUtil.OUTPUT_TABLE` gives output = `facing` rotated by `rotation`.
2. **Placement is the native port's.** `rotation` is a declared property, so no
   template sets it; and `end_rod`'s own placement writes `facing` = the *clicked
   face*, the opposite of the plate's "toward the support" convention. A plate placed
   before the native `place` hook therefore renders on the far face of its cell. With
   an explicit state (`setblock`, or a native placement) everything is exact —
   that is what the 152-state check covers. Done natively: `facing` =
   `clickedFace.getOpposite()`, `rotation` =
   `getRotationIndexForDirection(facing, outputDirection)` from the click position
   (Java's `getStateForPlacedGatePlate`).
3. **The stick rotation** (right-click with any wooden rod advances `rotation`, Java's
   `useItemOn`) is native.
4. **No lit/unlit tint.** The carried models use the jar's tint indices (1 `FALSE`,
   2 `TRUE`, 3 `A`, 4 `B`, 5 `C`, 6 `!A`, 7 `!B`, 8 `!C`, 9 `AND`, 10 `NAND`, 11 `OR`,
   12 `NOR`, 13 `XOR_AC`, 14 `XNOR_AC`, 16/17 the XOR model's intermediates, 20 `AND_2`,
   21 `NAND_2`) and Java colours them `0xFFFFFF` lit / `0x560000` unlit. Horizon's
   mesher tints only vanilla's own sources (`mesher.rs`: `Tint` by block name), and the
   content format has no block-colour provider, so the overlays render as their raw
   texture instead of lit/unlit. A client-side hook is needed for the redstone state to
   be visible; the geometry and textures themselves render (screenshot).
5. **`can_occlude: false` / `solid_render: false`** come from `end_rod` (Java's plate
   occludes and is solid-rendered), so neighbouring faces next to a plate are not culled
   and the plate does not darken like a cube. Java's plate does let sky light through
   (its collision shape is not full), so `skylight_down: true` and `dampening: 0` do
   match. The render flags are not declarable (`BlockProperties` has no
   occlusion/render field).
6. **Soldering recipes skipped.** Each gate also has a `morered:soldering` recipe; the
   carry lists 43 recipes of types Horizon does not have, and the soldering table
   itself is not in this slice. Only the crafting recipes are here.
7. **No gate in `mineable/pickaxe`.** The jar's tag lists posts, the cable plates, the
   hexidecrubrometer, the soldering table and `stone_plate` — the gates are
   `strength(0)` and break instantly, so they were never in it. The tag here matches
   the jar among the carried blocks. (Tag membership changes nothing anyway:
   `requires_tool` is false for the gates.)
8. **Not the whole mod.** The bundled cable and its relay plate are carried
   (slice 5) and the six bitwise plates (slice 6); the sixteen coloured network
   cables, the cables' posts, the wire spool and the soldering table are still
   out of scope (see `ports/morered/notes.md`); their assets are pruned, so the
   lang files still name them (inert keys) and `assets/morered/lang` is
   upstream-verbatim. The logic gates, multiplexer, latch, pulse gate, red
   alloy wire, bundled
   cable/relay *are* carried (slices 1–5).
9. **`zh_cn` is upstream's**, including its missing entries, as the jar has it.

## Re-running

`check <jar> carry-out` rewrites `carry-out/` only; this folder is hand-owned and is
**not** touched. A fresh carry means re-applying the fixes above (the per-block
declarations, recipes, item definitions and models, the creative tab and the tags),
then `horizon-mod build`, `check.sh` and `craft.sh` again.

## Slice 2: the multiplexer

Added `morered:multiplexer` from the same jar: blockstate (`facing`/`rotation`/`input_b`
variants), four block models (`multiplexer`, `_alt`, `_switched`, `_switched_alt`),
three overlay textures, the item model/definition, a 26.3 shaped recipe
(`#c:quarter_slabs/smooth_stone`, `#c:ingots/iron`, `#c:dusts/redstone`), the
loot table, `c:ingots/iron`, and the creative-tab entry. Its declaration reuses
the `end_rod` plate template and the `facing`/`rotation`/`input_a/b/c` states.
The `.hmod` is 59,027 bytes; the in-game multiplexer checks A/C selected by B
pass (see `../notes.md`).

## Slice 3: the pulse gate

Added `morered:pulse_gate` from the same jar: the shipped blockstate (48
variants: `facing` × `rotation` × `input_b`), four block models (`pulse_gate`,
`_alt`, `_switched`, `_switched_alt`), the item model/definition, a 26.3 shaped
recipe (`"rir"` / `"###"`, `#c:quarter_slabs/smooth_stone`, `#c:ingots/iron`,
`#c:dusts/redstone`), the loot table and the creative-tab entry (after the
latch, its registration order upstream).

Its declaration reuses the `end_rod` plate template and the plate's
`facing`/`rotation` states, and adds **two** booleans:

| property | values | default | role |
|---|---|---|---|
| `input_b` | true/false | false | the stored B edge (the only input side) |
| `powered` | true/false | false | the 2-tick output, not keyed by the blockstate |

`powered` is deliberately absent from the jar's blockstate selectors, so each
of the 48 shipped variants binds both `powered` values (96 states), the same
wildcard rule the other gates use for their unkeyed inputs. No new textures:
the models reuse `morered:block/diode_input_overlay`, `plate_overlay` and the
vanilla `block/smooth_stone` / `block/iron_block`; no new tag (`c:ingots/iron`
came with the multiplexer, the other two with slice 1).

`data/morered/horizon/block/pulse_gate.json` is the AND-gate declaration minus
`input_a`/`input_c`, plus the `powered` boolean; the in-game check sets all 96
states with `setblock` and reads representative ones back (see `../notes.md`
§ Pulse gate verification). The rebuilt `.hmod` is 68,283 bytes (SHA-256
`d9925bd970d982a3153c0b7212827ddd930d91f06024b95b2e8be3b1d8d418e5`), and
`horizon-mod check` reports it clean.

## Slice 4: the red alloy wire (and its ingot)

Added `morered:red_alloy_wire` and the `morered:red_alloy_ingot` it is crafted
from. Content only; the per-face power, connection state and placement state
belong to the native port (`../notes.md` § Red alloy wire verification).

| File | Notes |
|---|---|
| `assets/morered/blockstates/red_alloy_wire.json` | **written here**: vanilla multipart, six parts keyed on the six subwire booleans, each the plate model rotated onto its face |
| `assets/morered/models/block/red_alloy_wire_node.json` | **written here**: a 2-px plate with the carried wire texture; the texture is a transparent red cross, so each part renders as a cross of wire on its face |
| `assets/morered/textures/block/red_alloy_wire.png` | carried verbatim (16×16, transparent body, 2-px red cross) |
| `assets/morered/models/item/red_alloy_wire.json`, `items/red_alloy_wire.json` | carried item model (no custom loader) + the 26.3 item definition |
| `assets/morered/{models/item,items,textures/item}/red_alloy_ingot.*` | carried model + texture, new 26.3 item definition |
| `data/morered/horizon/block/red_alloy_wire.json` | **written here**: `minecraft:basalt` template, `axis` kept as the shape selector, six declared booleans, `shapes` → three 2-px slabs (y bottom, x west, z north), default `axis=y` and all false, light 0, hardness 0, red map colour, stone sound |
| `data/morered/horizon/item/red_alloy_ingot.json` | **written here**: `minecraft:iron_ingot` template |
| `data/morered/recipe/red_alloy_wire.json` | 26.3 shaped: 3 `#c:ingots/redstone_alloy` → **12** wire (upstream's count) |
| `data/morered/recipe/red_alloy_ingot.json` | 26.3 shapeless: `#morered:red_alloyable_ingots` + 4 `#c:dusts/redstone` → 1 ingot |
| `data/morered/loot_table/blocks/red_alloy_wire.json` | **written here**: one wire (the jar's `morered:set_wire_count` loot function has no pin equivalent) |
| `data/c/tags/item/ingots/redstone_alloy.json`, `.../copper.json`, `data/morered/tags/item/red_alloyable_ingots.json`, `.../red_alloy_wires.json` | the wire/ingot recipe tags |
| `data/morered/horizon/creative_tab/morered.json` | both items appended (16 entries) |

### Why `basalt` and not the jar's model/state

The shipped blockstate is a multipart whose first part uses More Red's
`morered:wire_parts` model loader and whose node/line/elbow models use
`morered:rotate_tints` — both native loaders the pin cannot run, so the model
set above is a vanilla-compatible simplification.

The state has an extra `axis` property because of a **pin limitation**: at
`c9d474d` the content loader lays a mod block's states out as template states ×
declared combinations (`mod_content.rs`, `NewBlock::state_combinations`), while
the engine reads them as vanilla's `StateDefinition` (all property names sorted,
first slowest). `down` and `east` sort before `facing`, so an `end_rod`-template
wire with upstream's six booleans stopped the game at load
("registry state order differs from vanilla's state definition"). `basalt`'s
only template property is `axis`, which sorts before all six, so the state table
is valid; `shapes` selects the thin box by `axis` (a ceiling/south/east
attachment collides on the min side of its cell, the deviation). The check for
this is the game reaching its control socket at all, plus `setblock` reads.

### Evidence

```bash
# from content/ (CARGO_TARGET_DIR kept outside the folder)
cargo run --locked --offline --quiet \
  --manifest-path ../../../horizon/crates/cargo-horizon-mod/Cargo.toml -- horizon-mod build
cargo run --locked --offline --quiet \
  --manifest-path ../../../horizon/crates/cargo-horizon-mod/Cargo.toml -- horizon-mod check morered.hmod
# morered 1.21.1-6.0.0.3 (mod, API 0.2, both)
#   73645 bytes, SHA-256 de543aa7009f8e1ce57bb79de1c3b143400df7baaaad8052785132caf80385fc, unsigned
#   no code: content only, nothing to run
```

* In-game (`../tools/e2e.sh`, two runs): all 64 upstream wire combinations are
  accepted with `setblock` and read back; the placed item gives
  `axis=y,down=true`; the wire powers a lamp, decays exactly as the rule says,
  feeds a diode, and reads a lever and a repeater.
* Offline binding (`blockstate_binding_fixture` from the pin tree, over the
  built `model-states.json`): **192 states, 0 unbound**; the part count is the
  number of true subwires (3 states with 0 parts, 18 with 1, …, 3 with 6).
* Craft (`/tmp/opencode/morered-wire-craft4.log`, offscreen): copper ingot + 4
  redstone → `morered:red_alloy_ingot`; 3 ingots → result slot
  `{"count": 12, "item": "morered:red_alloy_wire"}` and 12 in the inventory.
* `horizon-mod-content-warnings.txt`: 0 bytes.

`/recipe give morered:<id>` answers "Unknown recipe" for these (and for the
existing `morered:diode`): the pin's recipe-book index only reads
`data/minecraft/recipe` plus world packs, not content namespaces, so it is not a
recipe-load check here. The crafting table is.

### Gaps specific to this slice

Bundled/coloured cables, wire posts, the wire spool, soldering, shears and the
`TRANSFORM` structure property are out of scope. The wire loot drops 1, not the
subwire count; adding a subwire to an existing wire block (`WireBlockItem.useOn`)
is not ported; there is no lit/unlit tint or occlusion rule. See `../notes.md`
for the full deviation list.

## Slice 5: the bundled cable and its relay plate

Added `morered:bundled_network_cable` (six subwire booleans, sixteen channels,
64 upstream states) and `morered:bundled_cable_relay_plate` (a declared
`channel` 0–15, exactly 16 states). Content only: the per-channel propagation,
the relay's bridge and the block entity belong to the native port
(`../notes.md` § Bundled cable and relay plate verification).

| File | Notes |
|---|---|
| `assets/morered/blockstates/bundled_network_cable.json` | **adapted here**: the jar's multipart minus its first part, whose `morered:wire_parts` model loader the pin cannot run; the six node and twelve elbow parts are the jar's vanilla models |
| `assets/morered/models/block/bundled_network_cable_node.json`, `..._elbow.json` | carried verbatim (vanilla `block/block` parents and the jar's bundled texture) |
| `assets/morered/blockstates/bundled_cable_relay_plate.json` | **written here**: 16 `channel=` variants, all the jar's relay plate model (vanilla `plate_template` parent) |
| `assets/morered/models/block/bundled_cable_relay_plate.json`, `models/item/bundled_cable_relay_plate.json`, `models/item/bundled_network_cable.json` | carried verbatim (vanilla format) |
| `assets/morered/textures/block/bundled_network_cable.png`, `bundled_cable_plate_overlay.png` | carried verbatim |
| `assets/morered/items/bundled_network_cable.json`, `bundled_cable_relay_plate.json` | **written here**: the 26.3 item model definitions |
| `data/morered/horizon/block/bundled_network_cable.json` | **written here**: `minecraft:basalt` template, six declared booleans, `axis` shape selector, default all false, light 0, hardness 0, blue map colour, stone sound (192 registry states = 3 axes × 64 upstream combinations) |
| `data/morered/horizon/block/bundled_cable_relay_plate.json` | **written here**: `minecraft:stone` template (no template properties, so the declared `channel` enum is the whole state space), 16 states, hardness 2 / resistance 5 |
| `data/morered/loot_table/blocks/{bundled_network_cable,bundled_cable_relay_plate}.json` | **written here**: one item each (the cable's `morered:set_wire_count` loot function has no pin equivalent) |
| `data/morered/recipe/bundled_network_cable.json` | 26.3 shaped: 3 `#morered:red_alloy_wires` in a column → 3 cable (upstream keys `#morered:network_cables`; the coloured cables are out of scope) |
| `data/morered/recipe/bundled_cable_relay_plate.json` | 26.3 shaped: its jar pattern (`b`/`F`/`#`) |
| `data/morered/tags/item/bundled_network_cables.json` | the relay recipe's `#morered:bundled_network_cables` tag (the cable) |
| `data/minecraft/tags/block/mineable/pickaxe.json` | the relay plate appended (the jar lists it; the cable is instabreak) |
| `data/morered/horizon/creative_tab/morered.json` | both items appended (18 entries) |

### Why the extra `axis` on the cable

Same pin limitation as the red alloy wire: at `c9d474d` a mod block's states
are laid out as template states × declared combinations while the engine reads
them in vanilla's sorted-name order. `minecraft:chorus_plant` would have given
literal connection booleans, but its state bits mean "connects to the
neighbour", not upstream's "subwire attached to that support face", so the
propagation rule (which shares an attachment face, not a neighbour) would be
wrong. The cable therefore keeps upstream's attachment bits on the `basalt`
template (`axis` sorts before all six booleans), exactly like the wire.

The relay uses `minecraft:stone`, which has no template properties, so a single
declared `channel` enum lays out in vanilla's order without an `axis` and gives
exactly 16 states; its collision is the template's full cube (the jar's model is
a floor plate) and it renders in a fixed orientation because it has no
`facing` property.

### Evidence

```bash
# from content/ (CARGO_TARGET_DIR kept outside the folder), pinned tree:
cargo run --locked --offline --quiet \
  --manifest-path ../target/mixins/modded/crates/cargo-horizon-mod/Cargo.toml -- horizon-mod build
cargo run --locked --offline --quiet \
  --manifest-path ../target/mixins/modded/crates/cargo-horizon-mod/Cargo.toml -- \
  horizon-mod check target/horizon-mod/morered.hmod
# morered 1.21.1-6.0.0.3 (mod, API 0.2, both)
#   84877 bytes, SHA-256 e83780f093c3caa62c7f80bd798c3ed1e33889e847b17c8aac817e84309c2ae6, unsigned
#   no code: content only, nothing to run
```

* In-game (`../tools/e2e.sh`, three runs): all 64 cable combinations and all 16
  relay channel states accepted and read back; a relay on channel 3 lights its
  lamp while a channel-5 relay stays dark; a vanilla lamp beside the cable is
  dark; removing the source, a middle cable or the relay darkens it.
* Offline binding (`../tools/check_binding.py` over the client's built
  `model-states.json`): cable **192 states, every state binds** (18 parts; the
  three all-false axis states render none); relay **16 states, 16 variants,
  one each, 0 unbound**.
* `horizon-mod-content-warnings.txt`: 0 bytes.
* `give morered:bundled_network_cable 2` and
  `give morered:bundled_cable_relay_plate 2` both answered `{"given":2}`; the
  creative tab lists both.

### Gaps specific to this slice

The bundled cable post, the spool, the coloured network cables and the
channel-bridge-across-posts behaviour are out of scope; the relay plate is the
brief's single-channel ↔ vanilla transducer with a `channel` property
(upstream's relay has only `facing`/`TRANSFORM` and no vanilla output). The
cable's multipart base part (a custom model loader) is dropped, so a straight
run renders nodes only. NBT, loot and recipe deviations are listed in
`../notes.md`.

## Slice 6: the bitwise logic plates

Added the six `bitwise_logic/` plates from the same jar: `morered:bitwise_diode`,
`bitwise_not_gate`, `bitwise_and_gate`, `bitwise_or_gate`, `bitwise_xor_gate`,
`bitwise_xnor_gate`. Content only; the per-channel read/write/update behaviour
belongs to the native port (`../notes.md` § Bitwise logic plates verification).

| File | Notes |
|---|---|
| `assets/morered/blockstates/bitwise_*.json` | carried verbatim: `facing` × `rotation`, 24 variants each (no input booleans) |
| `assets/morered/models/block/bitwise_{diode,not_gate,and_gate,or_gate,xor_gate,xnor_gate}{,_alt}.json` | carried verbatim (vanilla format) |
| `assets/morered/models/block/{single,two}_input_bitwise_logic_plate_template{,_alt}.json` | carried verbatim; parent `morered:block/plate_template` (already carried) |
| `assets/morered/textures/block/*_symbol.png`, `{unary,binary}_cable_operator_overlay.png` | carried verbatim (the six symbols + the two cable overlays) |
| `assets/morered/models/item/bitwise_*.json`, `assets/morered/items/bitwise_*.json` | carried item models + 26.3 item definitions |
| `data/morered/horizon/block/bitwise_*.json` | **written here**: `minecraft:end_rod` template, declared `rotation` enum, 4-px `shapes` keyed on `facing`, default `facing=down,rotation=0`, quartz map colour 14, light 0, hardness 0 |
| `data/morered/loot_table/blocks/bitwise_*.json` | carried, already 26.3 format (one plate each) |
| `data/morered/horizon/creative_tab/morered.json` | the six items appended (24 entries) |

The declaration is the gate template minus the input booleans: upstream keeps
the channel powers in the block entity (`ChanneledPowerStorageBlockEntity`'s
`byte[16] power`), so the state is only `facing` + `rotation` — 24 registry
states per plate, exactly the jar's 24 blockstate variants. The 2-px gate
shapes become the 4-px `DOUBLE_PLATE_SHAPES_BY_DIRECTION` boxes.

**No recipe is carried**: upstream has no crafting recipe for these six — only
`morered:soldering` recipes (2 `#c:quarter_slabs/smooth_stone`, quartz,
redstone, `#morered:bundled_network_cables`), and Horizon has no soldering
recipe type (the same reason the other gates' soldering variants are out).
The plates are creative/`give` acquisitions, with the jar's loot tables.

### Evidence

* Offline binding (`../tools/check_binding.py`, extended to the six plates,
  over the e2e's copied `model-states.json`): each plate **24 states, 24
  variants, 0 unbound, variant counts {1: 24}**; cable/relay unchanged.
* In-game (`../tools/e2e.sh`): all 144 states (6 plates × 6 `facing` × 4
  `rotation`) accepted with `setblock`, representative states read back,
  `input_b` rejected as undeclared, `give morered:bitwise_and_gate 2`
  accepted, and the built creative tab lists all six.
* `horizon-mod-content-warnings.txt`: 0 bytes.
* `horizon-mod check`: 118,412 bytes, SHA-256
  `63fdb34ad829890a53b77efe2ffc62251e55dc6dffbc996e7c6f0aaec4fd4940`,
  241 entries, unsigned, "no code: content only". The delivered
  `content/morered.hmod` is that file.

### Gaps specific to this slice

The overlay/symbol textures render untinted (the plate's power is a block
entity value, so there is no state bit to tint and the pin has no block-colour
provider); the `end_rod` template's render flags (no occlusion/solid render)
remain; placement orientation and the stick rotation are the native port's
existing gaps. The coloured cables, cable posts and soldering table remain out
of scope.

## Slice 7 additions — the red-alloy wire posts and the spool

The red-alloy family is now carried complete; the channel-carrying
`bundled_cable_post` / `bundled_cable_spool` stay out (their templates and the
port's scope are recorded in the port's `notes.md` §slice 7 and FINDINGS §13.6).

| Path | Files | |
|---|---|---|
| `assets/morered/blockstates` | +3 | `redwire_post{,_plate,_relay_plate}.json`, six `facing=` variants each (`power` is the wildcard) |
| `assets/morered/models/block` | +3 | the three post models, parent `morered:block/plate_template` |
| `assets/morered/models/item` | +4 | the three post block items + `redwire_spool` (`minecraft:item/handheld`) |
| `assets/morered/items` | +4 | the 26.3 item model definitions |
| `assets/morered/textures/block` | +2 | `redwire_post_rod`, `redwire_post_plate_overlay` |
| `assets/morered/textures/item` | +1 | `redwire_spool` |
| `data/morered/horizon/block` | +3 | the three declarations (96 states each) |
| `data/morered/horizon/item` | +1 | `redwire_spool` (`minecraft:stick`, stack 1) |
| `data/morered/loot_table/blocks` | +3 | one item each |
| `data/morered/recipe` | +4 | the three posts + the spool (soldering variants left out) |
| `data/morered/tags/block` | +1 | `redwire_posts` |
| `data/c/tags/item` | +1 | `rods/wooden` (`minecraft:stick`) for the spool |

Current totals: **270 content files + `horizon-mod.toml`; the `.hmod` holds 271
entries**, 129,166 bytes, SHA-256
`ca7270144a1ca18f6d3fc9f20280c953649c3cdfdd6f597c6b0a459580fdf910`,
`horizon-mod check` clean. The creative tab now lists 28 items (the four added
ones are the three posts and the spool). The counts table earlier in this file
is the slice-1 baseline.

| Block id | state | states | connection sides |
|---|---|---|---|
| `morered:redwire_post` | `facing` × `power` (0–15) | 96 | its attachment (reads the support, emits away from it) |
| `morered:redwire_post_plate` | `facing` × `power` | 96 | none (link only) |
| `morered:redwire_post_relay_plate` | `facing` × `power` | 96 | the four faces perpendicular to the attachment |

Offline binding over the client's built `model-states.json`
(`tools/check_binding.py`, extended with the three posts): **each post 96
states, 6 variants, 0 unbound, variant count 1**; the cable, relay and bitwise
results are unchanged. The in-game e2e accepted all 96 plain-post states, gave
the three post items and the spool, and the tab lists them. The post link is
the block entity's `connections` list (relative offsets); the full state table
and gaps are in the port's `notes.md` §slice 7.
