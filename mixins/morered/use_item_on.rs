// `use_item_on`: a spool click on a wire post selects the first post, links
// the pair, cuts an existing link or starts again (upstream
// `WireSpoolItem.onUseOnPost`).
fn observe_use_item_on(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    claimed: &mut bool,
) {
    crate::game::morered_state::on_use_item_on(game, stack, pos, claimed);
}
