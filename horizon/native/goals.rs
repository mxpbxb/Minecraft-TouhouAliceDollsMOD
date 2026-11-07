// The dolls' goals, compiled into the game as crate::mobs::alice_doll_goals. The priorities and
// numbers are the original mod's: DollBase.onInitializeAI, each doll's onInitializeAI, and their AI
// classes. Horai's and Ooedo's jobs are at the end.
//
// The mod's api crate owns what is saved (owner, mode, chat level). It tells this module each doll's
// state through `set_state`, and collects what the dolls have to say through `take_messages`.
use super::goals::{self, Cx, Goal};
use super::{Damage, Mob, MobEvent, MobWorld, Seen, Species, Who};
use horizon_core::random::RandomSource;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

/// The names that make a chicken a doll, and which doll. The api crate uses these too.
pub const SHANGHAI: &str = "Shanghai Doll";
pub const HORAI: &str = "Horai Doll";
pub const OOEDO: &str = "Ooedo Doll";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Shanghai,
    Horai,
    Ooedo,
}

fn kind_of(m: &Mob) -> Option<Kind> {
    if m.species() != Species::Chicken {
        return None;
    }
    match m.custom_name.as_deref()? {
        SHANGHAI => Some(Kind::Shanghai),
        HORAI => Some(Kind::Horai),
        OOEDO => Some(Kind::Ooedo),
        _ => None,
    }
}

pub const STANDBY: u8 = 0;
pub const PATROL: u8 = 1;
pub const FOLLOW: u8 = 2;
pub const RIDE: u8 = 3;

const FOLLOW_START: f64 = 3.0;
const FOLLOW_STOP: f64 = 1.5;
const TELEPORT_FROM: f64 = 12.0;
/// `searchRange` and `searchHeight`: how far a doll notices anything.
const SEARCH_RANGE: f64 = 16.0;
const SEARCH_HEIGHT: f64 = 8.0;
/// A stone sword's hit (the original's `attackStrength` was 2), and the 9.0 its squared reach is
/// compared with.
const ATTACK_STRENGTH: f32 = 5.0;
const REACH_SQR: f64 = 6.25;
/// How often a doll looks for a target, re-paths to it, and can strike it, in ticks.
const SCAN_EVERY: i64 = 10;
const PATH_EVERY: i64 = 5;
const HIT_EVERY: i64 = 12;
/// How far from its owner a following doll defends.
const OWNER_GUARD: f64 = 12.0;
/// Ticks without seeing its target before a doll gives up.
const LOST_AFTER: i32 = 60;
/// A doll moves 35% faster than a chicken.
const SPEED_SCALE: f64 = 1.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DollState {
    mode: u8,
    enabled: bool,
}

static STATES: LazyLock<Mutex<HashMap<u32, DollState>>> = LazyLock::new(Default::default);
static MESSAGES: Mutex<Vec<(u32, u8, String)>> = Mutex::new(Vec::new());
/// Sheep a doll can still shear: nothing another mob's goal can read says so.
static SHEARABLE: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(Default::default);
/// Sheep a doll has just sheared: a goal cannot change another mob, so the sheep does it itself.
static SHEARED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// The api crate's word on a doll. A mob is only a doll once this has named it.
pub fn set_state(id: u32, mode: u8, enabled: bool) {
    if let Ok(mut states) = STATES.lock() {
        states.insert(id, DollState { mode, enabled });
    }
}

/// What dolls have said since the last call: the doll, the chat level it needs, and the text.
pub fn take_messages() -> Vec<(u32, u8, String)> {
    MESSAGES.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default()
}

fn say(id: u32, level: u8, text: impl Into<String>) {
    if let Ok(mut messages) = MESSAGES.lock() {
        messages.push((id, level, text.into()));
    }
}

/// Dolls used up since the last call (an Ooedo that exploded), whose items go back to their owners.
static SPENT: Mutex<Vec<u32>> = Mutex::new(Vec::new());

pub fn take_spent() -> Vec<u32> {
    SPENT.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default()
}

fn spend(id: u32) {
    if let Ok(mut spent) = SPENT.lock() {
        spent.push(id);
    }
}

