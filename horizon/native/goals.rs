// The dolls' goals, compiled into the game as crate::mobs::alice_doll_goals. The priorities and
// numbers are the original mod's: DollBase.onInitializeAI, each doll's onInitializeAI, and their AI
// classes. Horai's, Ooedo's, Germany's and London's jobs are at the end.
//
// The mod's api crate owns what is saved (owner, mode, chat level). It tells this module each doll's
// state through `set_state`, and collects what the dolls have to say through `take_messages`.
use super::goals::{self, Cx, Goal};
use super::{Damage, Mob, MobEvent, MobWorld, Seen, Species, Who};
use horizon_core::random::RandomSource;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

/// The names that make a chicken a doll, and which doll. The api crate uses these too.
pub const SHANGHAI: &str = "Shanghai Doll";
pub const HORAI: &str = "Horai Doll";
pub const DIAMOND_HORAI: &str = "Diamond Horai Doll";
pub const OOEDO: &str = "Ooedo Doll";
pub const GERMANY: &str = "Germany Doll";
pub const LONDON: &str = "London Doll";
pub const RUSSIA: &str = "Russia Doll";
pub const GRAND_GUIGNOL: &str = "Grand Guignol Doll";
pub const MARISA: &str = "Marisa Doll";
pub const REIMU: &str = "Reimu Doll";
pub const AYA: &str = "Aya Doll";
pub const WAKASAGI: &str = "Wakasagi Doll";
pub const GOLIATH: &str = "Goliath Doll";
pub const YUUKA: &str = "Yuuka Doll";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Shanghai,
    Horai,
    Ooedo,
    Germany,
    London,
    Russia,
    GrandGuignol,
    Marisa,
    Reimu,
    Aya,
    Wakasagi,
    Goliath,
    Yuuka,
}

fn kind_of(m: &Mob) -> Option<Kind> {
    if m.species() != Species::Chicken {
        return None;
    }
    match m.custom_name.as_deref()? {
        SHANGHAI => Some(Kind::Shanghai),
        HORAI | DIAMOND_HORAI => Some(Kind::Horai),
        OOEDO => Some(Kind::Ooedo),
        GERMANY => Some(Kind::Germany),
        LONDON => Some(Kind::London),
        RUSSIA => Some(Kind::Russia),
        GRAND_GUIGNOL => Some(Kind::GrandGuignol),
        MARISA => Some(Kind::Marisa),
        REIMU => Some(Kind::Reimu),
        AYA => Some(Kind::Aya),
        WAKASAGI => Some(Kind::Wakasagi),
        GOLIATH => Some(Kind::Goliath),
        YUUKA => Some(Kind::Yuuka),
        _ => None,
    }
}

