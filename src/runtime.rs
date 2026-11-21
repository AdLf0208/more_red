// More Red for Horizon — the logic-gate plate model (pure).
//
// Ported from More Red v1.21.1-6.0.0.3 (Commoble/morered, MIT) for Minecraft
// Java 1.21.1 NeoForge. Upstream source read for behaviour only; this is a
// clean-room Rust rewrite.
//
// A plate attaches to one face, rotates around that attachment, and emits 15
// from one output face when its three stored input booleans satisfy its logic
// function. Inputs A/B/C are the faces 90/180/270 degrees clockwise from the
// output (seen from outside the attachment face). The output direction table
// and the input rotation are `BlockStateUtil`'s.
//
// Two plates store an extra `powered` bit instead of a function: the latch
// (A sets, C resets, complementary output) and the pulse gate (a rising edge
// on B emits for `PULSE_DURATION` ticks, then clears, and B staying high does
// not hold it).

/// The six faces, in Minecraft's `Direction` ordinal order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir {
    pub const ALL: [Dir; 6] = [
        Dir::Down,
        Dir::Up,
        Dir::North,
        Dir::South,
        Dir::West,
        Dir::East,
    ];

    pub fn opposite(self) -> Dir {
        match self {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::West => Dir::East,
            Dir::East => Dir::West,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Dir::Down => 0,
            Dir::Up => 1,
            Dir::North => 2,
            Dir::South => 3,
            Dir::West => 4,
            Dir::East => 5,
        }
    }

    /// The unit vector of the face (`Direction.getStepX/Y/Z`).
    pub fn normal(self) -> [f32; 3] {
        match self {
            Dir::Down => [0.0, -1.0, 0.0],
            Dir::Up => [0.0, 1.0, 0.0],
            Dir::North => [0.0, 0.0, -1.0],
            Dir::South => [0.0, 0.0, 1.0],
            Dir::West => [-1.0, 0.0, 0.0],
            Dir::East => [1.0, 0.0, 0.0],
        }
    }
}

/// `BlockStateUtil.OUTPUT_TABLE`, indexed by attachment then rotation.
pub const OUTPUT_TABLE: [[Dir; 4]; 6] = [
    [Dir::North, Dir::East, Dir::South, Dir::West],
    [Dir::North, Dir::West, Dir::South, Dir::East],
    [Dir::Up, Dir::East, Dir::Down, Dir::West],
    [Dir::Up, Dir::West, Dir::Down, Dir::East],
    [Dir::Up, Dir::North, Dir::Down, Dir::South],
    [Dir::Up, Dir::South, Dir::Down, Dir::North],
];

/// The output face of a plate (`rotation` 0..3).
pub fn output_direction(attachment: Dir, rotation: u8) -> Dir {
    OUTPUT_TABLE[attachment.index()][usize::from(rotation % 4)]
}

/// An input side: A is 90°, B 180°, C 270° clockwise from the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    A,
    B,
    C,
}

impl Side {
    pub fn rotations_from_output(self) -> u8 {
        match self {
            Side::A => 1,
            Side::B => 2,
            Side::C => 3,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Side::A => 0,
            Side::B => 1,
            Side::C => 2,
        }
    }
}

/// The face an input side reads from (`BlockStateUtil.getInputDirection`).
pub fn input_direction(attachment: Dir, rotation: u8, side: Side) -> Dir {
    output_direction(
        attachment,
        rotation.wrapping_add(side.rotations_from_output()),
    )
}

/// The plate gates and state-memory plates (`MoreRed.java`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Diode,
    Not,
    Or,
    Nor,
    Nand,
    And,
    Xor,
    Xnor,
    Multiplex,
    And2,
    Nand2,
    /// The state-memory plate: A sets, C resets; a complementary output.
    Latch,
    /// The one-shot plate: a rising edge on B emits for `PULSE_DURATION`.
    Pulse,
}

impl Gate {
    /// The gate whose item/block id is `id` (`morered:and_gate`).
    pub fn from_id(id: &str) -> Option<Gate> {
        Some(match id.strip_prefix("morered:")? {
            "diode" => Gate::Diode,
            "not_gate" => Gate::Not,
            "or_gate" => Gate::Or,
            "nor_gate" => Gate::Nor,
            "nand_gate" => Gate::Nand,
            "and_gate" => Gate::And,
            "xor_gate" => Gate::Xor,
            "xnor_gate" => Gate::Xnor,
            "multiplexer" => Gate::Multiplex,
            "and_2_gate" => Gate::And2,
            "nand_2_gate" => Gate::Nand2,
            "latch" => Gate::Latch,
            "pulse_gate" => Gate::Pulse,
            _ => return None,
        })
    }

    /// The input sides the block state stores (`getInputSides`).
    pub fn sides(self) -> &'static [Side] {
        match self {
            Gate::Diode | Gate::Not | Gate::Pulse => &[Side::B],
            Gate::Xor | Gate::Xnor | Gate::And2 | Gate::Nand2 | Gate::Latch => &[Side::A, Side::C],
            Gate::Or | Gate::Nor | Gate::Nand | Gate::And | Gate::Multiplex => {
                &[Side::A, Side::B, Side::C]
            }
        }
    }

    /// The logic function (`LogicFunctions`). Sides the gate doesn't use are
    /// false in the state and ignored by the function. The plates that store
    /// a `powered` bit instead (latch, pulse) have no function; see
    /// [`Gate::signal_with_powered`].
    pub fn apply(self, a: bool, b: bool, c: bool) -> bool {
        match self {
            Gate::Diode => b,
            Gate::Not => !b,
            Gate::Or => a || b || c,
            Gate::Nor => !(a || b || c),
            Gate::Nand => !(a && b && c),
            Gate::And => a && b && c,
            Gate::Xor => a ^ c,
            Gate::Xnor => !(a ^ c),
            Gate::Multiplex => {
                if b {
                    c
                } else {
                    a
                }
            }
            Gate::And2 => a && c,
            Gate::Nand2 => !(a && c),
            // State-memory: the function is not used; see `latch_powered`
            // and `pulse_update`.
            Gate::Latch | Gate::Pulse => false,
        }
    }

    /// The output strength (`OUTPUT_STRENGTH`).
    pub const OUTPUT_STRENGTH: i32 = 15;

    /// The signal this gate emits on `face` (the face the signal leaves).
    pub fn signal(self, inputs: [bool; 3], attachment: Dir, rotation: u8, face: Dir) -> i32 {
        if !self.applies(inputs[0], inputs[1], inputs[2]) {
            return 0;
        }
        if output_direction(attachment, rotation) != face {
            return 0;
        }
        Self::OUTPUT_STRENGTH
    }

    /// `apply` from the stored A/B/C booleans.
    pub fn applies(self, a: bool, b: bool, c: bool) -> bool {
        self.apply(a, b, c)
    }

    /// Signal including the stored `powered` bit. Function gates ignore it;
    /// the latch outputs on the primary face when powered and on the opposite
    /// face when not, except that both inputs active silences it; the pulse
    /// emits 15 on its output face only while powered.
    pub fn signal_with_powered(
        self,
        inputs: [bool; 3],
        powered: bool,
        attachment: Dir,
        rotation: u8,
        face: Dir,
    ) -> i32 {
        match self {
            Gate::Latch => {
                if inputs[Side::A.index()] && inputs[Side::C.index()] {
                    return 0;
                }
                let output = output_direction(attachment, rotation);
                let emitting = if powered { output } else { output.opposite() };
                if face == emitting {
                    Self::OUTPUT_STRENGTH
                } else {
                    0
                }
            }
            Gate::Pulse => {
                if powered && face == output_direction(attachment, rotation) {
                    Self::OUTPUT_STRENGTH
                } else {
                    0
                }
            }
            _ => self.signal(inputs, attachment, rotation, face),
        }
    }

    /// Whether this gate stores `side` at all.
    #[allow(dead_code)]
    pub fn uses(self, side: Side) -> bool {
        self.sides().contains(&side)
    }
}

#[allow(dead_code)]
/// The next stored `powered` bit of a latch (`LatchBlock.tick`): A sets,
/// C resets, and C wins when both are on.
pub fn latch_powered(was: bool, a: bool, c: bool) -> bool {
    (was && !c) || (!was && a)
}

/// The pulse gate's output duration in ticks (`PulseGateBlock.PULSE_DURATION`).
pub const PULSE_DURATION: u8 = 2;

/// What one pulse-gate evaluation stores (`PulseGateBlock.neighborChanged`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PulseUpdate {
    /// The `input_b` bit to store.
    pub input_b: bool,
    /// The `powered` bit to store.
    pub powered: bool,
    /// `Some(PULSE_DURATION)` when this evaluation arms the output's clear.
    pub clear_in: Option<u8>,
}

/// The stored bits after comparing the stored B with the world's B.
///
/// A rising edge arms the output and starts the clear; a falling edge only
/// cuts the stored input, so the output keeps emitting until its clear (that
/// is what makes a short notch still produce a full pulse). A steady input
/// changes nothing.
pub fn pulse_update(stored_input_b: bool, new_input_b: bool, powered: bool) -> PulseUpdate {
    match (stored_input_b, new_input_b) {
        (false, true) => PulseUpdate {
            input_b: true,
            powered: true,
            clear_in: Some(PULSE_DURATION),
        },
        (true, false) => PulseUpdate {
            input_b: false,
            powered,
            clear_in: None,
        },
        (stored, _) => PulseUpdate {
            input_b: stored,
            powered,
            clear_in: None,
        },
    }
}

/// The outcome of one tick of a pending pulse clear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PulseTick {
    /// The clear is still pending, with this many ticks left.
    Waiting(u8),
    /// The output clears now.
    Cleared,
}

/// Advance a pending clear: `PULSE_DURATION` ticks after arming, it clears.
pub fn pulse_tick(remaining: u8) -> PulseTick {
    match remaining {
        0 | 1 => PulseTick::Cleared,
        left => PulseTick::Waiting(left - 1),
    }
}

/// The output rotation index for an already-known attachment and output face
/// (`BlockStateUtil.getRotationIndexForDirection`; the placement path).
pub fn rotation_for_output(attachment: Dir, output: Dir) -> Option<u8> {
    OUTPUT_TABLE[attachment.index()]
        .iter()
        .position(|candidate| *candidate == output)
        .map(|index| index as u8)
}

/// The pin's `Direction::approximate_nearest` threshold: the smallest positive
/// `f32` (vanilla starts from `Double.MIN_VALUE`), so a face wins on any
/// positive dot and `NORTH` is the answer when none is positive.
const NEAREST_FLOOR: f32 = f32::from_bits(1);

/// The face a click at `hit` points at, with the attachment axis ignored
/// (`BlockStateUtil.getOutputDirectionFromRelativeHitVec`). `hit` is the click
/// location relative to the placed cell's lower corner, so a face centre is
/// `[0.5, 0.5, 0.5]`.
pub fn output_from_hit(attachment: Dir, [x, y, z]: [f64; 3]) -> Dir {
    let flatten = |value: f64| (value * 2.0 - 1.0) as f32;
    let [vx, vy, vz] = match attachment {
        Dir::Down | Dir::Up => [flatten(x), 0.0, flatten(z)],
        Dir::North | Dir::South => [flatten(x), flatten(y), 0.0],
        Dir::West | Dir::East => [0.0, flatten(y), flatten(z)],
    };
    let mut best = NEAREST_FLOOR;
    let mut result = Dir::North;
    for direction in Dir::ALL {
        let [nx, ny, nz] = direction.normal();
        let dot = vx * nx + vy * ny + vz * nz;
        if dot > best {
            best = dot;
            result = direction;
        }
    }
    result
}

/// The rotation index a click at `hit` gives an already-known attachment
/// (`getRotationIndexForDirection`). A dead-centre click answers `NORTH`,
/// which is not in a horizontal attachment's row, so it falls back to 0.
pub fn placement_rotation(attachment: Dir, hit: [f64; 3]) -> u8 {
    rotation_for_output(attachment, output_from_hit(attachment, hit)).unwrap_or(0)
}

/// The state a plate takes when placed against `clicked_face` at `hit`
/// (`PlateBlockStateProperties.getStateForPlacedGatePlate`): the attachment
/// (the face toward the support) and the rotation index. The click point, not
/// the placer's yaw, chooses the rotation.
pub fn placement(clicked_face: Dir, hit: [f64; 3]) -> (Dir, u8) {
    let attachment = clicked_face.opposite();
    (attachment, placement_rotation(attachment, hit))
}

