//! The base doll's boxes, read from the original `DollBase.getModelInstance`, and how they are drawn.
//!
//! The original is a 1.7 `ModelBiped`: a 64x32 texture, pixels, y down, feet at y = 24 and the mob
//! facing -z. Horizon's pose has the feet at the origin, blocks, y up, +x the mob's left and +z the
//! way it faces, which is that space turned 180 degrees about x and scaled by 1/16.

use horizon_client::mod_api::{BlockEntityDraw, DisplayContext, Host, ItemStack, NativeMobRender};

const TEXTURE_W: f32 = 64.0;
const TEXTURE_H: f32 = 32.0;

/// One `ModelRenderer` box, in the original's terms.
#[derive(Clone, Copy, Debug)]
pub struct ModelBox {
    pub tex: [f32; 2],
    /// The corner `addBox` names, from the pivot.
    pub offset: [f32; 3],
    pub size: [f32; 3],
    pub inflate: f32,
    pub pivot: [f32; 3],
}

const fn part(tex: [f32; 2], offset: [f32; 3], size: [f32; 3], pivot: [f32; 3]) -> ModelBox {
    ModelBox { tex, offset, size, inflate: 0.0, pivot }
}

const HEAD: ModelBox = part([0.0, 0.0], [-2.0, -4.0, -2.0], [4.0, 4.0, 4.0], [0.0, 14.0, 0.0]);
const BODY: ModelBox = part([0.0, 8.0], [-2.0, 0.0, -1.0], [4.0, 5.0, 2.0], [0.0, 14.0, 0.0]);
const RIGHT_ARM: ModelBox = part([0.0, 15.0], [-2.0, -1.0, -1.0], [2.0, 4.0, 2.0], [-2.0, 15.0, 0.0]);
const LEFT_ARM: ModelBox = part([8.0, 15.0], [0.0, -1.0, -1.0], [2.0, 4.0, 2.0], [2.0, 15.0, 0.0]);
const RIGHT_LEG: ModelBox = part([0.0, 21.0], [-1.0, 0.0, -1.0], [2.0, 5.0, 2.0], [-1.0, 19.0, 0.0]);
const LEFT_LEG: ModelBox = part([8.0, 21.0], [-1.0, 0.0, -1.0], [2.0, 5.0, 2.0], [1.0, 19.0, 0.0]);
const SKIRT_1: ModelBox = part([16.0, 0.0], [-3.0, 4.0, -2.0], [6.0, 2.0, 4.0], [0.0, 14.0, 0.0]);
const SKIRT_2: ModelBox = part([16.0, 6.0], [-4.0, 6.0, -3.0], [8.0, 2.0, 6.0], [0.0, 14.0, 0.0]);
const RIBBON: ModelBox = ModelBox {
    tex: [36.0, 0.0],
    offset: [-3.0, -1.2, -0.8],
    size: [6.0, 3.0, 1.0],
    inflate: 0.1,
    pivot: [0.0, 10.0, 2.0],
};
/// The ribbon's tilt, about x through its pivot, on top of the head's turn.
const RIBBON_TILT: f32 = 30.0;

/// A point of the original's model space in Horizon's pose.
pub fn to_pose(p: [f32; 3]) -> [f32; 3] {
    [p[0] / 16.0, (24.0 - p[1]) / 16.0, -p[2] / 16.0]
}

/// One quad: top left, bottom left, bottom right, top right seen from outside, and its UVs on
/// `draw_quad`'s 0 to 16 scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub corners: [[f32; 3]; 4],
    pub uv: [f32; 4],
}

fn scaled_uv(u0: f32, v0: f32, u1: f32, v1: f32) -> [f32; 4] {
    let (su, sv) = (16.0 / TEXTURE_W, 16.0 / TEXTURE_H);
    [u0 * su, v0 * sv, u1 * su, v1 * sv]
}

