// The wire block declares a block entity for its six per-face powers.
fn observe_block_entity(
    block: &str,
    entity: &mut Option<(String, horizon_world::nbt::Compound)>,
) {
    crate::game::morered_state::on_block_entity(block, entity);
}
