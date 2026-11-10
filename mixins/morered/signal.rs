// `block_signal`: answer from the wire's block entity or the plate's stored
// state (hot path; no writes).
fn observe_block_signal(
    level: &crate::game::GameLevel<'_>,
    pos: BlockPos,
    state: BlockStateId,
    face: Direction,
    direct: bool,
    signal: &mut i32,
) {
    crate::game::morered_state::on_block_signal(level, pos, state, face, direct, signal);
}
