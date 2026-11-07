// Before an item entity is hurt: a doll's item takes no damage.
fn doll_item_hurt(item: &crate::entities::ItemEntity) -> core::ops::ControlFlow<Option<Vec<ItemStack>>> {
    if crate::mobs::alice_doll_goals::is_doll_item(&item.stack.item) {
        core::ops::ControlFlow::Break(None)
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
