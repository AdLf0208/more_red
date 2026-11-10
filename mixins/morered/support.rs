// Game-side glue for More Red's redstone: the logic plates, the bitwise
// plates, the red alloy wire, the sixteen coloured network cables, and the
// bundled cable with its relay plate.
//
// Injected into `crate::game` as `morered_state`; it needs the game, so it is
// not part of the standalone library (only `runtime.rs` is).
//
// Upstream recomputes a plate's input booleans in `neighborChanged` and
// schedules a 1-tick block tick. At the pin `native_neighbor_changed` is
// `&self` with no `Ticks`, so the port observes there (and in
// `block_changed`) into a dirty set and applies it at `level_tick_post`, where
// the level is in place and `native_set_block` works. The tick chain still
// moves one gate per server tick, like upstream's `TICK_DELAY = 1`.
//
// The pulse gate's clear is upstream's `scheduleTick(pos, this, 2)`: a map of
// armed positions counts down at `level_tick_post`, and the due ones are
// written there before the dirty batch. A clear already pending is not
// rescheduled (upstream's `willTickThisTick` guard).
//
// The wire keeps its six per-face powers in a mod block entity (upstream's
// `WireBlockEntity.POWER`), because a power state property would multiply the
// 64 wire states by 16. `block_signal` reads that array at its `pos`;
// neighbours are read with `signal_toward`, and the solve runs at
// `level_tick_post` as a worklist from every wire a neighbour change queued:
// a changed wire pushes its wire neighbours so a line settles in one tick,
// with a per-tick step and per-wire visit cap against runaway loops.
//
// The bitwise plates keep sixteen channel powers in their own block entity
// (upstream's `ChanneledPowerStorageBlockEntity`): they read the cable on an
// input side and seed one on their output face, and a cable face takes the
// strongest supplier (relay or plate). They emit no vanilla signal.
//
// There is also no per-direction redstone read for mods at the pin, so
// `signal_toward` reconstructs the engine's own answer: our gates and wires
// and comparators by state, a redstone wire by its power, everything else from
// `Behaviours::facts(state).weak` (getSignal over an empty level, four bits
// per direction). Another mod's `block_signal` answer, and indirect (strong)
// power conducted through a solid cube, are therefore invisible to a plate or
// wire (recorded deviations).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use horizon_client::containers::{BlockEntity, ModBlockEntity};
use horizon_client::world::World;
use horizon_core::geometry::Direction as GeoDir;
use horizon_core::position::BlockPos;
use horizon_world::level::Dimension as LevelDimension;
use horizon_world::level::{PlacementContext, flags};
use horizon_world::nbt::{Compound, JavaString, Tag};
use horizon_world::registry::{BlockRegistry, BlockStateId};

use crate::game::native_hooks::NativeLootDrops;
use horizon_client::items::ItemStack;

use crate::game::morered::{
    Attached, BitwiseGate, CHANNELS, CableColor, CablePower, Channel, ChannelPower, Dir, Gate,
    POST_LINK_RANGE, PostKind, PostLinks, PulseTick, Side, SpoolClick, bitwise_output,
    cable_channel_power, cable_power, connects, input_direction, latch_powered, output_direction,
    perpendicular, placement, placement_attached, plate_seed, post_power, post_signal, pulse_tick,
    pulse_update, relay_seed, relay_signal, retained, rod_rotation, same_wire_block,
    shares_attachment, source_power, spool_click, wire_power, wire_signal,
};

/// Entries one tick may apply; a safety budget against runaway feedback.
const MAX_DIRTY: usize = 4096;
/// Wire recomputes one level tick may do (all its wires together).
const MAX_WIRE_STEPS: usize = 16_384;
/// Times one wire may be recomputed in a single level tick.
const MAX_WIRE_VISITS: u8 = 8;
/// The one wire block this slice ships (`MoreRed.java`).
const WIRE_BLOCK: &str = "morered:red_alloy_wire";
/// The bundled cable (`MoreRed.java` `BUNDLED_NETWORK_CABLE`).
const CABLE_BLOCK: &str = "morered:bundled_network_cable";
/// The bundled cable relay plate (`MoreRed.java` `BUNDLED_CABLE_RELAY_PLATE`).
const RELAY_BLOCK: &str = "morered:bundled_cable_relay_plate";
/// The block entity's saved power array (`WireBlockEntity.POWER`).
const POWER_TAG: &str = "power";
/// The relay plate's declared channel selector.
const CHANNEL_PROP: &str = "channel";
/// The bitwise plates' stored channel powers (`ChanneledPowerStorageBlockEntity.POWER`).
const BITWISE_POWER_TAG: &str = "power";
/// A wire post's saved link offsets (`WirePostBlockEntity.CONNECTIONS`).
const CONNECTIONS_TAG: &str = "connections";
/// The red-alloy wire spool (`MoreRed.java` `REDWIRE_SPOOL`).
const SPOOL_ITEM: &str = "morered:redwire_spool";
/// The wooden rods a plate turns for (`Tags.Items.RODS_WOODEN`; the port ships
/// the tag with `minecraft:stick`).
const ROD_TAG: &str = "c:rods/wooden";

#[derive(Default)]
struct State {
    /// Plates queued for the next level tick.
    dirty: BTreeSet<(String, [i32; 3])>,
    /// Bitwise plates queued for the next level tick.
    bitwise: BTreeSet<(String, [i32; 3])>,
    /// Wires queued for the next power solve.
    wires: BTreeSet<(String, [i32; 3])>,
    /// Bundled cables queued for the next channel solve.
    cables: BTreeSet<(String, [i32; 3])>,
    /// Plates placed/changed this tick: notify their neighbours even when the
    /// stored inputs did not change (upstream's `onPlace` does).
    placed: BTreeSet<(String, [i32; 3])>,
    /// Positions this port is writing through `native_set_block` right now.
    /// The engine reports the write as `block_changed`; that is a state
    /// refinement, not a placement, so it must not seed another evaluation.
    writing: BTreeSet<(String, [i32; 3])>,
    /// Armed pulse gates: ticks left before their output clears.
    clearing: BTreeMap<(String, [i32; 3]), u8>,
    /// Wire posts queued for the next power solve.
    posts: BTreeSet<(String, [i32; 3])>,
    /// The first post each player's spool has selected (`spooled_post` on the
    /// Java item stack; the pin exposes no item-component write to mods).
    spooled: BTreeMap<u128, (String, [i32; 3])>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> std::sync::MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// A plate read from its state.
struct Plate {
    gate: Gate,
    attachment: Dir,
    rotation: u8,
    inputs: [bool; 3],
    powered: bool,
}

fn plate(registry: &BlockRegistry, state: BlockStateId) -> Option<Plate> {
    let state = registry.state(state)?;
    let gate = Gate::from_id(&state.name)?;
    let attachment = dir_of(state.properties.get("facing")?)?;
    let rotation = state.properties.get("rotation")?.parse::<u8>().ok()? % 4;
    let input = |key: &str| state.properties.get(key).is_some_and(|value| value == "true");
    Some(Plate {
        gate,
        attachment,
        rotation,
        inputs: [input("input_a"), input("input_b"), input("input_c")],
        powered: input("powered"),
    })
}

/// A wire's subwires from its state, or `None` for another block. The plain
/// red alloy wire and the sixteen coloured network cables share the six
/// booleans (`AbstractWireBlock`); their identity/dye is the block id.
fn wire_attached(registry: &BlockRegistry, state: BlockStateId) -> Option<Attached> {
    let state = registry.state(state)?;
    if &*state.name != WIRE_BLOCK && CableColor::from_id(&state.name).is_none() {
        return None;
    }
    let bit = |key: &str| state.properties.get(key).is_some_and(|value| value == "true");
    Some(Attached::from_bits([
        bit("down"),
        bit("up"),
        bit("north"),
        bit("south"),
        bit("west"),
        bit("east"),
    ]))
}

/// A wire-like block's dye: `Some(None)` is the plain red alloy wire,
/// `Some(Some(dye))` a coloured network cable, `None` any other block (the
/// bundled cable included — it has its own channel solve).
fn wire_colour(registry: &BlockRegistry, state: BlockStateId) -> Option<Option<CableColor>> {
    let state = registry.state(state)?;
    if &*state.name == WIRE_BLOCK {
        Some(None)
    } else {
        CableColor::from_id(&state.name).map(Some)
    }
}

/// A bundled cable's subwires from its state, or `None` for another block.
fn cable_attached(registry: &BlockRegistry, state: BlockStateId) -> Option<Attached> {
    let state = registry.state(state)?;
    if &*state.name != CABLE_BLOCK {
        return None;
    }
    let bit = |key: &str| state.properties.get(key).is_some_and(|value| value == "true");
    Some(Attached::from_bits([
        bit("down"),
        bit("up"),
        bit("north"),
        bit("south"),
        bit("west"),
        bit("east"),
    ]))
}

/// Both six-subwire blocks share placement, shape and survival behaviour.
fn attached(registry: &BlockRegistry, state: BlockStateId) -> Option<Attached> {
    wire_attached(registry, state).or_else(|| cable_attached(registry, state))
}

/// A bundled cable relay plate read from its state.
struct Relay {
    channel: Channel,
}

fn relay(registry: &BlockRegistry, state: BlockStateId) -> Option<Relay> {
    let state = registry.state(state)?;
    if &*state.name != RELAY_BLOCK {
        return None;
    }
    let channel = Channel::from_property(state.properties.get(CHANNEL_PROP)?)?;
    Some(Relay { channel })
}

/// A bitwise logic plate read from its state (`facing` + `rotation` only).
struct Bitwise {
    gate: BitwiseGate,
    attachment: Dir,
    rotation: u8,
}

fn bitwise(registry: &BlockRegistry, state: BlockStateId) -> Option<Bitwise> {
    let state = registry.state(state)?;
    let gate = BitwiseGate::from_id(&state.name)?;
    let attachment = dir_of(state.properties.get("facing")?)?;
    let rotation = state.properties.get("rotation")?.parse::<u8>().ok()? % 4;
    Some(Bitwise {
        gate,
        attachment,
        rotation,
    })
}

/// A red-alloy wire post read from its state (`facing` + `power`).
struct Post {
    kind: PostKind,
    attachment: Dir,
    power: u8,
}

fn post(registry: &BlockRegistry, state: BlockStateId) -> Option<Post> {
    let state = registry.state(state)?;
    let kind = PostKind::from_id(&state.name)?;
    let attachment = dir_of(state.properties.get("facing")?)?;
    let power = state
        .properties
        .get("power")?
        .parse::<u8>()
        .ok()?
        .min(15);
    Some(Post {
        kind,
        attachment,
        power,
    })
}

fn dir_of(value: &str) -> Option<Dir> {
    Some(match value {
        "down" => Dir::Down,
        "up" => Dir::Up,
        "north" => Dir::North,
        "south" => Dir::South,
        "west" => Dir::West,
        "east" => Dir::East,
        _ => return None,
    })
}

fn dir_name(dir: Dir) -> &'static str {
    match dir {
        Dir::Down => "down",
        Dir::Up => "up",
        Dir::North => "north",
        Dir::South => "south",
        Dir::West => "west",
        Dir::East => "east",
    }
}

