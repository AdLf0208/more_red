# Validation

Recorded for **`morered` 0.2.0**, the SDK package (`api = "0.2"`) at SDK
`0b14f8acfd97fa2c750e42795249dcc981178ca4` / mixins
`e59da9094f0c2fdda6c627fe9eff22dec6a906f0`, game 26.3, on Linux with the installed
Rust toolchain (1.97.0; the package has no `rust-toolchain.toml` by design — the pinned
Horizon commit decides the toolchain). Evidence logs:
`../_shared/evidence/morered/sdk-*.log`.

| Check | Command | Result |
|---|---|---|
| runtime only | `cargo test --offline` before any glue | **66 passed**, 0 failed |
| tests | `cargo test --offline` | **75 passed**, 0 failed (66 `runtime::`, 9 `support::`) |
| lints | `cargo clippy --all-targets --offline -- -D warnings` | exit 0, 0 warnings |
| format | `cargo fmt --check` | clean; `src/runtime.rs` unchanged (`cmp`) |
| fail-capability | a deliberate type error / a changed codec | red compile (`sdk-test-red.log`); `only_changed_properties_are_written` fails 74/1 (`sdk-test-assert-red.log`) |
| package | `rm -f Cargo.lock && cargo horizon-mod build` | `target/horizon-mod/morered.hmod`, EXIT 0 |
| registry check | `cargo horizon-mod check morered.hmod` | `morered 0.2.0 (native-mod, format native-1/0b14f8a…/e59da90…, for 26.3)`, 233,165 B, SHA-256 `03a19939…4128`, EXIT 0 |
| fits the pinned build | `cargo horizon-mod fits morered.hmod --build nightly` | `0b14f8a  made for`, EXIT 0 |
| reproducible | a second `rm -f Cargo.lock && cargo horizon-mod build` | same bytes, same SHA-256, `cmp` identical |
| runtime untouched | `unzip -p morered.hmod src/runtime.rs \| sha256sum` | `149b7bea…85a1` = `src/runtime.rs` = the retired copy |
| in-game e2e | needs a launcher-built game | **not run here**; see `../_shared/evidence/morered/sdk-e2e-status.md` |

Package: 413 entries (3 `src/`, 259 `assets/`, 145 `data/`, the manifest, `Cargo.toml`,
`README.md`, `LICENSE`, `LICENSE-NOTICE.md`, `uses.json`), no `Cargo.lock`, unsigned.
`uses.json`: 383 surface entries (14 hook ids, 42 methods, 4 Parts, 2 macros,
`registries`, 23 traits).
