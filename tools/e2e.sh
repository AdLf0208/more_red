#!/usr/bin/env bash
# In-game check for the More Red logic plates.
#
# AND gate at (0,64,0): facing=down, rotation=0 -> output north, inputs
# A=east, B=south, C=west; a redstone lamp at (0,64,-1) is the probe.
#  - unpowered: dark
#  - A+C only: dark (proves AND, not OR)
#  - A+B+C: lit
#  - remove C: dark again
#  - NOT gate: lit unpowered, dark with B powered
#  - OR gate with A only: lit (different function, same input)
#  - latch: unpowered -> complementary (south) lit; A sets; release holds;
#    C resets; both inputs -> both outputs dark, stored set; release holds set
#  - pulse gate: a rising edge emits for 2 ticks (sampled), stays off while B
#    stays powered, a falling edge keeps it off, a new edge pulses again; a
#    diode in the same wiring holds (differential) and stone emits nothing
#  - a pulse drives a latch (another gate), which records and holds it
#  - all 96 pulse states are accepted (`setblock` with every property)
#  - counterfactual: the gate replaced by stone, all inputs powered -> dark
#  - bitwise NOT: cable A channel 1 high writes channel 1 low on cable B (lamp
#    dark); removing the source writes 31 (seed 30) and lights it; a diode in
#    the same wiring passes the source, and a vanilla lamp on a diode's output
#    face stays dark (the plate has no vanilla signal)
#  - bitwise AND/OR/XOR/XNOR: per-channel truth tables on channel 1 with a
#    channel 5 control, proving channel isolation; a stone counterfactual
#  - all 144 bitwise states (6 plates x 6 facing x 4 rotation) are accepted,
#    and `input_b` is rejected as an undeclared property
#  - coloured cables: a red line lights its lamp with the plain wire's decay;
#    a blue line beside the powered red line stays dark (different dyes never
#    link); the blue line lights its own lamp from its own source; the powered
#    blue line does not feed an adjacent unpowered red line; cutting the blue
#    line darkens only its far side, and removing a source drains only its own
#    colour
#  - drops: a broken wire or cable drops one item per attached subwire (3 for
#    three subwires, nothing for an empty wire); a dirt block in the same cell
#    still drops its one item, so the hook leaves other drops alone
#  - plate placement: a click on a floor, a wall and a ceiling support sets
#    `facing` to the clicked face's opposite and `rotation` to the clicked
#    quadrant's index (the same quadrant clicked at a yaw ~180 degrees away
#    gives the same rotation); a bitwise plate inherits the rule
#  - plate rod rotation: a wooden-rod click advances `rotation` a quarter turn
#    (coal does nothing), and the west lamp lights at rotation 1 and goes dark
#    at rotation 2
set -euo pipefail

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/morered
# The control binary is shared at the campaign pin; the game is this port's build.
CTL=${CTL:-$CLONE/ports/_shared/bin/horizon-1d63e0a5/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/morered-e2e.sock}
# SELFTEST=1 keeps the default rig but asserts the unpowered AND gate is lit,
# so the run must fail: the counterfactual proving the lamp checks can fail.
#
# EMBEDDED=1: the game is launched with **only the native bundle** (no
# `--dev-mod`) and the content comes from the bundle's embedded copy
# (`mixins/morered/data/morered-content.hmod`, the AE2 pattern). The run also
# checks the file the helper wrote. CONTROL=1 (default with EMBEDDED) first
# launches the same binary with `.horizon-cache` unusable, where the plate and
# spool ids must be unknown.
SELFTEST=${SELFTEST:-0}
EMBEDDED=${EMBEDDED:-0}
CONTROL=${CONTROL:-$EMBEDDED}
WORLD=MoreRedE2E
CONTROL_WORLD=MoreRedControl

[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/morered-home.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")"
rm -f "$SOCK"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
# The script called `fail` on many error paths but never defined it, so a real
# failure died as "fail: command not found" (exit 127) with no message
# (2026-11-09). Defined here; the checks are unchanged.
fail() { echo "FAIL: $*" >&2; exit 1; }
setblock() { ctl command "execute in minecraft:the_end run setblock $1" >/dev/null; }
break_wire() {
    local reply aim
    # `ctl break X Y Z` resolves the block but needs a visible face from the
    # eye. Aim at the block without naming a face: a wire is a thin post, and
    # its south/west faces are empty (naming one reports "no face … visible").
    aim=$(ctl look-at-block "$1" "$2" "$3" 2>&1) \
        || fail "cannot aim at $1 $2 $3: $(tr -d '\n' <<<"$aim" | cut -c1-300) at $(ctl state | jq -c '.player.position')"
    ctl wait 2 >/dev/null
    reply=$(ctl break "$1" "$2" "$3") || fail "cannot break $1 $2 $3: $reply"
}
lamp_state() { ctl block 0 64 -1 | jq -r '.state'; }
lamp_at() { ctl block "$1" "$2" "$3" | jq -r '.state'; }
expect_lamp_at() {
    local state
    state=$(lamp_at "$1" "$2" "$3")
    if ! echo "$state" | grep -q "lit=$4"; then
        echo "FAIL: expected lamp at $1,$2,$3 lit=$4, got $state" >&2
        exit 1
    fi
    echo "lamp $1,$2,$3: $state"
}
expect_lamp() {
    local state
    state=$(lamp_state)
    if ! echo "$state" | grep -q "lit=$1"; then
        echo "FAIL: expected lit=$1, got $state" >&2
        ctl block 0 64 0 >&2 || true
        exit 1
    fi
    echo "lamp: $state"
}
expect_latch_state() {
    local state
    state=$(ctl block 0 64 0 | jq -r '.state')
    case "$state" in
        *"$1"*) echo "latch: $state" ;;
        *) echo "FAIL: expected latch state to contain $1, got $state" >&2; exit 1 ;;
    esac
}

# The block state at a position, or nothing if the read failed.
state_at() { ctl block "$1" "$2" "$3" | jq -r '.state // empty'; }
expect_state_has() { # x y z substring
    local state
    state=$(state_at "$1" "$2" "$3")
    case "$state" in
        *"$4"*) echo "state $1,$2,$3: $state" ;;
        *)
            echo "FAIL: expected state at $1,$2,$3 to contain $4, got $state" >&2
            exit 1
            ;;
    esac
}

# The wire's saved per-face power (`Data get block`; upstream
# `WireBlockEntity.POWER` is a six-int array in doubled units).
wire_power_text() {
    ctl command "execute in minecraft:the_end run data get block $1 $2 $3 power" \
        | jq -r '.chat // [] | join(" ")'
}
expect_wire_power() { # x y z expected-down-raw
    local text first
    text=$(wire_power_text "$1" "$2" "$3")
    # `|| true`: a reply with no `[I; ...` must report the assertion, not kill
    # the script under `set -e`.
    first=$(printf '%s' "$text" | grep -oE '\[I; *-?[0-9]+' | grep -oE -- '-?[0-9]+$' || true)
    if [ -z "$first" ]; then
        # A wire with nothing stored replies "Found no elements matching power"
        # at this pin (an unpowered wire saves no array). That is power 0, but
        # only for a block that is still our wire: check that first.
        local state
        state=$(ctl block "$1" "$2" "$3" | jq -r '.block // ""')
        case "$state" in
            morered:*wire*|morered:*cable*)
                if [ "$4" = 0 ]; then
                    echo "wire $1,$2,$3: no stored power at $state (0)"
                    return 0
                fi ;;
        esac
        echo "FAIL: expected wire $1,$2,$3 down power $4, got: '${text}' (block '${state}')" >&2
        exit 1
    fi
    if [ "$first" != "$4" ]; then
        echo "FAIL: expected wire $1,$2,$3 down power $4, got: '${text}'" >&2
        exit 1
    fi
    echo "wire $1,$2,$3 power: $text"
}

# A bundled cable's saved channel power: `power.<face>` is a sixteen-int array
# (`BundledCableBlockEntity`'s per-face per-channel table, doubled units).
cable_channel_power() { # x y z face channel
    local text
    text=$(ctl command "execute in minecraft:the_end run data get block $1 $2 $3 power.$4" \
        | jq -r '.chat // [] | join(" ")')
    printf '%s' "$text" \
        | sed -E 's/.*\[I; *([0-9, ]+)\].*/\1/' \
        | awk -v slot="$5" -F',' '{ value=$(slot + 1); gsub(/[^0-9]/,"",value); print value }'
}
expect_cable_power() { # x y z face channel expected
    local got
    got=$(cable_channel_power "$1" "$2" "$3" "$4" "$5")
    if [ "$got" != "$6" ]; then
        echo "FAIL: expected cable $1,$2,$3 $4 channel $5 raw $6, got '$got'" >&2
        ctl command "execute in minecraft:the_end run data get block $1 $2 $3 power.$4" >&2 || true
        exit 1
    fi
    echo "cable $1,$2,$3 $4 channel $5: $got"
}

# A bitwise plate's stored per-channel power (`ChanneledPowerStorageBlockEntity`
# `power`, a 16-byte array; printed as `[B; 31b, 0b, ...]`).
bitwise_channel_power() { # x y z channel
    local text
    text=$(ctl command "execute in minecraft:the_end run data get block $1 $2 $3 power" \
        | jq -r '.chat // [] | join(" ")')
    printf '%s' "$text" \
        | sed -E 's/.*\[B; *([0-9, b]+)\].*/\1/' \
        | awk -v slot="$4" -F',' '{ value=$(slot + 1); gsub(/[^0-9]/,"",value); print value }'
}
expect_bitwise_power() { # x y z channel expected
    local got
    got=$(bitwise_channel_power "$1" "$2" "$3" "$4")
    if [ "$got" != "$5" ]; then
        echo "FAIL: expected bitwise plate $1,$2,$3 channel $4 raw $5, got '$got'" >&2
        ctl command "execute in minecraft:the_end run data get block $1 $2 $3 power" >&2 || true
        exit 1
    fi
    echo "bitwise plate $1,$2,$3 channel $4: $got"
}

