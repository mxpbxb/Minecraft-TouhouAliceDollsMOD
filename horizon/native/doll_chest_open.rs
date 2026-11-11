// Every tick, before the open container's reach check: opens the doll inventory the doll mod asked
// for, the way the game opens a chest minecart's (`open_cart_container`): its slots under a stand-in
// container key, named after the doll, in the chest screen.
fn doll_chest_open(game: &mut crate::game::Game<'_>) {
    if game.remote.is_some() || !matches!(game.screen, crate::game::Screen::Playing) {
        return;
    }
    let Some((id, title, items)) = horizon_client::mobs::alice_doll_goals::take_chest_request() else {
        return;
    };
    let key = (
        horizon_client::mobs::alice_doll_goals::CHEST_KEY_X,
        crate::game::minecarts::CART_KEY_Y,
        id as i32,
    );
    game.world
        .block_entities
        .insert(key, horizon_client::containers::BlockEntity::Chest(items));
    game.world.block_names.insert(key, title);
    game.container = Some(key);
    game.screen = crate::game::Screen::Inventory;
    game.capture(false);
    game.right = false;
}