/// The template's `axis` value for a face: the content file's shape selector
/// (a wire cannot declare a shape keyed on one of its own booleans, and the
/// pin's state order rules out a template property sorting after them).
fn axis_name(dir: Dir) -> &'static str {
    match dir {
        Dir::Down | Dir::Up => "y",
        Dir::North | Dir::South => "z",
        Dir::West | Dir::East => "x",
    }
}

fn to_dir(dir: GeoDir) -> Dir {
    match dir {
        GeoDir::Down => Dir::Down,
        GeoDir::Up => Dir::Up,
        GeoDir::North => Dir::North,
        GeoDir::South => Dir::South,
        GeoDir::West => Dir::West,
        GeoDir::East => Dir::East,
    }
}

fn dimension_name(dimension: LevelDimension) -> &'static str {
    match dimension {
        LevelDimension::Overworld => "minecraft:overworld",
        LevelDimension::Nether => "minecraft:the_nether",
        LevelDimension::End => "minecraft:the_end",
    }
}

fn offset([x, y, z]: [i32; 3], dir: Dir) -> [i32; 3] {
    match dir {
        Dir::Down => [x, y - 1, z],
        Dir::Up => [x, y + 1, z],
        Dir::North => [x, y, z - 1],
        Dir::South => [x, y, z + 1],
        Dir::West => [x - 1, y, z],
        Dir::East => [x + 1, y, z],
    }
}

fn mark(dimension: &str, pos: [i32; 3], placed: bool) {
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.dirty.len() < MAX_DIRTY {
        let key = (dimension.to_owned(), pos);
        state.dirty.insert(key.clone());
        if placed {
            state.placed.insert(key);
        }
    }
}

fn mark_wire(dimension: &str, pos: [i32; 3]) {
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.wires.len() < MAX_DIRTY {
        state.wires.insert((dimension.to_owned(), pos));
    }
}

fn mark_cable(dimension: &str, pos: [i32; 3]) {
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.cables.len() < MAX_DIRTY {
        state.cables.insert((dimension.to_owned(), pos));
    }
}

fn mark_bitwise(dimension: &str, pos: [i32; 3]) {
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.bitwise.len() < MAX_DIRTY {
        state.bitwise.insert((dimension.to_owned(), pos));
    }
}

fn mark_post(dimension: &str, pos: [i32; 3]) {
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.posts.len() < MAX_DIRTY {
        state.posts.insert((dimension.to_owned(), pos));
    }
}

/// Queue every bundled cable beside a relay plate: a relay's vanilla input is
/// read by the cable's channel solve on its next pass.
fn mark_cables_around(level: &crate::game::GameLevel<'_>, dimension: &str, pos: [i32; 3]) {
    for face in Dir::ALL {
        let neighbour_pos = offset(pos, face);
        let Some(neighbour) = level_block(level, neighbour_pos) else {
            continue;
        };
        if cable_attached(level.registry, neighbour).is_some() {
            mark_cable(dimension, neighbour_pos);
        }
    }
}

/// `block_signal`: answer from the wire's stored per-face power, or from a
/// plate's stored state. The hook's `face` is the direction the signal travels
/// (the dispatcher flips vanilla's side), so a wire answers for the subwire
/// toward the reader.
#[allow(private_interfaces)]
pub fn on_block_signal(
    level: &crate::game::GameLevel<'_>,
    pos: BlockPos,
    state: BlockStateId,
    face: GeoDir,
    direct: bool,
    signal: &mut i32,
) {
    if let Some(attached) = wire_attached(level.registry, state) {
        let power = wire_power_at_level(level, [pos.x, pos.y, pos.z]);
        *signal = wire_signal(power, attached, to_dir(face));
        return;
    }
    if cable_attached(level.registry, state).is_some() {
        // Upstream's bundled cable is not a redstone signal source: only a
        // relay plate answers a vanilla reader. The block's basalt template
        // still counts as a solid conductor, so both the weak and the
        // conducted (direct) answer are forced to zero.
        *signal = 0;
        return;
    }
    if let Some(relay) = relay(level.registry, state) {
        // A relay is a weak source only: a direct answer would conduct
        // through the solid cable beside it and light a lamp above the cable.
        *signal = if direct {
            0
        } else {
            relay_output_level(level, [pos.x, pos.y, pos.z], relay.channel)
        };
        return;
    }
    if bitwise(level.registry, state).is_some() {
        // `BitwiseLogicPlateBlock` extends `PlateBlock`, not
        // `RedstonePlateBlock`: it has no vanilla signal; only a cable reads it.
        *signal = 0;
        return;
    }
    if let Some(post) = post(level.registry, state) {
        // `AbstractPoweredWirePostBlock.getSignal`: the stored POWER on a
        // connectable face, and `getDirectSignal` answers the same.
        *signal = post_signal(post.power, post.attachment, post.kind, to_dir(face));
        return;
    }
    let Some(plate) = plate(level.registry, state) else {
        return;
    };
    *signal = plate.gate.signal_with_powered(
        plate.inputs,
        plate.powered,
        plate.attachment,
        plate.rotation,
        to_dir(face),
    );
}

/// `is_signal_source`: every plate is one, a wire with at least one subwire is
/// one (upstream's `!isEmptyWireBlock`), and a relay plate is one. The bundled
/// cable itself is not (upstream inherits `AbstractWireBlock`'s default).
#[allow(private_interfaces)]
pub fn on_is_signal_source(
    level: &crate::game::GameLevel<'_>,
    state: BlockStateId,
    source: &mut bool,
) {
    let wire = wire_attached(level.registry, state).is_some_and(|attached| !attached.is_empty());
    if wire
        || relay(level.registry, state).is_some()
        || plate(level.registry, state).is_some()
        || post(level.registry, state).is_some()
    {
        *source = true;
    }
}

/// A neighbour changed: queue the plate for the next level tick, the wire for
/// the next power solve, or the cables beside a relay plate for the next
/// channel solve.
#[allow(private_interfaces)]
pub fn on_neighbor_changed(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    state: BlockStateId,
) {
    let at = [pos.x, pos.y, pos.z];
    if wire_attached(level.registry, state).is_some() {
        mark_wire(dimension_name(dimension), at);
    } else if cable_attached(level.registry, state).is_some() {
        mark_cable(dimension_name(dimension), at);
    } else if relay(level.registry, state).is_some() {
        mark_cables_around(level, dimension_name(dimension), at);
    } else if plate(level.registry, state).is_some() {
        mark(dimension_name(dimension), at, false);
    } else if bitwise(level.registry, state).is_some() {
        mark_bitwise(dimension_name(dimension), at);
    } else if post(level.registry, state).is_some() {
        mark_post(dimension_name(dimension), at);
    }
}