/// Each doll's `getSpeed` next to the base doll's 0.28: Aya is quicker, Yuuka and Wakasagi slower.
fn speed_factor(kind: Kind) -> f64 {
    match kind {
        Kind::Aya => 0.32 / 0.28,
        Kind::Yuuka => 0.24 / 0.28,
        Kind::Wakasagi => 0.22 / 0.28,
        _ => 1.0,
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
/// How far a fighting doll strikes: five blocks across.
const ATTACK_REACH_SQR: f64 = 25.0;
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
    /// The owner's UUID.
    owner: u128,
}

/// A doll's item (`alice_dolls:*_doll`): the item hooks keep these from being destroyed.
pub fn is_doll_item(item: &str) -> bool {
    item.starts_with("alice_dolls:") && item.ends_with("_doll")
}

/// The game's own reading and writing of item slots, for the api crate to save inventories with.
pub use horizon_items::item_nbt::{decode_items, encode_items};

static STATES: LazyLock<Mutex<HashMap<u32, DollState>>> = LazyLock::new(Default::default);
static MESSAGES: Mutex<Vec<(u32, u8, String)>> = Mutex::new(Vec::new());
/// Sheep a doll can still shear: nothing another mob's goal can read says so.
static SHEARABLE: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(Default::default);
/// Sheep a doll has just sheared: a goal cannot change another mob, so the sheep does it itself.
static SHEARED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// The api crate's word on a doll. A mob is only a doll once this has named it.
pub fn set_state(id: u32, mode: u8, enabled: bool, owner: u128) {
    if let Ok(mut states) = STATES.lock() {
        states.insert(id, DollState { mode, enabled, owner });
    }
}

/// Whether a shot or blow from doll `from` must spare `who`: its owner, and every doll of the same
/// owner. Reimu's and Marisa's shots only ever fly at a target that passes this, and hit nothing else.
/// The fork's mob world shows one player, the local one, so `Who::Player` counts as the owner.
pub fn spares(from: u32, who: Who) -> bool {
    match who {
        Who::Player => true,
        Who::Mob(id) => match (state(from), state(id)) {
            (Some(a), Some(b)) => a.owner == b.owner,
            _ => false,
        },
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

// ---- Inventories, shared with the api crate ----

use crate::items::ItemStack;

/// Each doll's inventory while the game runs. The api crate saves it and puts it here; the goals
/// that use it (Russia's torches and pickings, Yuuka's seeds) change it here and mark it changed.
static INVENTORIES: LazyLock<Mutex<HashMap<u32, Vec<Option<ItemStack>>>>> = LazyLock::new(Default::default);
static CHANGED: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(Default::default);

/// The api crate's copy of doll `id`'s inventory.
pub fn put_inventory(id: u32, items: Vec<Option<ItemStack>>) {
    if let Ok(mut all) = INVENTORIES.lock() {
        all.insert(id, items);
    }
}

pub fn inventory(id: u32) -> Option<Vec<Option<ItemStack>>> {
    INVENTORIES.lock().ok()?.get(&id).cloned()
}

/// The inventories the goals changed since the last call, for the api crate to save.
pub fn take_inventory_changes() -> Vec<(u32, Vec<Option<ItemStack>>)> {
    let ids: Vec<u32> = CHANGED.lock().map(|mut c| c.drain().collect()).unwrap_or_default();
    ids.into_iter().filter_map(|id| Some((id, inventory(id)?))).collect()
}

/// Changes doll `id`'s inventory, unless its owner has it open in a chest screen.
fn edit_inventory<R>(id: u32, f: impl FnOnce(&mut Vec<Option<ItemStack>>) -> R) -> Option<R> {
    if chest_open() == Some(id) {
        return None;
    }
    let mut all = INVENTORIES.lock().ok()?;
    let items = all.get_mut(&id)?;
    let r = f(items);
    drop(all);
    if let Ok(mut changed) = CHANGED.lock() {
        changed.insert(id);
    }
    Some(r)
}

/// Whether doll `id` holds an item `wanted` accepts, and which.
fn holds(id: u32, wanted: impl Fn(&str) -> bool) -> Option<Arc<str>> {
    inventory(id)?.into_iter().flatten().find(|s| s.count > 0 && wanted(&s.item)).map(|s| s.item.clone())
}

/// Takes one `item` out of doll `id`'s inventory: false when it had none.
fn take_one(id: u32, item: &str) -> bool {
    edit_inventory(id, |items| {
        for slot in items.iter_mut() {
            if let Some(s) = slot
                && &*s.item == item
                && s.count > 0
            {
                s.count -= 1;
                if s.count == 0 {
                    *slot = None;
                }
                return true;
            }
        }
        false
    })
    .unwrap_or(false)
}

/// The chest screen a doll's inventory opens in. A single player has no server menus, so the game
/// shows it as it shows a chest minecart's: the slots under a stand-in container key, copied back
/// when the screen closes. The hooks in the game's container code read these.
pub const CHEST_KEY_X: i32 = i32::MAX - 14;

static CHEST_REQUEST: Mutex<Option<(u32, String, Vec<Option<ItemStack>>)>> = Mutex::new(None);
static CHEST_CLOSED: Mutex<Vec<(u32, Vec<Option<ItemStack>>)>> = Mutex::new(Vec::new());
static CHEST_OPEN: Mutex<Option<u32>> = Mutex::new(None);

/// Asks the game to open doll `id`'s inventory, titled `title`, on its next tick.
pub fn request_chest(id: u32, title: String, items: Vec<Option<ItemStack>>) {
    if let Ok(mut request) = CHEST_REQUEST.lock() {
        *request = Some((id, title, items));
    }
}

/// The game's side: the inventory to open now, if one was asked for.
pub fn take_chest_request() -> Option<(u32, String, Vec<Option<ItemStack>>)> {
    let request = CHEST_REQUEST.lock().ok()?.take()?;
    if let Ok(mut open) = CHEST_OPEN.lock() {
        *open = Some(request.0);
    }
    Some(request)
}

/// The game's side: the doll inventory screen closed, holding `items`.
pub fn chest_closed(id: u32, items: Vec<Option<ItemStack>>) {
    if let Ok(mut open) = CHEST_OPEN.lock()
        && *open == Some(id)
    {
        *open = None;
    }
    if let Ok(mut closed) = CHEST_CLOSED.lock() {
        closed.push((id, items));
    }
}

/// The doll inventories closed since the last call, for the api crate to save.
pub fn take_chest_closed() -> Vec<(u32, Vec<Option<ItemStack>>)> {
    CHEST_CLOSED.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default()
}

pub fn chest_open() -> Option<u32> {
    *CHEST_OPEN.lock().ok()?
}

/// The dropped item each Russia doll is after, found by the api crate (which can see item
/// entities; a mob's goals cannot).
static ITEM_TARGETS: LazyLock<Mutex<HashMap<u32, [f64; 3]>>> = LazyLock::new(Default::default);

pub fn set_item_target(id: u32, at: Option<[f64; 3]>) {
    if let Ok(mut targets) = ITEM_TARGETS.lock() {
        match at {
            Some(at) => targets.insert(id, at),
            None => targets.remove(&id),
        };
    }
}

fn item_target(id: u32) -> Option<[f64; 3]> {
    ITEM_TARGETS.lock().ok()?.get(&id).copied()
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

/// A player has 20 health: every doll has half as much again.
const DOLL_HEALTH: f32 = 30.0;
/// The Goliath doll has three times a player's.
const GOLIATH_HEALTH: f32 = 60.0;

fn max_health(kind: Kind) -> f32 {
    match kind {
        Kind::Goliath => GOLIATH_HEALTH,
        _ => DOLL_HEALTH,
    }
}

static SNEAKING: AtomicBool = AtomicBool::new(false);

/// Whether the player was sneaking at the last doll's tick.
pub fn player_sneaking() -> bool {
    SNEAKING.load(Ordering::Relaxed)
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
    m.attrs.speed = m.kind.speed * SPEED_SCALE * speed_factor(kind);
    // A doll is sturdier than a chicken: a doll placed before this gets its new health in full.
    let health = max_health(kind);
    if m.attrs.max_health != health {
        m.attrs.max_health = health;
        if m.alive() {
            m.health = health;
        }
    }
    // Whether the player is sneaking, for the api crate's shift-click pickup.
    if let Some(p) = w.player.as_ref() {
        SNEAKING.store(p.sneaking, Ordering::Relaxed);
    }
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
        // PlantTree needs a sapling from the doll's inventory, which is not ported yet.
        Kind::Germany => {
            ai.goals.add(10, CutTree::default());
        }
        Kind::London => {
            ai.goals.add(11, ReportSpawners::default());
        }
        Kind::Russia => {
            ai.goals.add(5, CollectItem::default());
            ai.goals.add(6, Torcher::default());
        }
        Kind::Marisa => {
            ai.goals.add(1, Danmaku::new(Shot::Star));
        }
        Kind::Reimu => {
            ai.goals.add(1, Danmaku::new(Shot::Amulet));
        }
        Kind::Aya => {
            ai.goals.add(1, AyaShot::default());
        }
        Kind::Goliath => {
            ai.goals.add(1, GoliathAttack::default());
        }
        Kind::Yuuka => {
            ai.goals.add(1, Farmer::default());
        }
        // The two secret dolls only keep their owner company.
        Kind::GrandGuignol | Kind::Wakasagi => {}
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
    // Only JUMP: the hook before each tick holds the doll on the head, and its jobs (Horai's mining,
    // Shanghai's fighting) still need MOVE and LOOK while it rides.
    fn flags(&self) -> u8 {
        goals::JUMP
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

/// Close enough to steal: two and a half blocks across, and up to three above or below.
fn in_reach(m: &Mob, at: [f64; 3]) -> bool {
    within(m, at, REACH_SQR, 3.0)
}

/// Close enough to strike: five blocks across, and up to four above or below, so a big mob the
/// path stops short of, or a low-flying fairy or phantom, can still be hit.
fn in_strike_reach(m: &Mob, at: [f64; 3]) -> bool {
    within(m, at, ATTACK_REACH_SQR, 4.0)
}

fn within(m: &Mob, at: [f64; 3], across_sqr: f64, up: f64) -> bool {
    let (dx, dy, dz) = (m.position.x - at[0], m.position.y - at[1], m.position.z - at[2]);
    dx * dx + dz * dz < across_sqr && dy.abs() < up
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
        } else if in_mode(m, FOLLOW) || in_mode(m, RIDE) {
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
        } else if in_mode(m, FOLLOW) || in_mode(m, RIDE) {
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
        // A riding doll strikes from the head, at whatever comes within reach.
        if !in_mode(m, RIDE) && cx.w.game_time - self.last_path >= PATH_EVERY {
            self.last_path = cx.w.game_time;
            m.nav_move_to_entity(cx.w, at, 0, 1.0);
        }
        if alive && in_strike_reach(m, at) && cx.w.game_time - self.last_hit >= HIT_EVERY {
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

/// `EntityDollAIMineBlock`: on patrol, following, or riding its owner's head, digs towards the nearest ore it can reach,
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
        if !(in_mode(m, PATROL) || in_mode(m, FOLLOW) || in_mode(m, RIDE))
            || cx.w.game_time - self.last_scan < 20
        {
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
        (in_mode(m, PATROL) || in_mode(m, FOLLOW) || in_mode(m, RIDE))
            && self.counter >= 0
            && !is_air(&name_at(cx.w, self.mine))
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        let hardness = hardness_at(cx.w, self.mine);
        self.counter = ((20.0 * hardness / MINE_SPEED).floor() as i32).max(0);
        self.total = self.counter.max(1);
        // On its owner's head it digs from where it sits, within reach of the head.
        if !in_mode(m, RIDE) {
            let at = self.mine.map(|c| f64::from(c) + 0.5);
            m.nav_move_to(cx.w, at, 1.0);
        }
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

// ---- Germany: EntityDollAICutTree ----

/// `cutRange` and `cutSpeed`: how far Germany looks for a tree, and how fast it chops (twice the
/// original's 2.5).
const CUT_RANGE: i32 = 5;
const CUT_SPEED: f32 = 5.0;

/// `logBlockRegex` (`log|mushroom`): logs, nether stems, and giant mushrooms.
fn is_log(name: &str) -> bool {
    name.ends_with("_log") || name.ends_with("_stem") || name.contains("mushroom")
}

/// `leavesBlockRegex` (`leaves|mushroom|vine`).
fn is_leafy(name: &str) -> bool {
    name.contains("leaves") || name.contains("mushroom") || name.contains("vine") || name.ends_with("_wart_block")
}

/// On patrol or following, finds a tree (a trunk with leaves around it), hops to its top, and
/// chops it down layer by layer.
#[derive(Clone, Debug, Default)]
struct CutTree {
    /// The trunk's top block still standing, and what it is made of.
    target: [i32; 3],
    log: Option<Arc<str>>,
    cut: [i32; 3],
    counter: i32,
    strength: i32,
    last_scan: i64,
}

impl CutTree {
    /// The nearest trunk with at least four leaves beside it: its top block, the leaves beside it
    /// and how tall it is.
    fn find(&self, m: &Mob, w: &MobWorld) -> Option<([i32; 3], Arc<str>, u32, u32)> {
        let [x, y, z] = eye_block(m);
        let mut best: Option<([i32; 3], Arc<str>, u32, u32)> = None;
        let mut best_steps = i32::MAX;
        for dx in -CUT_RANGE..=CUT_RANGE {
            for dz in -CUT_RANGE..=CUT_RANGE {
                let steps = dx.abs() + dz.abs();
                if steps > CUT_RANGE || steps >= best_steps {
                    continue;
                }
                let log = name_at(w, [x + dx, y, z + dz]);
                if !is_log(&log) {
                    continue;
                }
                let mut ty = y;
                while ty > WORLD_BOTTOM && name_at(w, [x + dx, ty - 1, z + dz]) == log {
                    ty -= 1;
                }
                let (mut height, mut leaves) = (0, 0);
                while name_at(w, [x + dx, ty, z + dz]) == log {
                    for (ox, oz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        if is_leafy(&name_at(w, [x + dx + ox, ty, z + dz + oz])) {
                            leaves += 1;
                        }
                    }
                    height += 1;
                    ty += 1;
                }
                if leaves >= 4 {
                    best = Some(([x + dx, ty - 1, z + dz], log, leaves, height));
                    best_steps = steps;
                }
            }
        }
        best
    }
}

impl Goal for CutTree {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if !(in_mode(m, PATROL) || in_mode(m, FOLLOW) || in_mode(m, RIDE))
            || cx.w.game_time - self.last_scan < 20
        {
            return false;
        }
        self.last_scan = cx.w.game_time;
        let Some((top, log, leaves, height)) = self.find(m, cx.w) else {
            say(m.id, 2, "No target");
            return false;
        };
        say(m.id, 1, format!("Target[{leaves},{height}]"));
        self.target = top;
        self.log = Some(log);
        // On its owner's head it chops from where it sits; otherwise `teleportToXYZ` puts it
        // on top of the trunk.
        if !in_mode(m, RIDE) {
            m.position.x = f64::from(top[0]) + 0.5;
            m.position.y = f64::from(top[1] + 1);
            m.position.z = f64::from(top[2]) + 0.5;
            m.previous = m.position;
            m.nav_stop();
            cx.events.push(MobEvent::Sound("entity.enderman.teleport", here(m), 1.0, 1.0));
        }
        true
    }
    fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        let near = (m.position.x.floor() as i32 - self.target[0]).abs()
            + (m.position.y.floor() as i32 - self.target[1]).abs()
            + (m.position.z.floor() as i32 - self.target[2]).abs()
            <= CUT_RANGE;
        (in_mode(m, PATROL) || in_mode(m, FOLLOW) || in_mode(m, RIDE)) && self.log.is_some() && near
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.counter = 0;
    }
    fn stop(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.log = None;
        cx.events.push(MobEvent::DestroyProgress { mob: m.id, pos: self.cut, progress: -1 });
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(log) = self.log.clone() else { return };
        let w = cx.w;
        if self.counter == 0 {
            // Another log of this layer within reach first (branches, a wide trunk), else the trunk,
            // a layer lower each time.
            let [tx, ty, tz] = self.target;
            let mut branch = None;
            'search: for dx in -CUT_RANGE..=CUT_RANGE {
                for dz in -CUT_RANGE..=CUT_RANGE {
                    if dx.abs() + dz.abs() <= CUT_RANGE && name_at(w, [tx + dx, ty, tz + dz]) == log {
                        branch = Some([tx + dx, ty, tz + dz]);
                        break 'search;
                    }
                }
            }
            match branch {
                Some(at) => self.cut = at,
                None => {
                    self.cut = self.target;
                    self.target[1] -= 1;
                    if name_at(w, self.target) != log {
                        self.log = None;
                        return;
                    }
                }
            }
            let hardness = hardness_at(w, self.cut);
            self.strength = ((1.0 + 30.0 * hardness / CUT_SPEED).floor() as i32).max(0);
        }
        let at = self.cut.map(|c| f64::from(c) + 0.5);
        m.look_at(at);
        if self.counter >= self.strength {
            cx.events.push(MobEvent::BreakBlock { pos: self.cut, drop: true });
            self.counter = 0;
            return;
        }
        let progress = (self.counter * 10 / self.strength.max(1)).min(9);
        cx.events.push(MobEvent::DestroyProgress { mob: m.id, pos: self.cut, progress });
        if self.counter % 4 == 0 {
            let [x, y, z] = self.cut;
            let facts = w.facts.get(w.world.block(x, y, z));
            let sound = facts.sounds.break_sound.to_string();
            let volume = (facts.sounds.volume + 1.0) / 8.0;
            let pitch = facts.sounds.pitch * 0.5;
            cx.events.push(MobEvent::SoundId(sound, at, volume, pitch));
        }
        if self.counter % 6 == 0 {
            m.swing();
            mark_swing(m.id);
        }
        self.counter += 1;
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "CutTree"
    }
}

// ---- London: EntityDollAIReportArtifact ----

/// In any mode, sweeps the layers above and below it, 64 each way, over the chunks around it, one
/// layer a tick, and reports where the spawners are: `-` every 8 layers, a `[spawner]` at each
/// layer that has one, and `<->` at its own height (the original showed the stronghold's direction
/// there; Horizon gives a mob no way to find it).
#[derive(Clone, Debug, Default)]
struct ReportSpawners {
    counter: i32,
    chunk: [i32; 2],
    base_y: i32,
    found: BTreeMap<i32, Vec<String>>,
}

impl Goal for ReportSpawners {
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        state(m.id).is_some_and(|s| s.enabled)
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.counter = 0;
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if self.counter == 0 {
            self.chunk = [(m.position.x.floor() as i32).div_euclid(16), (m.position.z.floor() as i32).div_euclid(16)];
            self.base_y = (m.position.y + m.kind.eye_height).floor() as i32;
            self.found.clear();
        }
        let [cx0, cz0] = self.chunk;
        for dy in [-1, 1] {
            if self.counter == 0 && dy == 1 {
                continue;
            }
            let layer = self.counter * dy;
            let y = self.base_y + layer;
            for dx in -16..=32 {
                for dz in -16..=32 {
                    let name = name_at(cx.w, [cx0 * 16 + dx, y, cz0 * 16 + dz]);
                    if matches!(&*name, "minecraft:spawner" | "minecraft:trial_spawner") {
                        self.found.entry(layer).or_default().push(short(&name).to_owned());
                    }
                }
            }
        }
        if self.counter == 63 {
            let mut text = String::new();
            let mark = |text: &mut String, layer: i32, found: &BTreeMap<i32, Vec<String>>| {
                if let Some(names) = found.get(&layer) {
                    text.push_str(&format!("[{}]", names.join(",")));
                }
            };
            for i in (1..=64).rev() {
                if i % 8 == 0 {
                    text.push('-');
                }
                mark(&mut text, i, &self.found);
            }
            match self.found.get(&0) {
                Some(names) => text.push_str(&format!("<[{}]>", names.join(","))),
                None => text.push_str("<->"),
            }
            for i in (-64..=-1).rev() {
                mark(&mut text, i, &self.found);
                if i % 8 == 0 {
                    text.push('-');
                }
            }
            say(m.id, 2, text);
        }
        self.counter = (self.counter + 1) % 64;
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "ReportSpawners"
    }
}

// ---- Russia: EntityDollAICollectItem and EntityDollAITorcher ----

/// On patrol or following, walks to the dropped item the api crate found for it; the api crate
/// takes the item into its inventory once it is close.
#[derive(Clone, Debug, Default)]
struct CollectItem {
    last_path: i64,
}

impl Goal for CollectItem {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        (in_mode(m, PATROL) || in_mode(m, FOLLOW)) && item_target(m.id).is_some()
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.last_path = 0;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(at) = item_target(m.id) else { return };
        m.look_at(at);
        if cx.w.game_time - self.last_path >= 10 {
            self.last_path = cx.w.game_time;
            m.nav_move_to(cx.w, at, 1.0);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "CollectItem"
    }
}

/// `lightThreshold`: a spot this dark or darker gets a torch.
const LIGHT_THRESHOLD: i32 = 5;

/// With torches in its first slot, puts one down wherever it walks that is dark enough for
/// monsters to spawn, in any mode but Standby.
#[derive(Clone, Debug, Default)]
struct Torcher {
    last: i64,
}

impl Goal for Torcher {
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        state(m.id).is_some_and(|s| s.enabled && s.mode != STANDBY)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if cx.w.game_time - self.last < 20 {
            return;
        }
        self.last = cx.w.game_time;
        let first_is_torch = inventory(m.id)
            .and_then(|items| items.first().cloned().flatten())
            .is_some_and(|s| &*s.item == "minecraft:torch" && s.count > 0);
        if !first_is_torch {
            return;
        }
        let w = cx.w;
        let [x, eye, z] = eye_block(m);
        // The first air above the ground under its eyes.
        let mut y = eye;
        while !is_air(&name_at(w, [x, y, z])) && y < eye + 4 {
            y += 1;
        }
        while is_air(&name_at(w, [x, y, z])) && y > eye - 5 {
            y -= 1;
        }
        y += 1;
        if y < eye - 3 || y > eye + 3 || !is_air(&name_at(w, [x, y, z])) {
            return;
        }
        let light = i32::from(w.world.light(x, y, z));
        let (sky, block) = (light >> 4, light & 15);
        if sky > m.random.next_int_bound(32).unwrap_or(0) {
            return;
        }
        let darken = if w.thundering { 10 } else if w.daylight { 0 } else { 11 };
        if block.max(sky - darken) > LIGHT_THRESHOLD {
            return;
        }
        if !w.facts.get(w.world.block(x, y - 1, z)).full_collision {
            return;
        }
        let took = edit_inventory(m.id, |items| match items.first_mut() {
            Some(Some(s)) if &*s.item == "minecraft:torch" && s.count > 0 => {
                s.count -= 1;
                if s.count == 0 {
                    items[0] = None;
                }
                true
            }
            _ => false,
        });
        if took == Some(true) {
            cx.events.push(MobEvent::SetBlockName {
                mob: Some(m.id),
                pos: [x, y, z],
                name: "minecraft:torch",
                effect: false,
                flags: 3,
            });
            m.swing();
            mark_swing(m.id);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "Torcher"
    }
}

// ---- Marisa and Reimu: EntityDollAIAttackWithBullet and EntityDollAIAttackWithAmulet ----

/// Marisa's stars, or Reimu's amulets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shot {
    Star,
    Amulet,
}

/// One shot in flight: it homes in on its target and hits nothing else, so a doll's danmaku never
/// hurts its owner, another doll, or anything that just gets in the way.
#[derive(Clone, Debug)]
struct Bullet {
    at: [f64; 3],
    target: u32,
    /// Ticks in flight; at zero or below it has not left yet.
    age: i32,
}

/// Ticks a shot flies before it fades.
const BULLET_LIFE: i32 = 80;

impl Shot {
    /// Blocks a tick, damage, colour and the sparkle it trails.
    fn look(self) -> (f64, f32, [f32; 3], &'static str) {
        match self {
            Shot::Star => (0.6, 3.0, [1.0, 0.85, 0.2], "end_rod"),
            Shot::Amulet => (0.7, 8.0, [0.9, 0.1, 0.15], "crit"),
        }
    }
}

/// Marisa (on patrol or following) keeps within five blocks of a hostile mob and fires five stars
/// at it every second. Reimu (in any mode but Rideon) stands her ground and fires a ring of eight
/// amulets every four seconds, walking up only when she cannot see it.
#[derive(Clone, Debug)]
struct Danmaku {
    shot: Shot,
    target: Option<u32>,
    bullets: Vec<Bullet>,
    last_scan: i64,
    counter: i32,
    lost: i32,
}

impl Danmaku {
    fn new(shot: Shot) -> Self {
        Self { shot, target: None, bullets: Vec::new(), last_scan: 0, counter: 0, lost: 0 }
    }
    fn allowed(&self, m: &Mob) -> bool {
        match self.shot {
            Shot::Star => in_mode(m, PATROL) || in_mode(m, FOLLOW),
            Shot::Amulet => state(m.id).is_some_and(|s| s.enabled && s.mode != RIDE),
        }
    }
    fn wanted(m: &Mob, cx: &Cx, s: &Seen) -> bool {
        if !enemy(s) || spares(m.id, Who::Mob(s.id)) {
            return false;
        }
        // Following, it guards its owner and does not run off after anything far from them.
        !in_mode(m, FOLLOW) || matches!(s.target, Some(Who::Player)) || near_owner(cx, s)
    }
    fn fire(&mut self, m: &mut Mob, cx: &mut Cx, target: u32, at: [f64; 3]) {
        let muzzle = [m.position.x, m.position.y + m.kind.eye_height, m.position.z];
        let (dx, dz) = (at[0] - muzzle[0], at[2] - muzzle[2]);
        let across = (dx * dx + dz * dz).sqrt().max(0.01);
        let side = [-dz / across, 0.0, dx / across];
        match self.shot {
            Shot::Star => {
                for k in -2i32..=2 {
                    let off = f64::from(k) * 0.3;
                    self.bullets.push(Bullet {
                        at: [muzzle[0] + side[0] * off, muzzle[1], muzzle[2] + side[2] * off],
                        target,
                        age: -k.abs(),
                    });
                }
                cx.events.push(MobEvent::Sound("entity.blaze.shoot", muzzle, 0.4, 1.6));
            }
            Shot::Amulet => {
                for i in 0..8 {
                    let a = f64::from(i) * std::f64::consts::FRAC_PI_4;
                    let (c, s) = (a.cos() * 0.8, a.sin() * 0.8);
                    self.bullets.push(Bullet {
                        at: [muzzle[0] + side[0] * c, muzzle[1] + s, muzzle[2] + side[2] * c],
                        target,
                        age: -(i % 4),
                    });
                }
                cx.events.push(MobEvent::Sound("entity.arrow.shoot", muzzle, 1.0, 0.8));
            }
        }
        m.swing();
        mark_swing(m.id);
    }
    fn fly(&mut self, m: &Mob, cx: &mut Cx) {
        let (speed, damage, color, sparkle) = self.shot.look();
        let from = here(m);
        let w = cx.w;
        let events = &mut cx.events;
        self.bullets.retain_mut(|b| {
            b.age += 1;
            if b.age <= 0 {
                return true;
            }
            if b.age > BULLET_LIFE {
                return false;
            }
            let Some(s) = w.seen(b.target).filter(|s| s.alive) else { return false };
            let aim = [s.position.x, (s.position.y + s.eye_y) / 2.0, s.position.z];
            let d = [aim[0] - b.at[0], aim[1] - b.at[1], aim[2] - b.at[2]];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if len <= speed + 0.4 {
                // DamageSource.causePlayerDamage(owner): the kill is the owner's.
                events.push(MobEvent::AttackMob { target: b.target, amount: damage, source: Damage::player(from) });
                events.push(MobEvent::ParticleAt { kind: sparkle, at: aim, velocity: [0.0; 3] });
                return false;
            }
            for c in 0..3 {
                b.at[c] += d[c] / len * speed;
            }
            events.push(MobEvent::ParticleColored { at: b.at, color });
            events.push(MobEvent::ParticleAt { kind: sparkle, at: b.at, velocity: [0.0; 3] });
            true
        });
    }
}

impl Goal for Danmaku {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if !self.allowed(m) || cx.w.game_time - self.last_scan < 20 {
            return false;
        }
        self.last_scan = cx.w.game_time;
        let from = here(m);
        self.target = cx
            .w
            .others
            .iter()
            .filter(|s| s.id != m.id && noticed(from, s) && Self::wanted(m, cx, s))
            .min_by(|a, b| distance_sqr(m, pos(a)).total_cmp(&distance_sqr(m, pos(b))))
            .map(|s| s.id);
        self.target.is_some()
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let hunting = self.allowed(m)
            && self.lost < LOST_AFTER
            && self.target.and_then(|id| cx.w.seen(id)).is_some_and(|s| s.alive);
        hunting || !self.bullets.is_empty()
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.counter = 0;
        self.lost = 0;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        self.target = None;
        self.bullets.clear();
        m.nav_stop();
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.fly(m, cx);
        let Some(id) = self.target else { return };
        let Some((at, eye, alive)) = cx.w.seen(id).map(|s| (pos(s), s.eye_y, s.alive)) else {
            self.target = None;
            return;
        };
        if !alive || !self.allowed(m) {
            self.target = None;
            m.nav_stop();
            return;
        }
        let sees = m.sees(cx.w, Who::Mob(id));
        if sees {
            m.look_at([at[0], eye, at[2]]);
            self.lost = 0;
        } else {
            self.lost += 1;
        }
        let aim = [at[0], (at[1] + eye) / 2.0, at[2]];
        self.counter -= 1;
        match self.shot {
            Shot::Star => {
                if self.counter <= 0 {
                    self.counter = 20;
                    if distance_sqr(m, at) > 25.0 || !sees {
                        m.nav_move_to_entity(cx.w, at, 0, 1.0);
                    } else {
                        m.nav_stop();
                    }
                    if sees {
                        self.fire(m, cx, id, aim);
                    }
                }
            }
            Shot::Amulet => {
                if self.counter % 20 == 0 {
                    if !sees && (in_mode(m, PATROL) || in_mode(m, FOLLOW)) {
                        m.nav_move_to_entity(cx.w, at, 0, 1.0);
                    } else {
                        m.nav_stop();
                    }
                }
                if self.counter <= 0 {
                    if sees {
                        self.counter = 80;
                        self.fire(m, cx, id, aim);
                    } else {
                        self.counter = 20;
                    }
                }
            }
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "Danmaku"
    }
}

// ---- Aya: EntityDollAIAyaShot ----

/// `shotInterval`: Aya's photos are thirty seconds apart.
const AYA_INTERVAL: i32 = 30 * 20;

/// Every thirty seconds, and straight away when she is hurt (`emergencyShot`), Aya takes a photo:
/// the shutter clicks, and she tells her owner what she caught. (The original saved a screenshot
/// on the owner's computer.)
#[derive(Clone, Debug, Default)]
struct AyaShot {
    counter: i32,
    health: f32,
}

impl AyaShot {
    fn photo(&self, m: &mut Mob, cx: &mut Cx, hurt: bool) {
        let me = here(m);
        let head = [me[0], me[1] + m.kind.eye_height, me[2]];
        cx.events.push(MobEvent::Sound("block.wool.break", head, 0.2, 2.0));
        cx.events.push(MobEvent::Sound("ui.button.click", head, 0.2, 2.0));
        for (dx, dz) in [(0.3, 0.0), (-0.3, 0.0), (0.0, 0.3), (0.0, -0.3)] {
            let at = [head[0] + dx, head[1] + 0.2, head[2] + dz];
            cx.events.push(MobEvent::ParticleAt { kind: "end_rod", at, velocity: [dx * 0.1, 0.05, dz * 0.1] });
        }
        m.swing();
        mark_swing(m.id);
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for s in cx.w.others.iter().filter(|s| s.id != m.id && noticed(me, s) && !is_doll(s.id)) {
            *counts.entry(format!("{:?}", s.species)).or_default() += 1;
        }
        let list: String = counts.iter().map(|(name, n)| format!("{name}[{n}]")).collect();
        match (hurt, counts.is_empty()) {
            (true, true) => say(m.id, 1, "Ow! Whoever did that got away"),
            (true, false) => say(m.id, 1, format!("Caught the culprit on film! {list}")),
            (false, true) => say(m.id, 2, "Click! Nothing worth printing"),
            (false, false) => say(m.id, 1, format!("Scoop! {list}")),
        }
    }
}

impl Goal for AyaShot {
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        m.alive() && state(m.id).is_some_and(|s| s.enabled)
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        self.health = m.health;
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let hurt = m.health < self.health;
        self.health = m.health;
        self.counter += 1;
        if hurt || self.counter >= AYA_INTERVAL {
            self.counter = 0;
            self.photo(m, cx, hurt);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "AyaShot"
    }
}

// ---- Goliath: EntityDollAIAttackWithGoliath ----

/// `attackStrengthGoliath`, and four blocks of reach.
const GOLIATH_STRENGTH: f32 = 7.0;
const GOLIATH_REACH_SQR: f64 = 16.0;

/// On patrol, every hostile mob around it; following, whatever is after its owner. Once a second it
/// closes in and swings, throwing what it hits back a block.
#[derive(Clone, Debug, Default)]
struct GoliathAttack {
    target: Option<u32>,
    last_scan: i64,
    counter: i32,
    lost: i32,
}

impl Goal for GoliathAttack {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if cx.w.game_time - self.last_scan < 20 {
            return false;
        }
        self.last_scan = cx.w.game_time;
        self.target = if in_mode(m, PATROL) {
            nearest(m, cx, enemy)
        } else if in_mode(m, FOLLOW) {
            nearest(m, cx, |s| matches!(s.target, Some(Who::Player)) && !is_doll(s.id))
        } else {
            None
        };
        self.target.is_some()
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        (in_mode(m, PATROL) || in_mode(m, FOLLOW))
            && self.lost < LOST_AFTER
            && self.target.and_then(|id| cx.w.seen(id)).is_some_and(|s| s.alive)
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.counter = 0;
        self.lost = 0;
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
        let sees = m.sees(cx.w, Who::Mob(id));
        if sees {
            m.look_at([at[0], eye, at[2]]);
            self.lost = (self.lost - 1).max(0);
        } else {
            self.lost += 1;
        }
        self.counter -= 1;
        if self.counter > 0 {
            return;
        }
        self.counter = 20;
        m.nav_move_to_entity(cx.w, at, 0, 1.0);
        if alive && sees && distance_sqr(m, at) < GOLIATH_REACH_SQR {
            m.swing();
            mark_swing(m.id);
            cx.events.push(MobEvent::AttackMob {
                target: id,
                amount: GOLIATH_STRENGTH,
                source: Damage::player(here(m)),
            });
            let (dx, dz) = (at[0] - m.position.x, at[2] - m.position.z);
            let dr = (dx * dx + dz * dz + 0.01).sqrt();
            cx.events.push(MobEvent::Push { target: Who::Mob(id), velocity: [dx / dr, 0.1, dz / dr] });
            cx.events.push(MobEvent::Sound("entity.blaze.hurt", at, 0.8, 1.0));
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "GoliathAttack"
    }
}

// ---- Yuuka: EntityDollAIFarmer ----

/// `farmRange`: how far, in steps along the axes, Yuuka farms.
const FARM_RANGE: i32 = 5;

/// The crops Yuuka plants: the crop, the seed it grows from, and the age it is ripe at.
const CROPS: [(&str, &str, &str); 4] = [
    ("minecraft:wheat", "minecraft:wheat_seeds", "7"),
    ("minecraft:carrots", "minecraft:carrot", "7"),
    ("minecraft:potatoes", "minecraft:potato", "7"),
    ("minecraft:beetroots", "minecraft:beetroot_seeds", "3"),
];
/// Seeds that grow a stem: planted, but the stem itself is never harvested.
const STEMS: [(&str, &str); 2] = [
    ("minecraft:melon_stem", "minecraft:melon_seeds"),
    ("minecraft:pumpkin_stem", "minecraft:pumpkin_seeds"),
];

fn plant_for(seed: &str) -> Option<&'static str> {
    CROPS
        .iter()
        .find(|c| c.1 == seed)
        .map(|c| c.0)
        .or_else(|| STEMS.iter().find(|s| s.1 == seed).map(|s| s.0))
}

fn is_stem(name: &str) -> bool {
    name.ends_with("melon_stem") || name.ends_with("pumpkin_stem")
}

fn stem_beside(w: &MobWorld, p: [i32; 3]) -> bool {
    [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]].iter().any(|&d| is_stem(&name_at(w, add(p, d))))
}

/// A crop at its last age, or a melon or pumpkin with a stem beside it.
fn harvestable(w: &MobWorld, p: [i32; 3]) -> Option<&'static str> {
    let name = name_at(w, p);
    if let Some(crop) = CROPS.iter().find(|c| *c.0 == *name) {
        let [x, y, z] = p;
        let state = w.world.block(x, y, z);
        let ripe = w
            .registry
            .state(state)
            .and_then(|s| s.properties.get("age"))
            .is_some_and(|age| age.as_str() == crop.2);
        return ripe.then_some(crop.0);
    }
    match &*name {
        "minecraft:melon" if stem_beside(w, p) => Some("minecraft:melon"),
        "minecraft:pumpkin" if stem_beside(w, p) => Some("minecraft:pumpkin"),
        _ => None,
    }
}

/// On patrol: tills the dirt it stands on between farmland, plants seeds from its inventory on the
/// farmland it stands on, and harvests what is ripe within reach, planting again from its seeds.
#[derive(Clone, Debug, Default)]
struct Farmer {
    target: [i32; 3],
    block: Option<&'static str>,
    counter: i32,
    last_scan: i64,
}

impl Farmer {
    fn till_and_plant(&self, m: &mut Mob, cx: &mut Cx) {
        let w = cx.w;
        let [x, y, z] = eye_block(m);
        for j in 0..FARM_RANGE {
            let p = [x, y - j, z];
            let name = name_at(w, p);
            if !matches!(&*name, "minecraft:dirt" | "minecraft:grass_block") {
                continue;
            }
            let farmland = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]]
                .iter()
                .filter(|&&d| &*name_at(w, add(p, d)) == "minecraft:farmland")
                .count();
            if farmland >= 2 && is_air(&name_at(w, add(p, [0, 1, 0]))) {
                let at = p.map(|c| f64::from(c) + 0.5);
                cx.events.push(MobEvent::Sound("item.hoe.till", at, 1.0, 1.0));
                cx.events.push(MobEvent::SetBlockName {
                    mob: Some(m.id),
                    pos: p,
                    name: "minecraft:farmland",
                    effect: false,
                    flags: 3,
                });
                m.swing();
                mark_swing(m.id);
                break;
            }
        }
        for j in 0..FARM_RANGE {
            let p = [x, y - j, z];
            let above = add(p, [0, 1, 0]);
            if &*name_at(w, p) != "minecraft:farmland" || !is_air(&name_at(w, above)) {
                continue;
            }
            if stem_beside(w, above) {
                break;
            }
            if let Some(seed) = holds(m.id, |item| plant_for(item).is_some())
                && let Some(plant) = plant_for(&seed)
                && take_one(m.id, &seed)
            {
                cx.events.push(MobEvent::SetBlockName {
                    mob: Some(m.id),
                    pos: above,
                    name: plant,
                    effect: false,
                    flags: 3,
                });
                m.swing();
                mark_swing(m.id);
            }
            break;
        }
    }
}

