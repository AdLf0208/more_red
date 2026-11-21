//! Game-side glue for More Red's redstone, migrated to the SDK package (`api = "0.2"`).
//!
//! The pure rules live in [`crate::runtime`] and are unit-tested on their own; this module is the
//! bridge to the game: it reads a block state's properties, keeps the port's per-position work
//! sets and power caches, answers `block_signal`/`is_signal_source`, queues work in
//! `neighbor_changed`/`block_changed`, and applies it at `level_tick_post` where the level can be
//! written.
//!
//! The retired `.hnmod` reached into `crate::game` (`game.world`, `game.pack.assets.registry`,
//! `native_set_block`); every one of those calls is now a documented SDK host or level call. The
//! behaviour is the same one-tick staged model: a plate's inputs are recomputed and written at
//! `level_tick_post`, the wire/cable/post solves run there as a worklist, and the pulse gate's
//! clear counts down in memory.
//!
//! Two things moved because the SDK cannot express them the old way, and both are recorded in
//! `notes.md`:
//!
//! * a block entity's saved compound is no longer reachable from `block_signal` (the hook gets a
//!   `LevelHost`, not a `LevelRef`), so the port keeps the wire/cable/bitwise power **and** the
//!   post links in a cache that the block entity's `block_entity_on_load` seeds from its saved
//!   data and every write updates;
//! * `update_shape` gets no sturdy-face query, so a wire's subwires are pruned by the next solve
//!   through `level_block_face_sturdy` instead of in the shape update itself.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Mutex, MutexGuard, OnceLock};

use mod_api::{
    self, BlockClick, BlockEntityEvent, BlockEntityOnLoadEvent, BlockEntityPart,
    BlockEntityTypeKey, BlockEntityUpdateTagEvent, BlockFace, BlockPart, BlockSignalEvent,
    BlockState, BlockUpdateFlags, Compound, Direction, HostSide, InteractionResult,
    IsSignalSourceEvent, ItemStack, LevelDimension, LevelHost, LevelRef, LevelTicksHost,
    LootDropsEvent, NeighborChangedEvent, NeighbourUpdate, Registrar, ServerHost,
    StateForPlacementEvent, SupportType, Tag, UseItemOnEvent,
};

use crate::runtime::{
    Attached, BitwiseGate, CHANNELS, CableColor, CablePower, Channel, ChannelPower, Dir, Gate,
    POST_LINK_RANGE, PostKind, PostLinks, PulseTick, Side, SpoolClick, bitwise_output,
    cable_channel_power, cable_power, connects, foreign_signal, in_chunk, input_direction,
    latch_powered, output_direction, perpendicular, placement, placement_attached, plate_seed,
    post_power, post_signal, pulse_tick, pulse_update, relay_seed, relay_signal, retained,
    rod_rotation, same_wire_block, shares_attachment, source_power, spool_click, vanilla_source,
    wire_power, wire_signal,
};

/// Entries one tick may apply; a safety budget against runaway feedback.
const MAX_DIRTY: usize = 4096;
/// Wire recomputes one level tick may do (all its wires together).
const MAX_WIRE_STEPS: usize = 16_384;
/// Times one wire may be recomputed in a single level tick.
const MAX_WIRE_VISITS: u8 = 8;
/// The one wire block this port ships (`MoreRed.java`).
const WIRE_BLOCK: &str = "morered:red_alloy_wire";
/// The bundled cable (`MoreRed.java` `BUNDLED_NETWORK_CABLE`).
const CABLE_BLOCK: &str = "morered:bundled_network_cable";
/// The bundled cable relay plate (`MoreRed.java` `BUNDLED_CABLE_RELAY_PLATE`).
const RELAY_BLOCK: &str = "morered:bundled_cable_relay_plate";
/// The block entity's saved power array (`WireBlockEntity.POWER`).
const POWER_TAG: &str = "power";
/// The relay plate's declared channel selector.
const CHANNEL_PROP: &str = "channel";
/// A wire post's saved link offsets (`WirePostBlockEntity.CONNECTIONS`).
const CONNECTIONS_TAG: &str = "connections";
/// The red-alloy wire spool (`MoreRed.java` `REDWIRE_SPOOL`).
const SPOOL_ITEM: &str = "morered:redwire_spool";
/// The wooden rods a plate turns for (`Tags.Items.RODS_WOODEN`; the port ships
/// the tag with `minecraft:stick`).
const ROD_TAG: &str = "c:rods/wooden";

/// The three block entity types this port declares.
static WIRE_TYPE: OnceLock<BlockEntityTypeKey> = OnceLock::new();
static CABLE_TYPE: OnceLock<BlockEntityTypeKey> = OnceLock::new();
static BITWISE_TYPE: OnceLock<BlockEntityTypeKey> = OnceLock::new();
static POST_TYPE: OnceLock<BlockEntityTypeKey> = OnceLock::new();

/// The port's cross-hook state. `Hooks` and Parts are `Send + Sync`, and a Part is
/// constructed by `register` once, so the shared sets live behind this lock (never a
/// `RefCell`).
#[derive(Default)]
struct State {
    /// Plates queued for the next level tick.
    dirty: BTreeSet<(String, [i32; 3])>,
    /// Bitwise plates queued for the next level tick.
    bitwise: BTreeSet<(String, [i32; 3])>,
    /// Wires queued for the next power solve.
    wires: BTreeSet<(String, [i32; 3])>,
    /// Queued wires whose neighbour changed, so `updateShape`'s subwire prune applies to them.
    /// A state written by `/setblock` is not a neighbour change: upstream keeps a forced
    /// subwire until one of its neighbours changes.
    wires_prune: BTreeSet<(String, [i32; 3])>,
    /// Bundled cables queued for the next channel solve.
    cables: BTreeSet<(String, [i32; 3])>,
    /// Plates placed/changed this tick: notify their neighbours even when the
    /// stored inputs did not change (upstream's `onPlace` does).
    placed: BTreeSet<(String, [i32; 3])>,
    /// Positions this port is writing right now. The engine reports the write
    /// as `block_changed`; that is a state refinement, not a placement, so it
    /// must not seed another evaluation.
    writing: BTreeSet<(String, [i32; 3])>,
    /// Armed pulse gates: ticks left before their output clears.
    clearing: BTreeMap<(String, [i32; 3]), u8>,
    /// Wire posts queued for the next power solve.
    posts: BTreeSet<(String, [i32; 3])>,
    /// Relay plates queued so their adjacent cables can be found at the tick.
    relays: BTreeSet<(String, [i32; 3])>,
    /// The first post each player's spool has selected (`spooled_post` on the
    /// Java item stack; the SDK exposes no item-component write to mods).
    spooled: BTreeMap<u128, (String, [i32; 3])>,
    /// Wires' per-face power, seeded from their block entity on load.
    wire_power: BTreeMap<(String, [i32; 3]), [u8; 6]>,
    /// Bundled cables' per-face per-channel power, seeded the same way.
    cable_power: BTreeMap<(String, [i32; 3]), CablePower>,
    /// Bitwise plates' channel power, seeded the same way.
    bitwise_power: BTreeMap<(String, [i32; 3]), ChannelPower>,
    /// Posts' links, seeded the same way (`block_changed` cannot read the block
    /// entity of a post that is being removed).
    post_links: BTreeMap<(String, [i32; 3]), PostLinks>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = state();
    f(guard.get_or_insert_with(State::default))
}

// ---------------------------------------------------------------------------
// State text: the canonical `/setblock` spelling carries the block id and every
// property, which is all the retired glue read from the registry.
// ---------------------------------------------------------------------------

/// A block state as its canonical text describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct View {
    name: String,
    properties: BTreeMap<String, String>,
}

/// Split `morered:and_gate[facing=down,rotation=0]` into its id and properties. A
/// state without properties is just the id.
fn parse_view(text: &str) -> View {
    let (name, properties) = text.split_once('[').unwrap_or((text, ""));
    let properties = properties.strip_suffix(']').unwrap_or(properties);
    let mut map = BTreeMap::new();
    for pair in properties.split(',') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        map.insert(key.to_owned(), value.to_owned());
    }
    View {
        name: name.to_owned(),
        properties: map,
    }
}

impl View {
    fn property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }

    fn flag(&self, key: &str) -> Option<bool> {
        self.property(key).map(|value| value == "true")
    }

    fn int(&self, key: &str) -> Option<u8> {
        self.property(key)?.parse::<u8>().ok()
    }

    /// The state's canonical text with `updates` applied (other properties kept).
    fn text_with(&self, updates: &[(&str, &str)]) -> String {
        let mut properties = self.properties.clone();
        for (key, value) in updates {
            properties.insert((*key).to_owned(), (*value).to_owned());
        }
        if properties.is_empty() {
            return self.name.clone();
        }
        let joined = properties
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{}[{joined}]", self.name)
    }

    /// The properties that differ from `old`, in name order: the property writes a
    /// whole-state write becomes when the block entity must survive it.
    fn changed_properties(&self, old: &View) -> Vec<(String, String)> {
        let mut changed = Vec::new();
        for (key, value) in &self.properties {
            if old.properties.get(key) != Some(value) {
                changed.push((key.clone(), value.clone()));
            }
        }
        changed
    }
}

fn level_view(level: &dyn LevelHost, state: BlockState) -> Option<View> {
    level.state_string(state).map(|text| parse_view(&text))
}

fn host_view(host: &dyn ServerHost, state: BlockState) -> Option<View> {
    host.block_state_string(state).map(|text| parse_view(&text))
}

/// A state of `state`'s block with the given properties changed, built through
/// the level read (no world write).
fn state_with_level(
    level: &dyn LevelHost,
    state: BlockState,
    updates: &[(&str, &str)],
) -> Option<BlockState> {
    let mut next = state;
    for (key, value) in updates {
        next = level.state_with_property(next, key, value)?;
    }
    Some(next)
}

// ---------------------------------------------------------------------------
// Directions and dimensions
// ---------------------------------------------------------------------------

fn to_dir(dir: Direction) -> Dir {
    match dir {
        Direction::Down => Dir::Down,
        Direction::Up => Dir::Up,
        Direction::North => Dir::North,
        Direction::South => Dir::South,
        Direction::West => Dir::West,
        Direction::East => Dir::East,
    }
}

fn from_dir(dir: Dir) -> Direction {
    match dir {
        Dir::Down => Direction::Down,
        Dir::Up => Direction::Up,
        Dir::North => Direction::North,
        Dir::South => Direction::South,
        Dir::West => Direction::West,
        Dir::East => Direction::East,
    }
}

fn block_face(dir: Dir) -> BlockFace {
    match dir {
        Dir::Down => BlockFace::Down,
        Dir::Up => BlockFace::Up,
        Dir::North => BlockFace::North,
        Dir::South => BlockFace::South,
        Dir::West => BlockFace::West,
        Dir::East => BlockFace::East,
    }
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

fn dimension_name(dimension: LevelDimension) -> &'static str {
    dimension.key().id()
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

// ---------------------------------------------------------------------------
// Saved-compound codecs (pure; the block entity's data is these tags)
// ---------------------------------------------------------------------------

/// `key`'s tag in a compound, with Java's UTF-16 key spelling.
fn get_tag<'a>(nbt: &'a Compound, key: &str) -> Option<&'a Tag> {
    let units: Vec<u16> = key.encode_utf16().collect();
    nbt.iter()
        .find(|(name, _)| name.0 == units)
        .map(|(_, tag)| tag)
}

fn encode_wire_power(power: [u8; 6]) -> Compound {
    let mut nbt = Compound::new();
    nbt.insert(
        POWER_TAG.into(),
        Tag::IntArray(power.into_iter().map(i32::from).collect()),
    );
    nbt
}