/// A block was placed/changed: seed the first evaluation too (`/setblock`,
/// pistons, world load). A state write of this port is not a placement.
#[allow(private_interfaces)]
pub fn on_block_changed(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    new: BlockStateId,
) {
    let key = (dimension_name(dimension).to_owned(), [pos.x, pos.y, pos.z]);
    if state()
        .get_or_insert_with(State::default)
        .writing
        .remove(&key)
    {
        return;
    }
    if wire_attached(level.registry, new).is_some() {
        mark_wire(dimension_name(dimension), [pos.x, pos.y, pos.z]);
    } else if cable_attached(level.registry, new).is_some() {
        mark_cable(dimension_name(dimension), [pos.x, pos.y, pos.z]);
    } else if relay(level.registry, new).is_some() {
        mark_cables_around(level, dimension_name(dimension), [pos.x, pos.y, pos.z]);
    } else if plate(level.registry, new).is_some() {
        mark(dimension_name(dimension), [pos.x, pos.y, pos.z], true);
    } else if bitwise(level.registry, new).is_some() {
        let at = [pos.x, pos.y, pos.z];
        mark_bitwise(dimension_name(dimension), at);
        // A fresh plate starts at zero power; its first evaluation may seed a
        // cable, so solve the cables beside it once it has run.
        mark_cables_around(level, dimension_name(dimension), at);
    } else if post(level.registry, new).is_some() {
        mark_post(dimension_name(dimension), [pos.x, pos.y, pos.z]);
    } else {
        // A removed post keeps its block entity until the engine removes it
        // just after this observation (`LevelChunk.setBlockState`), so its
        // peers can still be told to recompute (`clearRemoteConnections`'s
        // `neighborChanged` on each peer).
        let at = [pos.x, pos.y, pos.z];
        let links = post_links_in(level.world, at);
        if !links.offsets().is_empty() {
            for peer in links.absolute(at) {
                let Some(peer_state) = level_block(level, peer) else {
                    continue;
                };
                if post(level.registry, peer_state).is_some() {
                    mark_post(dimension_name(dimension), peer);
                }
            }
        }
    }
}

/// `updateShape`: a subwire whose supporting neighbour stopped being sturdy
/// disconnects (`AbstractWireBlock.updateShape`). A subwire is never added
/// here, and the wire is queued so its power follows the new shape.
#[allow(private_interfaces)]
pub fn on_update_shape(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    state: BlockStateId,
    direction: GeoDir,
    neighbour: BlockStateId,
    new: &mut Option<BlockStateId>,
) {
    let Some(attached_now) = attached(level.registry, state) else {
        return;
    };
    // The neighbour's face toward the wire.
    let sturdy = level
        .behaviours
        .is_face_sturdy(neighbour, direction.opposite());
    let direction = to_dir(direction);
    if !attached_now.get(direction) {
        return;
    }
    let next = retained(attached_now, direction, sturdy);
    if next == attached_now {
        return;
    }
    if let Some(new_state) = with_attached(level.registry, state, next) {
        *new = Some(new_state);
        if cable_attached(level.registry, state).is_some() {
            mark_cable(dimension_name(dimension), [pos.x, pos.y, pos.z]);
        } else {
            mark_wire(dimension_name(dimension), [pos.x, pos.y, pos.z]);
        }
    }
}

/// `getStateForPlacement`: a plate attaches to the clicked face's opposite and
/// turns its output toward the clicked quadrant; a post attaches to the
/// clicked face's opposite; a fresh wire attaches to the face it was clicked
/// against, and only when that face is sturdy (otherwise vanilla places
/// nothing).
#[allow(private_interfaces)]
pub fn on_state_for_placement(
    level: &crate::game::GameLevel<'_>,
    state: BlockStateId,
    context: &PlacementContext,
    answer: &mut Option<Option<BlockStateId>>,
) {
    if post(level.registry, state).is_some() {
        // `AbstractPostBlock.getStateForPlacement`: attach to the clicked
        // face's opposite (the nearest-looking scan collapses to this for a
        // face-on click). Power starts at zero and the first solve fills it.
        let attachment = to_dir(context.face).opposite();
        if let Some(new) = with_facing(level.registry, state, attachment) {
            *answer = Some(Some(new));
        }
        return;
    }
    if plate(level.registry, state).is_some() || bitwise(level.registry, state).is_some() {
        // `PlateBlockStateProperties.getStateForPlacedGatePlate`: attach to the
        // clicked face's opposite and point the output at the quadrant of the
        // face the player clicked (`BlockStateUtil`), not at the player's yaw.
        // The inputs are read by the first evaluation, as for every plate.
        let hit = [
            context.hit.x - f64::from(context.pos.x),
            context.hit.y - f64::from(context.pos.y),
            context.hit.z - f64::from(context.pos.z),
        ];
        let (attachment, rotation) = placement(to_dir(context.face), hit);
        if let Some(new) = with_facing_rotation(level.registry, state, attachment, rotation) {
            *answer = Some(Some(new));
        }
        return;
    }
    if attached(level.registry, state).is_none() {
        return;
    }
    let pos = [context.pos.x, context.pos.y, context.pos.z];
    let attachment = to_dir(context.face).opposite();
    let Some(support) = level_block(level, offset(pos, attachment)) else {
        return;
    };
    if !level
        .behaviours
        .is_face_sturdy(support, context.face)
    {
        *answer = Some(None);
        return;
    }
    if let Some(new) = with_attached(
        level.registry,
        state,
        placement_attached(to_dir(context.face)),
    ) {
        *answer = Some(Some(new));
    }
}

/// `use_item_on`: a wooden rod pokes a plate one quarter turn
/// (`RedstonePlateBlock.useItemOn` / `BitwiseLogicPlateBlock.useItemOn`); a
/// spool click on a post selects, links, cuts or re-selects
/// (`WireSpoolItem.onUseOnPost`). The first post is port-owned state keyed by
/// the player's UUID (the Java item's `spooled_post` component); the sagging
/// wire raytrace (`SlackInterpolator`) is render-only and not carried.
#[allow(private_interfaces)]
pub fn on_use_item_on(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    claimed: &mut bool,
) {
    if &*stack.item != SPOOL_ITEM {
        rotate_with_rod(game, stack, pos, claimed);
        return;
    }
    let Some(block_state) = block_at(game, pos) else {
        return;
    };
    if post(&game.pack.assets.registry, block_state).is_none() {
        return;
    }
    let Some(player) = game.native_acting_player() else {
        return;
    };
    let dimension = game.dim.id().to_owned();
    let stored = state()
        .get_or_insert_with(State::default)
        .spooled
        .get(&player.uuid)
        .cloned();
    // A post selected in another dimension cannot be connected to this one.
    let selected = stored
        .as_ref()
        .filter(|(dim, _)| dim == &dimension)
        .map(|(_, selected)| *selected);
    let connected = selected.is_some_and(|selected| posts_linked(game, pos, selected));
    match spool_click(selected, pos, connected, POST_LINK_RANGE) {
        SpoolClick::Select | SpoolClick::TooFar => {
            state()
                .get_or_insert_with(State::default)
                .spooled
                .insert(player.uuid, (dimension, pos));
        }
        SpoolClick::ClearSelection => {
            state()
                .get_or_insert_with(State::default)
                .spooled
                .remove(&player.uuid);
        }
        SpoolClick::Cut => {
            if let Some(selected) = selected {
                cut_link(game, &dimension, pos, selected);
            }
            state()
                .get_or_insert_with(State::default)
                .spooled
                .remove(&player.uuid);
        }
        SpoolClick::Link => {
            if let Some(selected) = selected {
                link_posts(game, &dimension, pos, selected);
            }
            state()
                .get_or_insert_with(State::default)
                .spooled
                .remove(&player.uuid);
        }
    }
    *claimed = true;
}

/// A wooden-rod poke on a plate: one quarter turn clockwise
/// (`RedstonePlateBlock.useItemOn` / `BitwiseLogicPlateBlock.useItemOn`). A
/// gate plate re-reads its inputs at the new rotation in the same write; a
/// bitwise plate's channel powers live in its block entity and are left alone.
/// The item is matched against the shipped `#c:rods/wooden` tag.
fn rotate_with_rod(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    claimed: &mut bool,
) {
    let is_rod = game
        .res
        .items()
        .item_tags
        .get(ROD_TAG)
        .is_some_and(|items| items.contains(&*stack.item));
    if !is_rod {
        return;
    }
    let Some(block_state) = block_at(game, pos) else {
        return;
    };
    let registry = &game.pack.assets.registry;
    let new = if let Some(plate) = plate(registry, block_state) {
        let rotation = rod_rotation(plate.rotation);
        let Some(rotated) = with_facing_rotation(registry, block_state, plate.attachment, rotation)
        else {
            return;
        };
        let mut inputs = plate.inputs;
        for side in plate.gate.sides() {
            let dir = input_direction(plate.attachment, rotation, *side);
            let neighbour_pos = offset(pos, dir);
            let Some(neighbour) = block_at(game, neighbour_pos) else {
                continue;
            };
            inputs[side.index()] = signal_toward(game, neighbour_pos, neighbour, dir) > 0;
        }
        with_inputs(game, rotated, plate.gate, inputs, plate.powered)
    } else if let Some(plate) = bitwise(registry, block_state) {
        with_facing_rotation(
            registry,
            block_state,
            plate.attachment,
            rod_rotation(plate.rotation),
        )
    } else {
        return;
    };
    let Some(new) = new else {
        return;
    };
    let dimension = game.dim.id().to_owned();
    if write(game, &dimension, pos, new) {
        *claimed = true;
    }
}

