//! More Red for Horizon — the one `impl Hooks` at the crate root.
//!
//! Ported from More Red v1.21.1-6.0.0.3 (`Commoble/morered`, MIT), migrated from the retired
//! `.hnmod` container to the flat SDK package (`api = "0.2"`). The pure rules — the gate table,
//! the output/input direction tables, the latch and pulse rules, the wire/cable decay solve, the
//! channel gates, the post links and the spool decision tree — live in [`runtime`] and are tested
//! by `cargo test` alone; [`support`] is the game glue and the four Parts.
//!
//! | Hook / Part | When it runs | What the glue does |
//! |---|---|---|
//! | `register` | once, before any world | declares the four block entity types and attaches every Part |
//! | `BlockPart::block_signal` / `is_signal_source` | the engine asks a block | answers from the port's cached power or the plate's stored state |
//! | `BlockPart::neighbor_changed` | a neighbour changed | queues the block's work for the tick |
//! | `BlockPart::state_for_placement` | a block is placed | sets the attachment/rotation from the click |
//! | `BlockPart::use_item_on` | a player right-clicks with the spool or a wooden rod | link/cut posts, turn a plate |
//! | `block_changed` | a state was committed | seeds the first evaluation; a removed post tells its peers |
//! | `level_tick_post` | after a level's tick pass | solves wires, cables, posts and plates |
//! | `block_entity_on_load` (Part) | a mod block entity attaches | seeds the power/link caches from saved data |
//! | `loot_drops` | a block broke | one item per attached subwire |
//! | `world_lifecycle` | the world closes | drops every queued set, cache and spool selection |

pub mod runtime;
pub mod support;

use mod_api::{
    self, BlockChangedEvent, Hooks, HostSide, LevelHost, LevelTickPostEvent, LootDropsEvent,
    Registrar, RegistrationContext, ServerHost, WorldLifecycleEvent,
};

/// The SDK's one `Default` session instance; all state lives behind the lock in
/// [`support`], because Parts are separate objects constructed at registration.
#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    /// Declare the four block entity types and attach the Parts to every block.
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        support::register(r);
    }

    /// An authoritative server committed a changed state: seed the first evaluation.
    fn block_changed(&self, level: &dyn LevelHost, event: &mut BlockChangedEvent) {
        support::on_block_changed(level, event);
    }

    /// After a level's tick pass, with that level in place: apply the queued work.
    fn level_tick_post(&self, host: HostSide<'_>, event: &mut LevelTickPostEvent) {
        let HostSide::Server(host) = host else {
            return;
        };
        support::on_level_tick_post(host, event.level);
    }

    /// A broken wire or cable drops one item per attached subwire.
    fn loot_drops(&self, host: &mut dyn ServerHost, event: &mut LootDropsEvent<'_>) {
        support::on_loot_drops(host, event);
    }

    /// A chunk left the world: the caches this port keeps for its positions go with it, so a
    /// player's building and removing blocks does not leak entries for the runtime's life.
    fn chunk_unload(&self, _: &mut dyn ServerHost, event: &mut mod_api::ChunkUnloadEvent<'_>) {
        support::on_chunk_unload(event.dimension, event.chunk);
    }

    /// The world is going away: nothing queued or cached belongs to the next one.
    fn world_lifecycle(&self, _: &mut dyn ServerHost, _: &mut WorldLifecycleEvent<'_>) {
        support::reset_world_state();
    }
}