# The two-input rig's two output lamps: channel 1 at (0,65,41), channel 5 at
# (1,65,42).
expect_outputs() { # channel-1 lit, channel-5 lit
    expect_lamp_at 0 65 41 "$1"
    expect_lamp_at 1 65 42 "$2"
}

SAMPLES=$SCRATCH/pulse-samples.txt
SAMPLE_IN=$SCRATCH/pulse-samples.in
# Raise B at (0,64,1) and sample the pulse gate in the same script, one
# command per tick, so the 2-tick `powered` window is visible. Fails unless a
# `powered=true` sample is followed by the clear (`input_b=true,powered=false`).
pulse_sample() {
    local seen_high=0 cleared=0 line state
    {
        echo "command execute in minecraft:the_end run setblock 0 64 1 minecraft:redstone_block"
        for _ in $(seq 1 20); do
            # `script` runs lines back to back, so without an explicit tick
            # between samples all 20 reads land inside the 2-tick pulse.
            echo "wait 1"
            echo "block 0 64 0"
        done
    } > "$SAMPLE_IN"
    ctl script "$SAMPLE_IN" > "$SAMPLES" || true
    echo "-- pulse samples --"
    while IFS= read -r line; do
        state=$(echo "$line" | jq -r '.state // empty')
        [ -n "$state" ] || continue
        echo "  $state"
        case "$state" in
            *powered=true*) seen_high=1 ;;
            *input_b=true,powered=false*)
                if [ "$seen_high" = 1 ]; then
                    cleared=1
                    break
                fi
                ;;
        esac
    done < "$SAMPLES"
    if [ "$seen_high" != 1 ]; then
        echo "FAIL: no powered=true sample after the rising edge" >&2
        exit 1
    fi
    if [ "$cleared" != 1 ]; then
        echo "FAIL: the sample never showed the pulse clear (input_b=true,powered=false)" >&2
        exit 1
    fi
    echo "pulse: armed and cleared in the sampled window"
}

cleanup() {
    local status=$?
    ctl quit >/dev/null 2>&1 || true
    sleep 1
    if [ -f "$SCRATCH/logs/latest.log" ]; then
        cp "$SCRATCH/logs/latest.log" "$PORT/target/last-game.log" 2>/dev/null || true
    fi
    rm -rf "$SCRATCH" 2>/dev/null || true
    # Last line of every run: `check-e2e-logs.py` reads it as the verdict.
    echo "EXIT:$status"
}
trap cleanup EXIT

trap cleanup EXIT

launch() { # launch [WORLD]
    local world=${1:-$WORLD}
    rm -f "$SOCK"
    cd "$SCRATCH"
    local args=(--game "$GAME" --world "$world" --seed 20261103 --creative --headless --wait
                --resources "$RES")
    if [ "$EMBEDDED" = 1 ]; then
        echo "  launch: no --dev-mod; the bundle carries its own content" >&2
    else
        args+=(--dev-mod "$PORT/content")
    fi
    "$CTL" --socket "$SOCK" launch "${args[@]}" >/dev/null
    ctl wait-ready 120
}

# The control for EMBEDDED=1: the same binary (every hook is in it) with the
# embedded content made unavailable — `.horizon-cache` is a plain file, so the
# helper can neither compare nor write its content file and falls back to
# `Continue`. The plate block and the spool item must then be unknown.
control_leg() {
    echo "== control: the same game with no embedded content file =="
    rm -rf "$SCRATCH/.horizon-cache"
    : > "$SCRATCH/.horizon-cache"
    launch "$CONTROL_WORLD"
    ctl command "execute in minecraft:the_end run tp @s 0.5 64 0.5" >/dev/null
    ctl wait 40
    ctl command "execute in minecraft:the_end run fill -6 63 -6 6 63 6 minecraft:stone" >/dev/null
    ctl command "execute in minecraft:the_end run fill -6 64 -6 6 70 6 minecraft:air" >/dev/null
    ctl wait 6
    ctl command "execute in minecraft:the_end run setblock 0 64 0 morered:and_gate[facing=down,rotation=0,input_a=false,input_b=false,input_c=false]" >/dev/null 2>&1 || true
    ctl wait 6
    local name
    name=$(ctl block 0 64 0 | jq -r '.name // .block // empty')
    [ "$name" = "minecraft:air" ] || fail "the control game holds $name, so the plate id was known"
    ctl command "item replace entity @s hotbar.0 with morered:redwire_spool 1" >/dev/null 2>&1 || true
    ctl wait 4
    ctl inventory | jq -e '[.hotbar[], .main[] | select(.item == "morered:redwire_spool")] | length == 0' >/dev/null \
        || fail "the control game has a redwire spool, so the content did not come from the embedded file"
    [ -f "$SCRATCH/.horizon-cache" ] || fail "the control's .horizon-cache was replaced: the helper wrote its content"
    echo "  without the embedded file: no spool item, the plate cell stayed $name"
    ctl quit >/dev/null 2>&1 || true
    rm -f "$SCRATCH/.horizon-cache"
}

# EMBEDDED=1: the bundle must have written its content out at startup, byte for
# byte the delivered content mod.
embedded_content_check() {
    local cache=$SCRATCH/.horizon-cache/morered-embedded-content.hmod
    [ -f "$cache" ] || fail "the bundle did not write its embedded content to $cache"
    cmp -s "$cache" "$PORT/content/morered.hmod" \
        || fail "the embedded content differs from content/morered.hmod"
    echo "  embedded content: $(wc -c < "$cache" | tr -d ' ') B at $cache, identical to content/morered.hmod"
}

echo "== launch (The End, $( [ "$EMBEDDED" = 1 ] && echo 'native bundle only (embedded content)' || echo 'morered content' )) =="
if [ "$CONTROL" = 1 ]; then control_leg; fi
launch
if [ "$EMBEDDED" = 1 ]; then embedded_content_check; fi
ctl command "execute in minecraft:the_end run tp @s 0.5 64 0.5"
ctl wait 10
ctl command "execute in minecraft:the_end run fill -6 63 -6 6 63 6 minecraft:stone"
ctl command "execute in minecraft:the_end run fill -6 64 -6 6 70 6 minecraft:air"
# The wire rigs: a floor and clear air in five bands (a three-wire lamp rig,
# the decay line, the counterfactual line, the diode input, the lever and
# repeater rig).
ctl command "execute in minecraft:the_end run fill -2 63 8 40 63 12 minecraft:stone"
ctl command "execute in minecraft:the_end run fill -2 64 8 40 67 12 minecraft:air"
ctl command "execute in minecraft:the_end run fill 0 63 13 33 63 15 minecraft:stone"
ctl command "execute in minecraft:the_end run fill 0 64 13 33 67 15 minecraft:air"
ctl command "execute in minecraft:the_end run fill 0 63 16 21 63 17 minecraft:stone"
ctl command "execute in minecraft:the_end run fill 0 64 16 21 67 17 minecraft:air"
ctl command "execute in minecraft:the_end run fill 0 63 20 6 63 22 minecraft:stone"
ctl command "execute in minecraft:the_end run fill 0 64 20 6 67 22 minecraft:air"
ctl command "execute in minecraft:the_end run fill 9 63 23 12 63 26 minecraft:stone"
ctl command "execute in minecraft:the_end run fill 9 64 23 12 67 26 minecraft:air"
ctl command "execute in minecraft:the_end run fill 13 63 27 21 63 29 minecraft:stone"
ctl command "execute in minecraft:the_end run fill 13 64 27 21 67 29 minecraft:air"
# The bundled cable band: relay bridges, a three-cable line and a red alloy
# wire differential on the same floor.
ctl command "execute in minecraft:the_end run fill -2 63 30 8 63 34 minecraft:stone"
ctl command "execute in minecraft:the_end run fill -2 64 30 8 67 34 minecraft:air"
# The bitwise rigs: a floor for the NOT/diode band (z=36..39) and the
# two-input truth-table band (z=41..46).
ctl command "execute in minecraft:the_end run fill -4 63 36 8 63 46 minecraft:stone"
ctl command "execute in minecraft:the_end run fill -4 64 36 8 68 46 minecraft:air"
# The coloured-cable band: a red line, a blue line beside it and an
# unpowered red line that the powered blue line must not feed. (z=54..58 is
# beyond the wire-post band, which owns z=48..52.)
ctl command "execute in minecraft:the_end run fill -4 63 53 10 63 58 minecraft:stone"
ctl command "execute in minecraft:the_end run fill -4 64 53 10 68 58 minecraft:air"
# The plate-placement band (z=59..67): a floor, a wall support and a ceiling
# support, plus the rod-rotation rig. Nothing is placed here at launch.
ctl command "execute in minecraft:the_end run fill -4 63 59 12 63 67 minecraft:stone"
ctl command "execute in minecraft:the_end run fill -4 64 59 12 72 67 minecraft:air"
ctl command "execute in minecraft:the_end run tp @s 3.5 64 3.5"
ctl wait 5

echo "== AND gate: unpowered is dark =="
setblock "0 64 -1 minecraft:air"
setblock "0 64 -1 minecraft:redstone_lamp"
setblock "0 64 0 morered:and_gate[facing=down,rotation=0,input_a=false,input_b=false,input_c=false]"
ctl wait 6
if [ "$SELFTEST" = 1 ]; then
    expect_lamp true   # selftest: the unpowered gate is asserted lit (must FAIL)
else
    expect_lamp false