/// The rotation after a wooden-rod click (`RedstonePlateBlock.useItemOn`):
/// one quarter turn clockwise, wrapping at four.
pub fn rod_rotation(rotation: u8) -> u8 {
    (rotation + 1) % 4
}

// ---------------------------------------------------------------------------
// Red alloy wire (`wires/` upstream).
//
// A wire block is six subwires, one per interior face: the boolean properties
// `down`, `up`, `north`, `south`, `west`, `east` say whether a subwire is
// attached to the block on that face, and a subwire needs a sturdy neighbour.
// Adjacent wire blocks connect through a shared attachment face (two floor
// wires side by side both have `down`); a corner between perpendicular
// attachments connects round the convex edge. The power itself is not a state
// property: upstream keeps six values in the block entity, because 16 power
// values per face would be too many states.
//
// Upstream stores power doubled: a redstone source of signal `s` gives
// `2s - 1`, every step along a wire subtracts 1, and `getSignal` answers
// `power / 2`. So a wire next to a 15 source carries 14, and the signal drops
// by one every two wire blocks (it reaches 0 at the 29th wire).
// ---------------------------------------------------------------------------

/// The axis a face is perpendicular to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

impl Dir {
    fn axis(self) -> Axis {
        match self {
            Dir::Down | Dir::Up => Axis::Y,
            Dir::North | Dir::South => Axis::Z,
            Dir::West | Dir::East => Axis::X,
        }
    }
}

/// A wire block's six subwires, in [`Dir::ALL`] order (Minecraft's
/// `Direction` ordinals). `true` means a subwire is attached to the block on
/// that face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attached([bool; 6]);

impl Attached {
    /// The empty wire block: the state upstream starts from and builds up.
    pub const NONE: Attached = Attached([false; 6]);

    /// A wire with the given subwires (the state's six booleans, in order).
    pub fn from_bits(bits: [bool; 6]) -> Attached {
        Attached(bits)
    }

    /// The six booleans, in `Dir::ALL` order.
    pub fn bits(self) -> [bool; 6] {
        self.0
    }

    /// Whether a subwire is attached to `face`.
    pub fn get(self, face: Dir) -> bool {
        self.0[face.index()]
    }

    /// The same wire with `face`'s subwire set or cleared.
    pub fn with(self, face: Dir, attached: bool) -> Attached {
        let mut bits = self.0;
        bits[face.index()] = attached;
        Attached(bits)
    }

    /// How many subwires the block has: the item count a broken wire or cable
    /// drops (upstream's `WireCountLootFunction`), and 0 for the empty block.
    pub fn count(self) -> usize {
        self.0.iter().filter(|attached| **attached).count()
    }

    /// The empty wire block (no subwires), which upstream removes.
    pub fn is_empty(self) -> bool {
        self.0.iter().all(|attached| !attached)
    }

    /// The face whose box the content model uses for the wire's shape: the
    /// first attached face in direction order (`facing` in the content file).
    pub fn primary(self) -> Option<Dir> {
        Dir::ALL.into_iter().find(|face| self.get(*face))
    }
}

/// `AbstractWireBlock.getStateForPlacement`: a fresh wire attaches to the
/// clicked face's opposite, i.e. the face toward the block that was clicked.
pub fn placement_attached(clicked_face: Dir) -> Attached {
    Attached::NONE.with(clicked_face.opposite(), true)
}

/// `AbstractWireBlock.updateShape`: a set subwire follows its neighbour's
/// sturdiness. A clear subwire is never added here — only placement and the
/// wire item add subwires.
pub fn retained(attached: Attached, direction: Dir, neighbour_sturdy: bool) -> Attached {
    if attached.get(direction) {
        attached.with(direction, neighbour_sturdy)
    } else {
        attached
    }
}

#[allow(dead_code)]
/// `AbstractWireBlock.canSurvive`: every attached subwire needs a sturdy
/// neighbour face. The empty wire block needs nothing.
pub fn can_survive(attached: Attached, sturdy: impl Fn(Dir) -> bool) -> bool {
    Dir::ALL
        .into_iter()
        .all(|face| !attached.get(face) || sturdy(face))
}

/// The four faces sharing an edge with `face` (the ones perpendicular to it).
pub fn perpendicular(face: Dir) -> [Dir; 4] {
    match face {
        Dir::Down | Dir::Up => [Dir::North, Dir::South, Dir::West, Dir::East],
        Dir::North | Dir::South => [Dir::Down, Dir::Up, Dir::West, Dir::East],
        Dir::West | Dir::East => [Dir::Down, Dir::Up, Dir::North, Dir::South],
    }
}

/// The raw (doubled) power a source of signal `signal` gives the wire beside
/// it (`PoweredWireBlock.updatePowerAfterBlockUpdate`: `2s - 1`).
pub fn source_power(signal: i32) -> u8 {
    let signal = signal.clamp(0, 15);
    if signal == 0 {
        0
    } else {
        (signal * 2 - 1) as u8
    }
}

/// The signal a raw power carries (`PoweredWireBlock.getPower`: `power / 2`).
pub fn power_signal(raw: u8) -> i32 {
    i32::from(raw) / 2
}

/// One step along a wire: raw power minus one.
pub fn decay(raw: u8) -> u8 {
    raw.saturating_sub(1)
}