fn state(id: u32) -> Option<DollState> {
    STATES.lock().ok()?.get(&id).copied()
}

fn is_doll(id: u32) -> bool {
    state(id).is_some()
}

fn in_mode(m: &Mob, mode: u8) -> bool {
    state(m.id).is_some_and(|s| s.enabled && s.mode == mode)
}

/// Runs before every mob's tick. Sheep take the shearing a doll gave them; a doll gets its goals.
pub fn ensure(m: &mut Mob, w: &MobWorld) {
    match m.species() {
        Species::Sheep => tick_sheep(m),
        Species::Chicken => tick_doll(m, w),
        _ => {}
    }
}

/// Rideon: on the owner's head every tick, before the doll moves, ahead by the owner's own motion
/// so it does not trail behind.
fn sit_on_head(m: &mut Mob, w: &MobWorld) {
    let Some(p) = w.player.as_ref() else { return };
    m.position.x = p.position.x + p.velocity.x;
    m.position.y = p.eye[1] + 0.25 + p.velocity.y;
    m.position.z = p.position.z + p.velocity.z;
    m.previous = m.position;
    m.velocity = horizon_core::geometry::Vec3::new(0.0, 0.0, 0.0);
    m.fall_distance = 0.0;
    m.yaw = p.yaw;
    m.nav_stop();
}

fn tick_sheep(m: &mut Mob) {
    if let Ok(mut sheared) = SHEARED.lock()
        && let Some(at) = sheared.iter().position(|&id| id == m.id)
    {
        sheared.swap_remove(at);
        m.extra.sheared = true;
    }
    if let Ok(mut shearable) = SHEARABLE.lock() {
        if !m.extra.sheared && !m.is_baby() && m.alive() {
            shearable.insert(m.id);
        } else {
            shearable.remove(&m.id);
        }
    }
}

/// A chicken named as a doll, whether or not the api crate has spoken for it yet: the sound hooks
/// ask this, so a doll is quiet from its first tick, on a client too.
pub fn is_doll_mob(m: &Mob) -> bool {
    kind_of(m).is_some()
}

fn tick_doll(m: &mut Mob, w: &MobWorld) {
    let Some(kind) = kind_of(m) else { return };
    // Silent stops the hurt, death and step sounds; the doll-voice hook stops the rest.
    m.silent = true;
    m.extra.egg_time = i32::MAX;
    // Checked every tick: the game may hand the mob its chicken type back.
    if !m.kind.sounds[0].is_empty() || m.kind.step_sound.is_some() {
        let kind = Arc::make_mut(&mut m.kind);
        kind.sounds = ["", "", ""];
        kind.step_sound = None;
    }
    // Every walk and run of a doll scales from this.
    m.attrs.speed = m.kind.speed * SPEED_SCALE;
    let Some(state) = state(m.id) else { return };
    m.no_gravity = state.mode == RIDE;
    if state.mode == RIDE && state.enabled {
        sit_on_head(m, w);
    }
    if m.ai.goals.has("DollFollow") {
        return;
    }
    let ai = &mut m.ai;
    ai.goals.clear();
    ai.targets.clear();
    ai.goals.add(0, goals::Float);
    ai.goals.add(0, DollRide);
    // Each doll's own jobs: its `onInitializeAI`.
    match kind {
        Kind::Shanghai => {
            ai.goals.add(1, DollReport::default());
            ai.goals.add(2, DollAttack::default());
            ai.goals.add(3, DollSteal::default());
        }
        Kind::Horai => {
            ai.goals.add(1, ReportOre::default());
            ai.goals.add(2, MineOre::default());
        }
        Kind::Ooedo => {
            ai.goals.add(4, Explode::default());
        }
    }
    ai.goals.add(12, DollFollow { recalculate_in: 0 });
    ai.goals.add(13, goals::Stroll::new(1.0).gate(patrolling).named("DollWander"));
    ai.goals.add(14, goals::LookAtPlayer::new(8.0));
    ai.goals.add(16, goals::RandomLookAround::default());
}

fn patrolling(m: &Mob, _: &MobWorld) -> bool {
    in_mode(m, PATROL)
}

