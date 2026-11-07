// Before a chicken counts down to an egg: a doll never lays one.
fn doll_egg(m: &crate::mobs::Mob) -> core::ops::ControlFlow<()> {
    if crate::mobs::alice_doll_goals::is_doll_mob(m) {
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
