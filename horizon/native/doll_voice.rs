// Before a mob speaks: a doll stays quiet.
fn doll_voice(m: &crate::mobs::Mob) -> core::ops::ControlFlow<()> {
    if crate::mobs::alice_doll_goals::is_doll_mob(m) {
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
