# More Red for Horizon

A port of [More Red](https://github.com/Commoble/morered) (`morered`, version
`v1.21.1-6.0.0.3`, MIT) to Horizon's SDK mod format (`api = "0.2"`, launcher
≥ 0.2.70). One flat `.hmod` carries the carried content **and** the native code.

## What it adds

* **Logic gate plates** — diode, NOT, AND/NAND, OR/NOR, XOR/XNOR, multiplexer,
  2-input AND/NAND, latch (set/reset with a complementary output) and pulse gate
  (a rising edge emits for two ticks). Each plate attaches to a face, turns with
  a wooden rod, reads its input sides and emits signal 15 from its output face.
* **Bitwise channeled plates** — diode, NOT, AND, OR, XOR, XNOR over the sixteen
  channels of a bundled cable.
* **Red alloy wire** — six subwires per cell with the upstream doubled-power
  solve; one item drops per attached subwire.
* **Coloured network cables** — the sixteen dyed variants; different dyes never
  connect, a coloured cable connects to the plain wire.
* **Bundled cable and its relay plate** — sixteen independent channels, and a
  relay that bridges one channel to vanilla redstone.
* **Wire posts and the wire spool** — link two posts up to 32 blocks apart with
  the spool; power decays one per link.

## Layout

```text
horizon-mod.toml     the only manifest: id/name/version/api/side/capabilities + [native] pins
Cargo.toml           one crate; depends on horizon-mod-api from .horizon/sdk/
src/lib.rs           the one #[mod_api::horizon_mod] impl Hooks
src/support.rs       the game glue and the four Parts (wire, cable, bitwise, post)
src/runtime.rs       the pure rules and their tests (no mod_api types)
assets/morered/      carried blockstates, models, textures, item definitions, lang
data/morered/        carried block/item declarations, recipes, loot, tags
tools/e2e.sh         the in-game scenario (needs a launcher-built game)
```

## Build and check

```sh
# The SDK is fetched into .horizon/sdk/ by the first horizon-mod command.
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
rm -f Cargo.lock
cargo horizon-mod build          # -> target/horizon-mod/morered.hmod
cargo horizon-mod check morered.hmod
cargo horizon-mod fits morered.hmod --build nightly
```

`cargo horizon-mod build` packs; it does not compile. The native compile is done
by the launcher (`horizon-launcher native-build … --trust-native-code`), which is
also what `tools/e2e.sh` needs: point `GAME=` at the game it built.

## Licence and provenance

Ported from More Red v1.21.1-6.0.0.3 (`Commoble/morered`, MIT). The upstream
copyright and licence travel in [`LICENSE`](LICENSE) and
[`LICENSE-NOTICE.md`](LICENSE-NOTICE.md), and the carried content keeps its
`data/morered/license/` notice. This port is **local-only** until the maintainer
approves publishing; it is not a redistribution of the original mod.