fi

echo "== A+C only is still dark (AND, not OR) =="
setblock "1 64 0 minecraft:redstone_block"
setblock "-1 64 0 minecraft:redstone_block"
ctl wait 6
ctl block 0 64 0 | jq -r '.state' | grep -q "input_a=true,input_b=false,input_c=true" || {
    echo "FAIL: the gate did not store A+C, state=$(ctl block 0 64 0 | jq -r '.state')" >&2
    exit 1
}
expect_lamp false

echo "== all three inputs light the lamp =="
setblock "0 64 1 minecraft:redstone_block"
ctl wait 6
expect_lamp true

echo "== removing C darkens it again =="
setblock "-1 64 0 minecraft:air"
ctl wait 6
expect_lamp false

echo "== NOT gate: dark input lights, powered B darkens =="
setblock "1 64 0 minecraft:air"
setblock "0 64 1 minecraft:air"
setblock "0 64 -1 minecraft:air"
setblock "0 64 -1 minecraft:redstone_lamp"
setblock "0 64 0 morered:not_gate[facing=down,rotation=0,input_b=false]"
ctl wait 6
expect_lamp true
setblock "0 64 1 minecraft:redstone_block"
ctl wait 6
expect_lamp false

echo "== OR gate with A only lights (same input, different function) =="
setblock "0 64 1 minecraft:air"
setblock "0 64 -1 minecraft:air"
setblock "0 64 -1 minecraft:redstone_lamp"
setblock "0 64 0 morered:or_gate[facing=down,rotation=0,input_a=false,input_b=false,input_c=false]"
setblock "1 64 0 minecraft:redstone_block"
ctl wait 6
expect_lamp true

echo "== multiplexer: B selects C, otherwise A =="
setblock "1 64 0 minecraft:air"
setblock "0 64 1 minecraft:air"
setblock "-1 64 0 minecraft:air"
setblock "0 64 -1 minecraft:air"
setblock "0 64 -1 minecraft:redstone_lamp"
setblock "0 64 0 morered:multiplexer[facing=down,rotation=0,input_a=false,input_b=false,input_c=false]"
ctl wait 6
expect_lamp false
setblock "1 64 0 minecraft:redstone_block"
ctl wait 6
expect_lamp true
setblock "1 64 0 minecraft:air"
setblock "-1 64 0 minecraft:redstone_block"
ctl wait 6
expect_lamp false
setblock "0 64 1 minecraft:redstone_block"
ctl wait 6
expect_lamp true
setblock "-1 64 0 minecraft:air"
ctl wait 6
expect_lamp false
setblock "1 64 0 minecraft:redstone_block"
ctl wait 6
expect_lamp false
setblock "0 64 1 minecraft:air"
ctl wait 6
expect_lamp true

echo "== latch: set/hold/reset, complementary output =="
setblock "1 64 0 minecraft:air"
setblock "-1 64 0 minecraft:air"
setblock "0 64 1 minecraft:air"
setblock "0 64 -1 minecraft:air"
setblock "0 64 -1 minecraft:redstone_lamp"
setblock "0 64 1 minecraft:redstone_lamp"
setblock "0 64 0 morered:latch[facing=down,rotation=0,input_a=false,input_c=false,powered=false]"
ctl wait 6
expect_lamp false
expect_lamp_at 0 64 1 true
setblock "1 64 0 minecraft:redstone_block"
ctl wait 6
expect_lamp true
expect_lamp_at 0 64 1 false
setblock "1 64 0 minecraft:air"
ctl wait 6
expect_lamp true
setblock "-1 64 0 minecraft:redstone_block"
ctl wait 6
expect_lamp false
expect_lamp_at 0 64 1 true
# C stays on (stored reset). One command raises A, so a single evaluation
# sees A=C=true with was-reset: A sets the stored bit, both outputs go dark
# (upstream `getSignal` silences both faces while A and C are both high).
setblock "1 64 0 minecraft:redstone_block"
ctl wait 6
expect_lamp false
expect_lamp_at 0 64 1 false
expect_latch_state "input_a=true,input_c=true,powered=true"
# Release C first (A still high): the stored set bit holds.
setblock "-1 64 0 minecraft:air"
ctl wait 6
expect_lamp true
setblock "1 64 0 minecraft:air"
ctl wait 6
expect_lamp true
expect_lamp_at 0 64 1 false
expect_latch_state "input_a=false,input_c=false,powered=true"

echo "== pulse gate: a rising edge emits for two ticks, then clears =="
setblock "1 64 0 minecraft:air"
setblock "-1 64 0 minecraft:air"
setblock "0 64 1 minecraft:air"
setblock "0 64 -1 minecraft:air"
setblock "0 64 -1 minecraft:redstone_lamp"
setblock "0 64 0 morered:pulse_gate[facing=down,rotation=0,input_b=false,powered=false]"
ctl wait 6
expect_lamp false
expect_state_has 0 64 0 "input_b=false,powered=false"

echo "== rising edge: powered=true at least once, cleared in the same window =="
pulse_sample
# `ctl script` can return on the clear's tick; let the lamp's neighbour update
# flush before reading it (otherwise the read races the clear's notification).
ctl wait 2
expect_state_has 0 64 0 "input_b=true,powered=false"
expect_lamp false

echo "== not held: B stays powered and the output stays low =="
ctl wait 10
expect_state_has 0 64 0 "input_b=true,powered=false"
expect_lamp false

echo "== falling edge keeps it low; a new rising edge pulses again =="
setblock "0 64 1 minecraft:air"
ctl wait 6
expect_state_has 0 64 0 "input_b=false,powered=false"
expect_lamp false
pulse_sample
expect_state_has 0 64 0 "input_b=true,powered=false"

echo "== differential: a diode in the same wiring holds (lamp lit with B on) =="
setblock "1 64 0 minecraft:air"
setblock "-1 64 0 minecraft:air"
setblock "0 64 0 morered:diode[facing=down,rotation=0,input_b=false]"
ctl wait 6
expect_lamp true
expect_state_has 0 64 0 "input_b=true"
setblock "0 64 0 minecraft:stone"
ctl wait 6
expect_lamp false
expect_state_has 0 64 0 "minecraft:stone"

echo "== a pulse drives a latch (another gate): set and hold, reset by C =="
setblock "0 64 1 minecraft:air"
setblock "3 64 0 morered:pulse_gate[facing=down,rotation=0,input_b=false,powered=false]"
setblock "3 64 -1 morered:latch[facing=down,rotation=1,input_a=false,input_c=false,powered=false]"
setblock "4 64 -1 minecraft:redstone_lamp"
setblock "3 64 -2 minecraft:air"
ctl wait 6
expect_lamp_at 4 64 -1 false
setblock "3 64 1 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 4 64 -1 true
expect_state_has 3 64 -1 "powered=true"
expect_lamp_at 4 64 -1 true
setblock "3 64 1 minecraft:air"
ctl wait 6
expect_lamp_at 4 64 -1 true
setblock "3 64 -2 minecraft:redstone_block"
ctl wait 6
expect_lamp_at 4 64 -1 false
expect_state_has 3 64 -1 "powered=false"
setblock "3 64 -2 minecraft:air"
setblock "3 64 1 minecraft:air"

echo "== all 96 pulse states are accepted (6 facing x 4 rotation x input_b x powered) =="
count=0
for facing in down up north south west east; do
    for rotation in 0 1 2 3; do
        for input_b in false true; do
            for powered in false true; do
                setblock "5 64 3 morered:pulse_gate[facing=$facing,rotation=$rotation,input_b=$input_b,powered=$powered]"
                count=$((count + 1))
            done
        done
    done
done
echo "accepted $count pulse states"
[ "$count" = 96 ] || { echo "FAIL: expected 96 states, set $count" >&2; exit 1; }
# A `command` is queued to the server thread, so let it land before reading.
setblock "5 64 3 morered:pulse_gate[facing=east,rotation=2,input_b=false,powered=false]"
ctl wait 2
expect_state_has 5 64 3 "facing=east,input_b=false,powered=false,rotation=2"
setblock "5 64 3 morered:pulse_gate[facing=up,rotation=3,input_b=false,powered=true]"
ctl wait 2
expect_state_has 5 64 3 "facing=up,input_b=false,powered=true,rotation=3"
# rotation 1 on a down attachment reads B from the west: power it so the
# stored `input_b=true` is also the world's answer and survives the tick.
setblock "4 64 3 minecraft:redstone_block"
setblock "5 64 3 morered:pulse_gate[facing=down,rotation=1,input_b=true,powered=false]"
ctl wait 2
expect_state_has 5 64 3 "facing=down"
expect_state_has 5 64 3 "rotation=1"
expect_state_has 5 64 3 "input_b=true"
setblock "5 64 3 minecraft:air"
setblock "4 64 3 minecraft:air"

echo "== counterfactual: stone in place of the gate stays dark with all inputs =="
setblock "0 64 0 minecraft:stone"
setblock "0 64 1 minecraft:redstone_block"
setblock "-1 64 0 minecraft:redstone_block"
setblock "0 64 -1 minecraft:redstone_lamp"
ctl wait 6
expect_lamp false

echo "== wire: a three-wire line lights a lamp past its end =="
# The lamp is placed before the source so its own neighbour update sees the
# powered wire (lamps compute lit on neighbour changes).
setblock "4 64 8 minecraft:air"
setblock "4 64 8 minecraft:redstone_lamp"
setblock "1 64 8 morered:red_alloy_wire[down=true]"
setblock "2 64 8 morered:red_alloy_wire[down=true]"
setblock "3 64 8 morered:red_alloy_wire[down=true]"
setblock "0 64 8 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 4 64 8 true
expect_state_has 1 64 8 "down=true"
expect_state_has 2 64 8 "down=true"
expect_state_has 3 64 8 "down=true"
expect_wire_power 1 64 8 29
expect_wire_power 2 64 8 28
expect_wire_power 3 64 8 27

