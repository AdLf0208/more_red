// `updateShape`: a wire subwire whose supporting neighbour stopped being
// sturdy disconnects.
fn observe_update_shape(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    state: BlockStateId,
    direction: Direction,
    neighbour: BlockStateId,
    new: &mut Option<BlockStateId>,
) {
    crate::game::morered_state::on_update_shape(
        level, dimension, pos, state, direction, neighbour, new,
    );
}