/// Apply the queued wires and plates for this dimension.
#[allow(private_interfaces)]
pub fn on_level_tick_post(game: &mut crate::game::Game<'_>, dimension: &str) {
    clear_due_pulses(game, dimension);
    solve_posts(game, dimension);
    solve_wires(game, dimension);
    solve_cables(game, dimension);
    apply_bitwise(game, dimension);
    apply_plates(game, dimension);
}

/// Recompute the queued wire posts and every post a changed link reaches, in
/// one tick (`AbstractPoweredWirePostBlock.neighborChanged` recomputes one post
/// and notifies its links; the port's worklist does the same one tick later).
fn solve_posts(game: &mut crate::game::Game<'_>, dimension: &str) {
    let mut queue: VecDeque<[i32; 3]> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let ready: Vec<(String, [i32; 3])> = state
            .posts
            .iter()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        for key in &ready {
            state.posts.remove(key);
        }
        ready.into_iter().map(|(_, pos)| pos).collect()
    };
    let mut visits: BTreeMap<[i32; 3], u8> = BTreeMap::new();
    let mut steps = MAX_WIRE_STEPS;
    while let Some(pos) = queue.pop_front() {
        if steps == 0 {
            break;
        }
        let count = visits.entry(pos).or_insert(0);
        if *count >= MAX_WIRE_VISITS {
            continue;
        }
        *count += 1;
        steps -= 1;
        let Some(state) = block_at(game, pos) else {
            continue;
        };
        let Some(post_now) = post(&game.pack.assets.registry, state) else {
            continue;
        };
        ensure_post_entity(game, state, pos);
        let (neighbour_power, linked, links) = post_supply(game, pos, &post_now);
        let next = post_power(neighbour_power, &linked);
        if next == post_now.power {
            continue;
        }
        let Some(new) = with_power(&game.pack.assets.registry, state, next) else {
            continue;
        };
        if !write(game, dimension, pos, new) {
            continue;
        }
        // The linked peers' own power may now rise or fall.
        for peer in links {
            queue.push_back(peer);
        }
    }
}

/// A post's supplies: the strongest signal on a connectable side (read toward
/// the post), and the stored power of every loaded linked post. Stale links
/// to a non-post are pruned from the block entity.
fn post_supply(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    post_now: &Post,
) -> (u8, Vec<u8>, Vec<[i32; 3]>) {
    let registry = &game.pack.assets.registry;
    let mut neighbour_power = 0u8;
    for dir in post_now.kind.connections(post_now.attachment) {
        let neighbour_pos = offset(pos, dir);
        let Some(neighbour) = block_at(game, neighbour_pos) else {
            continue;
        };
        let signal = signal_toward(game, neighbour_pos, neighbour, dir);
        neighbour_power = neighbour_power.max(u8::try_from(signal.clamp(0, 15)).unwrap_or(0));
    }
    let links = post_links_at(game, pos);
    let mut linked = Vec::new();
    let mut stale = Vec::new();
    for peer in links.absolute(pos) {
        match block_at(game, peer) {
            // An unloaded chunk is not a broken link; keep it.
            None => continue,
            Some(peer_state) => match post(registry, peer_state) {
                Some(peer_post) => linked.push(peer_post.power),
                None => stale.push([peer[0] - pos[0], peer[1] - pos[1], peer[2] - pos[2]]),
            },
        }
    }
    let peers = links.absolute(pos).collect();
    if !stale.is_empty() {
        let mut remaining = links;
        for offset in stale {
            remaining.remove(offset);
        }
        write_post_links(game, pos, &remaining);
    }
    (neighbour_power, linked, peers)
}

/// Recompute the queued wires and everything a change reaches, in one tick.
fn solve_wires(game: &mut crate::game::Game<'_>, dimension: &str) {
    let mut queue: VecDeque<[i32; 3]> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let ready: Vec<(String, [i32; 3])> = state
            .wires
            .iter()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        for key in &ready {
            state.wires.remove(key);
        }
        ready.into_iter().map(|(_, pos)| pos).collect()
    };
    let mut visits: BTreeMap<[i32; 3], u8> = BTreeMap::new();
    let mut steps = MAX_WIRE_STEPS;
    while let Some(pos) = queue.pop_front() {
        if steps == 0 {
            break;
        }
        let count = visits.entry(pos).or_insert(0);
        if *count >= MAX_WIRE_VISITS {
            continue;
        }
        *count += 1;
        steps -= 1;
        let Some(state) = block_at(game, pos) else {
            continue;
        };
        let Some(attached_now) = attached(&game.pack.assets.registry, state) else {
            continue;
        };
        let Some(colour) = wire_colour(&game.pack.assets.registry, state) else {
            // A bundled cable beside a wire is not solved by the single-channel
            // wire pass; its own channel solve handles it.
            continue;
        };
        ensure_wire_entity(game, state, pos);
        let (support, linked) = wire_supply(game, pos, attached_now, colour);
        let next = wire_power(attached_now, support, linked);
        if next == wire_power_at(game, pos) {
            continue;
        }
        if !write_wire_power(game, pos, next) {
            continue;
        }
        game.native_update_signal_neighbors(dimension, pos);
        game.native_block_entity_sync(pos);
        let mut reached = Vec::new();
        for face in Dir::ALL {
            let neighbour_pos = offset(pos, face);
            let Some(neighbour) = block_at(game, neighbour_pos) else {
                continue;
            };
            let registry = &game.pack.assets.registry;
            if attached(registry, neighbour).is_some() {
                reached.push(Some(neighbour_pos));
            } else if plate(registry, neighbour).is_some() {
                reached.push(None);
                mark(dimension, neighbour_pos, false);
            }
        }
        for wire in reached.into_iter().flatten() {
            queue.push_back(wire);
        }
    }
}

/// Recompute the queued bundled cables and everything a change reaches, in one
/// tick (`BundledCableBlock.updatePowerAfterBlockUpdate`, sixteen channels).
fn solve_cables(game: &mut crate::game::Game<'_>, dimension: &str) {
    let mut queue: VecDeque<[i32; 3]> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let ready: Vec<(String, [i32; 3])> = state
            .cables
            .iter()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        for key in &ready {
            state.cables.remove(key);
        }
        ready.into_iter().map(|(_, pos)| pos).collect()
    };
    let mut visits: BTreeMap<[i32; 3], u8> = BTreeMap::new();
    let mut steps = MAX_WIRE_STEPS;
    while let Some(pos) = queue.pop_front() {
        if steps == 0 {
            break;
        }
        let count = visits.entry(pos).or_insert(0);
        if *count >= MAX_WIRE_VISITS {
            continue;
        }
        *count += 1;
        steps -= 1;
        let Some(state) = block_at(game, pos) else {
            continue;
        };
        let Some(attached_now) = cable_attached(&game.pack.assets.registry, state) else {
            continue;
        };
        ensure_cable_entity(game, state, pos);
        let (seeds, linked) = cable_supply(game, pos, attached_now);
        let next = cable_power(attached_now, seeds, linked);
        if next == cable_power_at(game, pos) {
            continue;
        }
        if !write_cable_power(game, pos, next) {
            continue;
        }
        game.native_update_signal_neighbors(dimension, pos);
        game.native_block_entity_sync(pos);
        let mut reached = Vec::new();
        let mut relays = Vec::new();
        for face in Dir::ALL {
            let neighbour_pos = offset(pos, face);
            let Some(neighbour) = block_at(game, neighbour_pos) else {
                continue;
            };
            let registry = &game.pack.assets.registry;
            if cable_attached(registry, neighbour).is_some() {
                reached.push(Some(neighbour_pos));
            } else if relay(registry, neighbour).is_some() {
                // A relay answers its vanilla readers from this cable's block
                // entity, so its own neighbours (a lamp) must be updated.
                relays.push(neighbour_pos);
            } else if plate(registry, neighbour).is_some() {
                reached.push(None);
                mark(dimension, neighbour_pos, false);
            } else if bitwise(registry, neighbour).is_some() {
                // A bitwise plate reads this cable's channels on its next pass.
                mark_bitwise(dimension, neighbour_pos);
            }
        }
        for relay_pos in relays {
            game.native_update_signal_neighbors(dimension, relay_pos);
        }
        for cable in reached.into_iter().flatten() {
            queue.push_back(cable);
        }
    }
}