echo "== wire: a subwire disconnects when its support is removed (updateShape) =="
setblock "8 64 8 morered:red_alloy_wire[down=true]"
setblock "8 63 8 minecraft:air"
ctl wait 6
expect_state_has 8 64 8 "down=false"
# A restored floor does not add the subwire back: only placement does.
setblock "8 63 8 minecraft:stone"
ctl wait 6
expect_state_has 8 64 8 "down=false"

echo "== wire: power decays one per block (doubled) and reaches zero at wire 29 =="
# A 32-wire line on the floor, fed by a redstone block at its west end.
for x in $(seq 1 32); do
    setblock "$x 64 14 morered:red_alloy_wire[down=true]"
done
setblock "0 64 14 minecraft:redstone_block"
ctl wait 20
# Upstream stores power doubled: a 15 source gives 29, each step subtracts 1,
# and getSignal answers raw / 2. So wire n carries raw 30-n: 14,14,13,13,...
# and the last powered wire is 28 (raw 2); wire 29 (raw 1) already answers 0.
expect_wire_power 1 64 14 29
expect_wire_power 20 64 14 10
expect_wire_power 28 64 14 2
expect_wire_power 29 64 14 1
expect_wire_power 30 64 14 0
expect_wire_power 32 64 14 0

echo "== wire: the far lamp is dark, a lamp within 15 blocks is lit =="
setblock "30 64 15 minecraft:redstone_lamp"
ctl wait 6
expect_lamp_at 30 64 15 false
# A differential on the same rig: removing the source drops the whole line
# (data, because a lit lamp beside a wire would feed it back).
setblock "0 64 14 minecraft:air"
ctl wait 40
expect_wire_power 1 64 14 0
expect_wire_power 20 64 14 0
expect_lamp_at 30 64 15 false
setblock "0 64 14 minecraft:redstone_block"
ctl wait 20
expect_wire_power 1 64 14 29

echo "== wire: a twenty-wire line lights a lamp within 15 blocks =="
# A separate line (z=17) with the lamp placed before the source, so its own
# neighbour update sees the powered wire.
setblock "15 64 16 minecraft:redstone_lamp"
for x in $(seq 1 20); do
    setblock "$x 64 17 morered:red_alloy_wire[down=true]"
done
setblock "0 64 17 minecraft:redstone_block"
ctl wait 20
expect_lamp_at 15 64 16 true
# A redstone lamp is a signal sink, not a source: the wire keeps its own raw
# power (a 15 source gives 29 at wire 1, so wire 15 is 30 - 15 = 15).
expect_wire_power 15 64 17 15

echo "== wire: the middle wire carries the far side (lamp and data differential) =="
setblock "4 64 20 minecraft:air"
setblock "4 64 20 minecraft:redstone_lamp"
setblock "1 64 20 morered:red_alloy_wire[down=true]"
setblock "2 64 20 morered:red_alloy_wire[down=true]"
setblock "3 64 20 morered:red_alloy_wire[down=true]"
setblock "0 64 20 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 4 64 20 true
expect_wire_power 1 64 20 29
expect_wire_power 3 64 20 27
setblock "2 64 20 minecraft:air"
ctl wait 10
expect_lamp_at 4 64 20 false
expect_wire_power 1 64 20 29
expect_wire_power 3 64 20 0
setblock "2 64 20 morered:red_alloy_wire[down=true]"
ctl wait 10
expect_lamp_at 4 64 20 true
expect_wire_power 3 64 20 27
# A stone block in place of the source keeps the line and lamp dead.
setblock "0 64 20 minecraft:stone"
ctl wait 40
expect_lamp_at 4 64 20 false
expect_wire_power 1 64 20 0
expect_wire_power 3 64 20 0

echo "== wire feeds a gate: the diode reads the wire and drives its lamp =="
# Diode at (10,64,24), facing=down, rotation=0: output north (lamp), B south.
setblock "10 64 23 minecraft:air"
setblock "10 64 23 minecraft:redstone_lamp"
setblock "10 64 24 morered:diode[facing=down,rotation=0,input_b=false]"
setblock "10 64 25 morered:red_alloy_wire[down=true]"
setblock "11 64 25 minecraft:redstone_block"
ctl wait 10
expect_state_has 10 64 24 "input_b=true"
expect_lamp_at 10 64 23 true
setblock "11 64 25 minecraft:air"
ctl wait 10
expect_wire_power 10 64 25 0
expect_state_has 10 64 24 "input_b=false"
expect_lamp_at 10 64 23 false

echo "== wire reads a vanilla lever and a repeater =="
setblock "14 64 28 minecraft:lever[face=floor,facing=north,powered=false]"
setblock "15 64 28 morered:red_alloy_wire[down=true]"
ctl wait 6
expect_wire_power 15 64 28 0
setblock "14 64 28 minecraft:lever[face=floor,facing=north,powered=true]"
ctl wait 6
expect_wire_power 15 64 28 29
# The repeater faces west, so its output is the wire to its east.
setblock "17 64 28 minecraft:redstone_block"
setblock "18 64 28 minecraft:repeater[facing=west,delay=1,locked=false,powered=false]"
setblock "19 64 28 morered:red_alloy_wire[down=true]"
ctl wait 10
expect_state_has 18 64 28 "powered=true"
expect_wire_power 19 64 28 29

echo "== wire placement: the item attaches to the face it was clicked against =="
setblock "31 64 10 minecraft:air"
ctl command "execute in minecraft:the_end run tp @s 31.5 64 12.5"
ctl wait 4
ctl give morered:red_alloy_wire 4
ctl wait 2
ctl place 31 64 10 --item morered:red_alloy_wire
ctl wait 6
expect_state_has 31 64 10 "down=true"
expect_state_has 31 64 10 "axis=y"

echo "== all 64 wire states are accepted (6 interior faces) =="
count=0
for down in false true; do
    for up in false true; do
        for north in false true; do
            for south in false true; do
                for west in false true; do
                    for east in false true; do
                        setblock "5 64 4 morered:red_alloy_wire[down=$down,up=$up,north=$north,south=$south,west=$west,east=$east]"
                        count=$((count + 1))
                    done
                done
            done
        done
    done
done
echo "accepted $count wire states"
[ "$count" = 64 ] || { echo "FAIL: expected 64 states, set $count" >&2; exit 1; }
setblock "5 64 4 morered:red_alloy_wire[down=true,east=true]"
ctl wait 2
expect_state_has 5 64 4 "down=true"
expect_state_has 5 64 4 "east=true"
expect_state_has 5 64 4 "north=false"
# Every axis value (the content shape selector) is accepted too.
for axis in x y z; do
    setblock "6 64 4 morered:red_alloy_wire[axis=$axis,down=true]"
done
ctl wait 2
expect_state_has 6 64 4 "axis=z"

echo "== bundled cable: a relay bridge lights channel 3, channel 5 stays dark =="
# The bundle has no vanilla input or output (upstream `BundledCableBlock` is
# not a signal source); the relay plate is the single-channel bridge. Relay A
# (channel 3) reads the redstone block above it and seeds cable 1's west-side
# `down` subwire; the three-cable line carries channel 3 only; relay B lights
# its lamp and the channel-5 relay stays dark.
setblock "0 64 31 morered:bundled_cable_relay_plate[channel=3]"
setblock "1 64 31 morered:bundled_network_cable[down=true]"
setblock "2 64 31 morered:bundled_network_cable[down=true]"
setblock "3 64 31 morered:bundled_network_cable[down=true]"
setblock "4 64 31 morered:bundled_cable_relay_plate[channel=3]"
setblock "4 65 31 minecraft:redstone_lamp"
setblock "2 64 32 morered:bundled_cable_relay_plate[channel=5]"
setblock "2 65 32 minecraft:redstone_lamp"
setblock "3 65 31 minecraft:redstone_lamp"
setblock "0 65 31 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 4 65 31 true
expect_lamp_at 2 65 32 false
# Differential: a vanilla lamp beside a bundled cable carries nothing; the
# relay is what turns the channel into redstone.
expect_lamp_at 3 65 31 false
expect_cable_power 1 64 31 down 3 29
expect_cable_power 2 64 31 down 3 28
expect_cable_power 3 64 31 down 3 27
expect_cable_power 2 64 31 down 5 0
expect_state_has 1 64 31 "down=true"
expect_state_has 0 64 31 "channel=3"

echo "== bundled cable: the same rig with a red alloy wire lights its lamp directly =="
# The wire-to-lamp differential: a red alloy wire on the same floor and a lamp
# beside it light, while the cable's lamp above stays dark.
setblock "1 64 33 morered:red_alloy_wire[down=true]"
setblock "2 64 33 morered:red_alloy_wire[down=true]"
setblock "3 64 33 morered:red_alloy_wire[down=true]"
setblock "4 64 33 minecraft:redstone_lamp"
setblock "0 64 33 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 4 64 33 true

echo "== bundled cable: removing the source drains channel 3 =="
setblock "0 65 31 minecraft:air"
ctl wait 12
expect_lamp_at 4 65 31 false
expect_cable_power 1 64 31 down 3 0
expect_cable_power 3 64 31 down 3 0
setblock "0 65 31 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 4 65 31 true

echo "== bundled cable: removing a middle cable cuts the far side =="
setblock "2 64 31 minecraft:air"
ctl wait 12
expect_lamp_at 4 65 31 false
expect_cable_power 1 64 31 down 3 29
expect_cable_power 3 64 31 down 3 0
setblock "2 64 31 morered:bundled_network_cable[down=true]"
ctl wait 10
expect_lamp_at 4 65 31 true