/// The six faces of a box, with vanilla's box unwrap.
pub fn faces(b: &ModelBox) -> [Face; 6] {
    let [u, v] = b.tex;
    let [w, h, d] = b.size;
    let i = b.inflate;
    let low = [
        b.pivot[0] + b.offset[0] - i,
        b.pivot[1] + b.offset[1] - i,
        b.pivot[2] + b.offset[2] - i,
    ];
    let high = [low[0] + w + 2.0 * i, low[1] + h + 2.0 * i, low[2] + d + 2.0 * i];
    let (a, c) = (to_pose(low), to_pose(high));
    // Model y and z run the other way in the pose.
    let (x0, x1) = (a[0], c[0]);
    let (y1, y0) = (a[1], c[1]);
    let (z1, z0) = (a[2], c[2]);
    [
        // Front (+z).
        Face {
            corners: [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
            uv: scaled_uv(u + d, v + d, u + d + w, v + d + h),
        },
        // Back (-z).
        Face {
            corners: [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
            uv: scaled_uv(u + d + w + d, v + d, u + d + w + d + w, v + d + h),
        },
        // The doll's left (+x).
        Face {
            corners: [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
            uv: scaled_uv(u + d + w, v + d, u + d + w + d, v + d + h),
        },
        // The doll's right (-x).
        Face {
            corners: [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
            uv: scaled_uv(u, v + d, u + d, v + d + h),
        },
        // Top.
        Face {
            corners: [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
            uv: scaled_uv(u + d, v, u + d + w, v + d),
        },
        // Bottom.
        Face {
            corners: [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
            uv: scaled_uv(u + d + w, v, u + d + w + w, v + d),
        },
    ]
}

/// False when nothing could be drawn: the sprite is not in the atlas.
fn draw_box(host: &dyn Host, draw: &mut BlockEntityDraw, sprite: &str, b: &ModelBox, tint: u32) -> bool {
    let mut drawn = false;
    for face in faces(b) {
        drawn |= host.draw_quad(draw, sprite, face.corners, face.uv, tint);
    }
    drawn
}

/// Draws a box turned about x through its own pivot.
fn draw_swung(
    host: &dyn Host,
    draw: &mut BlockEntityDraw,
    sprite: &str,
    b: &ModelBox,
    degrees: f32,
    tint: u32,
) {
    draw.push();
    draw.rotate_about([1.0, 0.0, 0.0], degrees, to_pose(b.pivot));
    draw_box(host, draw, sprite, b, tint);
    draw.pop();
}

/// Draws the doll for one frame.
pub fn draw_doll(
    host: &dyn Host,
    mob: &NativeMobRender<'_>,
    draw: &mut BlockEntityDraw,
    sprite: &str,
    held: &str,
) -> bool {
    // The vanilla hurt flash: red while it lasts.
    let tint = if mob.hurt_time > 0 || mob.death_time > 0 { 0xFF_80_80 } else { 0xFF_FF_FF };

    draw.push();
    if mob.death_time > 0 {
        // Vanilla tips a dying mob over 20 ticks.
        let tipped = ((mob.death_time as f32 + mob.partial - 1.0) / 20.0 * 1.6).sqrt().min(1.0);
        draw.rotate_about([0.0, 0.0, 1.0], tipped * 90.0, [0.0, 0.0, 0.0]);
    }

    let phase = mob.walk_position * 0.6662;
    let leg = phase.cos() * 1.4 * mob.walk_speed;
    let arm = (phase + std::f32::consts::PI).cos() * mob.walk_speed;
    let degrees = |radians: f32| radians.to_degrees();
    // `ModelBiped.setupAttackAnimation`: the right arm chops forward and back over a swing.
    let swing = super::goals::swing_progress(mob.id);
    let arm = if swing > 0.0 {
        let eased = 1.0 - (1.0 - swing).powi(4);
        let pi = std::f32::consts::PI;
        arm - ((eased * pi).sin() * 1.2 + (swing * pi).sin() * 0.525)
    } else {
        arm
    };
    let left_arm = (phase + std::f32::consts::PI).cos() * mob.walk_speed;

    if !draw_box(host, draw, sprite, &BODY, tint) {
        draw.pop();
        return false;
    }
    draw_box(host, draw, sprite, &SKIRT_1, tint);
    draw_box(host, draw, sprite, &SKIRT_2, tint);
    draw_swung(host, draw, sprite, &RIGHT_LEG, degrees(leg), tint);
    draw_swung(host, draw, sprite, &LEFT_LEG, degrees(-leg), tint);
    draw_swung(host, draw, sprite, &RIGHT_ARM, degrees(arm), tint);
    draw_swung(host, draw, sprite, &LEFT_ARM, degrees(-left_arm), tint);

    // `getHeldItem`: the doll's item in the right hand, moving with that arm.
    draw.push();
    draw.rotate_about([1.0, 0.0, 0.0], degrees(arm), to_pose(RIGHT_ARM.pivot));
    draw.translate(to_pose([-3.0, 19.0, 0.0]));
    draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.0, 0.0, 0.0]);
    draw.scale([0.6, 0.6, 0.6]);
    let stack = ItemStack::new(held, 1);
    host.draw_item(draw, &stack, DisplayContext::ThirdPersonRightHand, None, 0);
    draw.pop();

    // The head and what hangs from it turn together.
    let neck = to_pose(HEAD.pivot);
    draw.push();
    draw.rotate_about([0.0, 1.0, 0.0], -mob.head_yaw, neck);
    draw.rotate_about([1.0, 0.0, 0.0], mob.pitch, neck);
    draw_box(host, draw, sprite, &HEAD, tint);
    draw_swung(host, draw, sprite, &RIBBON, RIBBON_TILT, tint);
    draw.pop();

    draw.pop();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feet_are_on_the_ground_and_the_doll_is_14_pixels_tall() {
        assert_eq!(to_pose([0.0, 24.0, 0.0]), [0.0, 0.0, 0.0]);
        let top = faces(&HEAD)[0].corners[0][1];
        assert_eq!(top, 14.0 / 16.0);
    }

    #[test]
    fn the_heads_front_is_where_vanilla_unwraps_it() {
        // A 4x4x4 box at (0, 0): the front is pixels (4, 4) to (8, 8) of a 64x32 texture.
        assert_eq!(faces(&HEAD)[0].uv, [1.0, 2.0, 2.0, 4.0]);
    }

    #[test]
    fn the_front_faces_forward_and_the_left_arm_is_on_the_left() {
        let front = faces(&HEAD)[0].corners;
        assert!(front.iter().all(|c| c[2] > 0.0));
        let arm = faces(&LEFT_ARM)[0].corners;
        assert!(arm.iter().all(|c| c[0] >= 2.0 / 16.0));
    }

    #[test]
    fn every_face_winds_outward() {
        for b in [HEAD, BODY, RIGHT_ARM, LEFT_ARM, RIGHT_LEG, LEFT_LEG, SKIRT_1, SKIRT_2, RIBBON] {
            let centre = {
                let f = faces(&b);
                let (a, c) = (f[0].corners[0], f[1].corners[1]);
                [(a[0] + c[0]) / 2.0, (a[1] + c[1]) / 2.0, (a[2] + c[2]) / 2.0]
            };
            for face in faces(&b) {
                let [tl, bl, _, tr] = face.corners;
                let e1 = [bl[0] - tl[0], bl[1] - tl[1], bl[2] - tl[2]];
                let e2 = [tr[0] - tl[0], tr[1] - tl[1], tr[2] - tl[2]];
                let n = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let out = [tl[0] - centre[0], tl[1] - centre[1], tl[2] - centre[2]];
                assert!(n[0] * out[0] + n[1] * out[1] + n[2] * out[2] > 0.0, "{face:?}");
            }
        }
    }
}
