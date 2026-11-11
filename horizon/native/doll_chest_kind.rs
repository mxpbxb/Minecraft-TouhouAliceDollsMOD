// Before the game asks which vehicle's container is open: a doll's inventory shows as a chest, in
// the chest screen, with the rows its slots fill.
fn doll_chest_kind(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<Option<CartKind>> {
    let doll = game
        .container
        .is_some_and(|k| k.0 == horizon_client::mobs::alice_doll_goals::CHEST_KEY_X && k.1 == CART_KEY_Y);
    if doll {
        core::ops::ControlFlow::Break(Some(CartKind::Chest))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