/// The two supplies of one bundled cable: an adjacent relay's channel seed and
/// the strongest connected neighbouring cable (per channel).
fn cable_supply(
    game: &crate::game::Game<'_>,
    pos: [i32; 3],
    attached_now: Attached,
) -> (CablePower, CablePower) {
    let registry = &game.pack.assets.registry;
    let mut seeds = CablePower::NONE;
    let mut linked = CablePower::NONE;
    for face in Dir::ALL {
        if !attached_now.get(face) {
            continue;
        }
        let face_pos = offset(pos, face);
        if let Some(neighbour) = block_at(game, face_pos) {
            if let Some(relay) = relay(registry, neighbour) {
                // Upstream's channeled capability on the attached face: a
                // relay seeds its selected channel with `2s - 1`, and a face
                // takes the strongest supplier (`max`), so an unpowered relay
                // must not overwrite another supplier on the same channel.
                let seed = relay_seed(relay_input(game, face_pos));
                seeds.raise(face, relay.channel, seed);
            } else if let Some(neighbour_attached) = cable_attached(registry, neighbour)
                && neighbour_attached.get(face.opposite())
            {
                let power = cable_power_at(game, face_pos);
                for channel in Channel::ALL {
                    linked.set(face, channel, power.get(face.opposite(), channel));
                }
            }
        }
        // The four faces sharing an edge with the attachment: a coplanar
        // cable (same attachment face) is one step away; a relay there seeds
        // its channel (upstream's orthogonal cable-connector path).
        for side in perpendicular(face) {
            let side_pos = offset(pos, side);
            let Some(neighbour) = block_at(game, side_pos) else {
                continue;
            };
            if let Some(relay) = relay(registry, neighbour) {
                // A face takes the strongest supplier (`max`, not a replace):
                // the bitwise plate below may seed the same face and channel.
                let seed = relay_seed(relay_input(game, side_pos));
                seeds.raise(face, relay.channel, seed);
            } else if let Some(plate) = bitwise(registry, neighbour) {
                // `canConnectToAdjacentCable` + `getChanneledPower`: a bitwise
                // plate feeds a cable only when the cable sits on the plate's
                // output face and the cable's subwire is attached to the
                // plate's attachment (`wireFace == plateAttachmentDir`). An
                // output face is always perpendicular to the attachment, so
                // this can only be the edge path, never the attached face
                // itself. A 31 output steps down to 30 (`max(0, supplier - 1)`).
                if plate.attachment == face
                    && output_direction(plate.attachment, plate.rotation) == side.opposite()
                {
                    let power = bitwise_power_at(game, side_pos);
                    for channel in Channel::ALL {
                        seeds.raise(face, channel, plate_seed(power.get(channel)));
                    }
                }
            } else if let Some(neighbour_attached) = cable_attached(registry, neighbour) {
                if shares_attachment(face, neighbour_attached, side.opposite()) {
                    let power = cable_power_at(game, side_pos);
                    for channel in Channel::ALL {
                        let seen = linked.get(face, channel).max(power.get(face, channel));
                        linked.set(face, channel, seen);
                    }
                } else {
                    let diagonal_pos = offset(side_pos, face);
                    let Some(diagonal) = block_at(game, diagonal_pos) else {
                        continue;
                    };
                    let Some(diagonal_attached) = cable_attached(registry, diagonal) else {
                        continue;
                    };
                    if diagonal_attached.get(side.opposite()) {
                        let power = cable_power_at(game, diagonal_pos);
                        for channel in Channel::ALL {
                            let seen = linked
                                .get(face, channel)
                                .max(power.get(side.opposite(), channel));
                            linked.set(face, channel, seen);
                        }
                    }
                }
            }
        }
    }
    (seeds, linked)
}

/// The vanilla signal a relay plate reads from its non-cable neighbours.
fn relay_input(game: &crate::game::Game<'_>, pos: [i32; 3]) -> i32 {
    let registry = &game.pack.assets.registry;
    let mut strongest = 0;
    for dir in Dir::ALL {
        let neighbour_pos = offset(pos, dir);
        let Some(state) = block_at(game, neighbour_pos) else {
            continue;
        };
        if cable_attached(registry, state).is_some() || relay(registry, state).is_some() {
            continue;
        }
        strongest = strongest.max(signal_toward(game, neighbour_pos, state, dir));
    }
    strongest
}

/// The vanilla signal a relay plate answers: the strongest attached channel
/// power of an adjacent cable, halved (upstream's `power / 2`).
fn relay_output_level(
    level: &crate::game::GameLevel<'_>,
    pos: [i32; 3],
    channel: Channel,
) -> i32 {
    let mut signal = 0;
    for dir in Dir::ALL {
        let neighbour_pos = offset(pos, dir);
        let Some(state) = level_block(level, neighbour_pos) else {
            continue;
        };
        let Some(attached) = cable_attached(level.registry, state) else {
            continue;
        };
        let power = cable_power_at_level(level, neighbour_pos);
        signal = signal.max(relay_signal(power, attached, channel));
    }
    signal
}

/// The two supplies of one wire's subwires: the block on each attached face,
/// and the strongest connected neighbouring subwire.
fn wire_supply(
    game: &crate::game::Game<'_>,
    pos: [i32; 3],
    attached_now: Attached,
    self_colour: Option<CableColor>,
) -> ([u8; 6], [u8; 6]) {
    let registry = &game.pack.assets.registry;
    let mut support = [0u8; 6];
    let mut linked = [0u8; 6];
    for face in Dir::ALL {
        if !attached_now.get(face) {
            continue;
        }
        let index = face.index();
        let face_pos = offset(pos, face);
        if let Some(neighbour) = block_at(game, face_pos) {
            // A face-to-face wire only feeds the same block. Legitimate
            // subwires attach to sturdy blocks, so this only pins forced
            // states; it keeps a different dye from seeding through a face.
            let feed = match wire_colour(registry, neighbour) {
                Some(neighbour_colour) => same_wire_block(self_colour, neighbour_colour),
                None => true,
            };
            if feed {
                support[index] = source_power(signal_toward(game, face_pos, neighbour, face));
            }
        }
        // Upstream reads the four faces sharing an edge with the attachment:
        // a wire there that shares the attachment face is a step away; a
        // perpendicular wire connects round the convex edge through the
        // diagonal block. A different dye never links either way.
        for side in perpendicular(face) {
            let side_pos = offset(pos, side);
            let Some(neighbour) = block_at(game, side_pos) else {
                continue;
            };
            if let Some(neighbour_attached) = attached(registry, neighbour) {
                let neighbour_colour = wire_colour(registry, neighbour).flatten();
                if connects(
                    face,
                    self_colour,
                    neighbour_attached,
                    neighbour_colour,
                    side.opposite(),
                ) {
                    linked[index] = linked[index].max(wire_power_at(game, side_pos)[index]);
                } else if same_wire_block(self_colour, neighbour_colour) {
                    let diagonal_pos = offset(side_pos, face);
                    let Some(diagonal) = block_at(game, diagonal_pos) else {
                        continue;
                    };
                    let diagonal_colour = wire_colour(registry, diagonal).flatten();
                    if !same_wire_block(self_colour, diagonal_colour) {
                        continue;
                    }
                    let Some(diagonal_attached) = attached(registry, diagonal) else {
                        continue;
                    };
                    let slot = side.opposite().index();
                    if diagonal_attached.get(side.opposite()) {
                        linked[index] =
                            linked[index].max(wire_power_at(game, diagonal_pos)[slot]);
                    }
                }
            } else {
                // A non-wire neighbour (a source, a solid block, a plate)
                // feeds the side subwire directly: upstream's default
                // expanded supplier gives `2 * signal`, so this is already a
                // full source value (no extra decay).
                support[index] = support[index].max(source_power(signal_toward(
                    game, side_pos, neighbour, side,
                )));
            }
        }
    }
    (support, linked)
}

/// Apply the queued bitwise plates for this dimension
/// (`BitwiseLogicPlateBlock.updatePower`: read the input sides' channels, apply
/// the gate per channel, store 31/0 in the plate's block entity).
fn apply_bitwise(game: &mut crate::game::Game<'_>, dimension: &str) {
    let batch: Vec<[i32; 3]> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let ready: Vec<(String, [i32; 3])> = state
            .bitwise
            .iter()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        for key in &ready {
            state.bitwise.remove(key);
        }
        ready.into_iter().map(|(_, pos)| pos).collect()
    };
    for pos in batch {
        let Some(current) = block_at(game, pos) else {
            continue;
        };
        let Some(plate) = bitwise(&game.pack.assets.registry, current) else {
            continue;
        };
        ensure_bitwise_entity(game, current, pos);
        let mut inputs = [ChannelPower::NONE; 3];
        for side in plate.gate.sides() {
            let dir = input_direction(plate.attachment, plate.rotation, *side);
            let neighbour_pos = offset(pos, dir);
            let Some(neighbour) = block_at(game, neighbour_pos) else {
                continue;
            };
            let registry = &game.pack.assets.registry;
            if let Some(attached) = cable_attached(registry, neighbour) {
                // The plate queries the cable with side = toward the plate and
                // `wireFace` = the plate's attachment (`SidedPowerSupplier`).
                inputs[side.index()] = cable_channel_power(
                    cable_power_at(game, neighbour_pos),
                    attached,
                    dir.opposite(),
                    plate.attachment,
                );
            } else if let Some(source) = bitwise(registry, neighbour)
                // A bitwise plate supplies only from its output face, and only
                // to a reader sharing its attachment (`getPowerOnChannel`'s
                // `wireFace == attachment`).
                && source.attachment == plate.attachment
                && output_direction(source.attachment, source.rotation) == dir.opposite()
            {
                inputs[side.index()] = bitwise_power_at(game, neighbour_pos);
            }
        }
        let next = bitwise_output(plate.gate, inputs[0], inputs[1], inputs[2]);
        if next == bitwise_power_at(game, pos) {
            continue;
        }
        if !write_bitwise_power(game, pos, next) {
            continue;
        }
        game.native_block_entity_sync(pos);
        // The stored power changed: a cable on the output face must re-seed,
        // and a bitwise plate on that face reads the new value next tick.
        let output_pos = offset(pos, output_direction(plate.attachment, plate.rotation));
        mark_cables_around_game(game, dimension, pos);
        let registry = &game.pack.assets.registry;
        if block_at(game, output_pos).is_some_and(|state| bitwise(registry, state).is_some()) {
            mark_bitwise(dimension, output_pos);
        }
    }
}

