// Before an item entity ages: a doll's item has an unlimited lifetime, as `setUnlimitedLifetime` gives.
fn doll_item_age(item: &mut crate::entities::ItemEntity) {
    if crate::mobs::alice_doll_goals::is_doll_item(&item.stack.item) {
        item.age = i16::MIN as i32;
    }
}
