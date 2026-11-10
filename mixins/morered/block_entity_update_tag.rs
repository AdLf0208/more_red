// Clients get the wire's saved compound (its power array).
fn observe_block_entity_update_tag(
    entity: &horizon_client::containers::ModBlockEntity,
    tag: &mut Option<horizon_world::nbt::Compound>,
) {
    crate::game::morered_state::on_block_entity_update_tag(entity, tag);
}