/// The power of every subwire, from the strongest of its supply:
///
/// * `support[face]` — the [`source_power`] of the block on that face;
/// * the wire's own other subwires, one step away;
/// * `linked[face]` — the strongest raw power of a connected neighbouring
///   wire's subwire, one step away.
///
/// Detached faces carry no power. The computation starts from zero and only
/// raises values, like upstream's worklist, so a removed source drops the
/// whole branch (and six passes always reach a wire's six faces).
pub fn wire_power(attached: Attached, support: [u8; 6], linked: [u8; 6]) -> [u8; 6] {
    let mut power = [0u8; 6];
    for _ in 0..6 {
        let mut changed = false;
        for face in Dir::ALL {
            if !attached.get(face) {
                continue;
            }
            let index = face.index();
            let mut next = support[index];
            for other in perpendicular(face) {
                if attached.get(other) {
                    next = next.max(decay(power[other.index()]));
                }
            }
            next = next.max(decay(linked[index]));
            if next > power[index] {
                power[index] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    power
}

/// The signal a wire gives a reader in direction `travel` (the `face` of
/// `block_signal`): the subwire toward the reader wins, otherwise the
/// strongest subwire sharing an edge with that face
/// (`PoweredWireBlock.getPower`).
pub fn wire_signal(power: [u8; 6], attached: Attached, travel: Dir) -> i32 {
    if attached.get(travel) {
        return power_signal(power[travel.index()]);
    }
    perpendicular(travel)
        .into_iter()
        .filter(|face| attached.get(*face))
        .map(|face| power_signal(power[face.index()]))
        .max()
        .unwrap_or(0)
}

/// `canWireConnectToAdjacentWireOrCable`: a neighbouring wire connects to this
/// block's subwire on `face` when it has a subwire on that same face, and
/// `direction` (from the neighbour to this block) is not along that face's
/// axis.
pub fn shares_attachment(face: Dir, neighbour: Attached, direction: Dir) -> bool {
    neighbour.get(face) && face.axis() != direction.axis()
}

/// A foreign block's own power as a source, from the two answers the engine gives.
///
/// `LevelRef::signal` is `SignalGetter.getSignal`, "power it conducts included", so a
/// **conductor** answers the power it receives — a lit redstone lamp beside a wire answers
/// that wire's power — while a source answers its own. `LevelRef::direct_signal_to` is
/// `getDirectSignalTo`, exactly what the block receives, so a block that reports nothing but
/// what it receives is a conductor (or a sink), not a source. Vanilla's sources are named by
/// [`vanilla_source`], so the corner where a source happens to receive exactly what it emits
/// is still a source; an emitter the table does not know (another mod's machine) is
/// recognised by the two answers differing. Measured at this pin: the table is in
/// `ports/redstonepen/notes.md`.
#[must_use]
pub fn foreign_signal(plain: i32, conducted: i32, known_source: bool) -> i32 {
    let plain = plain.clamp(0, 15);
    if !known_source && plain == conducted.clamp(0, 15) {
        0
    } else {
        plain
    }
}

/// Whether `block` is one of vanilla's redstone sources: a block whose own signal is its
/// power rather than the power it conducts. A redstone lamp, stone and air are not: they
/// only pass on what they receive.
#[must_use]
pub fn vanilla_source(block: &str) -> bool {
    let name = block.strip_prefix("minecraft:").unwrap_or(block);
    matches!(
        name,
        "redstone_block"
            | "redstone_torch"
            | "redstone_wall_torch"
            | "lever"
            | "repeater"
            | "comparator"
            | "observer"
            | "target"
            | "daylight_detector"
            | "trapped_chest"
            | "detector_rail"
            | "lectern"
            | "sculk_sensor"
            | "calibrated_sculk_sensor"
            | "tripwire_hook"
            | "redstone_wire"
            | "lightning_rod"
    ) || name.ends_with("_button")
        || name.ends_with("_pressure_plate")
}

// ---------------------------------------------------------------------------
// Coloured network cables (`wires/ColoredCableBlock*` upstream).
//
// Upstream registers **sixteen blocks**, one per `DyeColor`, all extending
// `PoweredWireBlock` with the same six subwire booleans and the same doubled
// per-face power; the dye is the block identity, not a state property and not
// a block-entity field (`MoreRed.java:231-234`, `ColoredCableBlock.java:38-44`).
// The only behavioural difference is the connection rule: a coloured cable
// refuses another coloured cable of a different dye and otherwise falls
// through to the plain wire's attachment rule
// (`ColoredCableBlock.java:52-59`), so a coloured cable and a plain red-alloy
// wire still join. A cable also reads and writes the bundled-cable channeled
// capability on its dye channel (`ColoredCableBlockEntity.java:55-85`); the
// port has no capability API, so a coloured cable behaves as a single-channel
// wire whose only seed is a vanilla signal, exactly like the plain wire.
// ---------------------------------------------------------------------------

/// The sixteen dyed network cables, in vanilla `DyeColor` ordinal order
/// (white = 0 … black = 15), which is also the bundled cable's channel index
/// upstream (`ColoredCableBlockEntity.java:121`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CableColor {
    White,
    Orange,
    Magenta,
    LightBlue,
    Yellow,
    Lime,
    Pink,
    Gray,
    LightGray,
    Cyan,
    Purple,
    Blue,
    Brown,
    Green,
    Red,
    Black,
}

impl CableColor {
    /// Every dye, in ordinal order.
    pub const ALL: [CableColor; 16] = [
        CableColor::White,
        CableColor::Orange,
        CableColor::Magenta,
        CableColor::LightBlue,
        CableColor::Yellow,
        CableColor::Lime,
        CableColor::Pink,
        CableColor::Gray,
        CableColor::LightGray,
        CableColor::Cyan,
        CableColor::Purple,
        CableColor::Blue,
        CableColor::Brown,
        CableColor::Green,
        CableColor::Red,
        CableColor::Black,
    ];

    /// `DyeColor.ordinal()`.
    #[allow(dead_code)] // used by the standalone tests; the injected module only needs `from_id`
    pub fn index(self) -> usize {
        self as usize
    }

    /// The content/code block id (`ObjectNames.NETWORK_CABLES_BY_COLOR`).
    pub fn id(self) -> &'static str {
        match self {
            CableColor::White => "morered:white_network_cable",
            CableColor::Orange => "morered:orange_network_cable",
            CableColor::Magenta => "morered:magenta_network_cable",
            CableColor::LightBlue => "morered:light_blue_network_cable",
            CableColor::Yellow => "morered:yellow_network_cable",
            CableColor::Lime => "morered:lime_network_cable",
            CableColor::Pink => "morered:pink_network_cable",
            CableColor::Gray => "morered:gray_network_cable",
            CableColor::LightGray => "morered:light_gray_network_cable",
            CableColor::Cyan => "morered:cyan_network_cable",
            CableColor::Purple => "morered:purple_network_cable",
            CableColor::Blue => "morered:blue_network_cable",
            CableColor::Brown => "morered:brown_network_cable",
            CableColor::Green => "morered:green_network_cable",
            CableColor::Red => "morered:red_network_cable",
            CableColor::Black => "morered:black_network_cable",
        }
    }

    /// The dye a block id names, or `None` for any other block.
    pub fn from_id(id: &str) -> Option<CableColor> {
        CableColor::ALL.into_iter().find(|colour| colour.id() == id)
    }
}

/// Whether two wire-like blocks share an electrical network
/// (`ColoredCableBlock.canConnectToAdjacentWireOrCable`): two coloured cables
/// connect only at the same dye; a coloured cable and the plain red-alloy wire
/// (`None`) connect; two plain wires connect.
pub fn cables_connect(left: Option<CableColor>, right: Option<CableColor>) -> bool {
    match (left, right) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// The coplanar connection rule: a neighbour's subwire on `face` links when
/// the two blocks share a network (see [`cables_connect`]) and `direction`
/// (from the neighbour to this block) is not along `face`'s axis
/// (`AbstractWireBlock.canWireConnectToAdjacentWireOrCable:159-184`).
pub fn connects(
    face: Dir,
    self_colour: Option<CableColor>,
    neighbour: Attached,
    neighbour_colour: Option<CableColor>,
    direction: Dir,
) -> bool {
    cables_connect(self_colour, neighbour_colour) && shares_attachment(face, neighbour, direction)
}

/// The convex-edge (diagonal) rule: upstream's
/// `AbstractWireBlock.canWireConnectToAdjacentWireOrCable` only takes that
/// branch when `neighborBlock == thisState.getBlock()`, so the diagonal wire
/// must be the same block. `None` is the plain wire; `Some(dye)` a cable.
pub fn same_wire_block(left: Option<CableColor>, right: Option<CableColor>) -> bool {
    left == right
}

// ---------------------------------------------------------------------------
// Bundled cable (`wires/BundledCable*` upstream).
//
// A bundled cable is the same six-subwire block as the red alloy wire, but it
// carries sixteen independent channels instead of one signal. The power lives
// in the block entity as six faces x sixteen channels of doubled units; a
// channel's seed is `2s - 1`, every cable step costs one, and a reader gets
// `power / 2` (upstream's `BundledCableBlock.updatePowerAfterBlockUpdate` and
// `BundledCableBlockEntity`).
//
// Upstream's bundled cable has no vanilla input and no vanilla output: its
// only supply is another channel-aware block. The relay plate is the port's
// single-channel bridge: it seeds `2s - 1` from a vanilla signal and answers
// `power / 2` to a vanilla reader, on the channel its state selects.
// ---------------------------------------------------------------------------

/// The sixteen channels a bundled cable carries (`BundledCableBlockEntity`'s
/// `byte[6][16] power`, indexed by dye-colour ordinal upstream).
pub const CHANNELS: usize = 16;

/// A bundled-cable channel, `0..15`. Construction is fallible so an
/// out-of-range state cannot reach the power array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Channel(u8);

impl Channel {
    /// Every channel, in index order.
    pub const ALL: [Channel; CHANNELS] = [
        Channel(0),
        Channel(1),
        Channel(2),
        Channel(3),
        Channel(4),
        Channel(5),
        Channel(6),
        Channel(7),
        Channel(8),
        Channel(9),
        Channel(10),
        Channel(11),
        Channel(12),
        Channel(13),
        Channel(14),
        Channel(15),
    ];

    /// The channel with this index, or `None` for 16 and above.
    pub fn new(value: u8) -> Option<Channel> {
        (usize::from(value) < CHANNELS).then_some(Channel(value))
    }

    /// The channel a state property spells (`channel=3`), or `None`.
    pub fn from_property(value: &str) -> Option<Channel> {
        value.parse::<u8>().ok().and_then(Channel::new)
    }

    /// The array index of this channel.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The channel number as a state property spells it.
    #[allow(dead_code)]
    pub fn value(self) -> u8 {
        self.0
    }
}

/// The doubled power of every subwire of a bundled cable, per channel
/// (upstream's `byte[6][16] power`, first index the face in [`Dir::ALL`]
/// order).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CablePower([[u8; CHANNELS]; 6]);

impl CablePower {
    /// No power on any face or channel.
    pub const NONE: CablePower = CablePower([[0; CHANNELS]; 6]);

    /// The doubled power of `channel` on `face`.
    pub fn get(self, face: Dir, channel: Channel) -> u8 {
        self.0[face.index()][channel.index()]
    }

    /// Store `power` for `channel` on `face`.
    pub fn set(&mut self, face: Dir, channel: Channel, power: u8) {
        self.0[face.index()][channel.index()] = power;
    }

    /// Raise the channel's power to `power` if it is stronger. A face takes
    /// the strongest of its suppliers (`BundledCableBlock`'s `Math.max`), so
    /// an unpowered supplier must not clear another one's seed.
    pub fn raise(&mut self, face: Dir, channel: Channel, power: u8) {
        let slot = &mut self.0[face.index()][channel.index()];
        *slot = (*slot).max(power);
    }

    /// The six face values of one channel, in [`Dir::ALL`] order.
    pub fn channel(self, channel: Channel) -> [u8; 6] {
        let mut values = [0u8; 6];
        for (face, value) in Dir::ALL.into_iter().zip(values.iter_mut()) {
            *value = self.get(face, channel);
        }
        values
    }
}

/// The sixteen-channel solve. Each channel follows the single-signal wire rule
/// over the same six faces: `seeds[face][channel]` is the strongest supply on
/// the face's attachment, `linked[face][channel]` the strongest connected
/// neighbour, and one step costs one unit. Detached faces stay zero.
pub fn cable_power(attached: Attached, seeds: CablePower, linked: CablePower) -> CablePower {
    let mut result = CablePower::NONE;
    for channel in Channel::ALL {
        let solved = wire_power(attached, seeds.channel(channel), linked.channel(channel));
        for (face, value) in Dir::ALL.into_iter().zip(solved) {
            result.set(face, channel, value);
        }
    }
    result
}

/// The seed a relay plate puts on its channel for a vanilla signal of
/// `signal`: `2s - 1`, the same doubled convention as a wire source.
pub fn relay_seed(signal: i32) -> u8 {
    source_power(signal)
}

/// The vanilla signal a relay plate answers for `channel`: half the strongest
/// attached subwire power of the cable it reads. A detached face carries
/// nothing, so a relay with no adjacent attached cable answers 0.
pub fn relay_signal(power: CablePower, attached: Attached, channel: Channel) -> i32 {
    let strongest = Dir::ALL
        .into_iter()
        .filter(|face| attached.get(*face))
        .map(|face| power.get(face, channel))
        .max()
        .unwrap_or(0);
    power_signal(strongest)
}

// ---------------------------------------------------------------------------
// Bitwise logic plates (`bitwise_logic/` upstream).
//
// Six plates operate on all sixteen cable channels at once: a single-input
// diode and NOT (input on the output's opposite), and two-input AND/OR/XOR/XNOR
// (inputs A and C). The function is applied per channel; a passing channel is
// stored as 31 and a failing one as 0 in the plate's block entity. The plate
// has no vanilla signal: its only consumer is a bundled cable on its output
// face, which steps the supplier down by one (so 31 becomes 30, a maximum
// signal of 15 after the cable halves it).
// ---------------------------------------------------------------------------

/// The `bitwise_logic/` plates (`MoreRed.java:251-260`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitwiseGate {
    /// `bitwise_diode`, `LogicFunctions.INPUT_B`.
    Diode,
    /// `bitwise_not_gate`, `LogicFunctions.NOT_B`.
    Not,
    /// `bitwise_and_gate`, `LogicFunctions.AND_2`.
    And,
    /// `bitwise_or_gate`, `LogicFunctions.OR`.
    Or,
    /// `bitwise_xor_gate`, `LogicFunctions.XOR_AC`.
    Xor,
    /// `bitwise_xnor_gate`, `LogicFunctions.XNOR_AC`.
    Xnor,
}

impl BitwiseGate {
    /// Every bitwise plate, in registration order (`MoreRed.java:255-260`).
    #[allow(dead_code)]
    pub const ALL: [BitwiseGate; 6] = [
        BitwiseGate::Diode,
        BitwiseGate::Not,
        BitwiseGate::And,
        BitwiseGate::Or,
        BitwiseGate::Xor,
        BitwiseGate::Xnor,
    ];

    /// The plate whose block id is `id` (`morered:bitwise_and_gate`).
    pub fn from_id(id: &str) -> Option<BitwiseGate> {
        Some(match id.strip_prefix("morered:")? {
            "bitwise_diode" => BitwiseGate::Diode,
            "bitwise_not_gate" => BitwiseGate::Not,
            "bitwise_and_gate" => BitwiseGate::And,
            "bitwise_or_gate" => BitwiseGate::Or,
            "bitwise_xor_gate" => BitwiseGate::Xor,
            "bitwise_xnor_gate" => BitwiseGate::Xnor,
            _ => return None,
        })
    }

    /// The input sides the plate reads. The single-input plates read B (the
    /// output's opposite, `outputDir.getOpposite()`); the two-input plates
    /// read A and C.
    pub fn sides(self) -> &'static [Side] {
        match self {
            BitwiseGate::Diode | BitwiseGate::Not => &[Side::B],
            BitwiseGate::And | BitwiseGate::Or | BitwiseGate::Xor | BitwiseGate::Xnor => {
                &[Side::A, Side::C]
            }
        }
    }

    /// The plate's `LogicFunction`. Sides it does not read are false in the
    /// state and ignored here.
    pub fn apply(self, a: bool, b: bool, c: bool) -> bool {
        match self {
            BitwiseGate::Diode => b,
            BitwiseGate::Not => !b,
            BitwiseGate::And => a && c,
            BitwiseGate::Or => a || c,
            BitwiseGate::Xor => a ^ c,
            BitwiseGate::Xnor => !(a ^ c),
        }
    }
}

/// The power a bitwise plate stores on a channel whose function passes
/// (`updatePower`: `outputBit ? 31 : 0`). A failing channel stores 0.
pub const BITWISE_POWER: u8 = 31;

/// Sixteen channel powers in doubled units, the plate's `byte[16] power`
/// (`ChanneledPowerStorageBlockEntity`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChannelPower([u8; CHANNELS]);

impl ChannelPower {
    /// No power on any channel.
    pub const NONE: ChannelPower = ChannelPower([0; CHANNELS]);

    /// The doubled power of `channel`.
    pub fn get(self, channel: Channel) -> u8 {
        self.0[channel.index()]
    }

    /// Store `power` for `channel`.
    pub fn set(&mut self, channel: Channel, power: u8) {
        self.0[channel.index()] = power;
    }

    /// Whether `channel` carries any power (the plate's input bit test:
    /// `getPowerOnChannel(...) > 0`).
    pub fn bit(self, channel: Channel) -> bool {
        self.get(channel) > 0
    }
}

/// One plate evaluation: apply the gate's function to each channel on its own,
/// storing [`BITWISE_POWER`] for a passing channel and 0 otherwise.
///
/// The single-input plates read `b` only; the two-input plates read `a` and
/// `c`. Missing inputs are [`ChannelPower::NONE`], which is an unpowered input.
pub fn bitwise_output(
    gate: BitwiseGate,
    a: ChannelPower,
    b: ChannelPower,
    c: ChannelPower,
) -> ChannelPower {
    let mut power = ChannelPower::NONE;
    for channel in Channel::ALL {
        let bit = gate.apply(a.bit(channel), b.bit(channel), c.bit(channel));
        power.set(channel, if bit { BITWISE_POWER } else { 0 });
    }
    power
}

/// What a bundled cable answers a plate reading from it
/// (`BundledCableBlockEntity.SidedPowerSupplier.getPowerOnChannel`).
///
/// `toward_reader` is the direction from the cable to the plate (the side the
/// plate queries); `reader_attachment` is the plate's attachment direction
/// (the `wireFace` it passes). The subwire toward the plate wins; otherwise
/// the cable answers with its subwire on the plate's attachment face (zero
/// when that face is detached, because a detached face stores no power).
pub fn cable_channel_power(
    power: CablePower,
    attached: Attached,
    toward_reader: Dir,
    reader_attachment: Dir,
) -> ChannelPower {
    let face = if attached.get(toward_reader) {
        toward_reader
    } else {
        reader_attachment
    };
    let mut answer = ChannelPower::NONE;
    for channel in Channel::ALL {
        answer.set(channel, power.get(face, channel));
    }
    answer
}

/// The cable's `- 1` step from a channel supplier
/// (`BundledCableBlock.updatePowerAfterBlockUpdate`: `max(0, supplier - 1)`).
/// A bitwise plate's 31 therefore seeds 30; a failing 0 seeds 0.
pub fn plate_seed(plate_power: u8) -> u8 {
    plate_power.saturating_sub(1)
}