/// Apply the queued plates for this dimension.
fn apply_plates(game: &mut crate::game::Game<'_>, dimension: &str) {
    let batch: Vec<([i32; 3], bool)> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let ready: Vec<(String, [i32; 3])> = state
            .dirty
            .iter()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        let mut batch = Vec::with_capacity(ready.len());
        for key in ready {
            let placed = state.placed.remove(&key);
            state.dirty.remove(&key);
            batch.push((key.1, placed));
        }
        batch
    };
    for (pos, placed) in batch {
        let Some(current) = block_at(game, pos) else {
            continue;
        };
        let Some(plate) = plate(&game.pack.assets.registry, current) else {
            continue;
        };
        let mut inputs = plate.inputs;
        for side in plate.gate.sides() {
            let dir = input_direction(plate.attachment, plate.rotation, *side);
            let neighbor_pos = offset(pos, dir);
            let Some(neighbor) = block_at(game, neighbor_pos) else {
                continue;
            };
            inputs[side.index()] = signal_toward(game, neighbor_pos, neighbor, dir) > 0;
        }
        // Upstream schedules its one-tick update only when the *input state*
        // changed (`neighborChanged`), and the tick then moves the stored bit.
        // The port is re-entered by its own neighbour notifications, so a bit
        // may only move on a real input change; otherwise a latch with both
        // inputs high would flip every tick, and a cleared pulse would re-arm.
        let (inputs, powered, clear_in) = match plate.gate {
            Gate::Pulse => {
                let stored_b = plate.inputs[Side::B.index()];
                let world_b = inputs[Side::B.index()];
                let update = pulse_update(stored_b, world_b, plate.powered);
                inputs[Side::B.index()] = update.input_b;
                (inputs, update.powered, update.clear_in)
            }
            Gate::Latch if inputs != plate.inputs => (
                inputs,
                latch_powered(
                    plate.powered,
                    inputs[Side::A.index()],
                    inputs[Side::C.index()],
                ),
                None,
            ),
            _ => (inputs, plate.powered, None),
        };
        let changed = inputs != plate.inputs || powered != plate.powered;
        if !changed {
            if placed {
                game.native_update_signal_neighbors(dimension, pos);
            }
            continue;
        }
        let Some(new) = with_inputs(game, current, plate.gate, inputs, powered) else {
            continue;
        };
        if write(game, dimension, pos, new)
            && let Some(ticks) = clear_in
        {
            let mut guard = state();
            let state = guard.get_or_insert_with(State::default);
            if state.clearing.len() < MAX_DIRTY {
                // Upstream's `willTickThisTick` guard: an already scheduled
                // clear keeps its due tick, so a second edge cannot extend it.
                state
                    .clearing
                    .entry((dimension.to_owned(), pos))
                    .or_insert(ticks);
            }
        }
    }
}

/// Clear the pulse outputs whose two-tick timer is due, before this tick's
/// dirty batch runs. Upstream's `PulseGateBlock.tick` only drops `powered`.
fn clear_due_pulses(game: &mut crate::game::Game<'_>, dimension: &str) {
    let due: Vec<[i32; 3]> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let ready: Vec<(String, [i32; 3])> = state
            .clearing
            .keys()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        let mut due = Vec::with_capacity(ready.len());
        for key in ready {
            let Some(remaining) = state.clearing.get(&key).copied() else {
                continue;
            };
            match pulse_tick(remaining) {
                PulseTick::Waiting(left) => {
                    state.clearing.insert(key, left);
                }
                PulseTick::Cleared => {
                    state.clearing.remove(&key);
                    due.push(key.1);
                }
            }
        }
        due
    };
    for pos in due {
        let Some(current) = block_at(game, pos) else {
            continue;
        };
        let Some(plate) = plate(&game.pack.assets.registry, current) else {
            continue;
        };
        if plate.gate != Gate::Pulse || !plate.powered {
            continue;
        }
        let Some(new) = with_inputs(game, current, Gate::Pulse, plate.inputs, false) else {
            continue;
        };
        write(game, dimension, pos, new);
    }
}

/// Write `new` at `pos` and notify its neighbours. The `writing` sentinel hides
/// the engine's `block_changed` self-report from `on_block_changed`.
fn write(
    game: &mut crate::game::Game<'_>,
    dimension: &str,
    pos: [i32; 3],
    new: BlockStateId,
) -> bool {
    let key = (dimension.to_owned(), pos);
    state()
        .get_or_insert_with(State::default)
        .writing
        .insert(key.clone());
    let wrote = game.native_set_block(dimension, pos, new, flags::UPDATE_ALL);
    // Also drop a sentinel the hook never consumed (a non-authoritative level,
    // or a refused write), so it cannot swallow a later placement.
    state()
        .get_or_insert_with(State::default)
        .writing
        .remove(&key);
    if wrote {
        game.native_update_signal_neighbors(dimension, pos);
    }
    wrote
}

/// A wire's power from its block entity (`WireBlockEntity.POWER`), zero when
/// there is no entity or no saved array yet.
fn wire_power_in(world: &World, pos: [i32; 3]) -> [u8; 6] {
    match world.block_entities.get(&(pos[0], pos[1], pos[2])) {
        Some(BlockEntity::Mod(entity)) => entity_power(entity),
        _ => [0u8; 6],
    }
}

fn entity_power(entity: &ModBlockEntity) -> [u8; 6] {
    let Some(Tag::IntArray(values)) = entity.nbt.get(&JavaString::from(POWER_TAG)) else {
        return [0u8; 6];
    };
    if values.len() != 6 {
        return [0u8; 6];
    }
    let mut power = [0u8; 6];
    for (slot, value) in power.iter_mut().zip(values) {
        *slot = u8::try_from((*value).clamp(0, 31)).unwrap_or(31);
    }
    power
}

fn wire_power_at(game: &crate::game::Game<'_>, pos: [i32; 3]) -> [u8; 6] {
    wire_power_in(&game.world, pos)
}

fn wire_power_at_level(level: &crate::game::GameLevel<'_>, pos: [i32; 3]) -> [u8; 6] {
    wire_power_in(level.world, pos)
}

/// A cable's sixteen-channel power from its block entity, zero when there is
/// no entity or no saved compound yet.
fn cable_power_in(world: &World, pos: [i32; 3]) -> CablePower {
    match world.block_entities.get(&(pos[0], pos[1], pos[2])) {
        Some(BlockEntity::Mod(entity)) => cable_power_of(entity),
        _ => CablePower::NONE,
    }
}

fn cable_power_of(entity: &ModBlockEntity) -> CablePower {
    if entity.id != CABLE_BLOCK {
        return CablePower::NONE;
    }
    let Some(Tag::Compound(faces)) = entity.nbt.get(&JavaString::from(POWER_TAG)) else {
        return CablePower::NONE;
    };
    let mut power = CablePower::NONE;
    for face in Dir::ALL {
        let Some(Tag::IntArray(values)) = faces.get(&JavaString::from(dir_name(face))) else {
            continue;
        };
        for (index, value) in values.iter().take(CHANNELS).enumerate() {
            let Some(channel) = u8::try_from(index).ok().and_then(Channel::new) else {
                continue;
            };
            power.set(face, channel, u8::try_from((*value).clamp(0, 31)).unwrap_or(31));
        }
    }
    power
}

/// The cable's saved per-face per-channel power as an NBT compound
/// (`power.down` is the first face's sixteen values).
fn power_compound(power: CablePower) -> Compound {
    let mut faces = Compound::new();
    for face in Dir::ALL {
        let values = Channel::ALL
            .into_iter()
            .map(|channel| i32::from(power.get(face, channel)))
            .collect();
        faces.insert(JavaString::from(dir_name(face)), Tag::IntArray(values));
    }
    faces
}

fn cable_power_at(game: &crate::game::Game<'_>, pos: [i32; 3]) -> CablePower {
    cable_power_in(&game.world, pos)
}

fn cable_power_at_level(level: &crate::game::GameLevel<'_>, pos: [i32; 3]) -> CablePower {
    cable_power_in(level.world, pos)
}

/// A cable placed through `/setblock` can arrive without its block entity; make
/// one so the channel power has somewhere to live.
fn ensure_cable_entity(game: &mut crate::game::Game<'_>, state: BlockStateId, pos: [i32; 3]) {
    let key = (pos[0], pos[1], pos[2]);
    if game.world.block_entities.get(&key).is_some() {
        return;
    }
    let Some(name) = game.pack.assets.registry.state(state).map(|s| s.name.to_string()) else {
        return;
    };
    let mut entity = ModBlockEntity::default();
    entity.id = name;
    entity.nbt.insert(
        JavaString::from(POWER_TAG),
        Tag::Compound(power_compound(CablePower::NONE)),
    );
    game.world
        .block_entities
        .insert(key, BlockEntity::Mod(Box::new(entity)));
}