echo "== bundled cable: stone in place of the relay bridge stays dark =="
setblock "0 64 31 minecraft:stone"
ctl wait 10
expect_lamp_at 4 65 31 false
setblock "0 64 31 morered:bundled_cable_relay_plate[channel=3]"
ctl wait 10
expect_lamp_at 4 65 31 true

echo "== all 64 bundled cable states are accepted (6 subwires) =="
count=0
for down in false true; do
    for up in false true; do
        for north in false true; do
            for south in false true; do
                for west in false true; do
                    for east in false true; do
                        setblock "5 64 4 morered:bundled_network_cable[down=$down,up=$up,north=$north,south=$south,west=$west,east=$east]"
                        count=$((count + 1))
                    done
                done
            done
        done
    done
done
echo "accepted $count cable states"
[ "$count" = 64 ] || { echo "FAIL: expected 64 cable states, set $count" >&2; exit 1; }
setblock "5 64 4 morered:bundled_network_cable[down=true,east=true]"
ctl wait 2
expect_state_has 5 64 4 "down=true"
expect_state_has 5 64 4 "east=true"
expect_state_has 5 64 4 "north=false"

echo "== all 16 bundled cable relay channel states are accepted =="
count=0
for channel in $(seq 0 15); do
    setblock "6 64 4 morered:bundled_cable_relay_plate[channel=$channel]"
    count=$((count + 1))
done
echo "accepted $count relay states"
[ "$count" = 16 ] || { echo "FAIL: expected 16 relay states, set $count" >&2; exit 1; }
setblock "6 64 4 morered:bundled_cable_relay_plate[channel=9]"
ctl wait 2
expect_state_has 6 64 4 "channel=9"

echo "== bitwise NOT: cable A channel 1 high writes channel 1 low on cable B =="
# NOT at (0,64,37) facing=down, rotation=0: output north (0,64,36), input B
# south (0,64,38). Input relay ch1 at (1,64,38) reads the block above it;
# output relay ch1 at (-1,64,36) drives the lamp above.
setblock "0 64 37 morered:bitwise_not_gate[facing=down,rotation=0]"
setblock "0 64 38 morered:bundled_network_cable[down=true]"
setblock "1 64 38 morered:bundled_cable_relay_plate[channel=1]"
setblock "1 65 38 minecraft:redstone_block"
setblock "0 64 36 morered:bundled_network_cable[down=true]"
setblock "-1 64 36 morered:bundled_cable_relay_plate[channel=1]"
setblock "-1 65 36 minecraft:redstone_lamp"
ctl wait 14
expect_state_has 0 64 37 "facing=down"
expect_state_has 0 64 37 "rotation=0"
expect_cable_power 0 64 38 down 1 29
expect_cable_power 0 64 36 down 1 0
expect_lamp_at -1 65 36 false
expect_bitwise_power 0 64 37 1 0

echo "== bitwise NOT: source removed -> output 31 (seed 30) and the lamp lights =="
setblock "1 65 38 minecraft:air"
ctl wait 14
expect_cable_power 0 64 38 down 1 0
expect_cable_power 0 64 36 down 1 30
expect_lamp_at -1 65 36 true
expect_bitwise_power 0 64 37 1 31

echo "== bitwise diode differential: same cable, same source, the diode passes it =="
# A diode has the same wiring and the same 1-tick update; only its function
# differs from NOT's. The wiring therefore demonstrably can light the lamp.
setblock "0 64 37 morered:bitwise_diode[facing=down,rotation=0]"
ctl wait 14
expect_lamp_at -1 65 36 false
setblock "1 65 38 minecraft:redstone_block"
ctl wait 14
expect_lamp_at -1 65 36 true
expect_cable_power 0 64 36 down 1 30
expect_bitwise_power 0 64 37 1 31

echo "== bitwise plate has no vanilla signal: a lamp on its output face is dark =="
# A second diode at (3,64,37), output north (3,64,36): a vanilla lamp sits on
# the output face instead of a cable. The same input (cable + relay + source)
# is wired south; forcing the lamp to recompute (a neighbour placed and
# removed) must still leave it dark, because `BitwiseLogicPlateBlock` is not a
# redstone signal source. The cable rig above lights on the same plate.
setblock "3 64 38 morered:bundled_network_cable[down=true]"
setblock "4 64 38 morered:bundled_cable_relay_plate[channel=1]"
setblock "4 65 38 minecraft:redstone_block"
setblock "3 64 37 morered:bitwise_diode[facing=down,rotation=0]"
setblock "3 64 36 minecraft:redstone_lamp"
ctl wait 14
setblock "4 64 36 minecraft:stone"
ctl wait 6
setblock "4 64 36 minecraft:air"
ctl wait 6
expect_lamp_at 3 64 36 false
expect_cable_power 3 64 38 down 1 29
expect_bitwise_power 3 64 37 1 31

echo "== bitwise NOT restored: the source lights nothing, the inverted output does =="
setblock "0 64 37 morered:bitwise_not_gate[facing=down,rotation=0]"
ctl wait 14
expect_lamp_at -1 65 36 false
expect_bitwise_power 0 64 37 1 0
setblock "1 65 38 minecraft:air"
ctl wait 14
expect_lamp_at -1 65 36 true
expect_bitwise_power 0 64 37 1 31
setblock "0 64 37 minecraft:stone"
ctl wait 14
expect_lamp_at -1 65 36 false
expect_cable_power 0 64 36 down 1 0

echo "== bitwise AND: per-channel truth table with channel 5 as the control =="
# AND at (0,64,44) facing=down, rotation=0: output north (0,64,43), inputs A
# east (1,64,44) and C west (-1,64,44). Each input cable has a channel-1 relay
# and a channel-5 relay; the output cable (0,64,43)-(0,64,42) has one relay per
# channel, each with a lamp.
setblock "0 64 44 morered:bitwise_and_gate[facing=down,rotation=0]"
setblock "1 64 44 morered:bundled_network_cable[down=true]"
setblock "-1 64 44 morered:bundled_network_cable[down=true]"
setblock "2 64 44 morered:bundled_cable_relay_plate[channel=1]"
setblock "1 64 45 morered:bundled_cable_relay_plate[channel=5]"
setblock "-2 64 44 morered:bundled_cable_relay_plate[channel=1]"
setblock "-1 64 45 morered:bundled_cable_relay_plate[channel=5]"
setblock "0 64 43 morered:bundled_network_cable[down=true]"
setblock "0 64 42 morered:bundled_network_cable[down=true]"
setblock "0 64 41 morered:bundled_cable_relay_plate[channel=1]"
setblock "0 65 41 minecraft:redstone_lamp"
setblock "1 64 42 morered:bundled_cable_relay_plate[channel=5]"
setblock "1 65 42 minecraft:redstone_lamp"
ctl wait 14
expect_outputs false false
# A channel 1 only -> still dark (AND, not OR).
setblock "2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs false false
# A and C channel 1 -> channel 1 lit, channel 5 dark.
setblock "-2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs true false
expect_cable_power 0 64 43 down 1 30
expect_bitwise_power 0 64 44 1 31
expect_bitwise_power 0 64 44 5 0
# A channel 5 only -> channel 5 still dark: channel 1 cannot leak into 5.
setblock "1 65 45 minecraft:redstone_block"
ctl wait 12
expect_outputs true false
# Both channel 5 inputs -> channel 5 lit, channel 1 still lit.
setblock "-1 65 45 minecraft:redstone_block"
ctl wait 12
expect_outputs true true
# C channel 5 off -> only channel 5 drops.
setblock "-1 65 45 minecraft:air"
ctl wait 12
expect_outputs true false
# C channel 1 off (source removal) -> channel 1 dark.
setblock "-2 65 44 minecraft:air"
ctl wait 12
expect_outputs false false
setblock "2 65 44 minecraft:air"
setblock "1 65 45 minecraft:air"
ctl wait 12
expect_outputs false false

echo "== bitwise OR: same rig and inputs, different function =="
setblock "0 64 44 morered:bitwise_or_gate[facing=down,rotation=0]"
ctl wait 14
expect_outputs false false
setblock "2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs true false
setblock "-2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs true false
# A channel 5 only, with both channel 1 inputs high: channel 5 stays dark.
setblock "1 65 45 minecraft:redstone_block"
ctl wait 12
expect_outputs true true
setblock "1 65 45 minecraft:air"
ctl wait 12
expect_outputs true false
setblock "-1 65 45 minecraft:redstone_block"
ctl wait 12
expect_outputs true true
setblock "2 65 44 minecraft:air"
setblock "-2 65 44 minecraft:air"
setblock "-1 65 45 minecraft:air"
ctl wait 12
expect_outputs false false

echo "== bitwise XOR: same rig =="
setblock "0 64 44 morered:bitwise_xor_gate[facing=down,rotation=0]"
ctl wait 14
expect_outputs false false
setblock "2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs true false
setblock "-2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs false false
setblock "2 65 44 minecraft:air"
ctl wait 12
expect_outputs true false
setblock "1 65 45 minecraft:redstone_block"
ctl wait 12
expect_outputs true true
setblock "-2 65 44 minecraft:air"
ctl wait 12
expect_outputs false true
setblock "1 65 45 minecraft:air"
ctl wait 12
expect_outputs false false
setblock "2 65 44 minecraft:redstone_block"
setblock "-2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs false false
setblock "2 65 44 minecraft:air"
setblock "-2 65 44 minecraft:air"
ctl wait 12
expect_outputs false false

