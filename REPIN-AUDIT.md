# More Red — re-pin audit (Horizon `83ad384f`)

Read-only audit. **No port code, content, manifest or campaign document changed.**
Nothing was built or run: every claim is a source read of the two pins.

| | Pin | Checkout |
|---|---|---|
| New | Horizon `83ad384f48d0742746d518737511882680aab5b7` (427 hooks / 159 helpers), mixins `e8cd0aa38cffef49cb2e088ef716baa107148c02` | `ports/_shared/pins/horizon-83ad384f` |
| Old (shipped) | Horizon `c9d474dd9116e84c243f8992d75c11314df56f06` (108 hooks), mixins `d9c83d7…` | `ports/artifacts/target/mixins/original/32051c454db029bf4671` |

Sources read: `notes.md` (§Gaps, slices 4–10 deviations), `FINDINGS.md` §15.3,
`ARCHITECTURE.md` item 12, `ports/_shared/repin-83ad384f.md`; then
`native-hooks.json` and the Rust sources of both pins. Paths are relative to
the **new** pin unless marked “c9d474d”.

## Summary

- **Deferred items audited: 16 (+1 verification-only). Newly realizable: 9.
  Still blocked: 7.**
- `TRANSFORM` is **still blocked**: the 427 hooks contain no name with
  `rotate`, `mirror` or `structure`, the rotate/mirror table is a private
  per-state table applied by the structure processor, and the content generator
  still clones the template's row (so a declared property falls back to the
  rotated template state's default combination). Slice 9's probe conclusion
  stands at the new pin.
- **Highest-value next step:** lit/unlit tint **and** the missing line/elbow
  geometry — both are now trivially reachable because a mod block entity no
  longer needs a template that owns one (API 129) and standalone models can be
  drawn with a tint (API 151). That is the port's most visible fidelity gap.
- Cheapest verifiable win: the subwire-count drop (`morered:set_wire_count`),
  which the notes called impossible; `native_block_drops` is given the broken
  block's **state** and position, so the bit count is a table lookup.

## Deferred matrix

