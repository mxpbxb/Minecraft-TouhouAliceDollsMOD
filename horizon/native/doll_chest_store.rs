// Before a vehicle's container copies its slots back: a doll's inventory screen hands what it holds
// to the doll mod instead (no cart or boat has the stand-in key), and drops its name.
fn doll_chest_store(game: &mut crate::game::Game<'_>, key: (i32, i32, i32)) {
    if key.0 != horizon_client::mobs::alice_doll_goals::CHEST_KEY_X {
        return;
    }
    if let Some(horizon_client::containers::BlockEntity::Chest(items)) = game.world.block_entities.get(&key) {
        horizon_client::mobs::alice_doll_goals::chest_closed(key.2 as u32, items.clone());
    }
    game.world.block_names.remove(&key);
}