fn write_cable_power(game: &mut crate::game::Game<'_>, pos: [i32; 3], power: CablePower) -> bool {
    let compound = power_compound(power);
    game.native_with_mod_block_entity(pos, move |entity| {
        entity
            .nbt
            .insert(JavaString::from(POWER_TAG), Tag::Compound(compound));
    })
    .is_some()
}

/// A bitwise plate's sixteen channel powers from its block entity
/// (`ChanneledPowerStorageBlockEntity.POWER`, a byte array), zero when there is
/// no entity or no saved array yet.
fn bitwise_power_in(world: &World, pos: [i32; 3]) -> ChannelPower {
    match world.block_entities.get(&(pos[0], pos[1], pos[2])) {
        Some(BlockEntity::Mod(entity)) => bitwise_power_of(entity),
        _ => ChannelPower::NONE,
    }
}

fn bitwise_power_of(entity: &ModBlockEntity) -> ChannelPower {
    if BitwiseGate::from_id(&entity.id).is_none() {
        return ChannelPower::NONE;
    }
    let Some(Tag::ByteArray(values)) = entity.nbt.get(&JavaString::from(BITWISE_POWER_TAG)) else {
        return ChannelPower::NONE;
    };
    let mut power = ChannelPower::NONE;
    for (index, value) in values.iter().take(CHANNELS).enumerate() {
        let Some(channel) = u8::try_from(index).ok().and_then(Channel::new) else {
            continue;
        };
        power.set(channel, (*value).min(31));
    }
    power
}

fn bitwise_power_at(game: &crate::game::Game<'_>, pos: [i32; 3]) -> ChannelPower {
    bitwise_power_in(&game.world, pos)
}

/// A bitwise plate placed through `/setblock` can arrive without its block
/// entity (its `end_rod` template has none); make one so the channels have
/// somewhere to live, as `ensure_cable_entity` does for the cable.
fn ensure_bitwise_entity(game: &mut crate::game::Game<'_>, state: BlockStateId, pos: [i32; 3]) {
    let key = (pos[0], pos[1], pos[2]);
    if game.world.block_entities.get(&key).is_some() {
        return;
    }
    let Some(name) = game.pack.assets.registry.state(state).map(|s| s.name.to_string()) else {
        return;
    };
    let mut entity = ModBlockEntity::default();
    entity.id = name;
    entity
        .nbt
        .insert(JavaString::from(BITWISE_POWER_TAG), Tag::ByteArray(vec![0; CHANNELS]));
    game.world
        .block_entities
        .insert(key, BlockEntity::Mod(Box::new(entity)));
}

fn write_bitwise_power(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    power: ChannelPower,
) -> bool {
    let values = Channel::ALL
        .into_iter()
        .map(|channel| power.get(channel))
        .collect();
    game.native_with_mod_block_entity(pos, move |entity| {
        entity
            .nbt
            .insert(JavaString::from(BITWISE_POWER_TAG), Tag::ByteArray(values));
    })
    .is_some()
}

/// Queue the bundled cables beside `pos` for the next channel solve.
fn mark_cables_around_game(game: &crate::game::Game<'_>, dimension: &str, pos: [i32; 3]) {
    let registry = &game.pack.assets.registry;
    for face in Dir::ALL {
        let neighbour_pos = offset(pos, face);
        if block_at(game, neighbour_pos)
            .is_some_and(|state| cable_attached(registry, state).is_some())
        {
            mark_cable(dimension, neighbour_pos);
        }
    }
}

/// A wire placed through `/setblock` can arrive without its block entity; make
/// one so the power has somewhere to live (the engine makes it on placement).
fn ensure_wire_entity(game: &mut crate::game::Game<'_>, state: BlockStateId, pos: [i32; 3]) {
    let key = (pos[0], pos[1], pos[2]);
    if game.world.block_entities.get(&key).is_some() {
        return;
    }
    let Some(name) = game.pack.assets.registry.state(state).map(|s| s.name.to_string()) else {
        return;
    };
    let mut entity = ModBlockEntity::default();
    entity.id = name;
    entity
        .nbt
        .insert(JavaString::from(POWER_TAG), Tag::IntArray(vec![0; 6]));
    game.world
        .block_entities
        .insert(key, BlockEntity::Mod(Box::new(entity)));
}

fn write_wire_power(game: &mut crate::game::Game<'_>, pos: [i32; 3], power: [u8; 6]) -> bool {
    let tag = Tag::IntArray(power.into_iter().map(i32::from).collect());
    game.native_with_mod_block_entity(pos, move |entity| {
        entity
            .nbt
            .insert(JavaString::from(POWER_TAG), tag);
    })
    .is_some()
}

/// A post's saved links (`WirePostBlockEntity.CONNECTIONS`, a list of relative
/// positions), empty when there is no entity or no saved list yet.
fn post_links_in(world: &World, pos: [i32; 3]) -> PostLinks {
    match world.block_entities.get(&(pos[0], pos[1], pos[2])) {
        Some(BlockEntity::Mod(entity)) => post_links_of(entity),
        _ => PostLinks::default(),
    }
}

fn post_links_of(entity: &ModBlockEntity) -> PostLinks {
    if PostKind::from_id(&entity.id).is_none() {
        return PostLinks::default();
    }
    let Some(Tag::List(entries)) = entity.nbt.get(&JavaString::from(CONNECTIONS_TAG)) else {
        return PostLinks::default();
    };
    let offsets = entries
        .iter()
        .filter_map(|entry| match entry {
            Tag::IntArray(values) if values.len() == 3 => {
                Some([values[0], values[1], values[2]])
            }
            _ => None,
        })
        .collect();
    PostLinks::from_offsets(offsets)
}

fn post_links_at(game: &crate::game::Game<'_>, pos: [i32; 3]) -> PostLinks {
    post_links_in(&game.world, pos)
}

/// A post's links as NBT (a list of three-int arrays, as upstream's
/// `BLOCKPOS_LISTER` writes relative positions).
fn links_tag(links: &PostLinks) -> Tag {
    Tag::List(
        links
            .offsets()
            .iter()
            .map(|offset| Tag::IntArray(offset.to_vec()))
            .collect(),
    )
}

fn write_post_links(game: &mut crate::game::Game<'_>, pos: [i32; 3], links: &PostLinks) -> bool {
    let tag = links_tag(links);
    game.native_with_mod_block_entity(pos, move |entity| {
        entity
            .nbt
            .insert(JavaString::from(CONNECTIONS_TAG), tag);
    })
    .is_some()
}

/// A post placed through `/setblock` can arrive without its block entity; make
/// one so the links have somewhere to live (the engine makes it on placement).
fn ensure_post_entity(game: &mut crate::game::Game<'_>, state: BlockStateId, pos: [i32; 3]) {
    let key = (pos[0], pos[1], pos[2]);
    if game.world.block_entities.get(&key).is_some() {
        return;
    }
    let Some(name) = game.pack.assets.registry.state(state).map(|s| s.name.to_string()) else {
        return;
    };
    let mut entity = ModBlockEntity::default();
    entity.id = name;
    entity
        .nbt
        .insert(JavaString::from(CONNECTIONS_TAG), Tag::List(Vec::new()));
    game.world
        .block_entities
        .insert(key, BlockEntity::Mod(Box::new(entity)));
}