| # | Item (source) | Pin capability | Contract | At `c9d474d` | Verdict | Exact next step |
|---|---|---|---|---|---|---|
| 1 | `TRANSFORM` state on wires/bundled cable/coloured cables/posts; structure rotate/mirror cannot compose it (`notes.md` slice 9; `FINDINGS.md` §15) | `StateTransforms` (`crates/horizon-world/src/worldgen/structure.rs:182`) with private `rotate` `:255` / `mirror` `:264`; applied at `worldgen/structure/processor.rs:1543-1544`; **no hook** (no `rotate`/`mirror`/`structure` in `native-hooks.json`) | absent | absent | **blocked** | Needs a per-block rotate/mirror hook (or per-property rotation metadata in the content format). Do not add the state: `/setblock` would accept it and a structure copy would silently drop it |
| 2 | Declared `power`/`rotation`/`channel` also reset on structure rotation (`notes.md` slice 9 gap 2) | Same table; content rows are cloned from the template and only the declared combination is set (`crates/horizon-client/src/mod_content.rs:1601-1606`, `states()` ~`:1216-1240`) | absent | absent | **blocked** | Same as #1 |
| 3 | No lit/unlit tint: the models' `tintindex` needs a block-colour provider (`notes.md` §Gaps; slice 4/5/6/7/8 gaps) | `block_entity` on **any** template (`native_hooks.rs:4238+`, doc API 129 `docs/native-mods.md:1113`, **new**); `block_entity_submit` (`native_hooks.rs:3190`, API 71); `standalone_models` (API 151) + `native_draw_model(draw, model, tint)` (`native_block_entity_draw.rs:689`); `native_draw_quad` `:548`, `native_draw_cube` `:620` | replace / listen / helper | API 129 **no**, API 151 **no**, API 71 yes | **realizable now** | Give each powered family a BE (API 129), send its power with `block_entity_update_tag`, and draw the part models with `native_draw_model(..., power_tint)` in `native_block_entity_submit` |
| 4 | No line/elbow geometry: the jar's `morered:wire_parts` multipart loader is native code the pin cannot run, so wires/cables show nodes only (`notes.md` slice 5/8 gap) | Same as #3, plus `native_draw_quad`/`native_draw_cube` with the carried wire texture; no custom model loader is needed | listen / helper | API 129/151 new | **realizable now** | In the same BE submit, draw one quad/cube per attached face (state bits) from the block's corner to the edge, tinted; keep the JSON model for the node |
| 5 | Loot drops 1 cable, not the subwire count; empty cable drops 1 (`morered:set_wire_count`, `notes.md` slice 5 gap 6, slice 8 gap 6) | `native_block_drops(pos, state, player, tool, explosion, drops)` (`native_hooks.rs:9366`, API 75) — the **state** is given; also `loot_function` (API 133, `crates/horizon-world/src/loot.rs:70`) with `ctx.place.block_entity` (`:146`) | collect | **yes** (API 75 present) | **realizable now** | Hook `loot_drops`: count the state's six subwire bits, set the block item's count, drop nothing when 0. The notes' “no pin equivalent” is wrong |
| 6 | The wire/cable item always places a fresh block; upstream adds a subwire to the clicked existing block (`notes.md` slice 8 gap 7) | `use_item_on` (`native_hooks.rs:6482`) + `native_set_block` (`:8464`) | cancel / helper | yes | **realizable now** | On a click whose target is the same family, set the state with the clicked face's bit instead of placing |
| 7 | No channeled-power capability: a third-party channel block is invisible; the relay is not upstream's bridge (`notes.md` slice 5/6/8 gaps) | No capability surface in `docs/native-mods.md` (no `capabilit*`); `block_signal` (`native_hooks.rs:1494+`) only answers vanilla redstone 0–15 | absent | absent | **blocked** | Needs a mod capability/data exchange API. Keep the relay transducer and its 16-state deviation |
| 8 | A comparator's strength collapses to a boolean (gates are binary) (`notes.md` §Gaps) | `StateFacts.weak/strong` are the static empty-level tables (`crates/horizon-world/src/level/data.rs:53-68`); the engine's per-position read is `adjust_signal` (`game.rs:1347`, output only); `native_neighbor_signal` (`native_hooks.rs:8501`) is the **max over six faces**, not per direction | absent | absent | **blocked** | Needs a directional `getSignal(pos, face)` read helper (or a block-entity signal read). Then gates can carry 0–15 |
| 9 | Pulse clear is an in-memory countdown: a gate unloaded at its due tick keeps `powered=true` (`notes.md` §Gaps) | Scheduled ticks are per-chunk and saved: `LevelTicks::schedule_block` (`crates/horizon-world/src/level/ticks.rs:110`), `Chunk::saved_ticks`/`set_saved_ticks` (`crates/horizon-world/src/chunk.rs:262/275`); `native_schedule_block_tick` (`native_hooks.rs:8328`), `native_tick_block` (`:1687`); `Ticks` is only in `random_tick`/`tick_block`/`update_shape` | helper | yes | **realizable now** | Schedule the clear from `update_shape` (when the pulse arms) and clear in `tick_block`; keep the map as a fallback. Prove persistence with a chunk-unload probe |
| 10 | A rise+fall inside one tick collapses (inputs sampled at `level_tick_post`) (`notes.md` §Gaps, slice 6 gap 2) | `neighbor_changed` is **listen** (`native-hooks.json`); no mutable neighbour hook | hook-only | hook-only | **blocked** | Needs a writable neighbour hook; nothing at this pin |
| 11 | No soldering recipe type / bench; the plates have no recipe (`notes.md` slice 6 gap 3; `ARCHITECTURE` item 12) | Recipes of a mod's own type load and are readable: `native_recipes_of_type` (`native_hooks.rs:9253`, API 36, `docs/native-mods.md:312`); content menu type + `native_screen` (`:3321`) + `native_open_menu` (`native_menus.rs:124`) + `block_entity`/`_tick` | helper / replace | API 36 yes | **realizable now** | A soldering-table BE: 4 input slots + output, match `morered:soldering` recipes in the tick, content menu/screen. Big but no pin gap |
| 12 | The sag raytrace is skipped (`notes.md` slice 7 gap 4) | Only a client view ray: `native_raycast(distance)` / `native_view_hit` (`game/native_claimed_items.rs:137/142`); no arbitrary segment ray, and the click that would hit the sag never reaches the wire | helper (view only) | yes | **blocked** | Needs a "redirect/cancel a left click with my own ray" surface; the view ray alone cannot hit an off-block curve |
| 13 | No break particles/sounds (and the client break packet) for wires/posts (`notes.md` slice 7 gap 4) | `native_play_sound` (`native_hooks.rs:8193`), `native_send_particles` (`:8233`) | helper | yes | **realizable now** | On break/erase, play `block.red_alloy_wire.break` and a dust burst; the server write already syncs |
| 14 | The spool's first post is process state keyed by player UUID, not item state (`notes.md` slice 7 gap 3) | `native_set_component` (`game/native_components.rs:49`), `native_carried_stack` (`native_hooks.rs:8450`), `item_container` hook (API 370, **new**, `:3109`) | helper / replace | components yes, API 370 no | **realizable now** | Store the first `BlockPos` in a registered component on the carried spool; keep the UUID map only as a fallback |
| 15 | A broken post can leave its BE behind (removal checks the template's BE flag) (`notes.md` slice 7 gap 5) | API 129 (**new**) gives the post a BE on its own template; the engine removes a BE whose `fits` fails on the new block (`game/block_edits.rs:429-436`, `fits` `crates/horizon-client-render/src/containers.rs:452-464`, `Self::Mod(_) => !name.starts_with("minecraft:")`) | engine | **no** (template had none) | **covered now** | Rely on the engine removal, but note the `Mod` arm keeps a stale BE when one **mod** block replaces another — keep the port's link pruning |
| 16 | A linked post in an unloaded chunk contributes nothing (`notes.md` slice 7 gap 8) | No cross-chunk read; `block_at` refuses unloaded chunks | absent | absent | **blocked** (as upstream) | Keep; upstream's BE is equally chunk-bound |
| — | `/recipe give` cannot read mod namespaces (`notes.md` slices 4–7 findings) | Not a pin surface; solved by #11 (a real bench) | — | — | verification only | Re-test after #11 |

## The four spot checks

| Check | Result |
|---|---|
| **Block-state rotate/mirror (`TRANSFORM`)** | **Absent.** No hook name contains `rotate`/`mirror`/`structure`; `StateTransforms` is a private table (`structure.rs:182`, `:255`, `:264`) applied by `processor.rs:1543-1544`; the content generator clones the template row and only sets the declared combination (`mod_content.rs:1601-1606`), so `power` and `transform` become the rotated template state's defaults. Also: template block-entity NBT is not rotated at all (`template.rs` has no BE transform), so even a state-only `TRANSFORM` would not normalize the power arrays. Slice 9 stands. |
| **Client world-space rendering** | Present (`native_world_drawn`, raw wgpu) but **not needed for this port**: per-block geometry and tint belong in `block_entity_submit` + `native_draw_model`, now unblocked by API 129/151. |
| **Heightmap** | Not used. |
| **Fluid / entity hooks** | No fluids. Entity hooks are irrelevant; `block_entity_submit` is the relevant one. |

## Corrections to the campaign re-pin note / port notes

- `repin-83ad384f.md` puts block-state rotate/mirror under *Still absent*: **correct**, confirmed here with the exact table/processor lines.
- `notes.md` (slice 5 gap 6, slice 8 gap 6) says `set_wire_count` “has no pin equivalent”. **Wrong even at `c9d474d`**: `native_block_drops` receives the broken block's `state` (`native_hooks.rs:9366`, API 75), which is where the subwire bits live. The drop count is a state read, not a capability.
- The notes' “no lit/unlit tint / no line geometry” is a *scope* choice, not a pin limit: API 71 existed at `c9d474d`; API 129 (any template) and API 151 (standalone models with tint) are new and make it cheap.

## Evidence (commands)

```
python3 -c '…json…'                    # 427 hook names; no rotate/mirror/structure
grep -n "pub struct StateTransforms" -A 12 crates/horizon-world/src/worldgen/structure.rs
grep -n "fn rotate\|fn mirror" -A 8 …/structure.rs
grep -n "transforms.rotate\|transforms.mirror" -B 3 -A 3 …/structure/processor.rs
grep -n "structure-blocks.json" -A 8 crates/horizon-client/src/mod_content.rs
grep -n "fn native_block_drops" -B 18 -A 14 crates/horizon-client/src/game/native_hooks.rs
grep -n "fn native_draw_model" -B 8 -A 14 crates/horizon-client/src/game/native_block_entity_draw.rs
grep -n "fn native_block_entity_submit" -B 12 -A 10 crates/horizon-client/src/game/native_hooks.rs
grep -n "API 129\|API 151\|API 133\|API 36" docs/native-mods.md
grep -n "fn schedule_block\|saved_ticks\|set_saved_ticks" crates/horizon-world/src/level/ticks.rs crates/horizon-world/src/chunk.rs
grep -n "Self::Mod(_) =>" -A 2 crates/horizon-client-render/src/containers.rs
```