// ---------------------------------------------------------------------------
// Wire posts (`wire_post/` upstream).
//
// A red-alloy wire post is a rod attached to one block face. It stores a
// vanilla `POWER` (0-15) in its state, reads the redstone signal on its
// connectable sides, and takes the strongest of that and its linked posts,
// minus one (`getNewPower`). Two posts are linked by a spool click; the link
// is stored on both posts as a set of relative positions in the block
// entity's `connections` list. The three blocks differ only in which sides
// they connect to redstone: the plain post toward its support, the empty
// plate nowhere, and the relay plate on the four sides around it. The link
// itself is the same for all three, so a plate can be an intermediate node.
//
// This is not the bundled-cable post family (which carries sixteen channels
// in `BundledCablePostBlockEntity`): those blocks have no vanilla signal at
// all, so a lamp cannot be driven from one at the pin.
// ---------------------------------------------------------------------------

/// The three red-alloy wire posts (`MoreRed.java:215-221`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostKind {
    /// `redwire_post`: connects toward its attachment (the support block).
    Plain,
    /// `redwire_post_plate`: link only, no vanilla connection.
    Plate,
    /// `redwire_post_relay_plate`: connects on the four perpendicular sides.
    Relay,
}

impl PostKind {
    /// The post whose block id is `id` (`morered:redwire_post`).
    pub fn from_id(id: &str) -> Option<PostKind> {
        Some(match id.strip_prefix("morered:")? {
            "redwire_post" => PostKind::Plain,
            "redwire_post_plate" => PostKind::Plate,
            "redwire_post_relay_plate" => PostKind::Relay,
            _ => return None,
        })
    }

    /// Every id this model knows, in registration order.
    #[allow(dead_code)]
    pub const ALL: [PostKind; 3] = [PostKind::Plain, PostKind::Plate, PostKind::Relay];

    /// `getConnectableDirections`: the sides whose neighbour the post reads,
    /// and the sides it emits into (the plain post's attachment, the relay's
    /// four perpendicular faces, none for the empty plate).
    pub fn connections(self, attachment: Dir) -> Vec<Dir> {
        match self {
            PostKind::Plain => vec![attachment],
            PostKind::Plate => Vec::new(),
            PostKind::Relay => perpendicular(attachment).to_vec(),
        }
    }

    /// Whether a signal travelling out on `face` leaves the post. Upstream's
    /// `getSignal` asks whether the connectable set contains the caller's
    /// side, i.e. the travel direction's opposite: a post reads from its
    /// support and emits away from it.
    pub fn emits(self, attachment: Dir, face: Dir) -> bool {
        self.connections(attachment).contains(&face.opposite())
    }
}

/// `getNewPower`: the strongest of the neighbour read and the linked posts,
/// one level down. The subtraction applies to the neighbour too, so a redstone
/// block (15) beside a post stores 14.
pub fn post_power(neighbour_power: u8, linked: &[u8]) -> u8 {
    linked
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .max(neighbour_power)
        .saturating_sub(1)
}

/// The signal a post answers on `face` (the hook's travel direction): its
/// stored power on a connectable face, nothing elsewhere.
pub fn post_signal(power: u8, attachment: Dir, kind: PostKind, face: Dir) -> i32 {
    if power == 0 || !kind.emits(attachment, face) {
        0
    } else {
        i32::from(power.min(15))
    }
}

/// The default link range (`ServerConfig.max_wire_plinth_connection_range`).
pub const POST_LINK_RANGE: f64 = 32.0;

/// The most links one post keeps. Upstream has no cap, so a post's peer list grows with every
/// spool click and every solve clones it and enqueues it; the port bounds the adjacency so a
/// saved post stays a fixed amount of work (the review finding). 32 is twice the links a
/// useful alloy-wire junction needs and keeps one visit's fan-out small.
pub const MAX_POST_LINKS: usize = 32;

/// Whether `pos` is inside chunk `chunk` (`[x, z]`), with the level's own negative-coordinate
/// floor, so a chunk unload can drop exactly its cached positions.
#[must_use]
pub fn in_chunk(pos: [i32; 3], chunk: [i32; 2]) -> bool {
    pos[0].div_euclid(16) == chunk[0] && pos[2].div_euclid(16) == chunk[1]
}

/// `BlockPos.closerThan`: strictly inside the range, Euclidean.
pub fn links_within(a: [i32; 3], b: [i32; 3], range: f64) -> bool {
    if range <= 0.0 {
        return false;
    }
    let distance_sq = [0, 1, 2]
        .map(|axis| i64::from(a[axis] - b[axis]))
        .into_iter()
        .map(|delta| delta * delta)
        .sum::<i64>();
    (distance_sq as f64) < range * range
}

/// A post's stored links, as offsets relative to the post
/// (`WirePostBlockEntity.remoteConnections`, saved as `connections`). The
/// port stores plain offsets; the `TRANSFORM` normalization of the Java
/// version has no state property to read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostLinks(Vec<[i32; 3]>);

impl PostLinks {
    /// Links from their relative offsets, as saved.
    pub fn from_offsets(offsets: Vec<[i32; 3]>) -> PostLinks {
        PostLinks(offsets)
    }

    /// The saved relative offsets.
    pub fn offsets(&self) -> &[[i32; 3]] {
        &self.0
    }

    /// Whether a link to `offset` exists.
    pub fn contains(&self, offset: [i32; 3]) -> bool {
        self.0.contains(&offset)
    }

    /// Add a link; false when it was already there or the post is at [`MAX_POST_LINKS`].
    pub fn add(&mut self, offset: [i32; 3]) -> bool {
        if self.contains(offset) || self.0.len() >= MAX_POST_LINKS {
            false
        } else {
            self.0.push(offset);
            true
        }
    }

    /// Whether the post has all the links it may keep.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.0.len() >= MAX_POST_LINKS
    }

    /// Drop a link; false when there was none.
    pub fn remove(&mut self, offset: [i32; 3]) -> bool {
        let before = self.0.len();
        self.0.retain(|candidate| *candidate != offset);
        self.0.len() != before
    }

    /// Every linked position, from this post's position.
    pub fn absolute(&self, pos: [i32; 3]) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.0
            .iter()
            .map(move |offset| [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]])
    }
}

/// What a spool click on a post does (`WireSpoolItem.onUseOnPost`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpoolClick {
    /// No first post yet, or the clicked post is too far: remember this one.
    Select,
    /// The first post was clicked again: forget it.
    ClearSelection,
    /// The clicked post is already linked to the first one: cut the link.
    Cut,
    /// Unlinked and in range: join the two posts.
    Link,
    /// Unlinked and beyond the range: start again from the clicked post.
    TooFar,
}

