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

/// The six faces of a box of the base doll's 64x32 texture.
pub fn faces(b: &ModelBox) -> [Face; 6] {
    faces_on(b, [TEXTURE_W, TEXTURE_H])
}

/// The six faces of a box, with vanilla's box unwrap, on a texture `tex` pixels across and down.
pub fn faces_on(b: &ModelBox, tex: [f32; 2]) -> [Face; 6] {
    let scaled_uv = |u0: f32, v0: f32, u1: f32, v1: f32| {
        let (su, sv) = (16.0 / tex[0], 16.0 / tex[1]);
        [u0 * su, v0 * sv, u1 * su, v1 * sv]
    };
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
    // `ItemInHandLayer` turns the item half round about its own up axis too: without it an axe's
    // blade faced the doll. (A sword or pickaxe looks the same either way.)
    draw.rotate_about([0.0, 1.0, 0.0], 180.0, [0.0, 0.0, 0.0]);
    draw.scale([0.6, 0.6, 0.6]);
    if !held.is_empty() {
        let stack = ItemStack::new(held, 1);
        host.draw_item(draw, &stack, DisplayContext::ThirdPersonRightHand, None, 0);
    }
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

// ---- The extra dolls' own models (touhou_alice_extras: `getModelInstance`) ----

/// Which of the original `ModelBiped` parts a box is, or hangs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Head,
    Headwear,
    Body,
    RightArm,
    LeftArm,
    RightLeg,
    LeftLeg,
}

/// One `ModelAliceDoll.addBox`: a part (`child` false) or an accessory hanging from one. Its corner
/// and pivot are in the model's own space, as the original wrote them; `rot` is its fixed turn in
/// degrees (an accessory's; a part's is its pose).
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub role: Role,
    pub child: bool,
    pub b: ModelBox,
    pub rot: [f32; 3],
}

/// A doll drawn from its own boxes.
#[derive(Debug)]
pub struct Model {
    /// The texture's size in pixels.
    pub tex: [f32; 2],
    pub parts: &'static [Part],
    /// How far the legs swing: the giant Goliath's half as far (`VENTI`).
    pub leg_angle: f32,
    /// How big the held item is drawn.
    pub item_scale: f32,
}

