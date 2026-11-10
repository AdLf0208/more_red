// `is_signal_source`: every logic plate is a signal source.
fn observe_is_signal_source(
    level: &crate::game::GameLevel<'_>,
    state: BlockStateId,
    source: &mut bool,
) {
    crate::game::morered_state::on_is_signal_source(level, state, source);
}