/// The first-post decision tree of `WireSpoolItem.onUseOnPost`, given the
/// stored first post, the clicked post, whether the clicked post already has a
/// link to the first one, and the configured range.
pub fn spool_click(
    selected: Option<[i32; 3]>,
    clicked: [i32; 3],
    connected: bool,
    range: f64,
) -> SpoolClick {
    let Some(selected) = selected else {
        return SpoolClick::Select;
    };
    if selected == clicked {
        return SpoolClick::ClearSelection;
    }
    if connected {
        return SpoolClick::Cut;
    }
    if links_within(selected, clicked, range) {
        SpoolClick::Link
    } else {
        SpoolClick::TooFar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_table_is_upstreams() {
        // `BlockStateUtil.OUTPUT_TABLE` (DUNSWE, DUNSWE order).
        assert_eq!(output_direction(Dir::Down, 0), Dir::North);
        assert_eq!(output_direction(Dir::Down, 3), Dir::West);
        assert_eq!(output_direction(Dir::Up, 1), Dir::West);
        assert_eq!(output_direction(Dir::North, 2), Dir::Down);
        assert_eq!(output_direction(Dir::South, 3), Dir::East);
        assert_eq!(output_direction(Dir::West, 0), Dir::Up);
        assert_eq!(output_direction(Dir::East, 1), Dir::South);
    }

    #[test]
    fn every_attachment_has_four_distinct_outputs() {
        for attachment in Dir::ALL {
            let mut seen = Vec::new();
            for rotation in 0..4 {
                let output = output_direction(attachment, rotation);
                assert!(!seen.contains(&output), "{attachment:?} repeats {output:?}");
                seen.push(output);
            }
        }
    }

    #[test]
    fn inputs_are_clockwise_from_the_output() {
        // Attached to the top (DOWN), output north: A east, B south, C west.
        assert_eq!(input_direction(Dir::Down, 0, Side::A), Dir::East);
        assert_eq!(input_direction(Dir::Down, 0, Side::B), Dir::South);
        assert_eq!(input_direction(Dir::Down, 0, Side::C), Dir::West);
        // The input direction is the output table at rotation + side.
        assert_eq!(
            input_direction(Dir::Down, 2, Side::A),
            output_direction(Dir::Down, 3)
        );
    }

    #[test]
    fn opposite_faces_pair_up() {
        for face in Dir::ALL {
            assert_eq!(face.opposite().opposite(), face);
            assert_ne!(face.opposite(), face);
        }
    }

    #[test]
    fn the_gate_ids_are_recognised() {
        assert_eq!(Gate::from_id("morered:diode"), Some(Gate::Diode));
        assert_eq!(Gate::from_id("morered:not_gate"), Some(Gate::Not));
        assert_eq!(Gate::from_id("morered:and_gate"), Some(Gate::And));
        assert_eq!(Gate::from_id("morered:nand_gate"), Some(Gate::Nand));
        assert_eq!(Gate::from_id("morered:or_gate"), Some(Gate::Or));
        assert_eq!(Gate::from_id("morered:nor_gate"), Some(Gate::Nor));
        assert_eq!(Gate::from_id("morered:xor_gate"), Some(Gate::Xor));
        assert_eq!(Gate::from_id("morered:xnor_gate"), Some(Gate::Xnor));
        assert_eq!(Gate::from_id("morered:multiplexer"), Some(Gate::Multiplex));
        assert_eq!(Gate::from_id("morered:and_2_gate"), Some(Gate::And2));
        assert_eq!(Gate::from_id("morered:nand_2_gate"), Some(Gate::Nand2));
        assert_eq!(Gate::from_id("morered:latch"), Some(Gate::Latch));
        assert_eq!(Gate::from_id("morered:pulse_gate"), Some(Gate::Pulse));
        assert_eq!(Gate::from_id("morered:wire"), None);
        assert_eq!(Gate::from_id("minecraft:and_gate"), None);
    }

    #[test]
    fn each_gate_uses_the_upstream_input_sides() {
        assert_eq!(Gate::Diode.sides(), &[Side::B]);
        assert_eq!(Gate::Not.sides(), &[Side::B]);
        assert_eq!(Gate::Pulse.sides(), &[Side::B]);
        assert_eq!(Gate::Xor.sides(), &[Side::A, Side::C]);
        assert_eq!(Gate::And2.sides(), &[Side::A, Side::C]);
        assert_eq!(Gate::And.sides(), &[Side::A, Side::B, Side::C]);
        assert!(!Gate::Diode.uses(Side::A));
        assert!(!Gate::Pulse.uses(Side::A));
        assert!(Gate::Multiplex.uses(Side::B));
    }

    #[test]
    fn the_function_table_matches_upstream() {
        // Truth tables for a few gates over all eight inputs.
        for a in [false, true] {
            for b in [false, true] {
                for c in [false, true] {
                    assert_eq!(Gate::Diode.apply(a, b, c), b);
                    assert_eq!(Gate::Not.apply(a, b, c), !b);
                    assert_eq!(Gate::And.apply(a, b, c), a && b && c);
                    assert_eq!(Gate::Nand.apply(a, b, c), !(a && b && c));
                    assert_eq!(Gate::Or.apply(a, b, c), a || b || c);
                    assert_eq!(Gate::Nor.apply(a, b, c), !(a || b || c));
                    assert_eq!(Gate::Xor.apply(a, b, c), a ^ c);
                    assert_eq!(Gate::Xnor.apply(a, b, c), !(a ^ c));
                    assert_eq!(Gate::And2.apply(a, b, c), a && c);
                    assert_eq!(Gate::Nand2.apply(a, b, c), !(a && c));
                    assert_eq!(Gate::Multiplex.apply(a, b, c), if b { c } else { a });
                }
            }
        }
    }

    #[test]
    fn a_gate_only_emits_on_its_output_face() {
        // Attached down, rotation 0, output north; AND with all three inputs on.
        let inputs = [true, true, true];
        assert_eq!(
            Gate::And.signal(inputs, Dir::Down, 0, Dir::North),
            Gate::OUTPUT_STRENGTH
        );
        for face in [Dir::South, Dir::East, Dir::West, Dir::Up, Dir::Down] {
            assert_eq!(Gate::And.signal(inputs, Dir::Down, 0, face), 0);
        }
    }

    #[test]
    fn a_false_function_emits_nothing() {
        assert_eq!(
            Gate::And.signal([true, false, true], Dir::Down, 0, Dir::North),
            0
        );
        assert_eq!(
            Gate::Nand.signal([true, false, true], Dir::Down, 0, Dir::North),
            Gate::OUTPUT_STRENGTH
        );
    }

    #[test]
    fn rotation_finds_the_output_face() {
        for attachment in Dir::ALL {
            for rotation in 0..4 {
                let output = output_direction(attachment, rotation);
                assert_eq!(rotation_for_output(attachment, output), Some(rotation));
            }
        }
    }

    #[test]
    fn a_placed_plate_attaches_toward_the_clicked_face() {
        // `PlateBlockStateProperties.getStateForPlacedGatePlate`: the attachment
        // (`facing`) is the clicked face's opposite, whatever the click point.
        for face in Dir::ALL {
            let (attachment, _) = placement(face, [0.5, 0.5, 0.5]);
            assert_eq!(attachment, face.opposite(), "clicked {face:?}");
        }
    }

    #[test]
    fn a_placed_plate_points_away_from_the_clicked_quadrant() {
        // Floor (clicked up): the north/east/south/west quadrant of the top
        // face gives rotation 0/1/2/3
        // (`BlockStateUtil.getOutputDirectionFromRelativeHitVec`).
        assert_eq!(placement(Dir::Up, [0.5, 1.0, 0.1]), (Dir::Down, 0));
        assert_eq!(placement(Dir::Up, [0.9, 1.0, 0.5]), (Dir::Down, 1));
        assert_eq!(placement(Dir::Up, [0.5, 1.0, 0.9]), (Dir::Down, 2));
        assert_eq!(placement(Dir::Up, [0.1, 1.0, 0.5]), (Dir::Down, 3));
        // Ceiling (clicked down): the Up row is north/west/south/east, so the
        // west and east quadrants give 1 and 3 (mirrored from the floor).
        assert_eq!(placement(Dir::Down, [0.5, 0.0, 0.1]), (Dir::Up, 0));
        assert_eq!(placement(Dir::Down, [0.1, 0.0, 0.5]), (Dir::Up, 1));
        assert_eq!(placement(Dir::Down, [0.5, 0.0, 0.9]), (Dir::Up, 2));
        assert_eq!(placement(Dir::Down, [0.9, 0.0, 0.5]), (Dir::Up, 3));
        // A wall (clicked north -> attached south): up/west/down/east.
        assert_eq!(placement(Dir::North, [0.5, 0.9, 1.0]), (Dir::South, 0));
        assert_eq!(placement(Dir::North, [0.1, 0.5, 1.0]), (Dir::South, 1));
        assert_eq!(placement(Dir::North, [0.5, 0.1, 1.0]), (Dir::South, 2));
        assert_eq!(placement(Dir::North, [0.9, 0.5, 1.0]), (Dir::South, 3));
        // The opposite wall (clicked south -> attached north): up/east/down/west.
        assert_eq!(placement(Dir::South, [0.5, 0.9, 0.0]), (Dir::North, 0));
        assert_eq!(placement(Dir::South, [0.9, 0.5, 0.0]), (Dir::North, 1));
        assert_eq!(placement(Dir::South, [0.5, 0.1, 0.0]), (Dir::North, 2));
        assert_eq!(placement(Dir::South, [0.1, 0.5, 0.0]), (Dir::North, 3));
        // The two remaining walls (clicked west/east): East's row is
        // up/south/down/north and West's is up/north/down/south.
        assert_eq!(placement(Dir::West, [1.0, 0.5, 0.9]), (Dir::East, 1));
        assert_eq!(placement(Dir::East, [0.0, 0.5, 0.9]), (Dir::West, 3));
    }

    #[test]
    fn every_face_and_rotation_is_reproduced_from_its_quadrant() {
        // The click 40% of the way toward the output face's edge must place
        // exactly that face's rotation, for all 6 attachments x 4 rotations.
        for attachment in Dir::ALL {
            for rotation in 0..4 {
                let output = output_direction(attachment, rotation);
                let [nx, ny, nz] = output.normal();
                let hit = [
                    0.5 + 0.4 * f64::from(nx),
                    0.5 + 0.4 * f64::from(ny),
                    0.5 + 0.4 * f64::from(nz),
                ];
                assert_eq!(
                    placement_rotation(attachment, hit),
                    rotation,
                    "{attachment:?} rotation {rotation} (output {output:?})"
                );
            }
        }
    }

    #[test]
    fn the_click_axis_along_the_attachment_is_ignored() {
        // Only the two in-plane components choose the output: the same in-plane
        // click with the depth varying picks the same quadrant.
        for depth in [0.0, 0.5, 1.0] {
            assert_eq!(placement_rotation(Dir::Down, [0.9, depth, 0.5]), 1);
            assert_eq!(placement_rotation(Dir::North, [0.5, 0.9, depth]), 0);
        }
    }

    #[test]
    fn a_dead_centre_click_falls_back_to_north() {
        // `Direction.getNearest(0, 0, 0)` answers NORTH, and the rotation is
        // that face's index in the attachment's row: 0 for four rows, but 1
        // for WEST and 3 for EAST, whose rows carry NORTH elsewhere.
        let expected = [0, 0, 0, 0, 1, 3];
        for (attachment, rotation) in Dir::ALL.into_iter().zip(expected) {
            assert_eq!(
                placement_rotation(attachment, [0.5, 0.5, 0.5]),
                rotation,
                "{attachment:?}"
            );
        }
    }

    #[test]
    fn a_click_outside_the_face_picks_the_nearest_edge() {
        // The relative hit may sit outside 0..1 (a click that slid off the
        // cell); the nearest-direction rule still answers the dominant quadrant.
        assert_eq!(placement_rotation(Dir::Down, [-0.2, 0.5, 0.5]), 3);
        assert_eq!(placement_rotation(Dir::Down, [1.4, 0.5, 0.5]), 1);
        assert_eq!(placement_rotation(Dir::Down, [0.5, 0.5, -0.3]), 0);
        assert_eq!(placement_rotation(Dir::Down, [0.5, 1.3, 1.2]), 2);
    }

    #[test]
    fn a_rod_click_advances_the_rotation_and_wraps() {
        // `RedstonePlateBlock.useItemOn`: (rotation + 1) % 4 per rod poke.
        let mut rotation = 0;
        for _ in 0..4 {
            rotation = rod_rotation(rotation);
        }
        assert_eq!(rotation, 0);
        assert_eq!(rod_rotation(0), 1);
        assert_eq!(rod_rotation(3), 0);
        // The output follows the row: attached south, r0 up, r1 west.
        assert_eq!(output_direction(Dir::South, rod_rotation(0)), Dir::West);
        assert_eq!(output_direction(Dir::South, rod_rotation(3)), Dir::Up);
    }

    #[test]
    fn the_latch_is_recognised_and_uses_a_and_c() {
        assert_eq!(Gate::from_id("morered:latch"), Some(Gate::Latch));
        assert_eq!(Gate::Latch.sides(), &[Side::A, Side::C]);
        assert!(!Gate::Latch.uses(Side::B));
    }

    #[test]
    fn the_latch_holds_and_resets() {
        // A sets, releasing A holds, C resets; C wins when both are on.
        assert!(latch_powered(false, true, false));
        assert!(latch_powered(true, false, false));
        assert!(!latch_powered(true, false, true));
        assert!(!latch_powered(false, false, true));
        assert!(!latch_powered(true, true, true));
        // A still sets the stored bit when both are on; the signal is silenced separately.
        assert!(latch_powered(false, true, true));
    }

    #[test]
    fn the_latch_outputs_on_the_primary_face_when_powered() {
        // Attached down, rotation 0: output north, complementary south.
        let off = [false, false, false];
        assert_eq!(
            Gate::Latch.signal_with_powered(off, false, Dir::Down, 0, Dir::South),
            Gate::OUTPUT_STRENGTH
        );
        assert_eq!(
            Gate::Latch.signal_with_powered(off, false, Dir::Down, 0, Dir::North),
            0
        );
        let set = [true, false, false];
        assert_eq!(
            Gate::Latch.signal_with_powered(set, true, Dir::Down, 0, Dir::North),
            Gate::OUTPUT_STRENGTH
        );
        assert_eq!(
            Gate::Latch.signal_with_powered(set, true, Dir::Down, 0, Dir::South),
            0
        );
        let both = [true, false, true];
        assert_eq!(
            Gate::Latch.signal_with_powered(both, true, Dir::Down, 0, Dir::North),
            0
        );
        assert_eq!(
            Gate::Latch.signal_with_powered(both, true, Dir::Down, 0, Dir::South),
            0
        );
    }

    #[test]
    fn a_rising_edge_arms_the_pulse_and_a_falling_edge_cuts_the_input() {
        // `PulseGateBlock.neighborChanged`: !old && new arms the output and
        // schedules the clear; old && !new only cuts the stored input.
        assert_eq!(
            pulse_update(false, true, false),
            PulseUpdate {
                input_b: true,
                powered: true,
                clear_in: Some(PULSE_DURATION),
            }
        );
        assert_eq!(
            pulse_update(true, false, true),
            PulseUpdate {
                input_b: false,
                powered: true,
                clear_in: None,
            }
        );
        assert_eq!(
            pulse_update(true, false, false),
            PulseUpdate {
                input_b: false,
                powered: false,
                clear_in: None,
            }
        );
    }

    #[test]
    fn a_steady_input_neither_arms_nor_cuts() {
        assert_eq!(
            pulse_update(true, true, true),
            PulseUpdate {
                input_b: true,
                powered: true,
                clear_in: None,
            }
        );
        assert_eq!(
            pulse_update(false, false, false),
            PulseUpdate {
                input_b: false,
                powered: false,
                clear_in: None,
            }
        );
    }

    #[test]
    fn the_pulse_is_not_held_after_its_clear() {
        // The clear drops `powered` but leaves `input_b` high, so a gate whose
        // B stays powered stays low: only a new rising edge arms it again.
        let cleared = pulse_update(true, true, false);
        assert_eq!(
            cleared,
            PulseUpdate {
                input_b: true,
                powered: false,
                clear_in: None,
            }
        );
        assert!(!cleared.powered);
    }

    #[test]
    fn a_pulse_clears_two_ticks_after_it_arms() {
        // Armed at tick 0 with PULSE_DURATION: tick 1 waits, tick 2 clears.
        let PulseTick::Waiting(left) = pulse_tick(PULSE_DURATION) else {
            panic!("the first tick after arming must not clear");
        };
        assert_eq!(left, 1);
        assert_eq!(pulse_tick(left), PulseTick::Cleared);
        // A stale or already-expired value still clears.
        assert_eq!(pulse_tick(0), PulseTick::Cleared);
    }

    #[test]
    fn a_placed_wire_attaches_only_to_the_clicked_face() {
        // `AbstractWireBlock.getStateForPlacement`: the clicked face's
        // opposite, i.e. the face toward the block that was clicked.
        assert_eq!(
            placement_attached(Dir::Up).bits(),
            [true, false, false, false, false, false]
        );
        assert_eq!(
            placement_attached(Dir::Down).bits(),
            [false, true, false, false, false, false]
        );
        assert_eq!(
            placement_attached(Dir::North).bits(),
            [false, false, false, true, false, false]
        );
        assert_eq!(placement_attached(Dir::East).count(), 1);
    }

    #[test]
    fn a_wire_drops_one_item_per_subwire() {
        assert_eq!(Attached::NONE.count(), 0);
        assert_eq!(Attached::from_bits([true; 6]).count(), 6);
        assert_eq!(
            Attached::from_bits([true, false, true, false, true, false]).count(),
            3
        );
    }

    #[test]
    fn a_shape_update_drops_a_subwire_whose_support_is_gone() {
        // `AbstractWireBlock.updateShape`: a set subwire follows its
        // neighbour's sturdiness; a clear one is never added by a shape
        // update (only placement or the wire item adds subwires).
        let attached = Attached::from_bits([true, false, false, true, false, false]);
        assert_eq!(
            retained(attached, Dir::Down, false).bits(),
            [false, false, false, true, false, false]
        );
        assert_eq!(retained(attached, Dir::East, true), attached);
        assert_eq!(retained(attached, Dir::South, true), attached);
        assert_eq!(
            retained(attached, Dir::South, false).bits(),
            [true, false, false, false, false, false]
        );
    }

    #[test]
    fn a_wire_survives_only_while_every_support_is_sturdy() {
        let attached = Attached::from_bits([true, false, false, true, false, false]);
        assert!(can_survive(attached, |face| face == Dir::Down || face == Dir::South));
        assert!(!can_survive(attached, |face| face == Dir::Down));
        // the empty wire block needs nothing (`canSurvive` over no faces)
        assert!(can_survive(Attached::NONE, |_| false));
    }

    #[test]
    fn a_source_is_halved_and_each_step_decays_by_one() {
        // `PoweredWireBlock`: power is stored doubled; a neighbour with
        // signal `s` gives `2s - 1`, a connected wire gives its raw - 1,
        // and `getSignal` answers raw / 2.
        assert_eq!(source_power(15), 29);
        assert_eq!(power_signal(29), 14);
        assert_eq!(source_power(1), 1);
        assert_eq!(power_signal(1), 0);
        assert_eq!(source_power(0), 0);
        assert_eq!(power_signal(0), 0);
        assert_eq!(decay(29), 28);
        assert_eq!(decay(1), 0);
        assert_eq!(decay(0), 0);
    }

    #[test]
    fn power_decays_one_signal_every_two_wires_and_stops_at_twenty_eight() {
        // A straight line of 40 wire blocks, all attached to the floor, fed
        // by a 15 source under the first. Raw power is 29 - i, so the signal
        // is 15 - ceil((i + 1) / 2): 14, 14, 13, 13, ... and wire 29 (index
        // 28) is the last with any power.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let mut previous = [0u8; 6];
        let mut signals = Vec::new();
        for wire in 0..40 {
            let mut support = [0u8; 6];
            if wire == 0 {
                support[0] = source_power(15);
            }
            let mut linked = [0u8; 6];
            linked[0] = previous[0];
            previous = wire_power(attached, support, linked);
            signals.push(power_signal(previous[0]));
        }
        assert_eq!(&signals[..4], &[14, 14, 13, 13]);
        assert_eq!(signals[27], 1);
        assert_eq!(signals[28], 0);
        assert!(signals[..28].iter().all(|signal| *signal > 0));
        assert!(signals[28..].iter().all(|signal| *signal == 0));
    }

    #[test]
    fn removing_the_source_returns_the_line_to_zero() {
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let mut support = [0u8; 6];
        support[0] = source_power(15);
        let powered = wire_power(attached, support, [0u8; 6]);
        assert_eq!(power_signal(powered[0]), 14);
        // `updatePowerAfterBlockUpdate` computes every subwire from scratch,
        // so a gone source cannot linger in the stored array.
        assert_eq!(wire_power(attached, [0u8; 6], [0u8; 6]), [0u8; 6]);
    }

    #[test]
    fn a_wire_feeds_a_reader_beside_it_but_not_through_a_parallel_face() {
        // `PoweredWireBlock.getPower`: the subwire toward the reader wins;
        // otherwise the strongest subwire sharing an edge with that face.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let mut power = [0u8; 6];
        power[0] = 29;
        assert_eq!(wire_signal(power, attached, Dir::Down), 14);
        for travel in [Dir::North, Dir::South, Dir::West, Dir::East] {
            assert_eq!(wire_signal(power, attached, travel), 14);
        }
        // Up is parallel to the attached down face: no edge in common.
        assert_eq!(wire_signal(power, attached, Dir::Up), 0);
        assert_eq!(wire_signal(power, Attached::NONE, Dir::North), 0);
    }

    /// A foreign block's own power, from the two answers the engine gives.
    ///
    /// Measured at this pin (`ports/redstonepen/notes.md`, probe table): the engine's
    /// `signal` is `SignalGetter.getSignal`, "power it conducts included", so a
    /// **conductor** answers the power it receives — a lit lamp beside a wire answers
    /// that wire's power — while a source answers its own. `direct_signal_to` is what
    /// the block receives, so a block that reports nothing but what it receives is a
    /// conductor, not a source. Vanilla's sources are named so the corner where a
    /// source receives exactly what it emits is still a source.
    #[test]
    fn a_block_that_only_conducts_gives_no_signal() {
        // Sources: their plain answer is their own power.
        assert_eq!(
            foreign_signal(15, 0, true),
            15,
            "a lever or a redstone block"
        );
        assert_eq!(
            foreign_signal(15, 7, true),
            15,
            "a source also conducted into"
        );
        assert_eq!(foreign_signal(12, 0, true), 12, "a powered redstone wire");
        assert_eq!(foreign_signal(0, 0, true), 0, "a dark wire");
        // Conductors and sinks: they answer what they receive, so they give nothing.
        assert_eq!(
            foreign_signal(13, 13, false),
            0,
            "a lit lamp beside the wire"
        );
        assert_eq!(foreign_signal(15, 15, false), 0, "stone under a lever");
        assert_eq!(foreign_signal(0, 0, false), 0, "plain stone, air");
        // An emitter the table does not know (another mod's machine) still emits.
        assert_eq!(foreign_signal(15, 7, false), 15);
        assert_eq!(foreign_signal(9, 3, false), 9);
        assert_eq!(foreign_signal(200, 0, true), 15, "clamped like the hook");
    }

    /// The vanilla blocks whose own signal is their power rather than what they
    /// conduct. A redstone lamp is a sink and is not one of them.
    #[test]
    fn the_vanilla_source_table_names_the_blocks_that_emit() {
        for block in [
            "minecraft:redstone_block",
            "minecraft:lever",
            "minecraft:redstone_torch",
            "minecraft:redstone_wall_torch",
            "minecraft:repeater",
            "minecraft:comparator",
            "minecraft:observer",
            "minecraft:daylight_detector",
            "minecraft:redstone_wire",
            "minecraft:oak_button",
            "minecraft:stone_pressure_plate",
            "minecraft:tripwire_hook",
        ] {
            assert!(vanilla_source(block), "{block}");
        }
        for block in [
            "minecraft:redstone_lamp",
            "minecraft:stone",
            "minecraft:air",
            "morered:red_alloy_wire",
        ] {
            assert!(!vanilla_source(block), "{block}");
        }
        assert!(
            vanilla_source("redstone_block"),
            "the namespace is optional"
        );
    }

    /// The in-game leg's wire phase: a three-wire line fed at the west end with a
    /// lamp beside wire 3. Cutting the middle wire leaves wire 3 with the lamp as
    /// its only remaining neighbour, and a lit lamp is a sink: the far side drains
    /// and the lamp goes dark. (A lamp that fed the line back would hold it up
    /// forever — the 0.2.1 failure.)
    #[test]
    fn a_cut_line_drains_even_with_a_lit_lamp_beside_its_end() {
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let east = Dir::East.index();
        let down = Dir::Down.index();
        // Wire 3's east face reads the lamp it lights: 13 is the wire's own signal
        // (raw 27) coming back through the engine's conducting aggregate.
        let lamp = foreign_signal(13, 13, vanilla_source("minecraft:redstone_lamp"));
        let mut support = [0u8; 6];
        support[east] = source_power(lamp);
        // Before the cut the middle wire's subwire (raw 28) feeds wire 3.
        let mut linked = [0u8; 6];
        linked[down] = 28;
        let powered = wire_power(attached, support, linked);
        assert_eq!(powered[down], 27, "the middle wire carries the far side");
        assert_eq!(
            wire_signal(powered, attached, Dir::East),
            13,
            "the lamp is lit"
        );
        // The middle wire is gone: nothing linked, and the sink gives nothing.
        let cut = wire_power(attached, support, [0u8; 6]);
        assert_eq!(cut, [0u8; 6], "a sink cannot hold the line up");
        assert_eq!(wire_signal(cut, attached, Dir::East), 0, "the lamp is dark");
    }

    #[test]
    fn one_wires_subwires_feed_each_other_and_detached_faces_carry_nothing() {
        // A floor wire that turns up a wall (down + north subwires).
        let attached = Attached::from_bits([true, false, true, false, false, false]);
        let mut support = [0u8; 6];
        support[0] = source_power(15);
        let power = wire_power(attached, support, [0u8; 6]);
        assert_eq!(power[0], 29);
        assert_eq!(power[2], 28);
        // A link on a face with no subwire is ignored.
        let mut linked = [0u8; 6];
        linked[5] = 31;
        assert_eq!(wire_power(attached, [0u8; 6], linked)[5], 0);
    }

    #[test]
    fn a_wire_connects_to_a_coplanar_neighbour_sharing_its_attachment_face() {
        // `canWireConnectToAdjacentWireOrCable`: the neighbour must have a
        // subwire on our attachment face, on an axis other than the
        // direction between the two blocks.
        let floor = Attached::from_bits([true, false, false, false, false, false]);
        assert!(shares_attachment(Dir::Down, floor, Dir::East));
        assert!(!shares_attachment(Dir::Down, Attached::NONE, Dir::East));
        assert!(!shares_attachment(Dir::Down, floor, Dir::Down));
        // A wall wire (south subwire) beside a floor wire does not share a
        // face; a corner needs the convex edge.
        let wall = Attached::from_bits([false, false, false, true, false, false]);
        assert!(!shares_attachment(Dir::Down, wall, Dir::East));
    }

    #[test]
    fn the_shape_facing_follows_the_first_attached_face() {
        assert_eq!(Attached::NONE.primary(), None);
        assert_eq!(
            Attached::from_bits([false, false, true, false, false, false]).primary(),
            Some(Dir::North)
        );
        assert_eq!(
            Attached::from_bits([true, false, true, false, false, false]).primary(),
            Some(Dir::Down)
        );
    }

    #[test]
    fn the_pulse_emits_only_on_its_output_face_while_powered() {
        // `PulseGateBlock.getSignal`: powered && the face is the output.
        let inputs = [false, true, false];
        assert_eq!(
            Gate::Pulse.signal_with_powered(inputs, true, Dir::Down, 0, Dir::North),
            Gate::OUTPUT_STRENGTH
        );
        assert_eq!(
            Gate::Pulse.signal_with_powered(inputs, false, Dir::Down, 0, Dir::North),
            0
        );
        for face in [Dir::South, Dir::East, Dir::West, Dir::Up, Dir::Down] {
            assert_eq!(
                Gate::Pulse.signal_with_powered(inputs, true, Dir::Down, 0, face),
                0
            );
        }
        // The input bit alone never powers it: the pulse is the `powered` bit.
        assert_eq!(
            Gate::Pulse.signal_with_powered([false, false, true], true, Dir::Down, 0, Dir::North),
            Gate::OUTPUT_STRENGTH
        );
    }

    #[test]
    fn the_channel_property_is_parsed_and_bounded() {
        assert_eq!(Channel::new(0), Some(Channel::ALL[0]));
        assert_eq!(Channel::new(15), Some(Channel::ALL[15]));
        assert_eq!(Channel::new(16), None);
        assert_eq!(Channel::from_property("7"), Some(Channel::ALL[7]));
        assert_eq!(Channel::from_property("16"), None);
        assert_eq!(Channel::from_property("-1"), None);
        assert_eq!(Channel::from_property("red"), None);
        for (index, channel) in Channel::ALL.into_iter().enumerate() {
            assert_eq!(channel.index(), index);
            assert_eq!(channel.value(), index as u8);
        }
    }

    #[test]
    fn a_bundled_cable_carries_each_channel_on_its_own() {
        // `BundledCableBlockEntity.power[6][16]`: sixteen independent values
        // per face. Seed two channels and check no other channel moved.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let mut seeds = CablePower::NONE;
        seeds.set(Dir::Down, Channel::ALL[3], 29);
        seeds.set(Dir::Down, Channel::ALL[5], 17);
        let power = cable_power(attached, seeds, CablePower::NONE);
        assert_eq!(power.get(Dir::Down, Channel::ALL[3]), 29);
        assert_eq!(power.get(Dir::Down, Channel::ALL[5]), 17);
        for channel in Channel::ALL {
            let expected = match channel.value() {
                3 => 29,
                5 => 17,
                _ => 0,
            };
            assert_eq!(
                power.get(Dir::Down, channel),
                expected,
                "channel {}",
                channel.value()
            );
        }
    }

    #[test]
    fn a_bundled_cable_line_decays_one_per_step_on_one_channel() {
        // A line of floor cables shares the `down` attachment: raw power is
        // 29, 28, 27, ... on the fed channel and 0 on every other channel.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let mut previous = CablePower::NONE;
        let mut fed = Vec::new();
        let mut other = Vec::new();
        for cable in 0..8 {
            let mut seeds = CablePower::NONE;
            if cable == 0 {
                seeds.set(Dir::Down, Channel::ALL[3], 29);
            }
            let mut linked = CablePower::NONE;
            for channel in Channel::ALL {
                linked.set(Dir::Down, channel, previous.get(Dir::Down, channel));
            }
            previous = cable_power(attached, seeds, linked);
            fed.push(previous.get(Dir::Down, Channel::ALL[3]));
            other.push(previous.get(Dir::Down, Channel::ALL[5]));
        }
        assert_eq!(fed, vec![29, 28, 27, 26, 25, 24, 23, 22]);
        assert!(other.iter().all(|power| *power == 0));
    }

    #[test]
    fn an_unattached_cable_face_carries_no_power() {
        // `BundledCableBlock.updatePowerAfterBlockUpdate` forces a detached
        // face to 0 before the worklist runs.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let mut seeds = CablePower::NONE;
        seeds.set(Dir::West, Channel::ALL[3], 29);
        let power = cable_power(attached, seeds, CablePower::NONE);
        assert_eq!(power.get(Dir::West, Channel::ALL[3]), 0);
        assert_eq!(power.get(Dir::Down, Channel::ALL[3]), 0);
    }

    #[test]
    fn a_relay_seeds_its_channel_at_double_the_vanilla_signal() {
        // The bundle has no vanilla input; the relay converts one channel.
        assert_eq!(relay_seed(15), 29);
        assert_eq!(relay_seed(1), 1);
        assert_eq!(relay_seed(0), 0);
    }

    #[test]
    fn a_relay_emits_half_the_strongest_attached_channel_power() {
        let attached = Attached::from_bits([true, false, false, false, true, false]);
        let mut power = CablePower::NONE;
        power.set(Dir::Down, Channel::ALL[3], 29);
        power.set(Dir::West, Channel::ALL[3], 28);
        assert_eq!(relay_signal(power, attached, Channel::ALL[3]), 14);
        // One attached face is enough; the strongest wins.
        let mut down_only = CablePower::NONE;
        down_only.set(Dir::Down, Channel::ALL[3], 27);
        assert_eq!(relay_signal(down_only, attached, Channel::ALL[3]), 13);
        // Raw 1 answers 0 (`power / 2`).
        let mut weak = CablePower::NONE;
        weak.set(Dir::Down, Channel::ALL[3], 1);
        assert_eq!(relay_signal(weak, attached, Channel::ALL[3]), 0);
    }

    #[test]
    fn a_relay_on_another_channel_emits_nothing() {
        // Channel isolation: the same cable carries channel 3 only.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let mut power = CablePower::NONE;
        power.set(Dir::Down, Channel::ALL[3], 29);
        assert_eq!(relay_signal(power, attached, Channel::ALL[3]), 14);
        assert_eq!(relay_signal(power, attached, Channel::ALL[5]), 0);
    }

    #[test]
    fn a_relay_ignores_a_cable_face_it_cannot_reach() {
        // `SidedPowerSupplier` only answers for an attached subwire; a face
        // with no subwire gives the relay nothing.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let mut power = CablePower::NONE;
        power.set(Dir::North, Channel::ALL[3], 29);
        assert_eq!(relay_signal(power, attached, Channel::ALL[3]), 0);
        assert_eq!(relay_signal(power, Attached::NONE, Channel::ALL[3]), 0);
    }

    #[test]
    fn the_bitwise_gate_ids_are_recognised() {
        assert_eq!(
            BitwiseGate::from_id("morered:bitwise_diode"),
            Some(BitwiseGate::Diode)
        );
        assert_eq!(
            BitwiseGate::from_id("morered:bitwise_not_gate"),
            Some(BitwiseGate::Not)
        );
        assert_eq!(
            BitwiseGate::from_id("morered:bitwise_or_gate"),
            Some(BitwiseGate::Or)
        );
        assert_eq!(
            BitwiseGate::from_id("morered:bitwise_and_gate"),
            Some(BitwiseGate::And)
        );
        assert_eq!(
            BitwiseGate::from_id("morered:bitwise_xor_gate"),
            Some(BitwiseGate::Xor)
        );
        assert_eq!(
            BitwiseGate::from_id("morered:bitwise_xnor_gate"),
            Some(BitwiseGate::Xnor)
        );
        // The plain logic gates are a different family.
        assert_eq!(BitwiseGate::from_id("morered:diode"), None);
        assert_eq!(BitwiseGate::from_id("minecraft:bitwise_diode"), None);
    }

    #[test]
    fn the_bitwise_gates_use_the_upstream_input_sides() {
        // `SingleInputBitwiseLogicPlateBlock` reads the output's opposite
        // (= side B); `TwoInputBitwiseLogicPlateBlock` reads A and C.
        assert_eq!(BitwiseGate::Diode.sides(), &[Side::B]);
        assert_eq!(BitwiseGate::Not.sides(), &[Side::B]);
        for gate in [
            BitwiseGate::And,
            BitwiseGate::Or,
            BitwiseGate::Xor,
            BitwiseGate::Xnor,
        ] {
            assert_eq!(gate.sides(), &[Side::A, Side::C]);
            assert!(!gate.sides().contains(&Side::B));
        }
    }

    #[test]
    fn the_bitwise_truth_tables_match_upstream() {
        // `LogicFunctions`: INPUT_B, NOT_B, OR, AND_2, XOR_AC, XNOR_AC.
        for a in [false, true] {
            for b in [false, true] {
                for c in [false, true] {
                    assert_eq!(BitwiseGate::Diode.apply(a, b, c), b);
                    assert_eq!(BitwiseGate::Not.apply(a, b, c), !b);
                    assert_eq!(BitwiseGate::And.apply(a, b, c), a && c);
                    assert_eq!(BitwiseGate::Or.apply(a, b, c), a || c);
                    assert_eq!(BitwiseGate::Xor.apply(a, b, c), a ^ c);
                    assert_eq!(BitwiseGate::Xnor.apply(a, b, c), !(a ^ c));
                }
            }
        }
    }

    fn channel_power(values: &[(usize, u8)]) -> ChannelPower {
        let mut power = ChannelPower::NONE;
        for (index, value) in values {
            power.set(Channel::ALL[*index], *value);
        }
        power
    }

    #[test]
    fn a_bitwise_plate_stores_the_maximum_power_on_a_passing_channel() {
        // `updatePower`: `power[i] = outputBit ? 31 : 0`.
        let input = channel_power(&[(1, 29), (2, 0)]);
        let diode = bitwise_output(
            BitwiseGate::Diode,
            ChannelPower::NONE,
            input,
            ChannelPower::NONE,
        );
        assert_eq!(diode.get(Channel::ALL[1]), 31);
        assert_eq!(diode.get(Channel::ALL[2]), 0);
        let not = bitwise_output(
            BitwiseGate::Not,
            ChannelPower::NONE,
            input,
            ChannelPower::NONE,
        );
        assert_eq!(not.get(Channel::ALL[1]), 0);
        assert_eq!(not.get(Channel::ALL[2]), 31);
    }

    #[test]
    fn the_bitwise_not_inverts_every_channel_on_its_own() {
        let mut input = ChannelPower::NONE;
        for channel in Channel::ALL {
            input.set(channel, if channel.index() % 2 == 0 { 31 } else { 0 });
        }
        let out = bitwise_output(
            BitwiseGate::Not,
            ChannelPower::NONE,
            input,
            ChannelPower::NONE,
        );
        for channel in Channel::ALL {
            let expected = if channel.index() % 2 == 0 { 0 } else { 31 };
            assert_eq!(out.get(channel), expected, "channel {}", channel.index());
        }
    }

    #[test]
    fn the_bitwise_two_input_gates_combine_the_same_channel_only() {
        // Channel 1 is high on both inputs, channel 5 only on A.
        let a = channel_power(&[(1, 29), (5, 29)]);
        let c = channel_power(&[(1, 29)]);
        let and = bitwise_output(BitwiseGate::And, a, ChannelPower::NONE, c);
        assert_eq!(and.get(Channel::ALL[1]), 31);
        assert_eq!(and.get(Channel::ALL[5]), 0);
        let or = bitwise_output(BitwiseGate::Or, a, ChannelPower::NONE, c);
        assert_eq!(or.get(Channel::ALL[1]), 31);
        assert_eq!(or.get(Channel::ALL[5]), 31);
        let xor = bitwise_output(BitwiseGate::Xor, a, ChannelPower::NONE, c);
        assert_eq!(xor.get(Channel::ALL[1]), 0);
        assert_eq!(xor.get(Channel::ALL[5]), 31);
    }

    #[test]
    fn a_missing_bitwise_input_reads_as_unpowered() {
        // `inputSupplier == null ? 0` (single) and `false` (two-input): a NOT
        // with nothing attached emits, a diode with nothing attached does not.
        let none = ChannelPower::NONE;
        let not = bitwise_output(BitwiseGate::Not, none, none, none);
        let diode = bitwise_output(BitwiseGate::Diode, none, none, none);
        let and = bitwise_output(BitwiseGate::And, none, none, none);
        let xnor = bitwise_output(BitwiseGate::Xnor, none, none, none);
        for channel in Channel::ALL {
            assert_eq!(not.get(channel), 31, "NOT channel {}", channel.index());
            assert_eq!(diode.get(channel), 0, "diode channel {}", channel.index());
            assert_eq!(and.get(channel), 0, "AND channel {}", channel.index());
            assert_eq!(xnor.get(channel), 31, "XNOR channel {}", channel.index());
        }
    }

    #[test]
    fn a_cable_answers_a_plate_from_the_near_face_or_the_plates_attachment() {
        // `BundledCableBlockEntity.SidedPowerSupplier`: the subwire toward the
        // querier wins; otherwise the `wireFace` the plate passed (its
        // attachment direction).
        let mut power = CablePower::NONE;
        power.set(Dir::Down, Channel::ALL[1], 27);
        power.set(Dir::East, Channel::ALL[1], 19);
        let floor = Attached::from_bits([true, false, false, false, false, false]);
        let from_attachment = cable_channel_power(power, floor, Dir::East, Dir::Down);
        assert_eq!(from_attachment.get(Channel::ALL[1]), 27);
        // A cable that also has an east subwire answers on the near face.
        let corner = Attached::from_bits([true, false, false, false, false, true]);
        let near = cable_channel_power(power, corner, Dir::East, Dir::Down);
        assert_eq!(near.get(Channel::ALL[1]), 19);
        // A plate whose attachment face carries no subwire reads zero.
        let detached = cable_channel_power(power, floor, Dir::East, Dir::North);
        assert_eq!(detached.get(Channel::ALL[1]), 0);
        // Every other channel is untouched by the branch.
        assert_eq!(near.get(Channel::ALL[5]), 0);
    }

    #[test]
    fn a_plate_seeds_a_cable_one_unit_below_its_stored_power() {
        // `BundledCableBlock`: `max(0, supplier - 1)`; a passing bit is 31.
        assert_eq!(plate_seed(31), 30);
        assert_eq!(plate_seed(1), 0);
        assert_eq!(plate_seed(0), 0);
    }

    #[test]
    fn a_cable_face_takes_the_strongest_of_two_suppliers() {
        // `BundledCableBlock.updatePowerAfterBlockUpdate` maxes every supplier
        // on a face: an unpowered relay on the same face/channel must not
        // overwrite a bitwise plate's seed.
        let mut seeds = CablePower::NONE;
        seeds.raise(Dir::Down, Channel::ALL[1], 30);
        seeds.raise(Dir::Down, Channel::ALL[1], 0);
        assert_eq!(seeds.get(Dir::Down, Channel::ALL[1]), 30);
        seeds.raise(Dir::Down, Channel::ALL[1], 29);
        assert_eq!(seeds.get(Dir::Down, Channel::ALL[1]), 30);
        seeds.raise(Dir::Down, Channel::ALL[1], 31);
        assert_eq!(seeds.get(Dir::Down, Channel::ALL[1]), 31);
        // Another channel or face is untouched.
        assert_eq!(seeds.get(Dir::Down, Channel::ALL[5]), 0);
        assert_eq!(seeds.get(Dir::Up, Channel::ALL[1]), 0);
        // And the solved cable carries it.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let solved = cable_power(attached, seeds, CablePower::NONE);
        assert_eq!(solved.get(Dir::Down, Channel::ALL[1]), 31);
    }

    #[test]
    fn the_post_ids_and_connection_faces_match_upstream() {
        // `MoreRed.java:215-221`; `WirePostBlock.getRedstoneConnectionDirections`
        // and `WirePostPlateBlock.getRedstoneConnectionDirectionsFor*`.
        assert_eq!(
            PostKind::from_id("morered:redwire_post"),
            Some(PostKind::Plain)
        );
        assert_eq!(
            PostKind::from_id("morered:redwire_post_plate"),
            Some(PostKind::Plate)
        );
        assert_eq!(
            PostKind::from_id("morered:redwire_post_relay_plate"),
            Some(PostKind::Relay)
        );
        assert_eq!(PostKind::from_id("morered:bundled_cable_post"), None);
        assert_eq!(PostKind::from_id("morered:diode"), None);

        // A plain post reads and emits toward its attachment; the empty plate
        // connects nowhere; the relay plate connects on the four perpendicular
        // faces (the attachment and its opposite are excluded).
        assert_eq!(PostKind::Plain.connections(Dir::Down), vec![Dir::Down]);
        assert!(PostKind::Plate.connections(Dir::Down).is_empty());
        assert_eq!(
            PostKind::Relay.connections(Dir::Down),
            vec![Dir::North, Dir::South, Dir::West, Dir::East]
        );
        assert_eq!(
            PostKind::Relay.connections(Dir::North),
            vec![Dir::Down, Dir::Up, Dir::West, Dir::East]
        );
    }

    #[test]
    fn a_post_always_powers_itself_from_a_source_one_level_down() {
        // `AbstractPoweredWirePostBlock.getNewPower`:
        // `max(0, max(neighborPower, connectionPower) - 1)`.
        assert_eq!(post_power(15, &[]), 14);
        assert_eq!(post_power(3, &[]), 2);
        assert_eq!(post_power(1, &[]), 0);
        assert_eq!(post_power(0, &[]), 0);
    }

    #[test]
    fn a_link_decays_the_strongest_linked_post_by_one() {
        // `getConnectionPower` then the `- 1`: a chain loses one per link.
        assert_eq!(post_power(0, &[14]), 13);
        assert_eq!(post_power(0, &[13, 14]), 13);
        assert_eq!(post_power(15, &[14]), 14);
        assert_eq!(post_power(0, &[1]), 0);
        let mut power = 14;
        let mut chain = Vec::new();
        for _ in 0..3 {
            chain.push(power);
            power = post_power(0, &[power]);
        }
        assert_eq!(chain, vec![14, 13, 12]);
    }

    #[test]
    fn a_post_emits_only_on_the_faces_it_connects() {
        // `getSignal`: the connectable set contains the caller's opposite, and
        // the hook's `face` is the travel direction, so a plain post answers
        // on the face away from its attachment.
        assert_eq!(post_signal(13, Dir::Down, PostKind::Plain, Dir::Up), 13);
        assert_eq!(post_signal(13, Dir::Down, PostKind::Plain, Dir::Down), 0);
        assert_eq!(post_signal(13, Dir::Down, PostKind::Plain, Dir::North), 0);
        // The relay plate answers on its four perpendicular faces only.
        assert_eq!(post_signal(9, Dir::Down, PostKind::Relay, Dir::North), 9);
        assert_eq!(post_signal(9, Dir::Down, PostKind::Relay, Dir::Up), 0);
        assert_eq!(post_signal(9, Dir::Down, PostKind::Relay, Dir::Down), 0);
        // The empty plate never answers a vanilla reader.
        assert_eq!(post_signal(15, Dir::Down, PostKind::Plate, Dir::Up), 0);
        // Zero power is zero on every face.
        assert_eq!(post_signal(0, Dir::Down, PostKind::Plain, Dir::Up), 0);
    }

    #[test]
    fn links_are_stored_relative_to_the_post_and_round_trip() {
        // `WirePostBlockEntity.remoteConnections`: absolute in memory, relative
        // in NBT; the port stores plain relative offsets (no TRANSFORM).
        let mut links = PostLinks::default();
        assert!(links.add([10, 0, 0]));
        assert!(!links.add([10, 0, 0]), "a duplicate link is a no-op");
        assert!(links.add([0, 2, -3]));
        assert!(links.contains([10, 0, 0]));
        assert_eq!(links.offsets(), &[[10, 0, 0], [0, 2, -3]]);
        assert_eq!(
            links.absolute([5, 64, 5]).collect::<Vec<_>>(),
            vec![[15, 64, 5], [5, 66, 2]]
        );
        assert!(links.remove([10, 0, 0]));
        assert!(
            !links.remove([10, 0, 0]),
            "removing a missing link is a no-op"
        );
        assert_eq!(
            links.absolute([5, 64, 5]).collect::<Vec<_>>(),
            vec![[5, 66, 2]]
        );
        assert_eq!(
            PostLinks::from_offsets(vec![[1, 0, 0]]).offsets(),
            &[[1, 0, 0]]
        );
    }

    #[test]
    fn all_sixteen_cable_colours_are_recognised_in_dye_order() {
        // `MoreRed.NETWORK_CABLES_BY_COLOR` is built in `DyeColor.values()`
        // order, which is also the bundled cable's channel index.
        let names = [
            "white",
            "orange",
            "magenta",
            "light_blue",
            "yellow",
            "lime",
            "pink",
            "gray",
            "light_gray",
            "cyan",
            "purple",
            "blue",
            "brown",
            "green",
            "red",
            "black",
        ];
        assert_eq!(CableColor::ALL.len(), names.len());
        for (index, (colour, name)) in CableColor::ALL.into_iter().zip(names).enumerate() {
            assert_eq!(colour.index(), index, "{colour:?}");
            let id = format!("morered:{name}_network_cable");
            assert_eq!(colour.id(), id);
            assert_eq!(CableColor::from_id(&id), Some(colour));
        }
        // The plain wire and the bundled cable are not a dye.
        assert_eq!(CableColor::from_id("morered:red_alloy_wire"), None);
        assert_eq!(CableColor::from_id("morered:bundled_network_cable"), None);
        assert_eq!(CableColor::from_id("morered:red_alloy_ingot"), None);
    }

    #[test]
    fn two_cables_of_the_same_colour_share_a_subwire() {
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        assert!(connects(
            Dir::Down,
            Some(CableColor::Red),
            attached,
            Some(CableColor::Red),
            Dir::North
        ));
        // Parallel to the attachment face: there is no shared edge.
        assert!(!connects(
            Dir::Down,
            Some(CableColor::Red),
            attached,
            Some(CableColor::Red),
            Dir::Down
        ));
    }

    #[test]
    fn cables_of_different_colours_do_not_connect() {
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        for a in CableColor::ALL {
            for b in CableColor::ALL {
                let expected = a == b;
                assert_eq!(
                    connects(Dir::Down, Some(a), attached, Some(b), Dir::North),
                    expected,
                    "{a:?} beside {b:?}"
                );
                assert_eq!(cables_connect(Some(a), Some(b)), expected);
            }
        }
    }

    #[test]
    fn a_coloured_cable_connects_to_the_plain_red_alloy_wire() {
        // `ColoredCableBlock.canConnectToAdjacentWireOrCable` only refuses a
        // *different coloured cable*; the plain wire falls through to
        // `AbstractWireBlock`'s shared-attachment rule.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        assert!(cables_connect(Some(CableColor::Red), None));
        assert!(cables_connect(None, Some(CableColor::Red)));
        assert!(connects(
            Dir::Down,
            Some(CableColor::Red),
            attached,
            None,
            Dir::North
        ));
        assert!(connects(
            Dir::Down,
            None,
            attached,
            Some(CableColor::Red),
            Dir::North
        ));
    }

    #[test]
    fn an_edge_link_only_joins_the_same_wire_block() {
        // Upstream's convex-edge branch requires `neighborBlock == this`, so a
        // plain wire never edge-links to a cable and two dyes never link.
        let kinds: Vec<Option<CableColor>> = std::iter::once(None)
            .chain(CableColor::ALL.into_iter().map(Some))
            .collect();
        for a in kinds.iter().copied() {
            for b in kinds.iter().copied() {
                assert_eq!(same_wire_block(a, b), a == b, "{a:?} diagonal to {b:?}");
            }
        }
    }

    #[test]
    fn a_powered_cable_of_another_colour_adds_no_link_power() {
        // A red down subwire next to a powered blue one: the blue line's power
        // must not arrive through the edge link. Same colour is the
        // differential: the link would arrive and decay by one.
        let attached = Attached::from_bits([true, false, false, false, false, false]);
        let blue = 29;
        let refused = connects(
            Dir::Down,
            Some(CableColor::Red),
            attached,
            Some(CableColor::Blue),
            Dir::North,
        );
        assert!(!refused);
        let linked = [if refused { blue } else { 0 }, 0, 0, 0, 0, 0];
        assert_eq!(wire_power(attached, [0; 6], linked)[0], 0);
        let shared = connects(
            Dir::Down,
            Some(CableColor::Red),
            attached,
            Some(CableColor::Red),
            Dir::North,
        );
        assert!(shared);
        let linked = [if shared { blue } else { 0 }, 0, 0, 0, 0, 0];
        assert_eq!(wire_power(attached, [0; 6], linked)[0], 28);
    }

    #[test]
    fn a_spool_click_selects_links_cuts_and_refuses_a_far_post() {
        // `WireSpoolItem.onUseOnPost`: first click stores; same post clears;
        // an existing link cuts; an unconnected post within the range links;
        // beyond it becomes the new first post.
        let range = 32.0;
        let a = [0, 64, 0];
        let b = [10, 64, 0];
        assert_eq!(spool_click(None, a, false, range), SpoolClick::Select);
        assert_eq!(
            spool_click(Some(a), a, false, range),
            SpoolClick::ClearSelection
        );
        assert_eq!(spool_click(Some(a), b, true, range), SpoolClick::Cut);
        assert_eq!(spool_click(Some(a), b, false, range), SpoolClick::Link);
        // `BlockPos.closerThan` is strict: 32 blocks is already too far.
        assert_eq!(
            spool_click(Some(a), [32, 64, 0], false, range),
            SpoolClick::TooFar
        );
        assert_eq!(
            spool_click(Some(a), [0, 64, 32], false, range),
            SpoolClick::TooFar
        );
        assert_eq!(
            spool_click(Some(a), [0, 64, 31], false, range),
            SpoolClick::Link
        );
        // Diagonal distance counts (10,10,10 is ~17.3).
        assert_eq!(
            spool_click(Some(a), [10, 74, 10], false, range),
            SpoolClick::Link
        );
        assert_eq!(
            spool_click(Some(a), [20, 84, 20], false, range),
            SpoolClick::TooFar
        );
    }

    /// The review finding: a post's adjacency had no per-post bound, so a saved peer list could
    /// grow without limit and every solve cloned and enqueued all of it.
    #[test]
    fn a_post_keeps_a_bounded_number_of_links() {
        let mut links = PostLinks::default();
        for index in 0..MAX_POST_LINKS {
            assert!(
                links.add([index as i32, 0, 0]),
                "link {index} is under the cap"
            );
        }
        assert!(links.is_full());
        assert_eq!(links.offsets().len(), MAX_POST_LINKS);
        assert!(
            !links.add([999, 0, 0]),
            "a post at its cap refuses another link"
        );
        assert_eq!(
            links.offsets().len(),
            MAX_POST_LINKS,
            "the list did not grow"
        );
        assert!(!links.add([0, 0, 0]), "a duplicate is still refused");
    }

    /// The second review finding's chunk half: a cached position belongs to exactly one chunk,
    /// including west of the origin, so a chunk unload can drop it.
    #[test]
    fn a_position_belongs_to_its_own_chunk() {
        assert!(in_chunk([0, 64, 0], [0, 0]));
        assert!(in_chunk([15, 64, 15], [0, 0]));
        assert!(in_chunk([16, 64, 0], [1, 0]));
        assert!(!in_chunk([16, 64, 0], [0, 0]));
        assert!(in_chunk([-1, 64, -1], [-1, -1]), "west and north");
        assert!(!in_chunk([-1, 64, 0], [0, 0]), "x and z are separate axes");
    }
}
