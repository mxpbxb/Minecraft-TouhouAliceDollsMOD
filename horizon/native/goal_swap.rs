// Runs before a mob's tick: gives a doll its goals, and keeps a riding doll on its owner's head.
fn doll_goals(m: &mut crate::mobs::Mob, w: &crate::mobs::MobWorld<'_>) {
    crate::mobs::alice_doll_goals::ensure(m, w);
}