impl Goal for Farmer {
    fn flags(&self) -> u8 {
        goals::MOVE | goals::LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if !in_mode(m, PATROL) || cx.w.game_time - self.last_scan < 20 {
            return false;
        }
        self.last_scan = cx.w.game_time;
        self.till_and_plant(m, cx);
        let eye = eye_block(m);
        let r = FARM_RANGE;
        let mut best: Option<([i32; 3], &'static str, i32)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                for dz in -r..=r {
                    let d = [dx, dy, dz];
                    let n = steps(d, [0; 3]);
                    if n > r || best.is_some_and(|b| b.2 <= n) {
                        continue;
                    }
                    if let Some(block) = harvestable(cx.w, add(eye, d)) {
                        best = Some((add(eye, d), block, n));
                    }
                }
            }
        }
        let Some((at, block, _)) = best else { return false };
        self.target = at;
        self.block = Some(block);
        true
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let at = [m.position.x.floor() as i32, m.position.y.floor() as i32, m.position.z.floor() as i32];
        in_mode(m, PATROL)
            && steps(at, self.target) <= FARM_RANGE + 1
            && self.block.is_some_and(|b| *name_at(cx.w, self.target) == *b)
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.counter = 0;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        self.block = None;
        m.nav_stop();
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(block) = self.block else { return };
        let at = self.target.map(|c| f64::from(c) + 0.5);
        m.look_at(at);
        if self.counter == 0 {
            m.nav_move_to(cx.w, at, 1.0);
        }
        if self.counter == 20 {
            cx.events.push(MobEvent::BreakBlock { pos: self.target, drop: true });
            m.swing();
            mark_swing(m.id);
            // A harvested crop goes straight back in from its seeds.
            if let Some(crop) = CROPS.iter().find(|c| c.0 == block)
                && take_one(m.id, crop.1)
            {
                cx.events.push(MobEvent::SetBlockName {
                    mob: Some(m.id),
                    pos: self.target,
                    name: crop.0,
                    effect: false,
                    flags: 3,
                });
            }
            self.block = None;
        }
        self.counter += 1;
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "Farmer"
    }
}
