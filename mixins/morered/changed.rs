// A block was placed/changed: seed the plate's first evaluation.
fn observe_block_changed(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    new: BlockStateId,
) {
    crate::game::morered_state::on_block_changed(level, dimension, pos, new);
}
