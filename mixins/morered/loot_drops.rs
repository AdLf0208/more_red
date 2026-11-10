// A broken wire or cable drops one item per subwire (`loot_drops`, API 75):
// the state the hook is given is where the six booleans live.
fn observe_loot_drops(
    game: &mut crate::game::Game<'_>,
    context: &NativeLootDrops<'_>,
    drops: &mut Vec<ItemStack>,
) {
    crate::game::morered_state::on_loot_drops(game, context, drops);
}