fn player_position(cx: &Cx) -> Option<[f64; 3]> {
    let p = cx.w.player.as_ref()?;
    (!p.spectator).then_some([p.position.x, p.position.y, p.position.z])
}

fn distance_sqr(m: &Mob, at: [f64; 3]) -> f64 {
    let (dx, dy, dz) = (m.position.x - at[0], m.position.y - at[1], m.position.z - at[2]);
    dx * dx + dy * dy + dz * dz
}

fn pos(s: &Seen) -> [f64; 3] {
    [s.position.x, s.position.y, s.position.z]
}

/// The box the original searched: `boundingBox.expand(range, height, range)`.
fn noticed(from: [f64; 3], s: &Seen) -> bool {
    s.alive
        && (s.position.x - from[0]).abs() <= SEARCH_RANGE
        && (s.position.y - from[1]).abs() <= SEARCH_HEIGHT
        && (s.position.z - from[2]).abs() <= SEARCH_RANGE
}

fn here(m: &Mob) -> [f64; 3] {
    [m.position.x, m.position.y, m.position.z]
}

/// The nearest noticed mob that passes `wanted`.
fn nearest(m: &Mob, cx: &Cx, wanted: impl Fn(&Seen) -> bool) -> Option<u32> {
    let from = here(m);
    cx.w.others
        .iter()
        .filter(|s| s.id != m.id && noticed(from, s) && wanted(s))
        .min_by(|a, b| distance_sqr(m, pos(a)).total_cmp(&distance_sqr(m, pos(b))))
        .map(|s| s.id)
}

/// What a patrolling doll attacks: every hostile mob (the `monster` category, plus the hostile mobs
/// filed elsewhere), as the original's `targetEntityRegex` named the hostile mobs of 1.7. Zombified
/// piglins and piglins are left alone: hitting one turns the whole group on the owner.
fn patrol_target(s: &Seen) -> bool {
    if matches!(s.species, Species::ZombifiedPiglin | Species::Piglin) {
        return false;
    }
    s.category == "monster"
        || matches!(
            s.species,
            Species::Slime
                | Species::MagmaCube
                | Species::Ghast
                | Species::Phantom
                | Species::Shulker
                | Species::Hoglin
                | Species::Zoglin
                | Species::PiglinBrute
                | Species::Guardian
                | Species::ElderGuardian
                | Species::Wither
                | Species::ZombieNautilus
        )
}

/// Rideon mode: sits on the player's head.
#[derive(Clone, Debug)]
struct DollRide;

impl Goal for DollRide {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK | goals::JUMP
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        in_mode(m, RIDE) && cx.w.player.is_some()
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        // Holds MOVE so nothing else walks it off; the hook before each tick does the sitting.
        sit_on_head(m, cx.w);
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "DollRide"
    }
}

/// `EntityDollAIReportEntity`: every second, what it can see, in any mode.
#[derive(Clone, Debug, Default)]
struct DollReport {
    last: i64,
}

impl Goal for DollReport {
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        state(m.id).is_some_and(|s| s.enabled)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if cx.w.game_time - self.last < 20 {
            return;
        }
        self.last = cx.w.game_time;
        let from = here(m);
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for s in cx.w.others.iter().filter(|s| s.id != m.id && noticed(from, s) && !is_doll(s.id)) {
            *counts.entry(format!("{:?}", s.species)).or_default() += 1;
        }
        if counts.is_empty() {
            say(m.id, 2, "No target");
        } else {
            let list: String = counts.iter().map(|(name, n)| format!("{name}[{n}]")).collect();
            say(m.id, 1, list);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "DollReport"
    }
}

/// `EntityDollAIAttackEnemy`, made steadier: on patrol, every hostile mob around it; following,
/// every hostile mob near its owner as well as whatever is after its owner. It looks again twice a
/// second, re-paths four times a second, and strikes as often as a stone sword can be swung.
#[derive(Clone, Debug, Default)]
struct DollAttack {
    target: Option<u32>,
    last_scan: i64,
    last_hit: i64,
    last_path: i64,
    lost: i32,
}

