# morered (Horizon port)

A Horizon port of [morered](Commoble/morered) — MIT.

**Status: first slices.** What is below is what the port actually does today.

## Implemented so far

- The **logic gate plate** family (diode, NOT, AND/OR/NAND/NOR, XOR/XNOR,
- multiplexer, 2-input AND/NAND, latch, pulse gate): a plate attaches to a face,
- rotates, reads its input sides, and emits 15 from its output face when its
- logic function passes. The pure model (`runtime.rs`) has the upstream function
- table, the 24-entry output-direction table, the clockwise input mapping, the
- latch's hold/reset rule and the pulse gate's edge/2-tick rule (18 tests).
- Content: eleven gate blocks (plus the `stone_plate` recipe dependency) on an
- `end_rod` template with the real `facing`/`rotation`/`input_*`/`powered`

## Not yet ported

- ~~Placement orientation is not ported (`state_for_placement`)~~: **resolved in
- slice 10** (see below). A plate placed through vanilla placement now takes
- its attachment from the clicked face and its rotation from the clicked
- quadrant (`state_for_placement` + `use_item_on` rod rotation); `/setblock`
- and native placement were already exact.
- The carried `end_rod` template gives the plates their 2-px shape and
- collision, but there is **no lit/unlit tint** (the models' tint indices need
- a block-colour provider).

## Install

Build the bundle with a native-mod capable launcher, or add the file from the
registry once it is listed. Pinned to Horizon `1d63e0a` and horizon-mixins
`5a64af93` for game 26.3.

## Build from source

```sh
cargo test
python3 tools/horizon.py check --horizon <horizon checkout>
python3 tools/horizon.py build --horizon <horizon checkout>
python3 tools/package.py
```

## Credits and licence

Original mod: Commoble/morered. Port: AdLf0208. Licence: MIT — see [LICENSE](LICENSE);
the original notices are preserved.