echo "== bitwise XNOR: same rig, the inversion of XOR =="
setblock "0 64 44 morered:bitwise_xnor_gate[facing=down,rotation=0]"
ctl wait 14
expect_outputs true true
setblock "2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs false true
setblock "-2 65 44 minecraft:redstone_block"
ctl wait 12
expect_outputs true true
setblock "1 65 45 minecraft:redstone_block"
ctl wait 12
expect_outputs true false
setblock "-1 65 45 minecraft:redstone_block"
ctl wait 12
expect_outputs true true
setblock "2 65 44 minecraft:air"
setblock "-2 65 44 minecraft:air"
setblock "1 65 45 minecraft:air"
setblock "-1 65 45 minecraft:air"
ctl wait 12
expect_outputs true true

echo "== bitwise counterfactual: stone in place of the plate stays dark =="
setblock "2 65 44 minecraft:redstone_block"
setblock "-2 65 44 minecraft:redstone_block"
setblock "1 65 45 minecraft:redstone_block"
setblock "-1 65 45 minecraft:redstone_block"
setblock "0 64 44 minecraft:stone"
ctl wait 14
expect_outputs false false

echo "== all 24 states of each bitwise plate are accepted (facing x rotation) =="
count=0
for gate in bitwise_diode bitwise_not_gate bitwise_and_gate bitwise_or_gate bitwise_xor_gate bitwise_xnor_gate; do
    for facing in down up north south west east; do
        for rotation in 0 1 2 3; do
            setblock "7 64 3 morered:$gate[facing=$facing,rotation=$rotation]"
            count=$((count + 1))
        done
    done
done
echo "accepted $count bitwise states"
[ "$count" = 144 ] || { echo "FAIL: expected 144 bitwise states, set $count" >&2; exit 1; }
setblock "7 64 3 morered:bitwise_xor_gate[facing=east,rotation=2]"
ctl wait 2
expect_state_has 7 64 3 "facing=east"
expect_state_has 7 64 3 "rotation=2"
# Counterfactual: the bitwise plates store no input booleans, so `input_b` is
# an undeclared property and must be rejected.
bitwise_reject=$(ctl command "execute in minecraft:the_end run setblock 7 64 3 morered:bitwise_and_gate[facing=down,input_b=true,rotation=0]" 2>&1 || true)
case "$bitwise_reject" in
    *"does not have property 'input_b'"* | *"unknown block property"*)
        echo "bitwise AND rejects input_b (undeclared property)" ;;
    *) echo "FAIL: expected input_b to be rejected, got: $bitwise_reject" >&2; exit 1 ;;
esac

echo "== bitwise items resolve (item definition and carried lang) =="
whence=$(ctl give morered:bitwise_and_gate 2 2>&1 || true)
case "$whence" in
    *'"given":2'* | *"Gave 2"* | *"Bitwise AND Gate"*)
        echo "gave morered:bitwise_and_gate 2" ;;
    *) echo "FAIL: give morered:bitwise_and_gate answered: $whence" >&2; exit 1 ;;
esac
# The creative tab lists all six plates (copied out before the scratch home is
# deleted, then checked below).
# The overlay is built in `mod-content/<stamp>/` at this pin (one folder
# per mod set), so the file is found rather than assumed.
CREATIVE=$(find "$SCRATCH/.horizon-cache/mod-content" -name creative-inventory.json 2>/dev/null | head -1)
if [ -n "$CREATIVE" ] && [ -f "$CREATIVE" ]; then
    for item in bitwise_diode bitwise_not_gate bitwise_and_gate bitwise_or_gate bitwise_xor_gate bitwise_xnor_gate \
        redwire_post redwire_post_plate redwire_post_relay_plate redwire_spool; do
        grep -q "morered:$item" "$CREATIVE" || {
            echo "FAIL: the creative tab does not list morered:$item" >&2
            exit 1
        }
    done
    echo "creative tab lists the six bitwise plates and the four post/spool items"
else
    echo "FAIL: no creative-inventory.json in the content cache" >&2
    exit 1
fi

echo "== wire posts: two posts 10 apart, a source at the near post's support =="
# A new floor band (z=48..52). Post A stands on a redstone block (its
# attachment side), post B on stone 10 blocks away, and the unlinked post C is
# the control. A plain post emits on the face away from its support, so the
# lamps sit above the posts.
ctl command "execute in minecraft:the_end run fill -2 63 48 14 63 52 minecraft:stone"
ctl command "execute in minecraft:the_end run fill -2 64 48 14 68 52 minecraft:air"
setblock "0 64 50 minecraft:redstone_block"
setblock "10 64 50 minecraft:stone"
setblock "5 64 51 minecraft:stone"
# The lamps first: a vanilla lamp only re-reads on a neighbour update, and a
# post placed after its lamp notifies it (as in the gate rigs).
setblock "0 66 50 minecraft:redstone_lamp"
setblock "10 66 50 minecraft:redstone_lamp"
setblock "5 66 51 minecraft:redstone_lamp"
setblock "0 65 50 morered:redwire_post[facing=down,power=0]"
setblock "10 65 50 morered:redwire_post[facing=down,power=0]"
setblock "5 65 51 morered:redwire_post[facing=down,power=0]"
ctl wait 12
# A source of 15 on the attachment side makes the post 14 (`getNewPower`).
expect_state_has 0 65 50 "power=14"
expect_state_has 10 65 50 "power=0"
expect_state_has 5 65 51 "power=0"
expect_lamp_at 0 66 50 true
expect_lamp_at 10 66 50 false
expect_lamp_at 5 66 51 false

echo "== wire posts: a spool click links the pair; the far post carries 13 =="
ctl tp 0.5 65 51.5
ctl wait 4
ctl command "execute in minecraft:the_end run clear"
ctl give morered:redwire_spool 1 >/dev/null
ctl wait 4
ctl select morered:redwire_spool >/dev/null
ctl interact 0 65 50 south >/dev/null
ctl wait 4
ctl tp 10.5 65 51.5
ctl wait 4
ctl interact 10 65 50 south >/dev/null
ctl wait 12
expect_state_has 0 65 50 "power=14"
expect_state_has 10 65 50 "power=13"
expect_lamp_at 10 66 50 true
expect_lamp_at 5 66 51 false
# The link is stored on both posts (`WirePostBlockEntity.CONNECTIONS`, relative
# offsets; A is 10 west of B).
link_a=$(ctl command "execute in minecraft:the_end run data get block 0 65 50 connections" | jq -r '.chat // [] | join(" ")')
link_b=$(ctl command "execute in minecraft:the_end run data get block 10 65 50 connections" | jq -r '.chat // [] | join(" ")')
case "$link_a" in *"10, 0, 0"*) echo "post A connections: $link_a" ;; *)
    echo "FAIL: post A has no link to B: $link_a" >&2; exit 1 ;; esac
case "$link_b" in *"-10, 0, 0"*) echo "post B connections: $link_b" ;; *)
    echo "FAIL: post B has no link to A: $link_b" >&2; exit 1 ;; esac

echo "== wire posts: unlinked control stays dark while the linked pair is lit =="
expect_lamp_at 5 66 51 false
expect_state_has 5 65 51 "power=0"

echo "== wire posts: spool-clicking the linked pair again cuts it =="
ctl tp 0.5 65 51.5
ctl wait 4
ctl interact 0 65 50 south >/dev/null
ctl wait 4
ctl tp 10.5 65 51.5
ctl wait 4
ctl interact 10 65 50 south >/dev/null
ctl wait 12
expect_state_has 0 65 50 "power=14"
expect_state_has 10 65 50 "power=0"
expect_lamp_at 10 66 50 false
# A second click on the same post clears the selection instead of linking.
ctl tp 0.5 65 51.5
ctl wait 4
ctl interact 0 65 50 south >/dev/null
ctl interact 0 65 50 south >/dev/null
ctl wait 6
expect_state_has 10 65 50 "power=0"

echo "== wire posts: relinked, then the source removed drains both =="
ctl tp 0.5 65 51.5
ctl wait 4
ctl interact 0 65 50 south >/dev/null
ctl wait 4
ctl tp 10.5 65 51.5
ctl wait 4
ctl interact 10 65 50 south >/dev/null
ctl wait 10
expect_state_has 10 65 50 "power=13"
setblock "0 64 50 minecraft:stone"
ctl wait 12
expect_state_has 0 65 50 "power=0"
expect_state_has 10 65 50 "power=0"
expect_lamp_at 0 66 50 false
expect_lamp_at 10 66 50 false
setblock "0 64 50 minecraft:redstone_block"
ctl wait 12
expect_state_has 0 65 50 "power=14"
expect_state_has 10 65 50 "power=13"
expect_lamp_at 10 66 50 true

echo "== wire posts: the empty plate carries the link but emits no vanilla signal =="
setblock "2 64 49 minecraft:redstone_block"
setblock "2 66 49 minecraft:redstone_lamp"
setblock "2 65 49 morered:redwire_post_plate[facing=down,power=0]"
ctl wait 10
expect_state_has 2 65 49 "power=0"
expect_lamp_at 2 66 49 false
# Link it to A (already 14): it stores 13 but still emits nowhere.
ctl tp 0.5 65 51.5
ctl wait 4
ctl interact 0 65 50 south >/dev/null
ctl wait 4
ctl tp 2.5 65 50.5
ctl wait 2
ctl interact 2 65 49 south >/dev/null
ctl wait 12
expect_state_has 2 65 49 "power=13"
expect_lamp_at 2 66 49 false

echo "== wire posts: the relay plate reads and emits on its four sides =="
setblock "4 64 47 minecraft:stone"
setblock "3 65 47 minecraft:redstone_block"
setblock "4 65 46 minecraft:redstone_lamp"
setblock "4 66 47 minecraft:redstone_lamp"
setblock "4 65 47 morered:redwire_post_relay_plate[facing=down,power=0]"
ctl wait 12
expect_state_has 4 65 47 "power=14"
expect_lamp_at 4 65 46 true
# The attachment side is excluded from the relay's connections, so the lamp
# above it stays dark even though the post carries 14.
expect_lamp_at 4 66 47 false
setblock "3 65 47 minecraft:stone"
ctl wait 12
expect_state_has 4 65 47 "power=0"
expect_lamp_at 4 65 46 false