/// Whether two posts are connected on `a`'s side (upstream's
/// `hasRemoteConnection`, one direction of the pair).
fn posts_linked(game: &crate::game::Game<'_>, a: [i32; 3], b: [i32; 3]) -> bool {
    post_links_at(game, a).contains([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
}

/// `WirePostBlockEntity.addConnection`: join both posts and queue them.
fn link_posts(
    game: &mut crate::game::Game<'_>,
    dimension: &str,
    a: [i32; 3],
    b: [i32; 3],
) {
    for (from, to) in [(a, b), (b, a)] {
        let Some(state) = block_at(game, from) else {
            continue;
        };
        if post(&game.pack.assets.registry, state).is_none() {
            continue;
        }
        ensure_post_entity(game, state, from);
        let mut links = post_links_at(game, from);
        let offset = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        if links.add(offset) {
            write_post_links(game, from, &links);
            game.native_block_entity_sync(from);
        }
        mark_post(dimension, from);
    }
}

/// `WirePostBlockEntity.removeConnection`: cut both directions and queue both.
fn cut_link(
    game: &mut crate::game::Game<'_>,
    dimension: &str,
    a: [i32; 3],
    b: [i32; 3],
) {
    for (from, to) in [(a, b), (b, a)] {
        let mut links = post_links_at(game, from);
        let offset = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        if links.remove(offset) {
            write_post_links(game, from, &links);
            game.native_block_entity_sync(from);
        }
        mark_post(dimension, from);
    }
}

/// The wire and bundled cable declare a block entity (upstream's
/// `WireBlockEntity` and `BundledCableBlockEntity`).
#[allow(private_interfaces)]
/// A broken wire or cable drops one item per attached subwire, and nothing
/// when no subwire is attached (upstream's `WireCountLootFunction`). The
/// `loot_drops` hook carries the broken block's **state**, so the six
/// booleans are read from it and the loot table's single stack is resized;
/// no block entity or capability is involved.
#[allow(private_interfaces)]
pub fn on_loot_drops(
    game: &mut crate::game::Game<'_>,
    context: &NativeLootDrops<'_>,
    drops: &mut Vec<ItemStack>,
) {
    let Some((_, state)) = context.block else {
        return;
    };
    let Some(block) = game.native_state_block(state) else {
        return;
    };
    if !is_subwire_block(block) {
        return;
    }
    let Some(attached) = subwire_attached(game, state) else {
        return;
    };
    let count = attached.count() as u32;
    for drop in drops.iter_mut().filter(|drop| &*drop.item == block) {
        drop.count = count;
    }
    drops.retain(|drop| drop.count > 0);
}

/// The families that are six subwires in one cell: the red alloy wire, the
/// bundled cable, and the sixteen coloured network cables.
fn is_subwire_block(block: &str) -> bool {
    block == WIRE_BLOCK
        || block == CABLE_BLOCK
        || (block.starts_with("morered:") && block.ends_with("_network_cable"))
}

/// `state`'s six subwire booleans in [`Dir::ALL`] order, or None for a block
/// that doesn't declare them (every plate, post and vanilla block).
fn subwire_attached(game: &crate::game::Game<'_>, state: BlockStateId) -> Option<Attached> {
    let mut bits = [false; 6];
    for (bit, key) in bits
        .iter_mut()
        .zip(["down", "up", "north", "south", "west", "east"])
    {
        *bit = game.native_block_state_property(state, key)? == "true";
    }
    Some(Attached::from_bits(bits))
}

pub fn on_block_entity(block: &str, entity: &mut Option<(String, Compound)>) {
    if block == WIRE_BLOCK
        || CableColor::from_id(block).is_some()
        || block == CABLE_BLOCK
        || BitwiseGate::from_id(block).is_some()
        || PostKind::from_id(block).is_some()
    {
        *entity = Some((block.to_owned(), Compound::new()));
    }
}

/// What clients get of a wire's or cable's block entity: its saved compound.
#[allow(private_interfaces)]
pub fn on_block_entity_update_tag(entity: &ModBlockEntity, tag: &mut Option<Compound>) {
    if BitwiseGate::from_id(&entity.id).is_some() {
        let power = bitwise_power_of(entity);
        let values = Channel::ALL
            .into_iter()
            .map(|channel| power.get(channel))
            .collect();
        let mut compound = Compound::new();
        compound.insert(JavaString::from(BITWISE_POWER_TAG), Tag::ByteArray(values));
        *tag = Some(compound);
        return;
    }
    if entity.id == CABLE_BLOCK {
        let mut compound = Compound::new();
        compound.insert(
            JavaString::from(POWER_TAG),
            Tag::Compound(power_compound(cable_power_of(entity))),
        );
        *tag = Some(compound);
        return;
    }
    if PostKind::from_id(&entity.id).is_some() {
        let mut compound = Compound::new();
        compound.insert(
            JavaString::from(CONNECTIONS_TAG),
            links_tag(&post_links_of(entity)),
        );
        *tag = Some(compound);
        return;
    }
    if entity.id != WIRE_BLOCK && CableColor::from_id(&entity.id).is_none() {
        return;
    }
    let mut compound = Compound::new();
    compound.insert(
        JavaString::from(POWER_TAG),
        Tag::IntArray(entity_power(entity).into_iter().map(i32::from).collect()),
    );
    *tag = Some(compound);
}

/// A post state with its `power` replaced.
fn with_power(
    registry: &BlockRegistry,
    state: BlockStateId,
    power: u8,
) -> Option<BlockStateId> {
    let mut descriptor = registry.state(state)?.clone();
    descriptor
        .properties
        .insert("power".to_owned(), power.min(15).to_string());
    registry.resolve(&descriptor).ok()
}

/// A post state with its attachment (`facing`) replaced.
fn with_facing(
    registry: &BlockRegistry,
    state: BlockStateId,
    attachment: Dir,
) -> Option<BlockStateId> {
    let mut descriptor = registry.state(state)?.clone();
    descriptor
        .properties
        .insert("facing".to_owned(), dir_name(attachment).to_owned());
    registry.resolve(&descriptor).ok()
}

/// A plate state with its attachment (`facing`) and rotation replaced
/// (`getStateForPlacedGatePlate`, `useItemOn`).
fn with_facing_rotation(
    registry: &BlockRegistry,
    state: BlockStateId,
    attachment: Dir,
    rotation: u8,
) -> Option<BlockStateId> {
    let mut descriptor = registry.state(state)?.clone();
    descriptor
        .properties
        .insert("facing".to_owned(), dir_name(attachment).to_owned());
    descriptor
        .properties
        .insert("rotation".to_owned(), (rotation % 4).to_string());
    registry.resolve(&descriptor).ok()
}

/// The plate state with its stored input booleans replaced.
fn with_inputs(
    game: &crate::game::Game<'_>,
    state: BlockStateId,
    gate: Gate,
    inputs: [bool; 3],
    powered: bool,
) -> Option<BlockStateId> {
    let registry = &game.pack.assets.registry;
    let mut descriptor = registry.state(state)?.clone();
    for side in gate.sides() {
        descriptor.properties.insert(
            match side {
                Side::A => "input_a".to_owned(),
                Side::B => "input_b".to_owned(),
                Side::C => "input_c".to_owned(),
            },
            inputs[side.index()].to_string(),
        );
    }
    if matches!(gate, Gate::Latch | Gate::Pulse) {
        descriptor
            .properties
            .insert("powered".to_owned(), powered.to_string());
    }
    registry.resolve(&descriptor).ok()
}

/// The wire state with its six subwires replaced. `facing` is the content
/// file's shape selector, so it follows the first attached face (a wire with
/// several subwires shows the shape of the first one — a content deviation).
fn with_attached(
    registry: &BlockRegistry,
    state: BlockStateId,
    attached: Attached,
) -> Option<BlockStateId> {
    let mut descriptor = registry.state(state)?.clone();
    for (face, set) in Dir::ALL.into_iter().zip(attached.bits()) {
        descriptor
            .properties
            .insert(dir_name(face).to_owned(), set.to_string());
    }
    if let Some(primary) = attached.primary() {
        descriptor
            .properties
            .insert("axis".to_owned(), axis_name(primary).to_owned());
    }
    registry.resolve(&descriptor).ok()
}

/// The block at `pos`, if the level has it loaded.
fn block_at(game: &crate::game::Game<'_>, [x, y, z]: [i32; 3]) -> Option<BlockStateId> {
    let world = &game.world;
    if !(-30_000_000..30_000_000).contains(&x)
        || !(-30_000_000..30_000_000).contains(&z)
        || !(world.shape.min_y..world.shape.max_y()).contains(&y)
        || !world.loaded(x, z)
    {
        return None;
    }
    Some(world.block(x, y, z))
}

/// The block at `pos` as seen from inside a hook that has the level in place.
fn level_block(level: &crate::game::GameLevel<'_>, [x, y, z]: [i32; 3]) -> Option<BlockStateId> {
    let world = &*level.world;
    if !(-30_000_000..30_000_000).contains(&x)
        || !(-30_000_000..30_000_000).contains(&z)
        || !(world.shape.min_y..world.shape.max_y()).contains(&y)
        || !world.loaded(x, z)
    {
        return None;
    }
    Some(world.block(x, y, z))
}

/// `getSignal(neighbour, reader -> neighbour)`, the engine's answer rebuilt
/// from public state (`from_reader` is the reader-to-source direction).
fn signal_toward(
    game: &crate::game::Game<'_>,
    pos: [i32; 3],
    state: BlockStateId,
    from_reader: Dir,
) -> i32 {
    let registry = &game.pack.assets.registry;
    let Some(block) = registry.state(state) else {
        return 0;
    };
    if let Some(gate) = Gate::from_id(&block.name) {
        let Some(plate) = plate(registry, state) else {
            return 0;
        };
        // The source's signal travels source -> reader.
        return gate.signal_with_powered(
            plate.inputs,
            plate.powered,
            plate.attachment,
            plate.rotation,
            from_reader.opposite(),
        );
    }
    if let Some(attached) = wire_attached(registry, state) {
        return wire_signal(wire_power_at(game, pos), attached, from_reader.opposite());
    }
    if let Some(post) = post(registry, state) {
        return post_signal(post.power, post.attachment, post.kind, from_reader.opposite());
    }
    match &*block.name {
        // Upstream reads `getSignal` first, then a wire's POWER > 0.
        "minecraft:redstone_wire" => block
            .properties
            .get("power")
            .and_then(|power| power.parse::<i32>().ok())
            .unwrap_or(0),
        "minecraft:comparator" => {
            if block.properties.get("powered").is_some_and(|v| v == "true")
                && block
                    .properties
                    .get("facing")
                    .and_then(|facing| dir_of(facing))
                    == Some(from_reader.opposite())
            {
                15
            } else {
                0
            }
        }
        _ => {
            let weak = game.res.behaviours().facts(state).weak;
            ((weak >> (4 * from_reader.index() as u32)) & 15) as i32
        }
    }
}
