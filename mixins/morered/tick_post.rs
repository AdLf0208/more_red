// After the level tick: apply the plates queued by neighbour changes.
fn observe_level_tick_post(game: &mut crate::game::Game<'_>, dimension: &str) {
    crate::game::morered_state::on_level_tick_post(game, dimension);
}