fn decode_wire_power(nbt: &Compound) -> [u8; 6] {
    let Some(Tag::IntArray(values)) = get_tag(nbt, POWER_TAG) else {
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

fn power_compound(power: CablePower) -> Compound {
    let mut faces = Compound::new();
    for face in Dir::ALL {
        let values = Channel::ALL
            .into_iter()
            .map(|channel| i32::from(power.get(face, channel)))
            .collect();
        faces.insert(dir_name(face).into(), Tag::IntArray(values));
    }
    faces
}

fn encode_cable_power(power: CablePower) -> Compound {
    let mut nbt = Compound::new();
    nbt.insert(POWER_TAG.into(), Tag::Compound(power_compound(power)));
    nbt
}

fn decode_cable_power(nbt: &Compound) -> CablePower {
    let Some(Tag::Compound(faces)) = get_tag(nbt, POWER_TAG) else {
        return CablePower::NONE;
    };
    let mut power = CablePower::NONE;
    for face in Dir::ALL {
        let Some(Tag::IntArray(values)) = get_tag(faces, dir_name(face)) else {
            continue;
        };
        for (index, value) in values.iter().take(CHANNELS).enumerate() {
            let Some(channel) = u8::try_from(index).ok().and_then(Channel::new) else {
                continue;
            };
            power.set(
                face,
                channel,
                u8::try_from((*value).clamp(0, 31)).unwrap_or(31),
            );
        }
    }
    power
}

fn encode_channel_power(power: ChannelPower) -> Compound {
    let values = Channel::ALL
        .into_iter()
        .map(|channel| power.get(channel))
        .collect();
    let mut nbt = Compound::new();
    nbt.insert(POWER_TAG.into(), Tag::ByteArray(values));
    nbt
}

fn decode_channel_power(nbt: &Compound) -> ChannelPower {
    let Some(Tag::ByteArray(values)) = get_tag(nbt, POWER_TAG) else {
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

fn encode_post_links(links: &PostLinks) -> Compound {
    let entries = links
        .offsets()
        .iter()
        .map(|offset| Tag::IntArray(offset.to_vec()))
        .collect();
    let mut nbt = Compound::new();
    nbt.insert(CONNECTIONS_TAG.into(), Tag::List(entries));
    nbt
}

fn decode_post_links(nbt: &Compound) -> PostLinks {
    let Some(Tag::List(entries)) = get_tag(nbt, CONNECTIONS_TAG) else {
        return PostLinks::default();
    };
    PostLinks::from_offsets(
        entries
            .iter()
            .filter_map(|entry| match entry {
                Tag::IntArray(values) if values.len() == 3 => {
                    Some([values[0], values[1], values[2]])
                }
                _ => None,
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Reading the port's blocks out of a state
// ---------------------------------------------------------------------------

/// A plate read from its state.
struct Plate {
    gate: Gate,
    attachment: Dir,
    rotation: u8,
    inputs: [bool; 3],
    powered: bool,
}

fn plate_of(view: &View) -> Option<Plate> {
    let gate = Gate::from_id(&view.name)?;
    let attachment = dir_of(view.property("facing")?)?;
    let rotation = view.int("rotation")? % 4;
    let input = |key: &str| view.flag(key).is_some_and(|flag| flag);
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
fn wire_attached_of(view: &View) -> Option<Attached> {
    if view.name != WIRE_BLOCK && CableColor::from_id(&view.name).is_none() {
        return None;
    }
    let bit = |key: &str| view.flag(key).is_some_and(|flag| flag);
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
fn wire_colour_of(view: &View) -> Option<Option<CableColor>> {
    if view.name == WIRE_BLOCK {
        Some(None)
    } else {
        CableColor::from_id(&view.name).map(Some)
    }
}

/// A bundled cable's subwires from its state, or `None` for another block.
fn cable_attached_of(view: &View) -> Option<Attached> {
    if view.name != CABLE_BLOCK {
        return None;
    }
    let bit = |key: &str| view.flag(key).is_some_and(|flag| flag);
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
fn attached_of(view: &View) -> Option<Attached> {
    wire_attached_of(view).or_else(|| cable_attached_of(view))
}

/// A bundled cable relay plate read from its state.
struct Relay {
    channel: Channel,
}

fn relay_of(view: &View) -> Option<Relay> {
    if view.name != RELAY_BLOCK {
        return None;
    }
    let channel = Channel::from_property(view.property(CHANNEL_PROP)?)?;
    Some(Relay { channel })
}

/// A bitwise logic plate read from its state (`facing` + `rotation` only).
struct Bitwise {
    gate: BitwiseGate,
    attachment: Dir,
    rotation: u8,
}

fn bitwise_of(view: &View) -> Option<Bitwise> {
    let gate = BitwiseGate::from_id(&view.name)?;
    let attachment = dir_of(view.property("facing")?)?;
    let rotation = view.int("rotation")? % 4;
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

fn post_of(view: &View) -> Option<Post> {
    let kind = PostKind::from_id(&view.name)?;
    let attachment = dir_of(view.property("facing")?)?;
    let power = view.int("power")?.min(15);
    Some(Post {
        kind,
        attachment,
        power,
    })
}

/// Whether a block id is one of the families whose saved compound is a block entity
/// this port declares. Those states are written property by property so the entity
/// survives (`LevelRef::set_block` drops it).
fn has_block_entity(name: &str) -> bool {
    name == WIRE_BLOCK
        || name == CABLE_BLOCK
        || CableColor::from_id(name).is_some()
        || BitwiseGate::from_id(name).is_some()
        || PostKind::from_id(name).is_some()
}

/// The families that are six subwires in one cell: the red alloy wire, the
/// bundled cable, and the sixteen coloured network cables.
fn is_subwire_block(name: &str) -> bool {
    name == WIRE_BLOCK || name == CABLE_BLOCK || CableColor::from_id(name).is_some()
}

// ---------------------------------------------------------------------------
// Queueing
// ---------------------------------------------------------------------------

fn mark(dimension: &str, pos: [i32; 3], placed: bool) {
    with_state(|state| {
        if state.dirty.len() < MAX_DIRTY {
            let key = (dimension.to_owned(), pos);
            state.dirty.insert(key.clone());
            if placed {
                state.placed.insert(key);
            }
        }
    });
}

fn mark_wire(dimension: &str, pos: [i32; 3]) {
    with_state(|state| {
        if state.wires.len() < MAX_DIRTY {
            state.wires.insert((dimension.to_owned(), pos));
        }
    });
}

/// Queue a wire whose **neighbour** changed: the next solve also applies
/// `AbstractWireBlock.updateShape`'s subwire prune. A state written at the wire's own
/// position (`/setblock`, a piston) does not arm this — upstream re-evaluates a block's
/// shape only when a neighbour changes, so a forced state stays until then.
fn mark_wire_prune(dimension: &str, pos: [i32; 3]) {
    with_state(|state| {
        let key = (dimension.to_owned(), pos);
        if state.wires.len() < MAX_DIRTY {
            state.wires.insert(key.clone());
        }
        if state.wires_prune.len() < MAX_DIRTY {
            state.wires_prune.insert(key);
        }
    });
}

fn mark_cable(dimension: &str, pos: [i32; 3]) {
    with_state(|state| {
        if state.cables.len() < MAX_DIRTY {
            state.cables.insert((dimension.to_owned(), pos));
        }
    });
}

fn mark_bitwise(dimension: &str, pos: [i32; 3]) {
    with_state(|state| {
        if state.bitwise.len() < MAX_DIRTY {
            state.bitwise.insert((dimension.to_owned(), pos));
        }
    });
}

fn mark_post(dimension: &str, pos: [i32; 3]) {
    with_state(|state| {
        if state.posts.len() < MAX_DIRTY {
            state.posts.insert((dimension.to_owned(), pos));
        }
    });
}

/// Queue every bundled cable beside `pos`: a relay's vanilla input is read by the
/// cable's channel solve on its next pass.
fn mark_cables_around(host: &dyn ServerHost, level: LevelRef, dimension: &str, pos: [i32; 3]) {
    for face in Dir::ALL {
        let neighbour_pos = offset(pos, face);
        let Some(state) = block_state(host, level, neighbour_pos) else {
            continue;
        };
        if host_view(host, state).is_some_and(|view| cable_attached_of(&view).is_some()) {
            mark_cable(dimension, neighbour_pos);
        }
    }
}

// ---------------------------------------------------------------------------
// Level reads and writes
// ---------------------------------------------------------------------------

/// The state at `pos`, or `None` when the position is not loaded.
fn block_state(host: &dyn ServerHost, level: LevelRef, pos: [i32; 3]) -> Option<BlockState> {
    if !level.is_loaded(host, pos) {
        return None;
    }
    level.block_state(host, pos)
}

/// Write `new` over `old` at `pos`, keeping the block entity alive. A state of a
/// declared block entity is written one property at a time
/// (`set_block_property` keeps the entity; `set_block` removes it), a plate
/// without one is a single state write. The `writing` sentinel hides the
/// engine's `block_changed` self-report from [`on_block_changed`].
fn write_state(
    host: &mut dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    old: BlockState,
    new: BlockState,
) -> bool {
    let key = (dimension.to_owned(), pos);
    with_state(|state| {
        state.writing.insert(key.clone());
    });
    let wrote = match (host_view(host, old), host_view(host, new)) {
        (Some(old_view), Some(new_view)) if has_block_entity(&new_view.name) => new_view
            .changed_properties(&old_view)
            .iter()
            .any(|(property, value)| host.set_block_property(pos, property, value)),
        _ => level
            .set_block(
                host,
                pos,
                new,
                BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS,
            )
            .unwrap_or(false),
    };
    with_state(|state| {
        state.writing.remove(&key);
    });
    if wrote {
        let _ = level.update_neighbors(host, pos, new, NeighbourUpdate::Neighbours);
    }
    wrote
}

/// The block entity at `pos`'s saved compound.
fn entity_data(host: &dyn ServerHost, level: LevelRef, pos: [i32; 3]) -> Option<Compound> {
    host.block_entity(level, pos)?.data(host)
}

fn merge_entity(host: &mut dyn ServerHost, level: LevelRef, pos: [i32; 3], data: Compound) -> bool {
    host.block_entity(level, pos)
        .is_some_and(|entity| entity.merge_data(host, data).unwrap_or(false))
}

fn key_of(dimension: &str, pos: [i32; 3]) -> (String, [i32; 3]) {
    (dimension.to_owned(), pos)
}

/// A wire's stored power: the cache (seeded by `block_entity_on_load`) or its block
/// entity, which the cache then remembers.
fn wire_power_at(
    host: &dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
) -> [u8; 6] {
    if let Some(power) = with_state(|state| state.wire_power.get(&key_of(dimension, pos)).copied())
    {
        return power;
    }
    let power = entity_data(host, level, pos)
        .map(|data| decode_wire_power(&data))
        .unwrap_or([0u8; 6]);
    with_state(|state| {
        state.wire_power.insert(key_of(dimension, pos), power);
    });
    power
}

fn write_wire_power(
    host: &mut dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    power: [u8; 6],
) -> bool {
    with_state(|state| {
        state.wire_power.insert(key_of(dimension, pos), power);
    });
    merge_entity(host, level, pos, encode_wire_power(power))
}

fn cable_power_at(
    host: &dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
) -> CablePower {
    if let Some(power) = with_state(|state| state.cable_power.get(&key_of(dimension, pos)).copied())
    {
        return power;
    }
    let power = entity_data(host, level, pos)
        .map(|data| decode_cable_power(&data))
        .unwrap_or(CablePower::NONE);
    with_state(|state| {
        state.cable_power.insert(key_of(dimension, pos), power);
    });
    power
}

fn write_cable_power(
    host: &mut dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    power: CablePower,
) -> bool {
    with_state(|state| {
        state.cable_power.insert(key_of(dimension, pos), power);
    });
    merge_entity(host, level, pos, encode_cable_power(power))
}

fn bitwise_power_at(
    host: &dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
) -> ChannelPower {
    if let Some(power) =
        with_state(|state| state.bitwise_power.get(&key_of(dimension, pos)).copied())
    {
        return power;
    }
    let power = entity_data(host, level, pos)
        .map(|data| decode_channel_power(&data))
        .unwrap_or(ChannelPower::NONE);
    with_state(|state| {
        state.bitwise_power.insert(key_of(dimension, pos), power);
    });
    power
}

fn write_bitwise_power(
    host: &mut dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    power: ChannelPower,
) -> bool {
    with_state(|state| {
        state.bitwise_power.insert(key_of(dimension, pos), power);
    });
    merge_entity(host, level, pos, encode_channel_power(power))
}

fn post_links_at(
    host: &dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
) -> PostLinks {
    if let Some(links) = with_state(|state| state.post_links.get(&key_of(dimension, pos)).cloned())
    {
        return links;
    }
    let links = entity_data(host, level, pos)
        .map(|data| decode_post_links(&data))
        .unwrap_or_default();
    with_state(|state| {
        state
            .post_links
            .insert(key_of(dimension, pos), links.clone());
    });
    links
}

fn write_post_links(
    host: &mut dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    links: &PostLinks,
) -> bool {
    with_state(|state| {
        state
            .post_links
            .insert(key_of(dimension, pos), links.clone());
    });
    merge_entity(host, level, pos, encode_post_links(links))
}

// ---------------------------------------------------------------------------
// Building the next state
// ---------------------------------------------------------------------------

fn state_with_host(
    host: &dyn ServerHost,
    state: BlockState,
    updates: &[(&str, &str)],
) -> Option<BlockState> {
    let view = host_view(host, state)?;
    host.block_state_id(&view.text_with(updates))
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// A post state with its `power` replaced.
fn with_power(host: &dyn ServerHost, state: BlockState, power: u8) -> Option<BlockState> {
    state_with_host(host, state, &[("power", &power.min(15).to_string())])
}

/// A post state with its attachment (`facing`) replaced.
fn with_facing(level: &dyn LevelHost, state: BlockState, attachment: Dir) -> Option<BlockState> {
    state_with_level(level, state, &[("facing", dir_name(attachment))])
}

/// A plate state with its attachment (`facing`) and rotation replaced
/// (`getStateForPlacedGatePlate`, `useItemOn`).
fn with_facing_rotation(
    level: &dyn LevelHost,
    state: BlockState,
    attachment: Dir,
    rotation: u8,
) -> Option<BlockState> {
    state_with_level(
        level,
        state,
        &[
            ("facing", dir_name(attachment)),
            ("rotation", &(rotation % 4).to_string()),
        ],
    )
}

/// [`with_facing_rotation`] for a write context (the host builds the state text).
fn with_facing_rotation_host(
    host: &dyn ServerHost,
    state: BlockState,
    attachment: Dir,
    rotation: u8,
) -> Option<BlockState> {
    state_with_host(
        host,
        state,
        &[
            ("facing", dir_name(attachment)),
            ("rotation", &(rotation % 4).to_string()),
        ],
    )
}

/// The plate state with its stored input booleans (and `powered`, where the gate
/// stores one) replaced.
fn with_inputs(
    host: &dyn ServerHost,
    state: BlockState,
    gate: Gate,
    inputs: [bool; 3],
    powered: bool,
) -> Option<BlockState> {
    let mut updates: Vec<(&str, &str)> = Vec::new();
    for side in gate.sides() {
        let key = match side {
            Side::A => "input_a",
            Side::B => "input_b",
            Side::C => "input_c",
        };
        updates.push((key, bool_text(inputs[side.index()])));
    }
    if matches!(gate, Gate::Latch | Gate::Pulse) {
        updates.push(("powered", bool_text(powered)));
    }
    state_with_host(host, state, &updates)
}

/// The wire state with its six subwires replaced. `axis` is the content file's
/// shape selector, so it follows the first attached face (a wire with several
/// subwires shows the shape of the first one — a content deviation).
fn with_attached(
    host: &dyn ServerHost,
    state: BlockState,
    attached: Attached,
) -> Option<BlockState> {
    let mut updates: Vec<(&str, &str)> = Dir::ALL
        .into_iter()
        .zip(attached.bits())
        .map(|(face, set)| (dir_name(face), bool_text(set)))
        .collect();
    if let Some(primary) = attached.primary() {
        updates.push(("axis", axis_name(primary)));
    }
    state_with_host(host, state, &updates)
}

// ---------------------------------------------------------------------------
// The signal a neighbour gives: the engine's own `getSignal`
// ---------------------------------------------------------------------------

/// The signal `pos` gives toward a reader at `from_reader` (the reader-to-source
/// direction): the engine's `getSignal(pos, source-to-reader)` for a foreign block, and
/// this port's own answer for its own blocks.
fn signal_toward(host: &dyn ServerHost, level: LevelRef, pos: [i32; 3], from_reader: Dir) -> i32 {
    let toward_reader = from_reader.opposite();
    let Some(state) = block_state(host, level, pos) else {
        return 0;
    };
    let Some(view) = host_view(host, state) else {
        return 0;
    };
    // A plate of this port is directional and answers for the face a query names, while the
    // engine's own query passes the reading side: the two conventions disagree for a directional
    // block (a lamp saw a pulse gate's output while the latch beside it did not), so a plate is
    // read from its own state, toward the side this reader is on.
    if let Some(plate) = plate_of(&view) {
        return plate.gate.signal_with_powered(
            plate.inputs,
            plate.powered,
            plate.attachment,
            plate.rotation,
            toward_reader,
        );
    }
    if let Some(attached) = wire_attached_of(&view) {
        // The wire's per-face power is the same cache its `block_signal` answers from.
        let dimension = level.dimension().id(host).unwrap_or_default();
        let power = with_state(|state| state.wire_power.get(&key_of(&dimension, pos)).copied())
            .unwrap_or([0u8; 6]);
        return wire_signal(power, attached, toward_reader);
    }
    // The bundled cable and a bitwise plate answer no vanilla signal at all (upstream
    // `AbstractWireBlock` / `BitwiseLogicPlateBlock`), and the cable's basalt template would
    // otherwise conduct its neighbours' power through the engine's aggregate.
    if cable_attached_of(&view).is_some() || bitwise_of(&view).is_some() {
        return 0;
    }
    // A foreign block: the engine's `signal` includes the power it conducts, so a lit lamp
    // (a sink) answers the power it receives and would hold a wire up forever. Only a named
    // source — or an emitter whose two answers differ — gives its `signal`; a block that
    // reports nothing but what it receives (`direct_signal_to`) is a conductor and gives 0.
    //
    // `LevelRef::signal` takes the **reader-to-source** direction ("as a block reads its
    // neighbour `pos` that lies `direction` from it", `SignalGetter.getSignal`; the level's
    // own `has_neighbor_signal` calls `signal(rel(pos, d), d)`), so a directional block is
    // asked for the side this reader is on with `from_reader`, not its opposite: a repeater
    // `facing=west` answers a reader on its east through `signal(pos, West)`.
    let plain = level.signal(host, pos, from_dir(from_reader)).unwrap_or(0);
    let conducted = level.direct_signal_to(host, pos).unwrap_or(0);
    let known_source = vanilla_source(&view.name) || own_signal_source(&view);
    foreign_signal(plain, conducted, known_source)
}

/// Whether a block of this port answers a redstone signal of its own: the relay plate and the
/// wire posts. The wire and the gate plates are read from their own state above; the bundled
/// cable and the bitwise plates answer nothing.
fn own_signal_source(view: &View) -> bool {
    relay_of(view).is_some() || post_of(view).is_some()
}

// ---------------------------------------------------------------------------
// Parts attached to the port's blocks
// ---------------------------------------------------------------------------

/// The per-block behaviour of every More Red block: its signal answer, its source
/// flag, neighbour-change queueing and placement state.
struct RedstonePart;

#[mod_api::horizon_part]
impl BlockPart for RedstonePart {
    /// `block_signal`: answer from the wire's cached per-face power, or from a
    /// plate's stored state. The hook's `face` is the direction the signal travels
    /// (the dispatcher flips vanilla's side), so a wire answers for the subwire
    /// toward the reader.
    fn block_signal(&self, level: &dyn LevelHost, event: &mut BlockSignalEvent) {
        let Some(view) = level_view(level, event.state) else {
            return;
        };
        let dimension = dimension_name(event.dimension);
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        if let Some(attached) = wire_attached_of(&view) {
            let power = with_state(|state| state.wire_power.get(&key_of(dimension, pos)).copied())
                .unwrap_or([0u8; 6]);
            event.signal = wire_signal(power, attached, to_dir(event.face));
            return;
        }
        if cable_attached_of(&view).is_some() {
            // Upstream's bundled cable is not a redstone signal source: only a
            // relay plate answers a vanilla reader. The block's basalt template
            // still counts as a solid conductor, so both the weak and the
            // conducted (direct) answer are forced to zero.
            event.signal = 0;
            return;
        }
        if let Some(relay) = relay_of(&view) {
            // A relay is a weak source only: a direct answer would conduct
            // through the solid cable beside it and light a lamp above the cable.
            event.signal = if event.direct {
                0
            } else {
                relay_output_level(level, dimension, pos, relay.channel)
            };
            return;
        }
        if bitwise_of(&view).is_some() {
            // `BitwiseLogicPlateBlock` extends `PlateBlock`, not
            // `RedstonePlateBlock`: it has no vanilla signal; only a cable reads it.
            event.signal = 0;
            return;
        }
        if let Some(post) = post_of(&view) {
            // `AbstractPoweredWirePostBlock.getSignal`: the stored POWER on a
            // connectable face, and `getDirectSignal` answers the same.
            event.signal = post_signal(post.power, post.attachment, post.kind, to_dir(event.face));
            return;
        }
        let Some(plate) = plate_of(&view) else {
            return;
        };
        event.signal = plate.gate.signal_with_powered(
            plate.inputs,
            plate.powered,
            plate.attachment,
            plate.rotation,
            to_dir(event.face),
        );
    }

    /// `is_signal_source`: every plate is one, a wire with at least one subwire is
    /// one (upstream's `!isEmptyWireBlock`), and a relay plate is one. The bundled
    /// cable itself is not (upstream inherits `AbstractWireBlock`'s default).
    fn is_signal_source(&self, level: &dyn LevelHost, event: &mut IsSignalSourceEvent) {
        let Some(view) = level_view(level, event.state) else {
            return;
        };
        let wire = wire_attached_of(&view).is_some_and(|attached| !attached.is_empty());
        if wire
            || relay_of(&view).is_some()
            || plate_of(&view).is_some()
            || post_of(&view).is_some()
        {
            event.source = true;
        }
    }

    /// A neighbour changed: queue the plate for the next level tick, the wire for
    /// the next power solve, or the relay so the cables beside it are queued.
    fn neighbor_changed(&self, level: &mut dyn LevelTicksHost, event: &mut NeighborChangedEvent) {
        let Some(view) = level_view(level, event.state) else {
            return;
        };
        let dimension = dimension_name(event.dimension);
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        if wire_attached_of(&view).is_some() {
            mark_wire_prune(dimension, pos);
        } else if cable_attached_of(&view).is_some() {
            mark_cable(dimension, pos);
        } else if relay_of(&view).is_some() {
            // The relay's adjacent cables need the level to be found; the tick
            // does that once it has a `LevelRef`.
            with_state(|state| {
                state.relays.insert((dimension.to_owned(), pos));
            });
        } else if plate_of(&view).is_some() {
            mark(dimension, pos, false);
        } else if bitwise_of(&view).is_some() {
            mark_bitwise(dimension, pos);
        } else if post_of(&view).is_some() {
            mark_post(dimension, pos);
        }
    }

    /// `getStateForPlacement`: a plate attaches to the clicked face's opposite and
    /// turns its output toward the clicked quadrant; a post attaches to the
    /// clicked face's opposite; a fresh wire attaches to the face it was clicked
    /// against.
    fn state_for_placement(
        &self,
        level: &dyn LevelHost,
        event: &mut StateForPlacementEvent<'_>,
    ) -> Option<Option<BlockState>> {
        let view = level_view(level, event.state)?;
        let clicked = to_dir(event.context.face);
        if post_of(&view).is_some() {
            // `AbstractPostBlock.getStateForPlacement`: attach to the clicked
            // face's opposite. Power starts at zero and the first solve fills it.
            return with_facing(level, event.state, clicked.opposite()).map(Some);
        }
        if plate_of(&view).is_some() || bitwise_of(&view).is_some() {
            // `PlateBlockStateProperties.getStateForPlacedGatePlate`: attach to
            // the clicked face's opposite and point the output at the quadrant of
            // the face the player clicked (`BlockStateUtil`), not at the yaw.
            let hit = [
                event.context.hit[0] - f64::from(event.context.pos.x),
                event.context.hit[1] - f64::from(event.context.pos.y),
                event.context.hit[2] - f64::from(event.context.pos.z),
            ];
            let (attachment, rotation) = placement(clicked, hit);
            return with_facing_rotation(level, event.state, attachment, rotation).map(Some);
        }
        attached_of(&view)?;
        with_attached_host(level, event.state, placement_attached(clicked)).map(Some)
    }
}

/// [`with_attached`] for the level-read side of placement (no `ServerHost`).
fn with_attached_host(
    level: &dyn LevelHost,
    state: BlockState,
    attached: Attached,
) -> Option<BlockState> {
    let mut updates: Vec<(&'static str, &'static str)> = Dir::ALL
        .into_iter()
        .zip(attached.bits())
        .map(|(face, set)| (dir_name(face), bool_text(set)))
        .collect();
    if let Some(primary) = attached.primary() {
        updates.push(("axis", axis_name(primary)));
    }
    state_with_level(level, state, &updates)
}

/// A click on one of the port's blocks: the spool's link/cut and the wooden rod's
/// quarter turn (`WireSpoolItem.onUseOnPost`, `RedstonePlateBlock.useItemOn`).
struct ClickPart;

#[mod_api::horizon_part]
impl BlockPart for ClickPart {
    fn use_item_on(
        &self,
        host: &mut dyn ServerHost,
        event: &mut UseItemOnEvent<'_>,
    ) -> Option<InteractionResult> {
        let stack = event.click.stack?;
        let acted = if stack.item().id(&*host) == SPOOL_ITEM {
            spool_click_at(host, event.click, stack)
        } else {
            rotate_with_rod(host, event.click, stack)
        };
        acted.then_some(InteractionResult::Success)
    }
}

// ---------------------------------------------------------------------------
// The spool and the wooden rod
// ---------------------------------------------------------------------------

fn spool_click_at(host: &mut dyn ServerHost, click: &BlockClick<'_>, _stack: &ItemStack) -> bool {
    let dimension = dimension_name(click.dimension);
    let pos = click.pos;
    let Some(level) = host.level(dimension) else {
        return false;
    };
    let Some(state) = block_state(host, level, pos) else {
        return false;
    };
    if host_view(host, state).is_none_or(|view| post_of(&view).is_none()) {
        return false;
    }
    let uuid = click.player;
    let stored = with_state(|state| state.spooled.get(&uuid).cloned());
    // A post selected in another dimension cannot be connected to this one.
    let selected = stored
        .as_ref()
        .filter(|(dim, _)| dim == dimension)
        .map(|(_, selected)| *selected);
    let connected =
        selected.is_some_and(|selected| posts_linked(host, level, dimension, pos, selected));
    match spool_click(selected, pos, connected, POST_LINK_RANGE) {
        SpoolClick::Select | SpoolClick::TooFar => {
            with_state(|state| {
                state.spooled.insert(uuid, (dimension.to_owned(), pos));
            });
        }
        SpoolClick::ClearSelection => {
            with_state(|state| {
                state.spooled.remove(&uuid);
            });
        }
        SpoolClick::Cut => {
            if let Some(selected) = selected {
                cut_link(host, level, dimension, pos, selected);
            }
            with_state(|state| {
                state.spooled.remove(&uuid);
            });
        }
        SpoolClick::Link => {
            if let Some(selected) = selected {
                link_posts(host, level, dimension, pos, selected);
            }
            with_state(|state| {
                state.spooled.remove(&uuid);
            });
        }
    }
    true
}

/// A wooden-rod poke on a plate: one quarter turn clockwise. A gate plate
/// re-reads its inputs at the new rotation in the same write; a bitwise plate's
/// channel powers live in its block entity and are left alone.
fn rotate_with_rod(host: &mut dyn ServerHost, click: &BlockClick<'_>, stack: &ItemStack) -> bool {
    let Some(tag) = mod_api::Identifier::parse(ROD_TAG).ok() else {
        return false;
    };
    let Some(key) = host.tags().key(&tag) else {
        return false;
    };
    if !host.tags().is_in_tag(stack.item(), key).unwrap_or(false) {
        return false;
    }
    let dimension = dimension_name(click.dimension);
    let pos = click.pos;
    let Some(level) = host.level(dimension) else {
        return false;
    };
    let Some(state) = block_state(host, level, pos) else {
        return false;
    };
    let Some(view) = host_view(host, state) else {
        return false;
    };
    let new = if let Some(plate) = plate_of(&view) {
        let rotation = rod_rotation(plate.rotation);
        let Some(rotated) = with_facing_rotation_host(host, state, plate.attachment, rotation)
        else {
            return false;
        };
        let mut inputs = plate.inputs;
        for side in plate.gate.sides() {
            let dir = input_direction(plate.attachment, rotation, *side);
            let neighbour_pos = offset(pos, dir);
            let Some(neighbour) = block_state(host, level, neighbour_pos) else {
                continue;
            };
            let _ = neighbour;
            inputs[side.index()] = signal_toward(host, level, neighbour_pos, dir) > 0;
        }
        with_inputs(host, rotated, plate.gate, inputs, plate.powered)
    } else if let Some(plate) = bitwise_of(&view) {
        with_facing_rotation_host(host, state, plate.attachment, rod_rotation(plate.rotation))
    } else {
        return false;
    };
    let Some(new) = new else {
        return false;
    };
    write_state(host, level, dimension, pos, state, new)
}

// ---------------------------------------------------------------------------
// Posts: links, supply, solve
// ---------------------------------------------------------------------------

/// Whether two posts are connected on `a`'s side (upstream's
/// `hasRemoteConnection`, one direction of the pair).
fn posts_linked(
    host: &dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    a: [i32; 3],
    b: [i32; 3],
) -> bool {
    post_links_at(host, level, dimension, a).contains([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
}

/// `WirePostBlockEntity.addConnection`: join both posts and queue them.
fn link_posts(
    host: &mut dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    a: [i32; 3],
    b: [i32; 3],
) {
    for (from, to) in [(a, b), (b, a)] {
        let Some(state) = block_state(host, level, from) else {
            continue;
        };
        if host_view(host, state).is_none_or(|view| post_of(&view).is_none()) {
            continue;
        }
        let mut links = post_links_at(host, level, dimension, from);
        let offset = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        if links.add(offset) {
            write_post_links(host, level, dimension, from, &links);
            host.block_entity_sync(from);
        }
        mark_post(dimension, from);
    }
}

/// `WirePostBlockEntity.removeConnection`: cut both directions and queue both.
fn cut_link(host: &mut dyn ServerHost, level: LevelRef, dimension: &str, a: [i32; 3], b: [i32; 3]) {
    for (from, to) in [(a, b), (b, a)] {
        let mut links = post_links_at(host, level, dimension, from);
        let offset = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        if links.remove(offset) {
            write_post_links(host, level, dimension, from, &links);
            host.block_entity_sync(from);
        }
        mark_post(dimension, from);
    }
}

/// A post's supplies: the strongest signal on a connectable side (read toward
/// the post), and the stored power of every loaded linked post. Stale links to
/// a non-post are pruned from the block entity.
fn post_supply(
    host: &mut dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    post_now: &Post,
) -> (u8, Vec<u8>, Vec<[i32; 3]>) {
    let mut neighbour_power = 0u8;
    for dir in post_now.kind.connections(post_now.attachment) {
        let neighbour_pos = offset(pos, dir);
        if block_state(host, level, neighbour_pos).is_none() {
            continue;
        }
        let signal = signal_toward(host, level, neighbour_pos, dir);
        neighbour_power = neighbour_power.max(u8::try_from(signal.clamp(0, 15)).unwrap_or(0));
    }
    let links = post_links_at(host, level, dimension, pos);
    let mut linked = Vec::new();
    let mut stale = Vec::new();
    for peer in links.absolute(pos) {
        match block_state(host, level, peer) {
            // An unloaded chunk is not a broken link; keep it.
            None => continue,
            Some(peer_state) => match host_view(host, peer_state).as_ref().and_then(post_of) {
                Some(peer_post) => linked.push(peer_post.power),
                None => stale.push([peer[0] - pos[0], peer[1] - pos[1], peer[2] - pos[2]]),
            },
        }
    }
    let peers: Vec<[i32; 3]> = links.absolute(pos).collect();
    if !stale.is_empty() {
        let mut remaining = links;
        for offset in stale {
            remaining.remove(offset);
        }
        write_post_links(host, level, dimension, pos, &remaining);
    }
    (neighbour_power, linked, peers)
}

/// Recompute the queued wire posts and every post a changed link reaches, in
/// one tick.
fn solve_posts(host: &mut dyn ServerHost, level: LevelRef, dimension: &str) {
    let mut queue: VecDeque<[i32; 3]> = {
        let ready: Vec<(String, [i32; 3])> = with_state(|state| {
            let ready: Vec<(String, [i32; 3])> = state
                .posts
                .iter()
                .filter(|(dim, _)| dim == dimension)
                .cloned()
                .collect();
            for key in &ready {
                state.posts.remove(key);
            }
            ready
        });
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
        let Some(state) = block_state(host, level, pos) else {
            continue;
        };
        let Some(post_now) = host_view(host, state).as_ref().and_then(post_of) else {
            continue;
        };
        let (neighbour_power, linked, links) = post_supply(host, level, dimension, pos, &post_now);
        let next = post_power(neighbour_power, &linked);
        if next == post_now.power {
            continue;
        }
        let Some(new) = with_power(host, state, next) else {
            continue;
        };
        if !write_state(host, level, dimension, pos, state, new) {
            continue;
        }
        for peer in links {
            queue.push_back(peer);
        }
    }
}

// ---------------------------------------------------------------------------
// Wires: supply, solve
// ---------------------------------------------------------------------------

/// The two supplies of one wire's subwires: the block on each attached face,
/// and the strongest connected neighbouring subwire.
fn wire_supply(
    host: &dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    attached_now: Attached,
    self_colour: Option<CableColor>,
) -> ([u8; 6], [u8; 6]) {
    let mut support = [0u8; 6];
    let mut linked = [0u8; 6];
    for face in Dir::ALL {
        if !attached_now.get(face) {
            continue;
        }
        let index = face.index();
        let face_pos = offset(pos, face);
        if let Some(neighbour) = block_state(host, level, face_pos)
            && let Some(view) = host_view(host, neighbour)
        {
            // A face-to-face wire only feeds the same block. Legitimate subwires
            // attach to sturdy blocks, so this only pins forced states.
            let feed = match wire_colour_of(&view) {
                Some(neighbour_colour) => same_wire_block(self_colour, neighbour_colour),
                None => true,
            };
            if feed {
                support[index] = source_power(signal_toward(host, level, face_pos, face));
            }
        }
        // Upstream reads the four faces sharing an edge with the attachment: a
        // wire there that shares the attachment face is a step away; a
        // perpendicular wire connects round the convex edge through the diagonal.
        for side in perpendicular(face) {
            let side_pos = offset(pos, side);
            let Some(neighbour) = block_state(host, level, side_pos) else {
                continue;
            };
            let Some(neighbour_view) = host_view(host, neighbour) else {
                continue;
            };
            if let Some(neighbour_attached) = attached_of(&neighbour_view) {
                let neighbour_colour = wire_colour_of(&neighbour_view).flatten();
                if connects(
                    face,
                    self_colour,
                    neighbour_attached,
                    neighbour_colour,
                    side.opposite(),
                ) {
                    linked[index] =
                        linked[index].max(wire_power_at(host, level, dimension, side_pos)[index]);
                } else if same_wire_block(self_colour, neighbour_colour) {
                    let diagonal_pos = offset(side_pos, face);
                    let Some(diagonal) = block_state(host, level, diagonal_pos) else {
                        continue;
                    };
                    let Some(diagonal_view) = host_view(host, diagonal) else {
                        continue;
                    };
                    let diagonal_colour = wire_colour_of(&diagonal_view).flatten();
                    if !same_wire_block(self_colour, diagonal_colour) {
                        continue;
                    }
                    let Some(diagonal_attached) = attached_of(&diagonal_view) else {
                        continue;
                    };
                    let slot = side.opposite().index();
                    if diagonal_attached.get(side.opposite()) {
                        linked[index] = linked[index]
                            .max(wire_power_at(host, level, dimension, diagonal_pos)[slot]);
                    }
                }
            } else {
                // A non-wire neighbour (a source, a solid block, a plate) feeds
                // the side subwire directly.
                support[index] =
                    support[index].max(source_power(signal_toward(host, level, side_pos, side)));
            }
        }
    }
    (support, linked)
}

/// Drop every subwire whose supporting neighbour is no longer sturdy
/// (`AbstractWireBlock.updateShape`; the SDK's `update_shape` has no sturdy query,
/// so the solve prunes instead). Returns the state to solve and the shape it set.
fn prune_subwires(
    host: &mut dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    state: BlockState,
    attached_now: Attached,
) -> BlockState {
    let mut kept = attached_now;
    for face in Dir::ALL {
        if !attached_now.get(face) {
            continue;
        }
        let support = offset(pos, face);
        let sturdy = level
            .block_face_sturdy(host, support, block_face(face), SupportType::Full)
            .unwrap_or(true);
        kept = retained(kept, face, sturdy);
    }
    if kept == attached_now {
        return state;
    }
    let Some(new) = with_attached(host, state, kept) else {
        return state;
    };
    if write_state(host, level, dimension, pos, state, new) {
        new
    } else {
        state
    }
}

/// Recompute the queued wires and everything a change reaches, in one tick.
fn solve_wires(host: &mut dyn ServerHost, level: LevelRef, dimension: &str) {
    let mut queue: VecDeque<([i32; 3], bool)> = {
        let ready: Vec<(String, [i32; 3])> = with_state(|state| {
            let ready: Vec<(String, [i32; 3])> = state
                .wires
                .iter()
                .filter(|(dim, _)| dim == dimension)
                .cloned()
                .collect();
            for key in &ready {
                state.wires.remove(key);
            }
            ready
        });
        ready
            .into_iter()
            .map(|(dimension, pos)| {
                let key = (dimension, pos);
                let prune = with_state(|state| state.wires_prune.remove(&key));
                (pos, prune)
            })
            .collect()
    };
    let mut visits: BTreeMap<[i32; 3], u8> = BTreeMap::new();
    let mut steps = MAX_WIRE_STEPS;
    while let Some((pos, prune)) = queue.pop_front() {
        if steps == 0 {
            break;
        }
        let count = visits.entry(pos).or_insert(0);
        if *count >= MAX_WIRE_VISITS {
            continue;
        }
        *count += 1;
        steps -= 1;
        let Some(state) = block_state(host, level, pos) else {
            continue;
        };
        let Some(view) = host_view(host, state) else {
            continue;
        };
        let Some(attached_now) = attached_of(&view) else {
            continue;
        };
        let Some(colour) = wire_colour_of(&view) else {
            // A bundled cable beside a wire is not solved by the single-channel
            // wire pass; its own channel solve handles it.
            continue;
        };
        // `updateShape`'s prune applies only when a neighbour changed; a state written at
        // this position keeps a forced subwire (upstream never re-evaluates the block's own
        // shape on a write).
        let state = if prune {
            prune_subwires(host, level, dimension, pos, state, attached_now)
        } else {
            state
        };
        let Some(view) = host_view(host, state) else {
            continue;
        };
        let Some(attached_now) = attached_of(&view) else {
            continue;
        };
        let (support, linked) = wire_supply(host, level, dimension, pos, attached_now, colour);
        let next = wire_power(attached_now, support, linked);
        if next == wire_power_at(host, level, dimension, pos) {
            continue;
        }
        if !write_wire_power(host, level, dimension, pos, next) {
            continue;
        }
        let _ = level.update_neighbors(host, pos, state, NeighbourUpdate::Neighbours);
        host.block_entity_sync(pos);
        let mut reached = Vec::new();
        for face in Dir::ALL {
            let neighbour_pos = offset(pos, face);
            let Some(neighbour) = block_state(host, level, neighbour_pos) else {
                continue;
            };
            let Some(neighbour_view) = host_view(host, neighbour) else {
                continue;
            };
            if attached_of(&neighbour_view).is_some() {
                reached.push(Some(neighbour_pos));
            } else if plate_of(&neighbour_view).is_some() {
                reached.push(None);
                mark(dimension, neighbour_pos, false);
            }
        }
        for wire in reached.into_iter().flatten() {
            queue.push_back((wire, false));
        }
    }
}

// ---------------------------------------------------------------------------
// Bundled cables: supply, solve
// ---------------------------------------------------------------------------

/// The two supplies of one bundled cable: an adjacent relay's channel seed and
/// the strongest connected neighbouring cable (per channel).
fn cable_supply(
    host: &dyn ServerHost,
    level: LevelRef,
    dimension: &str,
    pos: [i32; 3],
    attached_now: Attached,
) -> (CablePower, CablePower) {
    let mut seeds = CablePower::NONE;
    let mut linked = CablePower::NONE;
    for face in Dir::ALL {
        if !attached_now.get(face) {
            continue;
        }
        let face_pos = offset(pos, face);
        if let Some(neighbour) = block_state(host, level, face_pos)
            && let Some(view) = host_view(host, neighbour)
        {
            if let Some(relay) = relay_of(&view) {
                // Upstream's channeled capability on the attached face: a relay
                // seeds its selected channel with `2s - 1`, and a face takes the
                // strongest supplier (`max`).
                let seed = relay_seed(relay_input(host, level, face_pos));
                seeds.raise(face, relay.channel, seed);
            } else if let Some(neighbour_attached) = cable_attached_of(&view)
                && neighbour_attached.get(face.opposite())
            {
                let power = cable_power_at(host, level, dimension, face_pos);
                for channel in Channel::ALL {
                    linked.set(face, channel, power.get(face.opposite(), channel));
                }
            }
        }
        // The four faces sharing an edge with the attachment: a coplanar cable
        // is one step away; a relay or bitwise plate there seeds its channel.
        for side in perpendicular(face) {
            let side_pos = offset(pos, side);
            let Some(neighbour) = block_state(host, level, side_pos) else {
                continue;
            };
            let Some(view) = host_view(host, neighbour) else {
                continue;
            };
            if let Some(relay) = relay_of(&view) {
                let seed = relay_seed(relay_input(host, level, side_pos));
                seeds.raise(face, relay.channel, seed);
            } else if let Some(plate) = bitwise_of(&view) {
                // `canConnectToAdjacentCable` + `getChanneledPower`: a bitwise
                // plate feeds a cable only when the cable sits on the plate's
                // output face and the cable's subwire is attached to the plate's
                // attachment.
                if plate.attachment == face
                    && output_direction(plate.attachment, plate.rotation) == side.opposite()
                {
                    let power = bitwise_power_at(host, level, dimension, side_pos);
                    for channel in Channel::ALL {
                        seeds.raise(face, channel, plate_seed(power.get(channel)));
                    }
                }
            } else if let Some(neighbour_attached) = cable_attached_of(&view) {
                if shares_attachment(face, neighbour_attached, side.opposite()) {
                    let power = cable_power_at(host, level, dimension, side_pos);
                    for channel in Channel::ALL {
                        let seen = linked.get(face, channel).max(power.get(face, channel));
                        linked.set(face, channel, seen);
                    }
                } else {
                    let diagonal_pos = offset(side_pos, face);
                    let Some(diagonal) = block_state(host, level, diagonal_pos) else {
                        continue;
                    };
                    let Some(diagonal_view) = host_view(host, diagonal) else {
                        continue;
                    };
                    let Some(diagonal_attached) = cable_attached_of(&diagonal_view) else {
                        continue;
                    };
                    if diagonal_attached.get(side.opposite()) {
                        let power = cable_power_at(host, level, dimension, diagonal_pos);
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
fn relay_input(host: &dyn ServerHost, level: LevelRef, pos: [i32; 3]) -> i32 {
    let mut strongest = 0;
    for dir in Dir::ALL {
        let neighbour_pos = offset(pos, dir);
        let Some(state) = block_state(host, level, neighbour_pos) else {
            continue;
        };
        let Some(view) = host_view(host, state) else {
            continue;
        };
        if cable_attached_of(&view).is_some() || relay_of(&view).is_some() {
            continue;
        }
        strongest = strongest.max(signal_toward(host, level, neighbour_pos, dir));
    }
    strongest
}

/// The vanilla signal a relay plate answers: the strongest attached channel
/// power of an adjacent cable, halved (upstream's `power / 2`).
fn relay_output_level(
    level: &dyn LevelHost,
    dimension: &str,
    pos: [i32; 3],
    channel: Channel,
) -> i32 {
    let mut signal = 0;
    for dir in Dir::ALL {
        let neighbour_pos = offset(pos, dir);
        let Some(state) = level.block_state(neighbour_pos) else {
            continue;
        };
        let Some(view) = level_view(level, state) else {
            continue;
        };
        let Some(attached) = cable_attached_of(&view) else {
            continue;
        };
        let power = with_state(|cache| {
            cache
                .cable_power
                .get(&key_of(dimension, neighbour_pos))
                .copied()
        })
        .unwrap_or(CablePower::NONE);
        signal = signal.max(relay_signal(power, attached, channel));
    }
    signal
}

/// Recompute the queued bundled cables and everything a change reaches, in one
/// tick (`BundledCableBlock.updatePowerAfterBlockUpdate`, sixteen channels).
fn solve_cables(host: &mut dyn ServerHost, level: LevelRef, dimension: &str) {
    let mut queue: VecDeque<[i32; 3]> = {
        let ready: Vec<(String, [i32; 3])> = with_state(|state| {
            let ready: Vec<(String, [i32; 3])> = state
                .cables
                .iter()
                .filter(|(dim, _)| dim == dimension)
                .cloned()
                .collect();
            for key in &ready {
                state.cables.remove(key);
            }
            ready
        });
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
        let Some(state) = block_state(host, level, pos) else {
            continue;
        };
        let Some(attached_now) = host_view(host, state).as_ref().and_then(cable_attached_of) else {
            continue;
        };
        let state = prune_subwires(host, level, dimension, pos, state, attached_now);
        let Some(attached_now) = host_view(host, state).as_ref().and_then(cable_attached_of) else {
            continue;
        };
        let (seeds, linked) = cable_supply(host, level, dimension, pos, attached_now);
        let next = cable_power(attached_now, seeds, linked);
        if next == cable_power_at(host, level, dimension, pos) {
            continue;
        }
        if !write_cable_power(host, level, dimension, pos, next) {
            continue;
        }
        let _ = level.update_neighbors(host, pos, state, NeighbourUpdate::Neighbours);
        host.block_entity_sync(pos);
        let mut reached = Vec::new();
        let mut relays = Vec::new();
        for face in Dir::ALL {
            let neighbour_pos = offset(pos, face);
            let Some(neighbour) = block_state(host, level, neighbour_pos) else {
                continue;
            };
            let Some(neighbour_view) = host_view(host, neighbour) else {
                continue;
            };
            if cable_attached_of(&neighbour_view).is_some() {
                reached.push(Some(neighbour_pos));
            } else if relay_of(&neighbour_view).is_some() {
                relays.push(neighbour_pos);
            } else if plate_of(&neighbour_view).is_some() {
                reached.push(None);
                mark(dimension, neighbour_pos, false);
            } else if bitwise_of(&neighbour_view).is_some() {
                mark_bitwise(dimension, neighbour_pos);
            }
        }
        for relay_pos in relays {
            let _ = level.update_neighbors(host, relay_pos, state, NeighbourUpdate::Neighbours);
        }
        for cable in reached.into_iter().flatten() {
            queue.push_back(cable);
        }
    }
}

// ---------------------------------------------------------------------------
// Bitwise plates and gate plates
// ---------------------------------------------------------------------------

/// Apply the queued bitwise plates for this dimension
/// (`BitwiseLogicPlateBlock.updatePower`: read the input sides' channels, apply
/// the gate per channel, store 31/0 in the plate's block entity).
fn apply_bitwise(host: &mut dyn ServerHost, level: LevelRef, dimension: &str) {
    let batch: Vec<[i32; 3]> = {
        let ready: Vec<(String, [i32; 3])> = with_state(|state| {
            let ready: Vec<(String, [i32; 3])> = state
                .bitwise
                .iter()
                .filter(|(dim, _)| dim == dimension)
                .cloned()
                .collect();
            for key in &ready {
                state.bitwise.remove(key);
            }
            ready
        });
        ready.into_iter().map(|(_, pos)| pos).collect()
    };
    for pos in batch {
        let Some(current) = block_state(host, level, pos) else {
            continue;
        };
        let Some(view) = host_view(host, current) else {
            continue;
        };
        let Some(plate) = bitwise_of(&view) else {
            continue;
        };
        let mut inputs = [ChannelPower::NONE; 3];
        for side in plate.gate.sides() {
            let dir = input_direction(plate.attachment, plate.rotation, *side);
            let neighbour_pos = offset(pos, dir);
            let Some(neighbour) = block_state(host, level, neighbour_pos) else {
                continue;
            };
            let Some(neighbour_view) = host_view(host, neighbour) else {
                continue;
            };
            if let Some(attached) = cable_attached_of(&neighbour_view) {
                // The plate queries the cable with side = toward the plate and
                // `wireFace` = the plate's attachment.
                inputs[side.index()] = cable_channel_power(
                    cable_power_at(host, level, dimension, neighbour_pos),
                    attached,
                    dir.opposite(),
                    plate.attachment,
                );
            } else if let Some(source) = bitwise_of(&neighbour_view)
                && source.attachment == plate.attachment
                && output_direction(source.attachment, source.rotation) == dir.opposite()
            {
                inputs[side.index()] = bitwise_power_at(host, level, dimension, neighbour_pos);
            }
        }
        let next = bitwise_output(plate.gate, inputs[0], inputs[1], inputs[2]);
        if next == bitwise_power_at(host, level, dimension, pos) {
            continue;
        }
        if !write_bitwise_power(host, level, dimension, pos, next) {
            continue;
        }
        host.block_entity_sync(pos);
        mark_cables_around(host, level, dimension, pos);
        let output_pos = offset(pos, output_direction(plate.attachment, plate.rotation));
        if block_state(host, level, output_pos)
            .and_then(|state| host_view(host, state))
            .is_some_and(|view| bitwise_of(&view).is_some())
        {
            mark_bitwise(dimension, output_pos);
        }
    }
}

/// Apply the queued plates for this dimension.
fn apply_plates(host: &mut dyn ServerHost, level: LevelRef, dimension: &str) {
    let batch: Vec<([i32; 3], bool)> = {
        let ready: Vec<(String, [i32; 3])> = with_state(|state| {
            let ready: Vec<(String, [i32; 3])> = state
                .dirty
                .iter()
                .filter(|(dim, _)| dim == dimension)
                .cloned()
                .collect();
            for key in &ready {
                state.dirty.remove(key);
            }
            ready
        });
        let mut batch = Vec::with_capacity(ready.len());
        for key in ready {
            let placed = with_state(|state| state.placed.remove(&key));
            batch.push((key.1, placed));
        }
        batch
    };
    for (pos, placed) in batch {
        let Some(current) = block_state(host, level, pos) else {
            continue;
        };
        let Some(plate) = host_view(host, current).as_ref().and_then(plate_of) else {
            continue;
        };
        let mut inputs = plate.inputs;
        for side in plate.gate.sides() {
            let dir = input_direction(plate.attachment, plate.rotation, *side);
            let neighbour_pos = offset(pos, dir);
            if block_state(host, level, neighbour_pos).is_none() {
                continue;
            }
            inputs[side.index()] = signal_toward(host, level, neighbour_pos, dir) > 0;
        }
        // Upstream schedules its one-tick update only when the *input state*
        // changed (`neighborChanged`), and the tick then moves the stored bit.
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
                let _ = level.update_neighbors(host, pos, current, NeighbourUpdate::Neighbours);
            }
            continue;
        }
        let Some(new) = with_inputs(host, current, plate.gate, inputs, powered) else {
            continue;
        };
        if write_state(host, level, dimension, pos, current, new)
            && let Some(ticks) = clear_in
        {
            with_state(|state| {
                if state.clearing.len() < MAX_DIRTY {
                    // Upstream's `willTickThisTick` guard: an already scheduled
                    // clear keeps its due tick, so a second edge cannot extend it.
                    state
                        .clearing
                        .entry((dimension.to_owned(), pos))
                        .or_insert(ticks);
                }
            });
        }
    }
}

/// Clear the pulse outputs whose two-tick timer is due, before this tick's dirty
/// batch runs. Upstream's `PulseGateBlock.tick` only drops `powered`.
fn clear_due_pulses(host: &mut dyn ServerHost, level: LevelRef, dimension: &str) {
    let due: Vec<[i32; 3]> = {
        let ready: Vec<(String, [i32; 3])> = with_state(|state| {
            let ready: Vec<(String, [i32; 3])> = state
                .clearing
                .keys()
                .filter(|(dim, _)| dim == dimension)
                .cloned()
                .collect();
            ready
        });
        let mut due = Vec::with_capacity(ready.len());
        for key in ready {
            let decision = with_state(|state| {
                let remaining = state.clearing.get(&key).copied()?;
                match pulse_tick(remaining) {
                    PulseTick::Waiting(left) => {
                        state.clearing.insert(key.clone(), left);
                        None
                    }
                    PulseTick::Cleared => {
                        state.clearing.remove(&key);
                        Some(key.1)
                    }
                }
            });
            if let Some(pos) = decision {
                due.push(pos);
            }
        }
        due
    };
    for pos in due {
        let Some(current) = block_state(host, level, pos) else {
            continue;
        };
        let Some(plate) = host_view(host, current).as_ref().and_then(plate_of) else {
            continue;
        };
        if plate.gate != Gate::Pulse || !plate.powered {
            continue;
        }
        let Some(new) = with_inputs(host, current, Gate::Pulse, plate.inputs, false) else {
            continue;
        };
        write_state(host, level, dimension, pos, current, new);
    }
}

// ---------------------------------------------------------------------------
// Hooks (`impl Hooks` in `lib.rs` delegates here)
// ---------------------------------------------------------------------------

/// A block was placed/changed: seed the first evaluation too (`/setblock`,
/// pistons, world load). A state write of this port is not a placement.
pub fn on_block_changed(level: &dyn LevelHost, event: &mut mod_api::BlockChangedEvent) {
    let dimension = dimension_name(event.dimension);
    let pos = [event.pos.x, event.pos.y, event.pos.z];
    let key = key_of(dimension, pos);
    if with_state(|state| state.writing.remove(&key)) {
        return;
    }
    let Some(view) = level_view(level, event.new) else {
        return;
    };
    if wire_attached_of(&view).is_some() {
        mark_wire(dimension, pos);
    } else if cable_attached_of(&view).is_some() {
        mark_cable(dimension, pos);
    } else if relay_of(&view).is_some() {
        with_state(|state| {
            state.relays.insert(key);
        });
    } else if plate_of(&view).is_some() {
        mark(dimension, pos, true);
    } else if bitwise_of(&view).is_some() {
        mark_bitwise(dimension, pos);
        // A fresh plate starts at zero power; its first evaluation may seed a
        // cable, so the cables beside it are solved once it has run.
        mark_cables_near_cached(dimension, pos);
    } else if post_of(&view).is_some() {
        mark_post(dimension, pos);
    } else {
        // A removed post keeps its saved links in the cache until now; tell its peers, then
        // forget everything this port cached for the position: its power, its links and its
        // queued work belong to a block that is gone (the review finding).
        let links = with_state(|state| state.post_links.get(&key).cloned().unwrap_or_default());
        for peer in links.absolute(pos) {
            mark_post(dimension, peer);
        }
        with_state(|state| evict_node(state, &key));
    }
}

/// Queue the bundled cables beside `pos` from the cached wire/cable tables (the
/// `block_changed` hook has no `ServerHost`).
fn mark_cables_near_cached(dimension: &str, pos: [i32; 3]) {
    // The cache holds only our own cables; a bare position lookup is enough.
    for face in Dir::ALL {
        let at = key_of(dimension, offset(pos, face));
        if with_state(|state| state.cable_power.contains_key(&at)) {
            mark_cable(dimension, at.1);
        }
    }
}

/// Apply the queued wires and plates for this level's dimension.
pub fn on_level_tick_post(host: &mut dyn ServerHost, level: LevelRef) {
    let Some(dimension) = level.dimension().id(host) else {
        return;
    };
    let dimension = dimension.as_str();
    // A relay's adjacent cables could not be found in `neighbor_changed`.
    let relays: Vec<[i32; 3]> = {
        let ready: Vec<(String, [i32; 3])> = with_state(|state| {
            let ready: Vec<(String, [i32; 3])> = state
                .relays
                .iter()
                .filter(|(dim, _)| dim == dimension)
                .cloned()
                .collect();
            for key in &ready {
                state.relays.remove(key);
            }
            ready
        });
        ready.into_iter().map(|(_, pos)| pos).collect()
    };
    for pos in relays {
        mark_cables_around(host, level, dimension, pos);
    }
    clear_due_pulses(host, level, dimension);
    solve_posts(host, level, dimension);
    solve_wires(host, level, dimension);
    solve_cables(host, level, dimension);
    apply_bitwise(host, level, dimension);
    apply_plates(host, level, dimension);
}

/// A broken wire or cable drops one item per attached subwire, and nothing when
/// no subwire is attached (upstream's `WireCountLootFunction`).
pub fn on_loot_drops(host: &mut dyn ServerHost, event: &mut LootDropsEvent<'_>) {
    let Some((_, state)) = event.context.block else {
        return;
    };
    let Some(view) = host_view(host, state) else {
        return;
    };
    if !is_subwire_block(&view.name) {
        return;
    }
    let Some(attached) = wire_attached_of(&view).or_else(|| cable_attached_of(&view)) else {
        return;
    };
    let count = attached.count() as u32;
    if count == 0 {
        event
            .drops
            .retain(|drop| drop.item().id(&*host) != view.name);
        return;
    }
    for drop in event
        .drops
        .iter_mut()
        .filter(|drop| drop.item().id(&*host) == view.name)
    {
        let _ = drop.set_count(count);
    }
}

/// A world opened or closed: every queued set, armed pulse, cached power and
/// spool selection belonged to the world that is going away.
pub fn reset_world_state() {
    *state() = None;
}

/// Drop every cache entry the port keeps for `key`. A position whose block is gone must not
/// keep its wire, cable or bitwise power, its saved post links or its queued work: the readout
/// would answer stale power and the solve would visit a block that is not there (the review
/// finding).
fn evict_node(state: &mut State, key: &(String, [i32; 3])) {
    state.wire_power.remove(key);
    state.cable_power.remove(key);
    state.bitwise_power.remove(key);
    state.post_links.remove(key);
    state.dirty.remove(key);
    state.bitwise.remove(key);
    state.wires.remove(key);
    state.wires_prune.remove(key);
    state.cables.remove(key);
    state.placed.remove(key);
    state.clearing.remove(key);
    state.posts.remove(key);
    state.relays.remove(key);
}

/// Drop every entry of `dimension` inside `chunk`: its blocks are leaving the world, and a
/// cache that outlives its chunk is memory the world will never need again (the review
/// finding). The chunk was saved before it unloads, so nothing here writes.
fn evict_chunk(state: &mut State, dimension: &str, chunk: [i32; 2]) {
    let stale = |key: &(String, [i32; 3])| key.0 == dimension && in_chunk(key.1, chunk);
    state.dirty.retain(|key| !stale(key));
    state.bitwise.retain(|key| !stale(key));
    state.wires.retain(|key| !stale(key));
    state.wires_prune.retain(|key| !stale(key));
    state.cables.retain(|key| !stale(key));
    state.placed.retain(|key| !stale(key));
    state.writing.retain(|key| !stale(key));
    state.posts.retain(|key| !stale(key));
    state.relays.retain(|key| !stale(key));
    state.clearing.retain(|key, _| !stale(key));
    state.wire_power.retain(|key, _| !stale(key));
    state.cable_power.retain(|key, _| !stale(key));
    state.bitwise_power.retain(|key, _| !stale(key));
    state.post_links.retain(|key, _| !stale(key));
}

/// A chunk of a dimension unloaded: forget what this port cached for its positions.
pub fn on_chunk_unload(dimension: &str, chunk: [i32; 2]) {
    with_state(|state| evict_chunk(state, dimension, chunk));
}

// ---------------------------------------------------------------------------
// Block entity parts
// ---------------------------------------------------------------------------

/// The block entity of the plain wire and the sixteen coloured network cables
/// (`WireBlockEntity.POWER`).
struct WireEntityPart;

#[mod_api::horizon_part]
impl BlockPart for WireEntityPart {}

#[mod_api::horizon_part]
impl BlockEntityPart for WireEntityPart {
    fn block_entity(
        &self,
        _: mod_api::LevelSide,
        _: &mut BlockEntityEvent<'_>,
    ) -> Option<Compound> {
        Some(encode_wire_power([0u8; 6]))
    }

    fn block_entity_on_load(&self, host: HostSide<'_>, event: &mut BlockEntityOnLoadEvent) {
        let HostSide::Server(host) = host else {
            return;
        };
        if WIRE_TYPE.get() != Some(&event.id) {
            return;
        }
        let Some(level) = event.entity.level(&*host) else {
            return;
        };
        let Some(dimension) = level.dimension().id(&*host) else {
            return;
        };
        let Some(pos) = event.entity.position(&*host) else {
            return;
        };
        let Some(data) = event.entity.data(&*host) else {
            return;
        };
        with_state(|state| {
            state
                .wire_power
                .insert(key_of(&dimension, pos), decode_wire_power(&data));
        });
    }

    fn block_entity_update_tag(
        &self,
        event: &mut BlockEntityUpdateTagEvent<'_>,
    ) -> Option<Compound> {
        Some(encode_wire_power(decode_wire_power(&event.entity.nbt)))
    }
}

/// The bundled cable's block entity (`BundledCableBlockEntity`'s `byte[6][16] power`).
struct CableEntityPart;

#[mod_api::horizon_part]
impl BlockPart for CableEntityPart {}

#[mod_api::horizon_part]
impl BlockEntityPart for CableEntityPart {
    fn block_entity(
        &self,
        _: mod_api::LevelSide,
        _: &mut BlockEntityEvent<'_>,
    ) -> Option<Compound> {
        Some(encode_cable_power(CablePower::NONE))
    }

    fn block_entity_on_load(&self, host: HostSide<'_>, event: &mut BlockEntityOnLoadEvent) {
        let HostSide::Server(host) = host else {
            return;
        };
        if CABLE_TYPE.get() != Some(&event.id) {
            return;
        }
        let Some(level) = event.entity.level(&*host) else {
            return;
        };
        let Some(dimension) = level.dimension().id(&*host) else {
            return;
        };
        let Some(pos) = event.entity.position(&*host) else {
            return;
        };
        let Some(data) = event.entity.data(&*host) else {
            return;
        };
        with_state(|state| {
            state
                .cable_power
                .insert(key_of(&dimension, pos), decode_cable_power(&data));
        });
    }

    fn block_entity_update_tag(
        &self,
        event: &mut BlockEntityUpdateTagEvent<'_>,
    ) -> Option<Compound> {
        Some(encode_cable_power(decode_cable_power(&event.entity.nbt)))
    }
}

/// The bitwise plates' block entity (`ChanneledPowerStorageBlockEntity.POWER`, a
/// byte array).
struct BitwiseEntityPart;

#[mod_api::horizon_part]
impl BlockPart for BitwiseEntityPart {}

#[mod_api::horizon_part]
impl BlockEntityPart for BitwiseEntityPart {
    fn block_entity(
        &self,
        _: mod_api::LevelSide,
        _: &mut BlockEntityEvent<'_>,
    ) -> Option<Compound> {
        Some(encode_channel_power(ChannelPower::NONE))
    }

    fn block_entity_on_load(&self, host: HostSide<'_>, event: &mut BlockEntityOnLoadEvent) {
        let HostSide::Server(host) = host else {
            return;
        };
        if BITWISE_TYPE.get() != Some(&event.id) {
            return;
        }
        let Some(level) = event.entity.level(&*host) else {
            return;
        };
        let Some(dimension) = level.dimension().id(&*host) else {
            return;
        };
        let Some(pos) = event.entity.position(&*host) else {
            return;
        };
        let Some(data) = event.entity.data(&*host) else {
            return;
        };
        with_state(|state| {
            state
                .bitwise_power
                .insert(key_of(&dimension, pos), decode_channel_power(&data));
        });
    }

    fn block_entity_update_tag(
        &self,
        event: &mut BlockEntityUpdateTagEvent<'_>,
    ) -> Option<Compound> {
        Some(encode_channel_power(decode_channel_power(
            &event.entity.nbt,
        )))
    }
}

/// The three wire posts' block entity (`WirePostBlockEntity.CONNECTIONS`).
struct PostEntityPart;

#[mod_api::horizon_part]
impl BlockPart for PostEntityPart {}

#[mod_api::horizon_part]
impl BlockEntityPart for PostEntityPart {
    fn block_entity(
        &self,
        _: mod_api::LevelSide,
        _: &mut BlockEntityEvent<'_>,
    ) -> Option<Compound> {
        Some(encode_post_links(&PostLinks::default()))
    }

    fn block_entity_on_load(&self, host: HostSide<'_>, event: &mut BlockEntityOnLoadEvent) {
        let HostSide::Server(host) = host else {
            return;
        };
        if POST_TYPE.get() != Some(&event.id) {
            return;
        }
        let Some(level) = event.entity.level(&*host) else {
            return;
        };
        let Some(dimension) = level.dimension().id(&*host) else {
            return;
        };
        let Some(pos) = event.entity.position(&*host) else {
            return;
        };
        let Some(data) = event.entity.data(&*host) else {
            return;
        };
        with_state(|state| {
            state
                .post_links
                .insert(key_of(&dimension, pos), decode_post_links(&data));
        });
    }

    fn block_entity_update_tag(
        &self,
        event: &mut BlockEntityUpdateTagEvent<'_>,
    ) -> Option<Compound> {
        Some(encode_post_links(&decode_post_links(&event.entity.nbt)))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// The id list behind `include!`: `Gate`/`BitwiseGate` have no id enumerator for
/// every value, so the ids are listed with the content.
const GATE_IDS: [&str; 13] = [
    "morered:diode",
    "morered:not_gate",
    "morered:or_gate",
    "morered:nor_gate",
    "morered:nand_gate",
    "morered:and_gate",
    "morered:xor_gate",
    "morered:xnor_gate",
    "morered:multiplexer",
    "morered:and_2_gate",
    "morered:nand_2_gate",
    "morered:latch",
    "morered:pulse_gate",
];

const BITWISE_IDS: [&str; 6] = [
    "morered:bitwise_diode",
    "morered:bitwise_not_gate",
    "morered:bitwise_and_gate",
    "morered:bitwise_or_gate",
    "morered:bitwise_xor_gate",
    "morered:bitwise_xnor_gate",
];

/// The wire family's block ids, one per dye plus the plain wire.
fn wire_ids() -> Vec<&'static str> {
    let mut ids = vec![WIRE_BLOCK];
    ids.extend(CableColor::ALL.into_iter().map(CableColor::id));
    ids
}

/// Every block of this port that answers a redstone signal, queues work on a neighbour
/// change and takes its shape from the click: the thirteen gate plates, the six bitwise
/// plates, the plain wire, the **sixteen coloured network cables** (separate content blocks,
/// one per dye, so each needs its own `block_part`), the bundled cable, its relay plate and
/// the three posts. A block missing here never hears `neighbor_changed` and never answers
/// `block_signal` — the 0.2.0 registration dropped the coloured cables, and the e2e's
/// coloured-cable phase is what caught it.
fn signal_block_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = GATE_IDS.to_vec();
    ids.extend(BITWISE_IDS);
    ids.extend(wire_ids());
    ids.extend([CABLE_BLOCK, RELAY_BLOCK]);
    ids.extend([
        "morered:redwire_post",
        "morered:redwire_post_plate",
        "morered:redwire_post_relay_plate",
    ]);
    ids
}

/// Declare the four block entity types, attach each Part, and give every block
/// the signal/shape behaviour. Called once from `Hooks::register`.
pub fn register(r: &mut dyn Registrar) {
    let wire_type = r.block_entity_type("wire").blocks(&wire_ids()).build();
    let _ = WIRE_TYPE.set(wire_type);
    let _ = r.block_entity_part(&wire_type, WireEntityPart);

    let cable_type = r
        .block_entity_type("bundled_cable")
        .blocks(&[CABLE_BLOCK])
        .build();
    let _ = CABLE_TYPE.set(cable_type);
    let _ = r.block_entity_part(&cable_type, CableEntityPart);

    let bitwise_type = r
        .block_entity_type("bitwise_plate")
        .blocks(&BITWISE_IDS)
        .build();
    let _ = BITWISE_TYPE.set(bitwise_type);
    let _ = r.block_entity_part(&bitwise_type, BitwiseEntityPart);

    let post_type = r
        .block_entity_type("wire_post")
        .blocks(&[
            "redwire_post",
            "redwire_post_plate",
            "redwire_post_relay_plate",
        ])
        .build();
    let _ = POST_TYPE.set(post_type);
    let _ = r.block_entity_part(&post_type, PostEntityPart);

    for id in GATE_IDS.into_iter().chain(BITWISE_IDS) {
        let key = r.block_key(id);
        r.block_part(key, RedstonePart);
        r.block_part(key, ClickPart);
    }
    // Every signal block — the wire family (the plain wire and the sixteen coloured cables),
    // the bundled cable, the relay plate and the three posts. The coloured cables are separate
    // content blocks, so each one needs its own `block_part`; leaving them out left them deaf
    // to `neighbor_changed` and invisible to `block_signal`.
    for id in signal_block_ids()
        .into_iter()
        .filter(|id| !GATE_IDS.contains(id) && !BITWISE_IDS.contains(id))
    {
        let key = r.block_key(id);
        r.block_part(key, RedstonePart);
    }
    for id in [
        "morered:redwire_post",
        "morered:redwire_post_plate",
        "morered:redwire_post_relay_plate",
    ] {
        let key = r.block_key(id);
        r.block_part(key, ClickPart);
    }
}

// ---------------------------------------------------------------------------
// Tests of the pure glue (state text and saved-compound codecs)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_text_round_trips_its_properties() {
        let view = parse_view("morered:and_gate[facing=down,input_a=true,rotation=2]");
        assert_eq!(view.name, "morered:and_gate");
        assert_eq!(view.property("facing"), Some("down"));
        assert!(view.flag("input_a").unwrap());
        assert_eq!(view.int("rotation"), Some(2));
        assert_eq!(
            view.text_with(&[]),
            "morered:and_gate[facing=down,input_a=true,rotation=2]"
        );
    }

    #[test]
    fn a_property_update_keeps_the_others() {
        let view =
            parse_view("morered:pulse_gate[facing=up,input_b=false,powered=true,rotation=1]");
        assert_eq!(
            view.text_with(&[("powered", "false")]),
            "morered:pulse_gate[facing=up,input_b=false,powered=false,rotation=1]"
        );
        assert!(view.text_with(&[("rotation", "3")]).contains("rotation=3"));
    }

    #[test]
    fn only_changed_properties_are_written() {
        let old = parse_view(
            "morered:and_gate[facing=down,input_a=false,input_b=false,input_c=false,rotation=0]",
        );
        let new = parse_view(
            "morered:and_gate[facing=down,input_a=true,input_b=false,input_c=false,rotation=0]",
        );
        assert_eq!(
            new.changed_properties(&old),
            vec![("input_a".to_owned(), "true".to_owned())]
        );
    }

    #[test]
    fn a_wire_power_array_round_trips_and_clamps() {
        let power = [0u8, 1, 7, 15, 30, 31];
        assert_eq!(decode_wire_power(&encode_wire_power(power)), power);
        let mut nbt = Compound::new();
        nbt.insert("power".into(), Tag::IntArray(vec![99, -1, 5, 5, 5, 5]));
        assert_eq!(decode_wire_power(&nbt), [31, 0, 5, 5, 5, 5]);
        assert_eq!(decode_wire_power(&Compound::new()), [0u8; 6]);
        let wrong = [0u8; 4];
        let mut short = Compound::new();
        short.insert(
            "power".into(),
            Tag::IntArray(wrong.into_iter().map(i32::from).collect()),
        );
        assert_eq!(decode_wire_power(&short), [0u8; 6]);
    }

    #[test]
    fn a_cable_power_compound_round_trips_every_channel() {
        let mut power = CablePower::NONE;
        for face in Dir::ALL {
            for channel in Channel::ALL {
                let value = (face.index() as u8 * 3 + channel.index() as u8) % 32;
                power.set(face, channel, value);
            }
        }
        assert_eq!(decode_cable_power(&encode_cable_power(power)), power);
        assert_eq!(decode_cable_power(&Compound::new()), CablePower::NONE);
    }

    #[test]
    fn a_bitwise_power_array_round_trips_every_channel() {
        let mut power = ChannelPower::NONE;
        for channel in Channel::ALL {
            power.set(channel, if channel.index() % 2 == 0 { 31 } else { 0 });
        }
        assert_eq!(decode_channel_power(&encode_channel_power(power)), power);
        assert_eq!(decode_channel_power(&Compound::new()), ChannelPower::NONE);
    }

    #[test]
    fn saved_post_links_round_trip_and_drop_malformed_entries() {
        let links = PostLinks::from_offsets(vec![[10, 0, 0], [-10, 0, 0]]);
        assert_eq!(decode_post_links(&encode_post_links(&links)), links);
        let mut nbt = Compound::new();
        nbt.insert(
            "connections".into(),
            Tag::List(vec![
                Tag::IntArray(vec![1, 2, 3]),
                Tag::IntArray(vec![1, 2]),
                Tag::Int(4),
            ]),
        );
        assert_eq!(
            decode_post_links(&nbt),
            PostLinks::from_offsets(vec![[1, 2, 3]])
        );
        assert_eq!(decode_post_links(&Compound::new()), PostLinks::default());
    }

    #[test]
    fn every_signal_block_gets_its_part() {
        // The registration list is the only place a Part is attached, and a block missing
        // from it never hears `neighbor_changed` (so it never re-solves) and never answers
        // `block_signal` (so no reader sees it). The 0.2.0 registration dropped the sixteen
        // coloured cables from the list while the wire's block entity type kept them, and
        // the e2e's coloured-cable phase failed with a dark lamp while the plain wire on the
        // same rig read 29.
        let ids = signal_block_ids();
        for colour in CableColor::ALL {
            assert!(ids.contains(&colour.id()), "{} has no part", colour.id());
        }
        for id in [
            "morered:red_alloy_wire",
            "morered:bundled_network_cable",
            "morered:bundled_cable_relay_plate",
            "morered:redwire_post",
            "morered:redwire_post_plate",
            "morered:redwire_post_relay_plate",
            "morered:and_gate",
            "morered:pulse_gate",
            "morered:bitwise_and_gate",
        ] {
            assert!(ids.contains(&id), "{id} has no part");
        }
        // Every content block of the port except the recipe-only `stone_plate` carries one.
        assert_eq!(ids.len(), 13 + 6 + 17 + 2 + 3);
        assert!(!ids.contains(&"morered:stone_plate"));
    }

    #[test]
    fn the_families_are_recognised_by_id() {
        assert!(is_subwire_block("morered:red_alloy_wire"));
        assert!(is_subwire_block("morered:bundled_network_cable"));
        assert!(is_subwire_block("morered:red_network_cable"));
        assert!(!is_subwire_block("morered:redwire_post"));
        assert!(has_block_entity("morered:red_network_cable"));
        assert!(has_block_entity("morered:bitwise_or_gate"));
        assert!(has_block_entity("morered:redwire_post_plate"));
        assert!(!has_block_entity("morered:and_gate"));
    }

    #[test]
    fn every_gate_id_is_classified_with_the_expected_shape() {
        for id in GATE_IDS {
            let view = parse_view(&format!(
                "{id}[facing=down,input_a=false,input_b=false,input_c=false,rotation=0]"
            ));
            assert!(plate_of(&view).is_some(), "{id} is not a plate");
        }
        for id in BITWISE_IDS {
            let view = parse_view(&format!("{id}[facing=down,rotation=0]"));
            assert!(bitwise_of(&view).is_some(), "{id} is not a bitwise plate");
        }
    }

    /// `/setblock` writes a state, it does not place a block: upstream prunes a wire's subwire
    /// only from `updateShape`, i.e. when a **neighbour** changed, so a state written with a
    /// subwire whose support is not sturdy stays until a neighbour changes (the 64-state sweep
    /// reads such a state back). A placement (`block_changed`) must not arm the prune.
    #[test]
    fn a_written_wire_is_pruned_only_after_a_neighbour_change() {
        let dimension = "minecraft:prune_probe";
        let placed = [5, 64, 4];
        let neighbour = [6, 64, 4];
        with_state(|state| {
            state.wires.clear();
            state.wires_prune.clear();
        });
        mark_wire(dimension, placed);
        assert!(with_state(|state| state
            .wires
            .contains(&(dimension.to_owned(), placed))));
        assert!(
            !with_state(|state| state.wires_prune.contains(&(dimension.to_owned(), placed))),
            "a written state is not a neighbour change"
        );
        mark_wire_prune(dimension, neighbour);
        assert!(with_state(|state| state
            .wires
            .contains(&(dimension.to_owned(), neighbour))));
        assert!(
            with_state(|state| state
                .wires_prune
                .contains(&(dimension.to_owned(), neighbour))),
            "a neighbour change prunes the shape"
        );
    }

    /// The review finding: replacing or removing a wire, cable, bitwise plate or post left its
    /// cached power in place for the life of the runtime, so the readout could answer stale
    /// power at a position that no longer holds the block.
    #[test]
    fn a_removed_block_loses_every_cached_entry() {
        let mut state = State::default();
        let key = ("minecraft:overworld".to_owned(), [4, 70, 4]);
        state.wire_power.insert(key.clone(), [1, 2, 3, 4, 5, 6]);
        state.cable_power.insert(key.clone(), CablePower::default());
        state
            .bitwise_power
            .insert(key.clone(), ChannelPower::default());
        state
            .post_links
            .insert(key.clone(), PostLinks::from_offsets(vec![[0, 0, 1]]));
        state.dirty.insert(key.clone());
        state.wires.insert(key.clone());
        state.posts.insert(key.clone());

        let neighbour = ("minecraft:overworld".to_owned(), [5, 70, 4]);
        state.wire_power.insert(neighbour.clone(), [7; 6]);

        evict_node(&mut state, &key);
        assert!(!state.wire_power.contains_key(&key));
        assert!(!state.cable_power.contains_key(&key));
        assert!(!state.bitwise_power.contains_key(&key));
        assert!(!state.post_links.contains_key(&key));
        assert!(!state.dirty.contains(&key));
        assert!(!state.wires.contains(&key));
        assert!(!state.posts.contains(&key));
        assert!(
            state.wire_power.contains_key(&neighbour),
            "another position keeps its entry"
        );
    }

    /// The second half of that finding: the caches also outlived their chunk. An unload drops
    /// exactly the entries of that chunk in that dimension.
    #[test]
    fn a_chunk_unload_drops_its_cached_positions() {
        let mut state = State::default();
        let inside = ("minecraft:overworld".to_owned(), [16, 70, -1]);
        let outside_x = ("minecraft:overworld".to_owned(), [32, 70, -1]);
        let outside_dimension = ("minecraft:the_nether".to_owned(), [16, 70, -1]);
        for key in [&inside, &outside_x, &outside_dimension] {
            state.wire_power.insert(key.clone(), [1; 6]);
            state
                .post_links
                .insert(key.clone(), PostLinks::from_offsets(vec![[0, 0, 1]]));
            state.dirty.insert(key.clone());
        }

        evict_chunk(&mut state, "minecraft:overworld", [1, -1]);
        assert!(!state.wire_power.contains_key(&inside));
        assert!(!state.post_links.contains_key(&inside));
        assert!(!state.dirty.contains(&inside));
        assert!(
            state.wire_power.contains_key(&outside_x),
            "the next chunk keeps its entry"
        );
        assert!(
            state.wire_power.contains_key(&outside_dimension),
            "another dimension keeps its entry"
        );
    }
}
