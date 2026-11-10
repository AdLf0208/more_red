#!/usr/bin/env python3
"""Check that the carried wire/cable/relay/post blockstates bind every state.

Reads the client's built state table (`model-states.json`, produced by a
`--dev-mod content` run; the e2e copies it to
`/tmp/opencode/morered-bundled-model-states.json`) and the content's blockstate
JSONs: the plain wire and the sixteen coloured cables' multiparts, the bundled
cable's multipart, the relay plate's variants, the six bitwise plates'
`facing`/`rotation` variants and the three posts. A state with no matching
selector is "unbound"; a selector naming a property or value the block does not
have is an error. Reports the part-count histogram for the multipart blocks.
"""
import json
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONTENT = ROOT / "content"
DEFAULT_STATES = Path("/tmp/opencode/morered-bundled-model-states.json")
BLOCKS = (
    ["morered:red_alloy_wire"]
    + [f"morered:{colours}_network_cable" for colours in (
        "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink",
        "gray", "light_gray", "cyan", "purple", "blue", "brown", "green",
        "red", "black",
    )]
    + [
        "morered:bundled_network_cable",
        "morered:bundled_cable_relay_plate",
        "morered:bitwise_diode",
        "morered:bitwise_not_gate",
        "morered:bitwise_and_gate",
        "morered:bitwise_or_gate",
        "morered:bitwise_xor_gate",
        "morered:bitwise_xnor_gate",
        "morered:redwire_post",
        "morered:redwire_post_plate",
        "morered:redwire_post_relay_plate",
    ]
)


def key_matches(selector, state):
    if not selector:
        return True
    for clause in selector.split(","):
        name, _, value = clause.partition("=")
        if state.get(name) != value:
            return False
    return True


def when_matches(when, state):
    return all(state.get(name) == value for name, value in when.items())


def check_selector_names(selector, state, block, where):
    names = set()
    if selector:
        for clause in selector.split(","):
            names.add(clause.partition("=")[0])
    for name in names:
        if name not in state:
            raise SystemExit(f"{block}: {where} selects unknown property {name}")
    return names


def main():
    states_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_STATES
    table = json.loads(states_path.read_text())
    failed = False
    for block in BLOCKS:
        entry = table.get(block)
        if entry is None:
            raise SystemExit(f"{block}: not in {states_path}")
        states = entry["states"]
        blockstate = json.loads(
            (CONTENT / "assets/morered/blockstates" / f"{block.split(':')[1]}.json").read_text()
        )
        if "variants" in blockstate:
            variants = blockstate["variants"]
            for selector in variants:
                check_selector_names(selector, states[0], block, "variant")
            counts = [sum(1 for selector in variants if key_matches(selector, state)) for state in states]
            unbound = sum(1 for count in counts if count == 0)
            print(
                f"{block}: {len(states)} states, {len(variants)} variants, "
                f"{unbound} unbound, variant counts {dict(Counter(counts))}"
            )
            failed |= unbound != 0 or any(count != 1 for count in counts)
        else:
            parts = blockstate["multipart"]
            names = set()
            for part in parts:
                names |= check_selector_names(
                    ",".join(f"{k}={v}" for k, v in part.get("when", {}).items()),
                    states[0],
                    block,
                    "multipart part",
                )
            if names - set(states[0]):
                raise SystemExit(f"{block}: multipart selects unknown property {names - set(states[0])}")
            counts = [sum(1 for part in parts if when_matches(part.get("when", {}), state)) for state in states]
            print(
                f"{block}: {len(states)} states, {len(parts)} parts, "
                f"part counts {dict(sorted(Counter(counts).items()))}"
            )
            all_false = {f"{face}": "false" for face in ("down", "up", "north", "south", "west", "east")}
            for state, count in zip(states, counts):
                empty = all(state.get(face) == "false" for face in all_false)
                if count == 0 and not empty:
                    print(f"  UNBOUND {state}")
                    failed = True
    print("FAIL: a state did not bind" if failed else "PASS: every state binds")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