fn near_owner(cx: &Cx, s: &Seen) -> bool {
    player_position(cx).is_some_and(|p| {
        let (dx, dy, dz) = (s.position.x - p[0], s.position.y - p[1], s.position.z - p[2]);
        dx * dx + dy * dy + dz * dz <= OWNER_GUARD * OWNER_GUARD
    })
}

fn enemy(s: &Seen) -> bool {
    patrol_target(s) && !is_doll(s.id)
}

/// Close enough to hit: two and a half blocks across, and up to three above or below, so a
/// low-flying fairy or phantom can be reached.
fn in_reach(m: &Mob, at: [f64; 3]) -> bool {
    let (dx, dy, dz) = (m.position.x - at[0], m.position.y - at[1], m.position.z - at[2]);
    dx * dx + dz * dz < REACH_SQR && dy.abs() < 3.0
}

impl Goal for DollAttack {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if cx.w.game_time - self.last_scan < SCAN_EVERY {
            return false;
        }
        self.last_scan = cx.w.game_time;
        self.target = if in_mode(m, PATROL) {
            nearest(m, cx, enemy)
        } else if in_mode(m, FOLLOW) {
            nearest(m, cx, |s| {
                matches!(s.target, Some(Who::Player)) || (enemy(s) && near_owner(cx, s))
            })
        } else {
            None
        };
        self.target.is_some()
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(s) = self.target.and_then(|id| cx.w.seen(id)) else { return false };
        let keep = if in_mode(m, PATROL) {
            true
        } else if in_mode(m, FOLLOW) {
            // A following doll does not chase off: it lets go of anything that leaves its owner.
            matches!(s.target, Some(Who::Player)) || near_owner(cx, s)
        } else {
            false
        };
        keep && s.alive && self.lost < LOST_AFTER
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.lost = 0;
        self.last_path = 0;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        self.target = None;
        m.nav_stop();
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(id) = self.target else { return };
        let Some((at, eye, alive)) = cx.w.seen(id).map(|s| (pos(s), s.eye_y, s.alive)) else {
            return;
        };
        m.look_at([at[0], eye, at[2]]);
        if m.sees(cx.w, Who::Mob(id)) {
            self.lost = 0;
        } else {
            self.lost += 1;
        }
        if cx.w.game_time - self.last_path >= PATH_EVERY {
            self.last_path = cx.w.game_time;
            m.nav_move_to_entity(cx.w, at, 0, 1.0);
        }
        if alive && in_reach(m, at) && cx.w.game_time - self.last_hit >= HIT_EVERY {
            self.last_hit = cx.w.game_time;
            m.swing();
            mark_swing(m.id);
            // DamageSource.causePlayerDamage(owner): the kill and its experience are the owner's.
            cx.events.push(MobEvent::AttackMob {
                target: id,
                amount: ATTACK_STRENGTH,
                source: Damage::player(here(m)),
            });
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "DollAttack"
    }
}

/// When each doll last swung, for its model: the renderer is told nothing of a mob's swing.
static SWINGS: LazyLock<Mutex<HashMap<u32, std::time::Instant>>> = LazyLock::new(Default::default);
/// A swing's length: six ticks.
const SWING_SECONDS: f32 = 0.3;

fn mark_swing(id: u32) {
    if let Ok(mut swings) = SWINGS.lock() {
        swings.insert(id, std::time::Instant::now());
    }
}

/// How far through its swing a doll is, 0 to 1, or 0 when it is not swinging.
pub fn swing_progress(id: u32) -> f32 {
    let Ok(swings) = SWINGS.lock() else { return 0.0 };
    let Some(start) = swings.get(&id) else { return 0.0 };
    let t = start.elapsed().as_secs_f32() / SWING_SECONDS;
    if t < 1.0 { t } else { 0.0 }
}

const WOOL: [&str; 16] = [
    "minecraft:white_wool",
    "minecraft:orange_wool",
    "minecraft:magenta_wool",
    "minecraft:light_blue_wool",
    "minecraft:yellow_wool",
    "minecraft:lime_wool",
    "minecraft:pink_wool",
    "minecraft:gray_wool",
    "minecraft:light_gray_wool",
    "minecraft:cyan_wool",
    "minecraft:purple_wool",
    "minecraft:blue_wool",
    "minecraft:brown_wool",
    "minecraft:green_wool",
    "minecraft:red_wool",
    "minecraft:black_wool",
];

