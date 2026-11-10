// A neighbour changed: queue the plate for the next level tick.
fn observe_neighbor_changed(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    state: BlockStateId,
) {
    crate::game::morered_state::on_neighbor_changed(level, dimension, pos, state);
}