echo "== wire posts: all 96 states of the plain post are accepted =="
post_count=0
for facing in down up north south west east; do
    for power in $(seq 0 15); do
        setblock "7 65 50 morered:redwire_post[facing=$facing,power=$power]"
        post_count=$((post_count + 1))
    done
done
echo "accepted $post_count post states"
[ "$post_count" = 96 ] || { echo "FAIL: expected 96 post states, set $post_count" >&2; exit 1; }
setblock "7 65 50 morered:redwire_post[facing=east,power=13]"
ctl wait 3
expect_state_has 7 65 50 "facing=east"
# The state was accepted at 13; with no source and no link the first solve
# corrects it to 0 (`getNewPower`), so the read-back is the solved value.
expect_state_has 7 65 50 "power=0"

echo "== wire posts: native placement attaches to the clicked face =="
# Only the north stone is a solid neighbour (the floor under the cell is
# removed), so `place` clicks its south face and the post attaches north.
setblock "8 63 50 minecraft:air"
setblock "8 64 49 minecraft:stone"
ctl tp 8.5 65 51.5
ctl wait 4
ctl give morered:redwire_post 1 >/dev/null
ctl wait 4
ctl select morered:redwire_post >/dev/null
place_reply=$(ctl place 8 64 50 --item morered:redwire_post)
echo "$place_reply" | jq -c '{ok,block,changed}'
ctl wait 3
expect_state_has 8 64 50 "facing=north"
expect_state_has 8 64 50 "power=0"

echo "== wire posts: items resolve and the creative tab lists them =="
for item in redwire_post redwire_post_plate redwire_post_relay_plate; do
    whence=$(ctl give "morered:$item" 2 2>&1 || true)
    case "$whence" in
        *'"given":2'* | *"Gave 2"*) echo "gave morered:$item 2" ;;
        *) echo "FAIL: give morered:$item answered: $whence" >&2; exit 1 ;;
    esac
done
whence=$(ctl give morered:redwire_spool 1 2>&1 || true)
case "$whence" in
    *'"given":1'* | *"Gave 1"* | *"Redwire Spool"*)
        echo "gave morered:redwire_spool 1" ;;
    *) echo "FAIL: give morered:redwire_spool answered: $whence" >&2; exit 1 ;;
esac

echo "== coloured cable: a red line lights its lamp =="
# 16 dyed cables are 16 blocks (the dye is the block id, not a state); a red
# cable is the plain wire's shape and propagation at a different dye. Same
# decay: raw 29, 28, 27 along the line. Every lamp sits north/south of its own
# colour only, so the cross-colour probes below are about the cables.
setblock "2 64 53 minecraft:air"
setblock "2 64 53 minecraft:redstone_lamp"
setblock "1 64 54 morered:red_network_cable[down=true]"
setblock "2 64 54 morered:red_network_cable[down=true]"
setblock "3 64 54 morered:red_network_cable[down=true]"
setblock "0 64 54 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 2 64 53 true
expect_wire_power 1 64 54 29
expect_wire_power 2 64 54 28
expect_wire_power 3 64 54 27

echo "== coloured cable: a blue line beside the powered red line stays dark =="
# The blue line runs beside the lit red line (z=54) with a lamp but no source.
# Different dyes never share a subwire, so blue must stay at raw 0 and the red
# line must keep its own power. The lamp goes in after the cables settle: this
# engine computes a lamp's `lit` on neighbour changes, and placing it late
# avoids latching the placement-time read.
setblock "1 64 55 morered:blue_network_cable[down=true]"
setblock "2 64 55 morered:blue_network_cable[down=true]"
setblock "3 64 55 morered:blue_network_cable[down=true]"
setblock "4 64 55 morered:blue_network_cable[down=true]"
ctl wait 10
setblock "2 64 56 minecraft:air"
setblock "2 64 56 minecraft:redstone_lamp"
ctl wait 6
expect_lamp_at 2 64 56 false
expect_wire_power 1 64 55 0
expect_wire_power 1 64 54 29
expect_lamp_at 2 64 53 true

echo "== coloured cable: the blue line with its own source lights its lamp =="
setblock "0 64 55 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 2 64 56 true
expect_wire_power 1 64 55 29
expect_wire_power 4 64 55 26
# The red line is unchanged by the blue line next to it.
expect_wire_power 1 64 54 29

echo "== coloured cable: the powered blue line does not feed an adjacent red line =="
# The differential: a red line continues the same row (x=5..7) with no source,
# directly beside the powered blue line. If colours were ignored, red would
# pick up raw 26 through the shared down subwire and its lamp would light.
setblock "5 64 55 morered:red_network_cable[down=true]"
setblock "6 64 55 morered:red_network_cable[down=true]"
setblock "7 64 55 morered:red_network_cable[down=true]"
ctl wait 10
setblock "6 64 56 minecraft:air"
setblock "6 64 56 minecraft:redstone_lamp"
ctl wait 6
expect_lamp_at 6 64 56 false
expect_wire_power 5 64 55 0

echo "== coloured cable: the red line with its own source lights its lamp =="
setblock "8 64 55 minecraft:redstone_block"
ctl wait 10
expect_lamp_at 6 64 56 true
expect_wire_power 7 64 55 29
expect_wire_power 5 64 55 27

echo "== coloured cable: cutting the blue line darkens its lamp =="
setblock "2 64 55 minecraft:air"
ctl wait 40
expect_lamp_at 2 64 56 false
expect_wire_power 1 64 55 29
expect_wire_power 4 64 55 0
setblock "2 64 55 morered:blue_network_cable[down=true]"
ctl wait 20
expect_lamp_at 2 64 56 true
expect_wire_power 4 64 55 26
# Removing the red source drains the red line, not the blue one.
setblock "8 64 55 minecraft:air"
ctl wait 40
expect_lamp_at 6 64 56 false
expect_wire_power 5 64 55 0
expect_wire_power 1 64 55 29

echo "== plate placement: the click face sets facing, the click quadrant sets rotation =="
# `PlateBlockStateProperties.getStateForPlacedGatePlate` (tag v1.21.1-6.0.0.3,
# commit 3bf5a8d): `facing` is the clicked face's opposite (toward the support)
# and `rotation` is the index of the clicked quadrant's face in that
# attachment's `BlockStateUtil.OUTPUT_TABLE` row
# (`getOutputDirectionFromRelativeHitVec` + `getRotationIndexForDirection`).
# The click *point*, not the player's yaw, is the driver: the same quadrant
# clicked from the opposite side of the cell (yaw differs by ~180 degrees) must
# give the same rotation. `look-at` + `mouse right click` aims the real
# crosshair at a known point of the support's face, so the engine hands the
# placement hook that point. Before this slice the plates took the `end_rod`
# template's placement (`facing` = the clicked face, `rotation=0`).
ctl give morered:and_gate 16 >/dev/null
ctl give morered:bitwise_and_gate 1 >/dev/null
ctl give morered:not_gate 1 >/dev/null
ctl wait 4
# Floor, east quadrant: facing=down (support below), rotation=1 (output east).
ctl tp 0.5 64 62.5
ctl wait 2
ctl select morered:and_gate >/dev/null
yaw_south=$(ctl look-at 0.85 64.0 60.5 | jq -r '.yaw')
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 0 64 60 "facing=down"
expect_state_has 0 64 60 "rotation=1"
# The same floor, the west quadrant: rotation=3 (same yaw, other quadrant).
ctl tp 1.5 64 62.5
ctl wait 2
ctl select morered:and_gate >/dev/null
ctl look-at 1.15 64.0 60.5 >/dev/null
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 1 64 60 "facing=down"
expect_state_has 1 64 60 "rotation=3"
# Yaw control: the same west quadrant clicked from the other side of the cell.
ctl tp 2.5 64 58.0
ctl wait 2
ctl select morered:and_gate >/dev/null
yaw_north=$(ctl look-at 2.15 64.0 60.5 | jq -r '.yaw')
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 2 64 60 "facing=down"
expect_state_has 2 64 60 "rotation=3"
yaw_turn=$(awk -v a="$yaw_south" -v b="$yaw_north" 'BEGIN { d = a - b; if (d < 0) d = -d; d = d % 360; if (d > 180) d = 360 - d; printf "%.1f", d }')
if ! awk -v d="$yaw_turn" 'BEGIN { exit !(d > 90) }'; then
    echo "FAIL: the yaw control did not turn (south $yaw_south, north $yaw_north)" >&2
    exit 1
fi
echo "yaw control: the same quadrant at yaw $yaw_south and yaw $yaw_north ($yaw_turn deg apart) gives rotation=3"
# Wall, upper quadrant of the support's north face: facing=south, rotation=0
# (output up); lower quadrant: facing=north, rotation=2 (output down).
setblock "4 64 60 minecraft:stone"
setblock "5 64 60 minecraft:stone"
setblock "8 68 60 minecraft:stone"
setblock "10 64 60 minecraft:stone"
ctl wait 2
ctl tp 4.5 64 58.0
ctl wait 2
ctl select morered:and_gate >/dev/null
ctl look-at 4.5 64.9 60.0 >/dev/null
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 4 64 59 "facing=south"
expect_state_has 4 64 59 "rotation=0"
ctl tp 5.5 64 62.5
ctl wait 2
ctl select morered:and_gate >/dev/null
ctl look-at 5.5 64.1 61.0 >/dev/null
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 5 64 61 "facing=north"
expect_state_has 5 64 61 "rotation=2"
# Ceiling: the support's bottom face, west quadrant -> facing=up, rotation=1.
ctl tp 8.5 64 62.5
ctl wait 2
ctl select morered:and_gate >/dev/null
ctl look-at 8.15 68.0 60.5 >/dev/null
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 8 67 60 "facing=up"
expect_state_has 8 67 60 "rotation=1"
# A bitwise plate inherits `PlateBlock.getStateForPlacement` too.
ctl tp 10.5 64 62.5
ctl wait 2
ctl select morered:bitwise_and_gate >/dev/null
ctl look-at 10.5 64.1 61.0 >/dev/null
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 10 64 61 "facing=north"
expect_state_has 10 64 61 "rotation=2"

