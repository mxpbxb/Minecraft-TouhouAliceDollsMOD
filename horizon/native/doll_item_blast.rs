// Before an explosion reaches an item entity: a doll's item is passed by.
fn doll_item_blast(item: &horizon_client::entities::ItemEntity) -> core::ops::ControlFlow<bool> {
    if horizon_client::mobs::alice_doll_goals::is_doll_item(&item.stack.item) {
        core::ops::ControlFlow::Break(true)
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