/// How a doll is drawn.
#[derive(Debug)]
pub enum Look {
    /// The base doll (`DollBase`): Shanghai, Horai, Ooedo, Germany, London, Russia, Grand Guignol.
    Base,
    Boxes(&'static Model),
}

/// `addBox(texX, texY, mirror, x, y, z, w, h, d, scale, pivotX, pivotY, pivotZ, rotX, rotY, rotZ, ..)`
/// (`mirror` is set after the box is made, so the original never mirrored anything).
#[allow(clippy::too_many_arguments)]
const fn bx(
    role: Role,
    child: bool,
    tex: [f32; 2],
    offset: [f32; 3],
    size: [f32; 3],
    inflate: f32,
    pivot: [f32; 3],
    rot: [f32; 3],
) -> Part {
    Part { role, child, b: ModelBox { tex, offset, size, inflate, pivot }, rot }
}

use Role::{Body, Head, Headwear, LeftArm, LeftLeg, RightArm, RightLeg};

const NO_TURN: [f32; 3] = [0.0, 0.0, 0.0];

/// The parts the tall dolls (`TALL`) share.
const TALL_HEAD: Part = bx(Head, false, [0.0, 0.0], [-3.0, -6.0, -3.0], [6.0, 6.0, 6.0], 0.0, [0.0, 8.0, 0.0], NO_TURN);
const TALL_HEADWEAR: Part =
    bx(Headwear, false, [24.0, 0.0], [-3.0, -6.0, -3.0], [6.0, 6.0, 6.0], 0.375, [0.0, 8.0, 0.0], NO_TURN);
const TALL_BODY: Part = bx(Body, false, [0.0, 12.0], [-3.0, 0.0, -1.5], [6.0, 7.0, 3.0], 0.0, [0.0, 8.0, 0.0], NO_TURN);
const TALL_CHEST: Part =
    bx(Body, true, [0.0, 24.0], [-3.0, 1.5, -1.0], [6.0, 3.0, 2.0], 0.0, [0.0, 8.0, 0.0], [-20.0, 0.0, 0.0]);
const TALL_RIGHT_ARM: Part =
    bx(RightArm, false, [30.0, 12.0], [-2.0, -1.0, -1.0], [2.0, 7.0, 2.0], 0.0, [-3.0, 9.0, 0.0], NO_TURN);
const TALL_LEFT_ARM: Part =
    bx(LeftArm, false, [30.0, 12.0], [0.0, -1.0, -1.0], [2.0, 7.0, 2.0], 0.0, [3.0, 9.0, 0.0], NO_TURN);
const TALL_RIGHT_LEG: Part =
    bx(RightLeg, false, [18.0, 12.0], [-3.0, 0.0, -1.5], [3.0, 8.0, 3.0], 0.0, [0.0, 16.0, 0.0], NO_TURN);
const TALL_LEFT_LEG: Part =
    bx(LeftLeg, false, [18.0, 12.0], [0.0, 0.0, -1.5], [3.0, 8.0, 3.0], 0.0, [0.0, 16.0, 0.0], NO_TURN);
const TALL_SKIRT: [Part; 3] = [
    bx(Body, true, [0.0, 29.0], [-3.0, 0.0, -2.0], [6.0, 1.0, 4.0], 0.0, [0.0, 14.0, 0.0], NO_TURN),
    bx(Body, true, [0.0, 34.0], [-4.0, 0.0, -3.0], [8.0, 2.0, 6.0], 0.0, [0.0, 15.0, 0.0], NO_TURN),
    bx(Body, true, [0.0, 42.0], [-5.0, 0.0, -4.0], [10.0, 3.0, 8.0], 0.0, [0.0, 17.0, 0.0], NO_TURN),
];

pub static MARISA: Model = Model {
    tex: [64.0, 64.0],
    parts: &[
        TALL_HEAD,
        bx(Head, true, [22.0, 23.0], [-5.0, -4.8, -6.0], [10.0, 1.0, 11.0], 0.0, [0.0, 8.0, 0.0], [-8.520_71, 0.0, 0.0]),
        bx(Head, true, [38.0, 12.0], [-2.0, -7.5, -3.0], [4.0, 3.0, 5.0], 0.0, [0.0, 8.0, 0.0], [-10.650_888, 0.0, 0.0]),
        bx(Head, true, [48.0, 5.0], [-1.0, -8.5, -2.0], [2.0, 1.0, 5.0], 0.0, [0.0, 8.0, 0.0], [-10.650_888, 0.0, 0.0]),
        TALL_HEADWEAR,
        TALL_BODY,
        TALL_CHEST,
        TALL_SKIRT[0],
        TALL_SKIRT[1],
        TALL_SKIRT[2],
        TALL_RIGHT_ARM,
        TALL_LEFT_ARM,
        TALL_RIGHT_LEG,
        TALL_LEFT_LEG,
    ],
    leg_angle: 1.4,
    item_scale: 0.8,
};

pub static REIMU: Model = Model {
    tex: [64.0, 64.0],
    parts: &[
        TALL_HEAD,
        bx(Head, true, [38.0, 19.0], [-4.0, -1.0, -1.0], [8.0, 1.0, 4.0], 0.0, [0.0, 2.0, 3.0], [-66.63, 0.0, 0.0]),
        bx(Head, true, [38.0, 12.0], [-1.0, -1.0, 0.0], [2.0, 1.0, 6.0], 0.0, [0.0, 2.0, 4.0], [-80.94, 0.0, 0.0]),
        TALL_HEADWEAR,
        TALL_BODY,
        TALL_CHEST,
        TALL_SKIRT[0],
        TALL_SKIRT[1],
        TALL_SKIRT[2],
        TALL_RIGHT_ARM,
        TALL_LEFT_ARM,
        TALL_RIGHT_LEG,
        TALL_LEFT_LEG,
    ],
    leg_angle: 1.4,
    item_scale: 0.8,
};

pub static AYA: Model = Model {
    tex: [64.0, 64.0],
    parts: &[
        TALL_HEAD,
        bx(Head, true, [24.0, 32.0], [-1.0, -7.5, -1.0], [2.0, 2.0, 2.0], 0.0, [0.0, 8.0, 0.0], [0.0, 45.0, 0.0]),
        TALL_HEADWEAR,
        bx(Body, false, [0.0, 12.0], [-3.0, 0.0, -1.5], [6.0, 8.0, 3.0], 0.0, [0.0, 8.0, 0.0], NO_TURN),
        bx(Body, true, [24.0, 12.0], [-3.0, 1.5, -1.0], [6.0, 3.0, 2.0], 0.0, [0.0, 8.0, 0.0], [-15.0, 0.0, 0.0]),
        bx(Body, true, [24.0, 17.0], [-4.0, 1.0, -2.0], [8.0, 3.0, 4.0], 0.0, [0.0, 14.0, 0.0], NO_TURN),
        bx(Body, true, [24.0, 24.0], [-4.5, 3.0, -3.0], [9.0, 2.0, 6.0], 0.0, [0.0, 14.0, 0.0], NO_TURN),
        bx(RightArm, false, [0.0, 23.0], [-2.0, -1.0, -1.0], [2.0, 7.0, 2.0], 0.0, [-3.0, 9.0, 0.0], NO_TURN),
        bx(LeftArm, false, [8.0, 23.0], [0.0, -1.0, -1.0], [2.0, 7.0, 2.0], 0.0, [3.0, 9.0, 0.0], NO_TURN),
        bx(RightLeg, false, [0.0, 32.0], [-1.0, 0.0, -1.5], [3.0, 8.0, 3.0], 0.0, [-2.0, 16.0, 0.0], NO_TURN),
        bx(LeftLeg, false, [12.0, 32.0], [-2.0, 0.0, -1.5], [3.0, 8.0, 3.0], 0.0, [2.0, 16.0, 0.0], NO_TURN),
    ],
    leg_angle: 1.4,
    item_scale: 0.8,
};

/// A mermaid: a tail, no legs.
pub static WAKASAGI: Model = Model {
    tex: [64.0, 64.0],
    parts: &[
        TALL_HEAD,
        bx(Head, true, [48.0, 0.0], [-2.5, -1.0, -1.0], [2.0, 1.0, 3.0], 0.0, [-2.0, 5.0, -1.0], [-76.15, 0.0, -25.56]),
        bx(Head, true, [48.0, 0.0], [0.0, -1.0, -1.0], [2.0, 1.0, 3.0], 0.0, [2.5, 5.0, -1.0], [-76.15, 0.0, 25.56]),
        TALL_HEADWEAR,
        TALL_BODY,
        TALL_CHEST,
        bx(Body, true, [0.0, 29.0], [-3.0, 0.0, -2.0], [6.0, 1.0, 5.0], 0.0, [0.0, 15.0, 0.0], NO_TURN),
        bx(Body, true, [22.0, 25.0], [-4.0, 0.0, -3.0], [8.0, 4.0, 6.0], 0.0, [0.0, 15.5, 0.5], [9.0, 0.0, 0.0]),
        bx(Body, true, [0.0, 42.0], [-3.0, 0.0, -4.0], [6.0, 7.0, 5.0], 0.0, [0.0, 16.0, 2.0], [15.0, 0.0, 0.0]),
        bx(Body, true, [22.0, 42.0], [-2.0, 0.0, 0.0], [4.0, 4.0, 7.0], 0.0, [0.0, 20.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 54.0], [-1.5, -1.5, 0.0], [3.0, 3.0, 4.0], 0.0, [0.0, 22.0, 6.0], [21.3, 0.0, 0.0]),
        bx(Body, true, [14.0, 54.0], [-4.0, 0.0, 1.0], [8.0, 1.0, 4.0], 0.0, [0.0, 21.0, 7.0], [21.3, 0.0, 0.0]),
        TALL_RIGHT_ARM,
        TALL_LEFT_ARM,
    ],
    leg_angle: 1.4,
    item_scale: 0.8,
};

/// Yuuka (`GRANDE`): a little taller, the head lower on the shoulders.
pub static YUUKA: Model = Model {
    tex: [64.0, 64.0],
    parts: &[
        bx(Head, false, [0.0, 0.0], [-3.0, -6.0, -3.0], [6.0, 6.0, 6.0], 0.0, [0.0, 6.0, 0.0], NO_TURN),
        bx(Headwear, false, [24.0, 0.0], [-3.0, -6.0, -3.0], [6.0, 6.0, 6.0], 0.375, [0.0, 6.0, 0.0], NO_TURN),
        bx(Body, false, [0.0, 12.0], [-3.0, 0.0, -1.5], [6.0, 8.0, 3.0], 0.0, [0.0, 6.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 24.0], [-3.0, 1.5, -1.0], [6.0, 4.0, 2.0], 0.0, [0.0, 5.0, 0.0], [-20.0, 0.0, 0.0]),
        bx(Body, true, [0.0, 30.0], [-3.0, 0.0, -2.0], [6.0, 1.0, 4.0], 0.0, [0.0, 13.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 35.0], [-4.0, 0.0, -3.0], [8.0, 2.0, 6.0], 0.0, [0.0, 14.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 43.0], [-5.0, 0.0, -4.5], [10.0, 5.0, 9.0], 0.0, [0.0, 16.0, 0.0], NO_TURN),
        bx(RightArm, false, [30.0, 12.0], [-2.0, -1.0, -1.0], [2.0, 8.0, 2.0], 0.0, [-3.0, 7.0, 0.0], NO_TURN),
        bx(LeftArm, false, [30.0, 12.0], [0.0, -1.0, -1.0], [2.0, 8.0, 2.0], 0.0, [3.0, 7.0, 0.0], NO_TURN),
        bx(RightLeg, false, [18.0, 12.0], [-3.0, 0.0, -1.5], [3.0, 9.0, 3.0], 0.0, [0.0, 15.0, 0.0], NO_TURN),
        bx(LeftLeg, false, [18.0, 12.0], [0.0, 0.0, -1.5], [3.0, 9.0, 3.0], 0.0, [0.0, 15.0, 0.0], NO_TURN),
    ],
    leg_angle: 1.4,
    item_scale: 0.8,
};

/// Goliath (`VENTI`): nearly three blocks tall, on a 128x128 texture.
pub static GOLIATH: Model = Model {
    tex: [128.0, 128.0],
    parts: &[
        bx(Head, false, [0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.0, [0.0, -12.0, 0.0], NO_TURN),
        bx(Head, true, [64.0, 17.0], [-7.0, -1.0, 0.0], [11.0, 4.0, 1.0], 0.0, [0.0, -20.0, 4.0], [28.822, 0.0, -21.302]),
        bx(Head, true, [64.0, 17.0], [-4.0, -1.0, 0.0], [11.0, 4.0, 1.0], 0.0, [0.0, -20.0, 4.0], [28.822, 0.0, 21.302]),
        bx(Head, true, [64.0, 0.0], [-4.0, -1.0, -4.0], [8.0, 9.0, 8.0], 0.5, [0.0, -12.0, 0.0], [14.911, 0.0, 0.0]),
        bx(Headwear, false, [32.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.5, [0.0, -12.0, 0.0], NO_TURN),
        bx(Body, false, [0.0, 40.0], [-4.0, 0.0, -2.0], [8.0, 7.0, 4.0], 0.0, [0.0, -12.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 51.0], [-3.0, 7.0, -2.0], [6.0, 5.0, 4.0], 0.0, [0.0, -12.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 60.0], [-4.0, 12.0, -2.0], [8.0, 4.0, 5.0], 0.0, [0.0, -12.0, 0.0], NO_TURN),
        bx(Body, true, [28.0, 16.0], [-4.0, 1.6, -1.5], [8.0, 5.0, 2.0], 0.0, [0.0, -12.0, 0.0], [-22.0, 0.0, 0.0]),
        bx(Body, true, [0.0, 69.0], [-5.0, 14.0, -3.0], [10.0, 1.0, 7.0], 0.0, [0.0, -12.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 77.0], [-6.0, 15.0, -4.0], [12.0, 2.0, 9.0], 0.0, [0.0, -12.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 88.0], [-7.0, 17.0, -5.0], [14.0, 3.0, 11.0], 0.0, [0.0, -12.0, 0.0], NO_TURN),
        bx(Body, true, [0.0, 102.0], [-8.0, 20.0, -6.0], [16.0, 4.0, 13.0], 0.0, [0.0, -12.0, 0.0], NO_TURN),
        bx(RightArm, false, [16.0, 16.0], [-3.0, -1.0, -1.5], [3.0, 14.0, 3.0], 0.0, [-4.0, -10.0, 0.0], NO_TURN),
        bx(RightArm, true, [28.0, 23.0], [-4.6, -2.0, -2.0], [5.0, 4.0, 4.0], 0.0, [-4.0, -10.0, 0.0], [0.0, 0.0, -12.781]),
        bx(RightArm, true, [28.0, 31.0], [-3.0, 10.0, -1.5], [3.0, 1.0, 3.0], 0.5, [-4.0, -10.0, 0.0], NO_TURN),
        bx(LeftArm, false, [16.0, 16.0], [0.0, -1.0, -1.5], [3.0, 14.0, 3.0], 0.0, [4.0, -10.0, 0.0], NO_TURN),
        bx(LeftArm, true, [46.0, 23.0], [-0.4, -2.0, -2.0], [5.0, 4.0, 4.0], 0.0, [4.0, -10.0, 0.0], [0.0, 0.0, 12.781]),
        bx(LeftArm, true, [28.0, 31.0], [-1.0, 10.0, -1.5], [3.0, 1.0, 3.0], 0.5, [5.0, -10.0, 0.0], NO_TURN),
        bx(RightLeg, false, [0.0, 16.0], [-2.0, 4.0, 0.0], [4.0, 20.0, 4.0], 0.0, [-2.0, 0.0, -2.0], NO_TURN),
        bx(LeftLeg, false, [0.0, 16.0], [2.0, 4.0, 0.0], [4.0, 20.0, 4.0], 0.0, [-2.0, 0.0, -2.0], NO_TURN),
    ],
    leg_angle: 0.7,
    item_scale: 1.5,
};

/// Turns what follows about `pivot` the way a `ModelRenderer` does (z, then y, then x), given in the
/// original's space: the pose has y and z the other way, so those two turn the other way.
fn turn(draw: &mut BlockEntityDraw, pivot: [f32; 3], rot: [f32; 3]) {
    let at = to_pose(pivot);
    if rot[2] != 0.0 {
        draw.rotate_about([0.0, 0.0, 1.0], -rot[2], at);
    }
    if rot[1] != 0.0 {
        draw.rotate_about([0.0, 1.0, 0.0], -rot[1], at);
    }
    if rot[0] != 0.0 {
        draw.rotate_about([1.0, 0.0, 0.0], rot[0], at);
    }
}

fn draw_box_on(host: &dyn Host, draw: &mut BlockEntityDraw, sprite: &str, b: &ModelBox, tex: [f32; 2], tint: u32) -> bool {
    let mut drawn = false;
    for face in faces_on(b, tex) {
        drawn |= host.draw_quad(draw, sprite, face.corners, face.uv, tint);
    }
    drawn
}

/// Draws one of the extra dolls for one frame: false when its texture is missing.
pub fn draw_model(
    host: &dyn Host,
    mob: &NativeMobRender<'_>,
    draw: &mut BlockEntityDraw,
    sprite: &str,
    held: &str,
    model: &Model,
) -> bool {
    let tint = if mob.hurt_time > 0 || mob.death_time > 0 { 0xFF_80_80 } else { 0xFF_FF_FF };
    let pi = std::f32::consts::PI;
    let phase = mob.walk_position * 0.6662;
    let leg = (phase.cos() * model.leg_angle * mob.walk_speed).to_degrees();
    let swing = super::goals::swing_progress(mob.id);
    let mut arm = (phase + pi).cos() * mob.walk_speed;
    if swing > 0.0 {
        let eased = 1.0 - (1.0 - swing).powi(4);
        arm -= (eased * pi).sin() * 1.2 + (swing * pi).sin() * 0.525;
    }
    let left_arm = (phase + pi).cos() * mob.walk_speed;
    let pose = |role: Role| -> [f32; 3] {
        match role {
            Head | Headwear => [mob.pitch, mob.head_yaw, 0.0],
            Body => NO_TURN,
            RightArm => [arm.to_degrees(), 0.0, 0.0],
            LeftArm => [-left_arm.to_degrees(), 0.0, 0.0],
            RightLeg => [leg, 0.0, 0.0],
            LeftLeg => [-leg, 0.0, 0.0],
        }
    };

    draw.push();
    if mob.death_time > 0 {
        let tipped = ((mob.death_time as f32 + mob.partial - 1.0) / 20.0 * 1.6).sqrt().min(1.0);
        draw.rotate_about([0.0, 0.0, 1.0], tipped * 90.0, [0.0, 0.0, 0.0]);
    }
    let mut drawn = false;
    for part in model.parts.iter().filter(|p| !p.child) {
        draw.push();
        turn(draw, part.b.pivot, pose(part.role));
        drawn |= draw_box_on(host, draw, sprite, &part.b, model.tex, tint);
        for child in model.parts.iter().filter(|c| c.child && c.role == part.role) {
            draw.push();
            turn(draw, child.b.pivot, child.rot);
            draw_box_on(host, draw, sprite, &child.b, model.tex, tint);
            draw.pop();
        }
        if part.role == RightArm && !held.is_empty() {
            // The hand: just past the bottom of the arm, in the middle of it.
            let b = part.b;
            let hand = [
                b.pivot[0] + b.offset[0] + b.size[0] / 2.0,
                b.pivot[1] + b.offset[1] + b.size[1] + 1.0,
                b.pivot[2] + b.offset[2] + b.size[2] / 2.0,
            ];
            draw.push();
            draw.translate(to_pose(hand));
            draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.0, 0.0, 0.0]);
            draw.rotate_about([0.0, 1.0, 0.0], 180.0, [0.0, 0.0, 0.0]);
            let s = model.item_scale;
            draw.scale([s, s, s]);
            host.draw_item(draw, &ItemStack::new(held, 1), DisplayContext::ThirdPersonRightHand, None, 0);
            draw.pop();
        }
        draw.pop();
    }
    draw.pop();
    drawn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_extra_model_stands_on_the_ground_and_hangs_things_only_from_parts_it_has() {
        for (model, tall) in [(&MARISA, 22.0), (&REIMU, 22.0), (&AYA, 22.0), (&YUUKA, 24.0), (&GOLIATH, 44.0)] {
            let feet = model
                .parts
                .iter()
                .map(|p| p.b.pivot[1] + p.b.offset[1] + p.b.size[1])
                .fold(f32::MIN, f32::max);
            assert_eq!(feet, 24.0, "{model:?}");
            let top = model
                .parts
                .iter()
                .filter(|p| p.role == Head && !p.child)
                .map(|p| p.b.pivot[1] + p.b.offset[1])
                .fold(f32::MAX, f32::min);
            assert_eq!(24.0 - top, tall);
        }
        for model in [&MARISA, &REIMU, &AYA, &WAKASAGI, &YUUKA, &GOLIATH] {
            for child in model.parts.iter().filter(|p| p.child) {
                assert!(model.parts.iter().any(|p| !p.child && p.role == child.role), "{child:?}");
            }
        }
    }

    #[test]
    fn goliaths_texture_is_unwrapped_at_its_own_size() {
        // The head's front: pixels (8, 8) to (16, 16) of 128x128.
        assert_eq!(faces_on(&GOLIATH.parts[0].b, GOLIATH.tex)[0].uv, [1.0, 1.0, 2.0, 2.0]);
    }

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
