# Validation

Recorded for **`morered` 0.2.2**, the SDK package (`api = "0.2"`) at SDK
`0b14f8acfd97fa2c750e42795249dcc981178ca4` / mixins
`e59da9094f0c2fdda6c627fe9eff22dec6a906f0`, game 26.3, on Linux with the installed
Rust toolchain (1.97.0; the package has no `rust-toolchain.toml` by design — the pinned
Horizon commit decides the toolchain). Evidence logs:
`../_shared/evidence/morered/sdk-*.log` (0.2.0) and
`../_shared/evidence/morered-e2e/*.log` (0.2.1/0.2.2 in game).

| Check | Command | Result |
|---|---|---|
| runtime only | `cargo test --offline` before any glue | **66 passed**, 0 failed |
| tests | `cargo test --offline` | **84 passed**, 0 failed (0.2.2; 0.2.0 was 75) |
| lints | `cargo clippy --all-targets --offline -- -D warnings` | exit 0, 0 warnings |
| format | `cargo fmt --check` | clean; `src/runtime.rs` unchanged (`cmp`) |
| fail-capability | a deliberate type error / a changed codec | red compile (`sdk-test-red.log`); `only_changed_properties_are_written` fails 74/1 (`sdk-test-assert-red.log`); the 0.2.2 wire tests ran red against the missing API before the fix |
| package | `rm -f Cargo.lock && cargo horizon-mod build` | `target/horizon-mod/morered.hmod`, EXIT 0 |
| registry check | `cargo horizon-mod check morered.hmod` | `morered 0.2.2 (native-mod, format native-1/0b14f8a…/e59da90…, for 26.3)`, 239,955 B, SHA-256 `9f3370b8…5d5c`, EXIT 0 |
| fits the pinned build | `cargo horizon-mod fits morered.hmod --build 0b14f8a` | `0b14f8a  made for`, EXIT 0 |
| reproducible | a second `rm -f Cargo.lock && cargo horizon-mod build` | same bytes, same SHA-256, `cmp` identical |
| runtime untouched | `unzip -p morered.hmod src/runtime.rs \| sha256sum` | `c53beb82…cd9d` = `src/runtime.rs` (the 0.2.2 file; 0.2.0 was `149b7bea…85a1`) |
| in-game e2e | `ports/tools/launcher-game.sh run morered` | **PASS + EXIT:0** (launcher-built game, pin `0b14f8a`); control `EXIT:1`; `check-e2e-logs.py morered-e2e --strict` exit 0 |

Package: 413 entries (3 `src/`, 259 `assets/`, 145 `data/`, the manifest, `Cargo.toml`,
`README.md`, `LICENSE`, `LICENSE-NOTICE.md`, `uses.json`), no `Cargo.lock`, unsigned.
`uses.json`: 403 surface entries (0.2.0: 383) — 188 types, 88 handles, 52 hook
ids and fields, 45 methods, 23 traits, 4 Parts, 2 macros, `capability:registries`.

## 26.3 dialect sweep (2026-11-21) — 0.2.3

| Check | Command | Result |
|---|---|---|
| static dialect | `python3 ports/tools/check_261_dialect.py morered.hmod` | exit 0; pre-fix package exit 1 (`../_shared/evidence/morered/dialect-check-red.log`) |
| tests | `cargo test --offline` | exit 0 (`dialect-test.log`) |
| package | `cargo horizon-mod build` twice after `rm -f Cargo.lock` | `cmp`-identical, built against 0a875da |
| registry check | `cargo horizon-mod check morered.hmod` | **239,369 B**, SHA-256 `1dfa52998bcd77825a093dd1f8292425f132aaf691363d9ecd860c205ea1f4fb`, exit 0 |
| fits the pin | `cargo horizon-mod fits morered.hmod --build 0a875da` | `0a875da  made for`, exit 0 |
| the gate | `python3 ports/tools/verify-sdk-package.py morered` | 1/1 |
| in game | `tools/e2e-dialect.sh` | `PASS` + `EXIT:0`; `SELFTEST=1` `FAIL` + `EXIT:1`; `check-e2e-logs.py morered --strict` exit 0 |

Changed: 4 recipes + 42 loot tables. Proof: redwire_post crafts 1; redwire_post drops 1; pre-fix 0.2.2 red in `dialect-check-red.log`. Full record in [`notes.md`](notes.md) §"26.3 dialect sweep".