fn shearable(id: u32) -> bool {
    SHEARABLE.lock().is_ok_and(|s| s.contains(&id))
}

/// `EntityDollAIStealItem`: on patrol, shears sheep, and now and then plucks a chicken, picks a
/// mooshroom's mushroom or takes the rose an iron golem is holding out.
#[derive(Clone, Debug, Default)]
struct DollSteal {
    target: Option<u32>,
    last_scan: i64,
    last_try: i64,
    lost: i32,
    rose_offered: bool,
}

impl DollSteal {
    /// What to do on reaching `target`: true when finished with it.
    fn take(&mut self, m: &mut Mob, cx: &mut Cx, target: u32, species: Species, color: u8, at: [f64; 3]) -> bool {
        let drop = |cx: &mut Cx, item: &str, count: u32| {
            cx.events.push(MobEvent::DropItem { mob: target, item: Arc::from(item), count, at });
        };
        match species {
            Species::Sheep => {
                if let Ok(mut sheared) = SHEARED.lock() {
                    sheared.push(target);
                }
                if let Ok(mut shearable) = SHEARABLE.lock() {
                    shearable.remove(&target);
                }
                let count = 1 + m.random.next_int_bound(3).unwrap_or(0) as u32;
                drop(cx, WOOL[usize::from(color) % WOOL.len()], count);
                cx.events.push(MobEvent::Sound("entity.sheep.shear", at, 0.5, 1.0));
                say(m.id, 2, "Wool!");
                m.swing();
                mark_swing(m.id);
                true
            }
            Species::Chicken | Species::Mooshroom => {
                let (item, text) = if species == Species::Chicken {
                    ("minecraft:feather", "Feather!")
                } else {
                    ("minecraft:red_mushroom", "Mushroom!")
                };
                drop(cx, item, 1);
                say(m.id, 2, text);
                m.swing();
                mark_swing(m.id);
                cx.events.push(MobEvent::AttackMob {
                    target,
                    amount: 0.0,
                    source: Damage::mob(m.id, here(m)),
                });
                true
            }
            Species::IronGolem => {
                if !self.rose_offered {
                    self.rose_offered = true;
                    cx.events.push(MobEvent::EntityEvent { mob: target, event: 11, at });
                }
                if m.random.next_float() >= 0.1 {
                    return false;
                }
                cx.events.push(MobEvent::EntityEvent { mob: target, event: 34, at });
                drop(cx, "minecraft:poppy", 1);
                say(m.id, 2, "Rose!");
                true
            }
            _ => true,
        }
    }
}

impl Goal for DollSteal {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if !in_mode(m, PATROL) || cx.w.game_time - self.last_scan < 20 {
            return false;
        }
        self.last_scan = cx.w.game_time;
        self.target = nearest(m, cx, |s| s.species == Species::Sheep && shearable(s.id));
        // Each of the others gets a one in twenty chance a second.
        if self.target.is_none() && m.random.next_float() < 0.05 {
            self.target =
                nearest(m, cx, |s| s.species == Species::Chicken && !s.baby && !is_doll(s.id));
        }
        if self.target.is_none() && m.random.next_float() < 0.05 {
            self.target = nearest(m, cx, |s| s.species == Species::Mooshroom && !s.baby);
        }
        if self.target.is_none() && m.random.next_float() < 0.05 {
            self.target = nearest(m, cx, |s| s.species == Species::IronGolem);
        }
        self.target.is_some()
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        in_mode(m, PATROL)
            && self.lost < LOST_AFTER
            && self.target.and_then(|id| cx.w.seen(id)).is_some_and(|s| s.alive)
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.lost = 0;
        self.rose_offered = false;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        self.target = None;
        m.nav_stop();
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(id) = self.target else { return };
        let Some((at, eye, species, color)) =
            cx.w.seen(id).map(|s| (pos(s), s.eye_y, s.species, s.color))
        else {
            return;
        };
        let sees = m.sees(cx.w, Who::Mob(id));
        if sees {
            m.look_at([at[0], eye, at[2]]);
            self.lost = (self.lost - 1).max(0);
        } else {
            self.lost += 1;
        }
        if cx.w.game_time - self.last_try < 20 {
            return;
        }
        self.last_try = cx.w.game_time;
        m.nav_move_to_entity(cx.w, at, 0, 1.0);
        if sees && in_reach(m, at) && self.take(m, cx, id, species, color, at) {
            self.target = None;
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "DollSteal"
    }
}