echo "== plate rod rotation: a wooden rod turns the plate a quarter turn =="
# `RedstonePlateBlock.useItemOn`: any `#c:rods/wooden` item advances
# `rotation` by one and re-reads the inputs; a non-rod click does nothing. The
# NOT plate attaches to the wall south of it, rotation 0 -> output up, so the
# west lamp starts dark. The lamp is placed before the plate (a lamp reads
# `lit` on neighbour changes, not placement), and the rod write notifies it.
setblock "7 64 60 minecraft:stone"
setblock "6 64 59 minecraft:redstone_lamp"
ctl wait 2
ctl tp 7.5 64 58.0
ctl wait 2
ctl select morered:not_gate >/dev/null
ctl look-at 7.5 64.9 60.0 >/dev/null
ctl mouse right click >/dev/null
ctl wait 6
expect_state_has 7 64 59 "facing=south"
expect_state_has 7 64 59 "rotation=0"
expect_lamp_at 6 64 59 false
# Control: coal is not a wooden rod, so the click changes nothing. The item
# goes into hotbar slot 1 (`give` lands in the main inventory once the hotbar
# is full, and `select` would then fail silently behind `>/dev/null`).
ctl command "item replace entity @s hotbar.0 with minecraft:coal 1" >/dev/null
ctl wait 2
ctl select 1
ctl look-at 7.5 64.5 59.875
ctl mouse right click
ctl wait 4
expect_state_has 7 64 59 "rotation=0"
expect_lamp_at 6 64 59 false
# A wooden rod: rotation 1, output west, so the west lamp lights.
ctl command "item replace entity @s hotbar.0 with minecraft:stick 1" >/dev/null
ctl wait 2
ctl select 1
ctl look-at 7.5 64.5 59.875
ctl mouse right click
ctl wait 4
expect_state_has 7 64 59 "rotation=1"
expect_lamp_at 6 64 59 true
# Rotation 2 points down into the floor: the lamp goes dark.
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 7 64 59 "rotation=2"
expect_lamp_at 6 64 59 false
# Rotation 3 points east (no lamp there), then 0 wraps back to up.
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 7 64 59 "rotation=3"
ctl mouse right click >/dev/null
ctl wait 4
expect_state_has 7 64 59 "rotation=0"
expect_lamp_at 6 64 59 false

echo "== drops: a broken wire gives one item per attached subwire =="
ctl command "gamemode survival @s" >/dev/null
ctl command "clear @s" >/dev/null
ctl command "execute in minecraft:the_end run fill 28 63 28 34 63 32 minecraft:stone" >/dev/null
ctl command "execute in minecraft:the_end run fill 28 64 28 34 67 32 minecraft:air" >/dev/null
ctl command "execute in minecraft:the_end run setblock 30 64 29 minecraft:stone" >/dev/null
ctl command "execute in minecraft:the_end run setblock 31 64 30 minecraft:stone" >/dev/null
ctl command "execute in minecraft:the_end run setblock 30 64 30 morered:red_alloy_wire[axis=y,down=true,up=false,north=true,south=false,west=false,east=true]" >/dev/null
# Teleport after the floor exists: a player put at y=64 before the fill falls
# through the missing floor and ends up below it (measured: y=62).
ctl command "execute in minecraft:the_end run tp @s 30.5 64 31.5" >/dev/null
ctl wait 10
echo "  drops rig player at $(ctl state | jq -c '{dimension,position:.player.position}')"
ctl wait 6
# 26.3 has no `Item.count` in entity NBT (the count is a component), so a
# `nbt={Item:{id:…,count:N}}` probe never matches. Count the dropped item
# entities instead, as the other ports' item scenarios do.
drops_of() { # drops_of ITEM: picked up plus still on the ground
    local held ground
    held=$(ctl inventory | jq -r --arg i "$1" '.counts[$i] // 0')
    ground=$(ctl entities 48 item \
        | jq --arg item "$1" '[.entities[] | select(.kind == "item" and .item.item == $item) | .item.count] | add // 0')
    echo $((held + ground))
}
ctl command "clear @s" >/dev/null
break_wire 30 64 30
ctl wait 3
n=$(drops_of morered:red_alloy_wire)
[ "$n" -eq 3 ] || fail "a 3-subwire wire dropped $n, expected a stack of 3"
echo "a 3-subwire wire dropped a stack of 3"
ctl command "kill @e[type=minecraft:item]" >/dev/null
ctl command "clear @s" >/dev/null
ctl command "execute in minecraft:the_end run setblock 30 64 29 minecraft:air" >/dev/null
ctl command "execute in minecraft:the_end run setblock 30 64 30 morered:red_alloy_wire[axis=y,down=true,up=false,north=false,south=false,west=false,east=false]" >/dev/null
ctl wait 6
break_wire 30 64 30
ctl wait 3
n=$(drops_of morered:red_alloy_wire)
[ "$n" -eq 1 ] || fail "a 1-subwire wire dropped $n, expected a single item"
echo "a 1-subwire wire dropped a single item"
# Differential: the vanilla loot table always drops one item, so a wire with no
# subwires proves the state-derived count (it must drop nothing). A plain dirt
# block in the same cell proves the hook leaves non-wire drops alone.
ctl command "kill @e[type=minecraft:item]" >/dev/null
ctl command "clear @s" >/dev/null
ctl command "execute in minecraft:the_end run setblock 30 64 30 morered:red_alloy_wire[axis=y,down=false,up=false,north=false,south=false,west=false,east=false]" >/dev/null
ctl wait 6
break_wire 30 64 30
ctl wait 3
n=$(ctl entities 48 item | jq '[.entities[] | select(.kind == "item")] | length')
[ "$n" -eq 0 ] || fail "an empty wire dropped $n item entities"
[ "$(ctl inventory | jq -r '.counts["morered:red_alloy_wire"] // 0')" -eq 0 ] \
    || fail "an empty wire left a wire in the inventory"
echo "an empty wire dropped nothing"
ctl command "execute in minecraft:the_end run setblock 30 64 30 minecraft:dirt" >/dev/null
ctl wait 2
break_wire 30 64 30
ctl wait 3
n=$(drops_of minecraft:dirt)
[ "$n" -eq 1 ] || fail "the dirt control dropped $n, expected one"
echo "the dirt control still dropped one item"
ctl command "gamemode creative @s" >/dev/null
ctl wait 2

# Keep the client's built state table and content warnings for the offline
# binding check (the scratch home is deleted below).
STATES=$(find "$SCRATCH/.horizon-cache/mod-content" -name model-states.json 2>/dev/null | head -1)
if [ -n "$STATES" ]; then
    cp "$STATES" /tmp/opencode/morered-bundled-model-states.json
fi
WARNINGS=$(find "$SCRATCH/.horizon-cache/mod-content" -name horizon-mod-content-warnings.txt 2>/dev/null | head -1)
if [ -n "$WARNINGS" ]; then
    cp "$WARNINGS" /tmp/opencode/morered-bundled-content-warnings.txt
fi

echo "PASS: AND all-three lit, A+C dark, removal dark, NOT inverted, OR by A, multiplexer selects C/A, latch sets/holds/resets with complementary output, both-high dark and held set, pulse arms/clears/not-held/re-arms with a diode differential, pulse drives a latch, 96 pulse states accepted, stone counterfactual dark, 3-wire line lights its lamp, wire power decays 29,10,2,1,0 along 32 wires, far lamp dark and near lamp lit, source removal and stone source dead, middle wire removed drops the far side, wire feeds a diode, wire reads a lever and a repeater, a removed support disconnects its subwire, placed wire attaches to the clicked face, 64 wire states accepted, a broken wire drops one item per subwire (3 for three subwires and nothing for an empty wire) while a dirt control still drops one, bundled cable: relay channel 3 lights its lamp, channel 5 dark, a vanilla lamp beside the cable dead, source and middle-cable removal dark, stone relay dark, 64 cable states and 16 relay states accepted, bitwise NOT inverts cable channel 1 (source on dark, source removed 30 and lit), a diode in the same wiring passes it, a vanilla lamp on the plate's output face is dead, bitwise AND/OR/XOR/XNOR per-channel truth tables with channel 5 isolation, stone bitwise counterfactual dark, 144 bitwise states accepted and input_b rejected, wire posts: a spool links two posts 10 apart, the near post stores 14 and the far post 13 with its lamp lit, an unlinked control stays dark, cutting the link darkens the far post, the source removal drains both, the empty plate carries the link but emits nothing, the relay plate reads and emits on its sides, 96 post states accepted, the post/spool items and tab resolve, coloured cable: a red line lights its lamp with the plain wire decay, a blue line beside it stays dark, blue with its own source lights its own lamp, a powered blue line does not feed an adjacent red line, cutting the blue line darkens only its far side, a removed source drains only its own dye, plate placement sets facing from the clicked face and rotation from the clicked quadrant (the same quadrant at a yaw 180 degrees away is identical), a wooden rod turns a plate through all four rotations and the west lamp follows"