/// Walks to the player, and teleports beside them from far away (`EntityDollAIFollowOwner`).
#[derive(Clone, Debug)]
struct DollFollow {
    recalculate_in: i32,
}

impl Goal for DollFollow {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        in_mode(m, FOLLOW)
            && player_position(cx).is_some_and(|at| distance_sqr(m, at) >= FOLLOW_START * FOLLOW_START)
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        in_mode(m, FOLLOW)
            && player_position(cx).is_some_and(|at| distance_sqr(m, at) > FOLLOW_STOP * FOLLOW_STOP)
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.recalculate_in = 0;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(at) = player_position(cx) else { return };
        let eye = cx.w.player.as_ref().map_or(at[1], |p| p.eye[1]);
        m.look_at([at[0], eye, at[2]]);
        self.recalculate_in -= 1;
        if self.recalculate_in > 0 {
            return;
        }
        self.recalculate_in = 10;
        if distance_sqr(m, at) >= TELEPORT_FROM * TELEPORT_FROM {
            teleport_near(m, cx, at);
        } else {
            m.nav_move_to(cx.w, at, 1.0);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "DollFollow"
    }
}

/// `TamableAnimal.tryToTeleportToOwner`: ten tries at a free block near the player.
fn teleport_near(m: &mut Mob, cx: &mut Cx, at: [f64; 3]) {
    let base = at.map(|c| c.floor() as i32);
    for _ in 0..10 {
        let dx = m.random.next_int_bound(7).unwrap_or(0) - 3;
        let dz = m.random.next_int_bound(7).unwrap_or(0) - 3;
        if dx.abs() < 2 && dz.abs() < 2 {
            continue;
        }
        let dy = m.random.next_int_bound(3).unwrap_or(0) - 1;
        let (x, y, z) = (base[0] + dx, base[1] + dy, base[2] + dz);
        let world = cx.w;
        let below = world.facts.get(world.world.block(x, y - 1, z));
        let here = world.facts.get(world.world.block(x, y, z));
        let head = world.facts.get(world.world.block(x, y + 1, z));
        if below.full_collision && !here.full_collision && !head.full_collision {
            m.position.x = f64::from(x) + 0.5;
            m.position.y = f64::from(y);
            m.position.z = f64::from(z) + 0.5;
            m.previous = m.position;
            m.nav_stop();
            // `teleportToXYZ`: mob.endermen.portal.
            let to = [m.position.x, m.position.y, m.position.z];
            cx.events.push(MobEvent::Sound("entity.enderman.teleport", to, 1.0, 1.0));
            return;
        }
    }
}

// ---- Horai: EntityDollAIReportBlock and EntityDollAIMineBlock ----

/// `mineRange`: how far, in steps along the axes, Horai looks for ore and digs.
const MINE_RANGE: i32 = 3;
/// `mineSpeed`: a block takes 20 * hardness / mineSpeed ticks to dig.
const MINE_SPEED: f32 = 2.5;
/// Obsidian's hardness: nothing harder is dug.
const HARDEST: f32 = 50.0;
/// Below this a dug block would open onto the void.
const WORLD_BOTTOM: i32 = -64;

fn name_at(w: &MobWorld, p: [i32; 3]) -> Arc<str> {
    w.facts.get(w.world.block(p[0], p[1], p[2])).name.clone()
}

fn hardness_at(w: &MobWorld, p: [i32; 3]) -> f32 {
    w.facts.get(w.world.block(p[0], p[1], p[2])).hardness
}

fn is_air(name: &str) -> bool {
    matches!(name, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
}

/// `targetBlockRegex` (`ore.+|netherquartz`): every ore, deepslate and nether ones included.
fn is_ore(name: &str) -> bool {
    name.ends_with("_ore")
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// `canDigBlock`: solid, no harder than obsidian, nothing that falls on top, and no drop into the
/// void or lava underneath.
fn can_dig(w: &MobWorld, p: [i32; 3]) -> bool {
    let name = name_at(w, p);
    if is_air(&name) || matches!(&*name, "minecraft:water" | "minecraft:lava") {
        return false;
    }
    let hardness = hardness_at(w, p);
    if !(0.0..=HARDEST).contains(&hardness) {
        return false;
    }
    let above = name_at(w, [p[0], p[1] + 1, p[2]]);
    if matches!(&*above, "minecraft:sand" | "minecraft:red_sand" | "minecraft:gravel") {
        return false;
    }
    let mut y = p[1] - 1;
    while is_air(&name_at(w, [p[0], y, p[2]])) {
        if y <= WORLD_BOTTOM || &*name_at(w, [p[0], y - 1, p[2]]) == "minecraft:lava" {
            return false;
        }
        y -= 1;
    }
    true
}

/// The block at the doll's eyes, where the original measured from.
fn eye_block(m: &Mob) -> [i32; 3] {
    [
        m.position.x.floor() as i32,
        (m.position.y + m.kind.eye_height).floor() as i32,
        m.position.z.floor() as i32,
    ]
}

/// Every offset within `MINE_RANGE` steps.
fn around() -> impl Iterator<Item = [i32; 3]> {
    let r = MINE_RANGE;
    (-r..=r).flat_map(move |dy| {
        (-r..=r).flat_map(move |dx| {
            (-r..=r).filter_map(move |dz| {
                (dx.abs() + dy.abs() + dz.abs() <= r).then_some([dx, dy, dz])
            })
        })
    })
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn steps(a: [i32; 3], b: [i32; 3]) -> i32 {
    (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs()
}

/// `EntityDollAIReportBlock`: every second, the ore within reach.
#[derive(Clone, Debug, Default)]
struct ReportOre {
    last: i64,
}

impl Goal for ReportOre {
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        state(m.id).is_some_and(|s| s.enabled)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if cx.w.game_time - self.last < 20 {
            return;
        }
        self.last = cx.w.game_time;
        let eye = eye_block(m);
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for d in around() {
            let name = name_at(cx.w, add(eye, d));
            if is_ore(&name) {
                *counts.entry(short(&name).to_owned()).or_default() += 1;
            }
        }
        if counts.is_empty() {
            say(m.id, 2, "No target");
        } else {
            let list: String = counts.iter().map(|(name, n)| format!("{name}[{n}]")).collect();
            say(m.id, 1, list);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "ReportOre"
    }
}

/// `EntityDollAIMineBlock`: on patrol or following, digs towards the nearest ore it can reach,
/// one block at a time, taking as long over each as its hardness says.
#[derive(Clone, Debug, Default)]
struct MineOre {
    mine: [i32; 3],
    counter: i32,
    total: i32,
    last_scan: i64,
}

impl MineOre {
    /// The nearest ore, then the first block to dig on the way to it.
    fn find(&mut self, m: &Mob, w: &MobWorld) -> Option<[i32; 3]> {
        let eye = eye_block(m);
        let target = around()
            .filter(|&d| is_ore(&name_at(w, add(eye, d))) && can_dig(w, add(eye, d)))
            .min_by_key(|&d| steps(d, [0; 3]))?;
        // Step from the doll towards the ore, through air, until a block to dig.
        let mut at = [0; 3];
        for _ in 0..MINE_RANGE {
            let mut next = None;
            for d in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
                let cell = add(at, d);
                if steps(target, cell) >= steps(target, at) {
                    continue;
                }
                if can_dig(w, add(eye, cell)) {
                    next = Some((cell, true));
                } else if next.is_none() && is_air(&name_at(w, add(eye, cell))) {
                    next = Some((cell, false));
                }
            }
            let (cell, dig) = next?;
            at = cell;
            if dig {
                return Some(add(eye, at));
            }
        }
        None
    }
}

impl Goal for MineOre {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if !(in_mode(m, PATROL) || in_mode(m, FOLLOW)) || cx.w.game_time - self.last_scan < 20 {
            return false;
        }
        self.last_scan = cx.w.game_time;
        match self.find(m, cx.w) {
            Some(mine) => {
                self.mine = mine;
                true
            }
            None => false,
        }
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        (in_mode(m, PATROL) || in_mode(m, FOLLOW))
            && self.counter >= 0
            && !is_air(&name_at(cx.w, self.mine))
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        let hardness = hardness_at(cx.w, self.mine);
        self.counter = ((20.0 * hardness / MINE_SPEED).floor() as i32).max(0);
        self.total = self.counter.max(1);
        let at = self.mine.map(|c| f64::from(c) + 0.5);
        m.nav_move_to(cx.w, at, 1.0);
    }
    fn stop(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_stop();
        cx.events.push(MobEvent::DestroyProgress { mob: m.id, pos: self.mine, progress: -1 });
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let at = self.mine.map(|c| f64::from(c) + 0.5);
        m.look_at(at);
        if self.counter == 0 {
            cx.events.push(MobEvent::BreakBlock { pos: self.mine, drop: true });
        } else {
            let progress = ((self.total - self.counter) * 10 / self.total).min(9);
            cx.events.push(MobEvent::DestroyProgress { mob: m.id, pos: self.mine, progress });
            if self.counter % 4 == 0 {
                let [x, y, z] = self.mine;
                let facts = cx.w.facts.get(cx.w.world.block(x, y, z));
                let sound = facts.sounds.break_sound.to_string();
                let volume = (facts.sounds.volume + 1.0) / 8.0;
                let pitch = facts.sounds.pitch * 0.5;
                cx.events.push(MobEvent::SoundId(sound, at, volume, pitch));
            }
        }
        if self.counter % 6 == 0 {
            m.swing();
            mark_swing(m.id);
        }
        self.counter -= 1;
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "MineOre"
    }
}

// ---- Ooedo: EntityDollAIExplosion ----

/// `explodeStrength`: a creeper's blast.
const EXPLODE_STRENGTH: f32 = 3.0;
/// Ticks of fuse once close, as a creeper's.
const FUSE: i32 = 30;

/// On patrol, walks up to the nearest hostile mob, fuses for a second and a half, and explodes,
/// its item going back to its owner.
#[derive(Clone, Debug, Default)]
struct Explode {
    target: Option<u32>,
    fuse: i32,
    last_scan: i64,
    last_path: i64,
}

impl Goal for Explode {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if !in_mode(m, PATROL) || cx.w.game_time - self.last_scan < 20 {
            return false;
        }
        self.last_scan = cx.w.game_time;
        self.target = nearest(m, cx, |s| patrol_target(s) && !is_doll(s.id));
        match self.target.and_then(|id| cx.w.seen(id)) {
            Some(s) => say(m.id, 1, format!("{:?}({})", s.species, s.id)),
            None => say(m.id, 2, "No target"),
        }
        self.target.is_some()
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        in_mode(m, PATROL) && self.target.and_then(|id| cx.w.seen(id)).is_some_and(|s| s.alive)
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.fuse = 0;
        self.last_path = 0;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        self.target = None;
        m.nav_stop();
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(id) = self.target else { return };
        let Some((at, eye)) = cx.w.seen(id).map(|s| (pos(s), s.eye_y)) else { return };
        m.look_at([at[0], eye, at[2]]);
        if cx.w.game_time - self.last_path >= 20 {
            self.last_path = cx.w.game_time;
            m.nav_move_to_entity(cx.w, at, 0, 1.0);
        }
        if distance_sqr(m, at) >= 5.0 {
            self.fuse = (self.fuse - 1).max(0);
            return;
        }
        self.fuse += 1;
        let me = here(m);
        if self.fuse == 1 {
            cx.events.push(MobEvent::Sound("entity.creeper.primed", me, 1.0, 0.5));
        }
        if self.fuse >= FUSE {
            cx.events.push(MobEvent::Explode(me, EXPLODE_STRENGTH, Species::Chicken));
            spend(m.id);
            m.removed = true;
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "Explode"
    }
}
