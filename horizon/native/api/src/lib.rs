//! Alice's Dolls for the eltanschauung/horizon fork: a port of the 1.7.10 mod by iwa_yukinoshita.
//!
//! Alice's dolls: Shanghai fights and steals, Horai finds and mines ore, Ooedo blows itself up
//! next to monsters, Germany chops trees, London finds spawners, Russia gathers what is dropped and
//! lights the dark with torches. The extras: Marisa and Reimu shoot danmaku, Aya takes photos,
//! Goliath fights, Yuuka farms; Grand Guignol and Wakasagi keep you company. Right-click one empty-handed to go
//! Standby, Patrol, Follow, Rideon and back to Standby; with a Doll Core to switch its AI on and
//! off, or to take over someone else's doll; with a book to change how much it says; with a chest
//! to open its inventory. Sneak and right-click it empty-handed, hit it with a Doll Core, or kill it,
//! to get the item back.
//!
//! The fork has no living mod entity types, so a doll is a silent chicken with a custom name:
//! this mod draws it, and `goals.rs` drives it. A chicken is close to the doll's size and falls
//! slowly, as the doll did. This crate saves each doll's state and hands it to the goals each tick.

mod model;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use horizon_client::mobs::alice_doll_goals as goals;
use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, AttackEntityEvent, Compound,
    EntityTickListenedEvent, EntityTickPostEvent, Flow, Holder, Hooks, Host, InteractEntityEvent,
    ItemStack, LivingIncomingDamageEvent, LootDropsEvent, MenuClosedEvent, MenuInputClosedEvent,
    MobSubmitEvent, NativeInventoryKind, NativeServerPlayer, StackData, Tag, Text, TooltipEvent,
    UseItemOnEvent,
};

// The kind every doll is underneath.
const KIND: &str = "minecraft:chicken";
const DOLL_CORE: &str = "alice_dolls:doll_core";
/// `DollBase.getSizeInventory`: one row.
const INVENTORY: usize = 9;

/// One of the dolls: the name that makes a chicken this doll, its item, its texture, what it holds
/// (`getHeldItem`, empty for nothing), how many slots its inventory has (`getSizeInventory`), and
/// how it is drawn.
#[derive(Debug)]
pub struct Doll {
    name: &'static str,
    item: &'static str,
    sprite: &'static str,
    held: &'static str,
    slots: usize,
    look: model::Look,
}

impl PartialEq for Doll {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

const fn base(name: &'static str, item: &'static str, sprite: &'static str, held: &'static str) -> Doll {
    Doll { name, item, sprite, held, slots: INVENTORY, look: model::Look::Base }
}

static DOLLS: [Doll; 14] = [
    base(goals::SHANGHAI, "alice_dolls:shanghai_doll", "alice_dolls:entity/shanghai", "minecraft:golden_sword"),
    base(goals::HORAI, "alice_dolls:horai_doll", "alice_dolls:entity/horai", "minecraft:golden_pickaxe"),
    base(goals::DIAMOND_HORAI, "alice_dolls:diamond_horai_doll", "alice_dolls:entity/horai", "minecraft:diamond_pickaxe"),
    base(goals::OOEDO, "alice_dolls:ooedo_doll", "alice_dolls:entity/ooedo", "minecraft:tnt"),
    base(goals::GERMANY, "alice_dolls:germany_doll", "alice_dolls:entity/germany", "minecraft:golden_axe"),
    base(goals::LONDON, "alice_dolls:london_doll", "alice_dolls:entity/london", "minecraft:ender_eye"),
    // Russia carries four rows.
    Doll { slots: 36, ..base(goals::RUSSIA, "alice_dolls:russia_doll", "alice_dolls:entity/russia", "minecraft:chest") },
    base(goals::GRAND_GUIGNOL, "alice_dolls:grand_guignol_doll", "alice_dolls:entity/grandguignol", ""),
    // The extras carry two rows. Their items were thKaguya's: Lunatic has Aya's fan, Yuuka's
    // parasol and a yin-yang orb; Marisa's mini-hakkero is a fire charge here.
    Doll {
        name: goals::MARISA,
        item: "alice_dolls:marisa_doll",
        sprite: "alice_dolls:entity/marisa",
        held: "minecraft:fire_charge",
        slots: 18,
        look: model::Look::Boxes(&model::MARISA),
    },
    Doll {
        name: goals::REIMU,
        item: "alice_dolls:reimu_doll",
        sprite: "alice_dolls:entity/reimu",
        held: "lunatic:yin_yang_orb_homing",
        slots: 18,
        look: model::Look::Boxes(&model::REIMU),
    },
    Doll {
        name: goals::AYA,
        item: "alice_dolls:aya_doll",
        sprite: "alice_dolls:entity/aya",
        held: "lunatic:hauchiwa_fan",
        slots: 18,
        look: model::Look::Boxes(&model::AYA),
    },
    Doll {
        name: goals::WAKASAGI,
        item: "alice_dolls:wakasagi_doll",
        sprite: "alice_dolls:entity/wakasagi",
        held: "",
        slots: 18,
        look: model::Look::Boxes(&model::WAKASAGI),
    },
    Doll {
        name: goals::GOLIATH,
        item: "alice_dolls:goliath_doll",
        sprite: "alice_dolls:entity/goliath",
        held: "minecraft:diamond_sword",
        // The first four are what it wears (`goals::wear_armour`), the rest a row and a half.
        slots: 18,
        look: model::Look::Boxes(&model::GOLIATH),
    },
    Doll {
        name: goals::YUUKA,
        item: "alice_dolls:yuuka_doll",
        sprite: "alice_dolls:entity/yuuka",
        held: "lunatic:yuuka_parasol",
        slots: 18,
        look: model::Look::Boxes(&model::YUUKA),
    },
];

fn doll_named(name: &str) -> Option<&'static Doll> {
    DOLLS.iter().find(|d| d.name == name)
}

fn doll_for_item(item: &str) -> Option<&'static Doll> {
    DOLLS.iter().find(|d| d.item == item)
}

/// Which doll the mob `id` is, by its name.
fn doll_of(host: &dyn Host, id: u32) -> Option<&'static Doll> {
    let name = host.entity(id).and_then(|e| host.entity_name(e))?;
    doll_named(&name)
}

/// Half as much again as a player's 20: `goals.rs` keeps every doll at this, and Goliath at twice it.
const HEALTH: f32 = 30.0;

fn health_of(doll: &Doll) -> f32 {
    if doll.name == goals::GOLIATH { 2.0 * HEALTH } else { HEALTH }
}
/// The original's starting state bits, 528: chat level 2.
const DEFAULT_CHAT_LEVEL: u8 = 2;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Mod)
}

struct Mod;

/// What is saved with a doll beyond the game's own data.
#[derive(Clone, Debug, PartialEq)]
struct DollData {
    /// The player's UUID.
    owner: u128,
    mode: u8,
    /// Work AI on or off.
    enabled: bool,
    chat_level: u8,
    /// Its inventory, by slot.
    items: Vec<Option<ItemStack>>,
}

impl Default for DollData {
    fn default() -> Self {
        Self {
            owner: 0,
            mode: goals::STANDBY,
            enabled: true,
            chat_level: DEFAULT_CHAT_LEVEL,
            items: vec![None; INVENTORY],
        }
    }
}

fn encode(data: &DollData) -> Result<Compound, AttachmentError> {
    Ok([
        ("owner_high".into(), Tag::Long((data.owner >> 64) as i64)),
        ("owner_low".into(), Tag::Long(data.owner as u64 as i64)),
        ("mode".into(), Tag::Byte(data.mode as i8)),
        ("enabled".into(), Tag::Byte(i8::from(data.enabled))),
        ("chat_level".into(), Tag::Byte(data.chat_level as i8)),
        ("Items".into(), goals::encode_items(&data.items)),
    ]
    .into())
}

fn decode(tag: &Compound) -> Result<DollData, AttachmentError> {
    let long = |key: &str| match tag.get(&key.into()) {
        Some(Tag::Long(n)) => Ok(*n as u64),
        _ => Err(AttachmentError::Decode),
    };
    let byte = |key: &str| match tag.get(&key.into()) {
        Some(Tag::Byte(n)) => Some(*n as u8),
        _ => None,
    };
    // A doll saved by the first version has only `standby`, and otherwise followed.
    let old_mode = if byte("standby") == Some(1) { goals::STANDBY } else { goals::FOLLOW };
    Ok(DollData {
        owner: (u128::from(long("owner_high")?) << 64) | u128::from(long("owner_low")?),
        mode: byte("mode").unwrap_or(old_mode),
        enabled: byte("enabled").is_none_or(|n| n != 0),
        chat_level: byte("chat_level").unwrap_or(DEFAULT_CHAT_LEVEL),
        items: goals::decode_items(tag.get(&"Items".into()), INVENTORY),
    })
}

/// `toggleMode`.
fn next_mode(mode: u8) -> u8 {
    match mode {
        goals::STANDBY => goals::PATROL,
        goals::PATROL => goals::FOLLOW,
        goals::FOLLOW => goals::RIDE,
        _ => goals::STANDBY,
    }
}

fn mode_name(mode: u8) -> &'static str {
    match mode {
        goals::PATROL => "Patrol mode",
        goals::FOLLOW => "Follow mode",
        goals::RIDE => "Rideon mode",
        _ => "Standby mode",
    }
}

/// `chatMessage`'s `lastMsg`: a doll does not repeat itself.
static LAST_SAID: LazyLock<Mutex<HashMap<u32, String>>> = LazyLock::new(Default::default);

/// `chatMessage`: tells the owner, if the doll's chat level allows and it is news.
fn say(host: &mut dyn Host, id: u32, data: &DollData, level: u8, text: &str) {
    if data.chat_level < level {
        return;
    }
    let Ok(mut last) = LAST_SAID.lock() else { return };
    if last.get(&id).is_some_and(|said| said.eq_ignore_ascii_case(text)) {
        return;
    }
    last.insert(id, text.to_owned());
    drop(last);
    let name = doll_of(host, id).map_or("Doll", |d| d.name);
    host.tell(data.owner, Text::literal(format!("{name} : {text}")));
}

fn click(host: &mut dyn Host, id: u32) {
    if let Some(at) = host.entity(id).and_then(|e| host.entity_position(e)) {
        host.play_local_sound("minecraft:ui.button.click", at, 0.3, 1.0, 0);
    }
}

fn tell_state(id: u32, data: &DollData) {
    goals::set_state(id, data.mode, data.enabled, data.owner);
}

static DOLL: AttachmentType<DollData> = AttachmentType {
    namespace: "alice_dolls",
    name: "doll",
    default: DollData::default,
    codec: Some(AttachmentCodec { encode, decode }),
    copy_on_death: false,
    sync: false,
};

/// What is remembered of each doll seen since the game started: a dying or exploding doll's item
/// and inventory go to its owner, and by then its attachment may be gone.
#[derive(Clone)]
struct Known {
    doll: &'static Doll,
    owner: u128,
    items: Vec<Option<ItemStack>>,
}

static KNOWN: LazyLock<Mutex<HashMap<u32, Known>>> = LazyLock::new(Default::default);

fn remember(id: u32, doll: &'static Doll, data: &DollData) {
    if let Ok(mut known) = KNOWN.lock() {
        known.insert(id, Known { doll, owner: data.owner, items: data.items.clone() });
    }
}

fn known(id: u32) -> Option<Known> {
    KNOWN.lock().ok()?.get(&id).cloned()
}

fn forget(id: u32) -> Option<Known> {
    KNOWN.lock().ok()?.remove(&id)
}

/// One right-click reaches the game more than once: each hand asks, and a held button asks again
/// every 4 ticks. A player's click counts only after a pause of `CLICK_GAP` since the last time they
/// asked, so holding the button is one click however long it is held.
static LAST_CLICK: LazyLock<Mutex<HashMap<u128, Instant>>> = LazyLock::new(Default::default);
const CLICK_GAP: Duration = Duration::from_millis(350);

fn fresh_click(player: u128) -> bool {
    let Ok(mut clicks) = LAST_CLICK.lock() else { return true };
    let now = Instant::now();
    let fresh = clicks.get(&player).is_none_or(|at| now.duration_since(*at) >= CLICK_GAP);
    clicks.insert(player, now);
    fresh
}

/// Hands `stacks` to `owner`: what did not fit, or everything when the owner is not here.
fn give_to_owner(host: &mut dyn Host, owner: u128, stacks: Vec<ItemStack>) -> Vec<ItemStack> {
    if owner == 0 {
        return stacks;
    }
    stacks.into_iter().filter_map(|stack| host.give_stack(owner, stack)).collect()
}

/// A gone doll's item, for its owner: it carries the doll's inventory as a shulker box does.
fn belongings(k: &Known) -> Vec<ItemStack> {
    vec![doll_item(k.doll, &k.items)]
}

/// `doll`'s item with `items` inside it, slot by slot (`minecraft:container`, a shulker box's).
fn doll_item(doll: &Doll, items: &[Option<ItemStack>]) -> ItemStack {
    let mut stack = ItemStack::new(doll.item, 1);
    let container: Vec<(u8, ItemStack)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, s)| Some((u8::try_from(i).ok()?, s.clone()?)))
        .collect();
    if !container.is_empty() {
        stack.extra = Some(Box::new(StackData { container, ..StackData::default() }));
    }
    stack
}

/// What a doll item carries, as `slots` slots: a stack in a slot past them takes the first empty one,
/// and is lost only when none is.
fn items_in(stack: &ItemStack, slots: usize) -> Vec<Option<ItemStack>> {
    let mut items = vec![None; slots];
    let mut over = Vec::new();
    for (i, s) in stack.container() {
        match items.get_mut(usize::from(*i)) {
            Some(slot @ None) => *slot = Some(s.clone()),
            _ => over.push(s.clone()),
        }
    }
    for s in over {
        if let Some(slot) = items.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(s);
        }
    }
    items
}

/// Takes the one doll item `stack` the player placed from their hand: a plain one as any item, one
/// carrying an inventory from the very slot, since it stacks with nothing else. Whether it was taken.
fn take_placed(host: &mut dyn Host, player: &NativeServerPlayer, stack: &ItemStack) -> bool {
    if stack.extra.is_none() {
        return host.take_items(&stack.item, 1) == 1;
    }
    let mut rest = stack.clone();
    rest.count -= 1;
    let rest = (rest.count > 0).then_some(rest);
    // The main hand, then the off hand (slot 40).
    [usize::from(player.selected_slot), 40].into_iter().any(|slot| {
        host.set_inventory_slot(player.uuid, NativeInventoryKind::Player, slot, Some(stack), rest.clone()).is_ok()
    })
}

/// `minecraft:iron_ingot` as "Iron Ingot", for a doll item's tooltip.
fn item_label(item: &str) -> String {
    let path = item.rsplit(':').next().unwrap_or(item);
    path.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// How many of a doll item's stacks its tooltip lists, as a shulker box's lists five.
const TOOLTIP_STACKS: usize = 5;

/// A line in `alice_dolls-debug.log` in the game folder: what happened when a doll item was used,
/// to find out why some will not place.
fn debug(host: &(impl Host + ?Sized), line: &str) {
    use std::io::Write;
    let path = host.instance_dir().join("alice_dolls-debug.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = writeln!(file, "{secs} {line}");
    }
}

/// Mobs already written to the debug log, so each shows once a session.
static RENDER_SEEN: LazyLock<Mutex<std::collections::HashSet<u32>>> = LazyLock::new(Default::default);
static TICK_SEEN: LazyLock<Mutex<std::collections::HashSet<u32>>> = LazyLock::new(Default::default);

fn first_time(seen: &Mutex<std::collections::HashSet<u32>>, id: u32) -> bool {
    seen.lock().is_ok_and(|mut seen| seen.insert(id))
}

/// The data a doll is summoned with.
fn doll_nbt(doll: &Doll) -> Compound {
    let max_health: Compound = [
        ("id".into(), Tag::String("minecraft:max_health".into())),
        ("base".into(), Tag::Double(f64::from(health_of(doll)).to_bits())),
    ]
    .into();
    [
        ("CustomName".into(), Tag::String(doll.name.into())),
        ("Silent".into(), Tag::Byte(1)),
        ("PersistenceRequired".into(), Tag::Byte(1)),
        // A doll lays no eggs: about three years of play until the first.
        ("EggLayTime".into(), Tag::Int(i32::MAX)),
        ("Health".into(), Tag::Float(health_of(doll).to_bits())),
        ("attributes".into(), Tag::List(vec![Tag::Compound(max_health)])),
    ]
    .into()
}

// ---- Inventories ----

/// The doll inventory each player has open: the doll, and the menu's window id.
static OPEN: LazyLock<Mutex<HashMap<u128, (u32, i32)>>> = LazyLock::new(Default::default);

fn same_kind(a: &ItemStack, b: &ItemStack) -> bool {
    a.item == b.item && a.damage == b.damage && a.extra == b.extra
}

/// How many of `stack` would fit into `items`.
fn room_for(items: &[Option<ItemStack>], stack: &ItemStack, max: u32) -> u32 {
    items
        .iter()
        .map(|slot| match slot {
            None => max,
            Some(s) if same_kind(s, stack) => max.saturating_sub(s.count),
            Some(_) => 0,
        })
        .sum()
}

/// Puts as much of `stack` into `items` as fits: how many are left over.
fn store_some(items: &mut [Option<ItemStack>], stack: &ItemStack, max: u32) -> u32 {
    let mut left = stack.count;
    for slot in items.iter_mut().filter(|s| s.is_some()) {
        if let Some(s) = slot
            && same_kind(s, stack)
        {
            let add = left.min(max.saturating_sub(s.count));
            s.count += add;
            left -= add;
        }
    }
    for slot in items.iter_mut() {
        if left == 0 {
            break;
        }
        if slot.is_none() {
            let mut s = stack.clone();
            s.count = left.min(max);
            left -= s.count;
            *slot = Some(s);
        }
    }
    left
}

/// Inventories asked for with a chest, to open on the next tick: a menu can't be opened while the
/// server is in the middle of the player's own click (single player is an integrated server too).
static PENDING: Mutex<Vec<(u128, u32)>> = Mutex::new(Vec::new());

impl Mod {
    /// Opens doll `id`'s inventory for `player` as a chest: they put items in and take them out as
    /// in any chest, and what is left in it when it closes is the doll's (`menu_input_closed`).
    /// While it is open the items are the menu's, so a doll that dies meanwhile leaves none to copy.
    fn open_inventory(&self, host: &mut dyn Host, player: u128, id: u32) {
        let holder = Holder::Entity(id);
        let already = OPEN.lock().is_ok_and(|open| open.contains_key(&player));
        if already || goals::chest_open().is_some() || !DOLL.has(host, holder).unwrap_or(false) {
            return;
        }
        let mut data = DOLL.get(host, holder).unwrap_or_default();
        let items = goals::inventory(id).unwrap_or_else(|| data.items.clone());
        let size = items.len();
        let rows = (size / 9).clamp(1, 6);
        let name = doll_of(host, id).map_or("Doll", |d| d.name);
        let window = host.open_menu_with_input(player, rows as _, Text::literal(name), items, 0..size);
        debug(host, &format!("inventory {id}: open_menu_with_input({rows} rows) -> {window:?}"));
        let Some(window) = window else { return };
        if let Ok(mut open) = OPEN.lock() {
            open.insert(player, (id, window));
        }
        goals::set_chest_open(Some(id));
        data.items = vec![None; size];
        goals::put_inventory(id, data.items.clone());
        if let Some(doll) = doll_of(host, id) {
            remember(id, doll, &data);
        }
        let _ = DOLL.set(host, holder, data);
    }

    /// What a doll's inventory holds now, from the chest screen or its own goals: saved with it.
    fn store_items(&self, host: &mut dyn Host, id: u32, items: Vec<Option<ItemStack>>, from_chest: bool) {
        let holder = Holder::Entity(id);
        if !DOLL.has(host, holder).unwrap_or(false) {
            debug(host, &format!("inventory {id}: changed, but the doll is gone"));
            return;
        }
        let mut data = DOLL.get(host, holder).unwrap_or_default();
        data.items = items;
        if from_chest {
            debug(host, &format!("inventory {id}: chest closed, {} stacks", data.items.iter().flatten().count()));
            goals::put_inventory(id, data.items.clone());
        }
        if let Some(doll) = doll_of(host, id) {
            remember(id, doll, &data);
        }
        let _ = DOLL.set(host, holder, data);
    }
}

/// `canCollectRange`: Russia takes an item within two blocks.
const COLLECT_RANGE_SQR: f64 = 4.0;

/// How many of its own ticks pass between Russia's looks for dropped items.
static COLLECT_TICKS: LazyLock<Mutex<HashMap<u32, u32>>> = LazyLock::new(Default::default);

/// `EntityDollAICollectItem`, the part a mob's goals cannot do: finds the nearest dropped item Russia
/// has room for, for its goal to walk to, and takes whatever is within reach into its inventory.
fn collect(host: &mut dyn Host, id: u32) {
    let due = COLLECT_TICKS.lock().is_ok_and(|mut ticks| {
        let n = ticks.entry(id).or_default();
        *n = (*n + 1) % 5;
        *n == 0
    });
    if !due {
        return;
    }
    let Some(entity) = host.entity(id) else { return };
    let (Some(at), Some(level)) = (host.entity_position(entity), host.entity_level(entity)) else {
        return;
    };
    let dimension = level.dimension();
    let found = host.item_entities(
        dimension,
        [at[0] - 16.0, at[1] - 8.0, at[2] - 16.0],
        [at[0] + 16.0, at[1] + 8.0, at[2] + 16.0],
    );
    let holder = Holder::Entity(id);
    let mut data = DOLL.get(host, holder).unwrap_or_default();
    // While its owner has it open, what Russia carries is the chest screen's.
    let can_take = goals::chest_open() != Some(id);
    let mut taken = Vec::new();
    let mut nearest: Option<(f64, [f64; 3])> = None;
    for item in found {
        if item.stack.count == 0 || goals::is_doll_item(&item.stack.item) {
            continue;
        }
        let max = host.max_stack_size(&item.stack.item).unwrap_or(64);
        if room_for(&data.items, &item.stack, max) == 0 {
            continue;
        }
        let d = item.position;
        let d2 = (d[0] - at[0]).powi(2) + (d[1] - at[1]).powi(2) + (d[2] - at[2]).powi(2);
        if can_take && d2 <= COLLECT_RANGE_SQR && item.pickup_delay == 0 {
            let mut items = data.items.clone();
            let left = store_some(&mut items, &item.stack, max);
            let mut rest = item.stack.clone();
            rest.count = left;
            // Only once the item on the ground is changed does Russia have it: nothing is copied.
            if host.set_item_entity(dimension, item.id, rest) {
                taken.push((item.stack.item.to_string(), item.stack.count - left));
                data.items = items;
            }
            continue;
        }
        if nearest.is_none_or(|(n, _)| d2 < n) {
            nearest = Some((d2, d));
        }
    }
    goals::set_item_target(id, nearest.map(|(_, d)| d));
    if taken.is_empty() {
        return;
    }
    host.play_local_sound("minecraft:entity.item.pickup", at, 0.2, 2.0, 0);
    for (item, count) in &taken {
        let short = item.strip_prefix("minecraft:").unwrap_or(item);
        say(host, id, &data, 3, &format!("{short} x{count}"));
    }
    goals::put_inventory(id, data.items.clone());
    if let Some(doll) = doll_of(host, id) {
        remember(id, doll, &data);
    }
    let _ = DOLL.set(host, holder, data);
}

impl Hooks for Mod {
    /// Places a doll from its item.
    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        let item = event.stack.item.to_string();
        if item.starts_with("alice_dolls:") {
            debug(host, &format!("use_item_on {item} at {:?} face {:?}", event.pos, event.face));
        }
        let Some(doll) = doll_for_item(&item) else {
            return Flow::Continue;
        };
        let Some(player) = host.acting_player() else {
            debug(host, &format!("  {item}: no acting player"));
            return Flow::Continue;
        };
        let at = [
            f64::from(event.pos[0] + event.face[0]) + 0.5,
            f64::from(event.pos[1] + event.face[1]),
            f64::from(event.pos[2] + event.face[2]) + 0.5,
        ];
        // What it carried when it was picked up comes back with it. The item goes first, so a doll
        // and its inventory are never in two places.
        let items = items_in(event.stack, doll.slots);
        let survival = player.game_mode != 1;
        if survival && !take_placed(host, &player, event.stack) {
            debug(host, &format!("  {item}: could not take it from the player's hand"));
            return Flow::Cancel;
        }
        let summoned = host.summon(&player.dimension, KIND, at, doll_nbt(doll));
        debug(host, &format!("  {item}: summon in {} at {at:?} -> {summoned:?}", player.dimension));
        let Some(id) = summoned else {
            if survival {
                // Back where it came from, which it has just left room in.
                let mut back = event.stack.clone();
                back.count = 1;
                give_to_owner(host, player.uuid, vec![back]);
            }
            let text = format!("{} : there is no room here", doll.name);
            host.action_bar(player.uuid, Text::literal(text));
            return Flow::Cancel;
        };
        let data = DollData { owner: player.uuid, items, ..DollData::default() };
        remember(id, doll, &data);
        tell_state(id, &data);
        goals::put_inventory(id, data.items.clone());
        let _ = DOLL.set(host, Holder::Entity(id), data);
        let text = format!("{} : Activation successful!", doll.name);
        host.action_bar(player.uuid, Text::literal(text));
        Flow::Cancel
    }

    /// `EntityAliceDoll.interact`: what a right-click does depends on who, and with what.
    fn interact_entity(&self, host: &mut dyn Host, event: &mut InteractEntityEvent<'_>) -> Flow {
        if event.kind != KIND {
            return Flow::Continue;
        }
        let id = event.entity as u32;
        let holder = Holder::Entity(id);
        if !DOLL.has(host, holder).unwrap_or(false) {
            return Flow::Continue;
        }
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        if !fresh_click(player.uuid) {
            return Flow::Cancel;
        }
        let mut data = DOLL.get(host, holder).unwrap_or_default();
        let held = event.stack.map(|stack| &*stack.item);
        let name = doll_of(host, id).map_or("Doll", |d| d.name);
        if data.owner != player.uuid {
            if held != Some(DOLL_CORE) {
                let text = format!("{name} : You are not my owner!");
                host.action_bar(player.uuid, Text::literal(text));
                return Flow::Cancel;
            }
            // A Doll Core takes the doll over, and it starts again from Standby.
            if player.game_mode != 1 {
                host.take_items(DOLL_CORE, 1);
            }
            data.owner = player.uuid;
            data.mode = goals::STANDBY;
            if let Some(doll) = doll_of(host, id) {
                remember(id, doll, &data);
            }
            tell_state(id, &data);
            if let Some(at) = host.entity(id).and_then(|e| host.entity_position(e)) {
                host.play_local_sound("minecraft:item.flintandsteel.use", at, 1.0, 1.0, 0);
            }
            say(host, id, &data, 2, "Activation successful!");
            let _ = DOLL.set(host, holder, data);
            return Flow::Cancel;
        }
        if held.is_none() && goals::player_sneaking() {
            // Sneak and right-click with an empty hand: the doll comes back as its item, with what
            // it carries.
            if let Some(doll) = doll_of(host, id) {
                forget(id);
                host.remove_mob(&player.dimension, id);
                let items = goals::inventory(id).unwrap_or(data.items);
                let k = Known { doll, owner: player.uuid, items };
                give_to_owner(host, player.uuid, belongings(&k));
            }
            return Flow::Cancel;
        }
        match held {
            // The original let a name tag rename a doll; here the name is what makes it one.
            Some("minecraft:name_tag") => {}
            Some(DOLL_CORE) => {
                data.enabled = !data.enabled;
                let text = if data.enabled { "Work AI ON" } else { "Work AI OFF" };
                say(host, id, &data, 1, text);
                click(host, id);
            }
            Some("minecraft:book") => {
                data.chat_level = (data.chat_level + 1) % 4;
                let text = format!("{name} : setChatLevel({})", data.chat_level);
                host.tell(data.owner, Text::literal(text));
                click(host, id);
            }
            Some("minecraft:chest") => {
                if let Ok(mut pending) = PENDING.lock() {
                    pending.push((player.uuid, id));
                }
                return Flow::Cancel;
            }
            _ => {
                data.mode = next_mode(data.mode);
                say(host, id, &data, 1, mode_name(data.mode));
                click(host, id);
            }
        }
        tell_state(id, &data);
        let _ = DOLL.set(host, holder, data);
        Flow::Cancel
    }

    /// A doll's chest menu closed: what is left in it is the doll's, saved on the next tick. If the
    /// doll is gone by then, the items stay in the menu and go back to the player.
    fn menu_input_closed(&self, host: &mut dyn Host, event: &mut MenuInputClosedEvent<'_>) {
        let Some((id, window)) = OPEN.lock().ok().and_then(|open| open.get(&event.uuid).copied()) else {
            return;
        };
        if window != event.window {
            return;
        }
        if let Ok(mut open) = OPEN.lock() {
            open.remove(&event.uuid);
        }
        if known(id).is_none() {
            goals::set_chest_open(None);
            debug(host, &format!("inventory {id}: closed after the doll went; the items go back"));
            return;
        }
        let items: Vec<Option<ItemStack>> = event.slots.iter_mut().map(Option::take).collect();
        goals::chest_closed(id, items);
    }

    fn menu_closed(&self, _: &mut dyn Host, event: &mut MenuClosedEvent) {
        if let Ok(mut open) = OPEN.lock()
            && open.get(&event.uuid).is_some_and(|(_, window)| *window == event.window)
        {
            open.remove(&event.uuid);
        }
    }

    /// `attackEntityFrom`: a blow with a Doll Core in hand picks the doll straight up, with
    /// everything it carried.
    fn attack_entity(&self, host: &mut dyn Host, event: &mut AttackEntityEvent<'_>) -> Flow {
        let id = event.entity as u32;
        if event.kind != KIND
            || !DOLL.has(host, Holder::Entity(id)).unwrap_or(false)
            || host.held().is_none_or(|(item, _, _)| item != DOLL_CORE)
        {
            return Flow::Continue;
        }
        let (Some(player), Some(doll)) = (host.acting_player(), doll_of(host, id)) else {
            return Flow::Continue;
        };
        let data = DOLL.get(host, Holder::Entity(id)).unwrap_or_default();
        forget(id);
        host.remove_mob(&player.dimension, id);
        let items = goals::inventory(id).unwrap_or(data.items);
        let k = Known { doll, owner: player.uuid, items };
        give_to_owner(host, player.uuid, belongings(&k));
        Flow::Cancel
    }

    fn entity_tick_listened(&self, _: &mut dyn Host, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }

    /// Tells the goals each doll's state, and passes on what the dolls said.
    fn entity_tick_post(&self, host: &mut dyn Host, event: &mut EntityTickPostEvent<'_>) {
        for (from, level, text) in goals::take_messages() {
            let data = DOLL.get(host, Holder::Entity(from)).unwrap_or_default();
            say(host, from, &data, level, &text);
        }
        let pending: Vec<(u128, u32)> = PENDING.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default();
        for (player, doll) in pending {
            self.open_inventory(host, player, doll);
        }
        // What the chest menu left in a doll, and what the dolls' own jobs changed.
        for (from, items) in goals::take_chest_closed() {
            self.store_items(host, from, items, true);
        }
        for (from, items) in goals::take_inventory_changes() {
            self.store_items(host, from, items, false);
        }
        if event.kind != KIND {
            return;
        }
        let id = event.entity;
        let holder = Holder::Entity(id);
        if !DOLL.has(host, holder).unwrap_or(false) {
            return;
        }
        let mut data = DOLL.get(host, holder).unwrap_or_default();
        if known(id).is_none_or(|k| k.owner != data.owner) {
            let Some(doll) = doll_of(host, id) else {
                if first_time(&TICK_SEEN, id) {
                    let name = host.entity(id).and_then(|e| host.entity_name(e));
                    debug(host, &format!("tick {id}: has doll data but is named {name:?}"));
                }
                return;
            };
            if first_time(&TICK_SEEN, id) {
                debug(host, &format!("tick {id} {:?}: mode {} owner set {}", doll.name, data.mode, data.owner != 0));
            }
            // A doll saved before its inventory grew gets the rest of its slots.
            if data.items.len() < doll.slots {
                data.items.resize(doll.slots, None);
                let _ = DOLL.set(host, holder, data.clone());
            }
            goals::put_inventory(id, data.items.clone());
            remember(id, doll, &data);
            // The first version stood by with NoAI, which would stop the goals now.
            if let Some(entity) = host.entity(id) {
                host.entity_merge_nbt(entity, [("NoAI".into(), Tag::Byte(0))].into());
            }
        }
        if goals::inventory(id).is_none() {
            goals::put_inventory(id, data.items.clone());
        }
        tell_state(id, &data);
        if known(id).is_some_and(|k| k.doll.name == goals::RUSSIA) {
            if data.enabled && (data.mode == goals::PATROL || data.mode == goals::FOLLOW) {
                collect(host, id);
            } else {
                goals::set_item_target(id, None);
            }
        }
    }

    /// A dead doll leaves itself and what it carried, not feathers and raw chicken, and they go
    /// straight to its owner. They drop where it died only when the owner is away or has no room.
    fn loot_drops(&self, host: &mut dyn Host, event: &mut LootDropsEvent<'_>) {
        let Some((id, KIND)) = event.context.entity else {
            return;
        };
        if let Some(k) = forget(id) {
            event.drops.clear();
            let left = give_to_owner(host, k.owner, belongings(&k));
            event.drops.extend(left);
        }
    }

    /// Ooedo walks away from its own blasts, and from everyone else's.
    fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
        if event.kind != KIND || !event.damage_type.ends_with("explosion") {
            return Flow::Continue;
        }
        let ooedo = u32::try_from(event.entity).ok().and_then(known).is_some_and(|k| k.doll.name == goals::OOEDO);
        if ooedo { Flow::Cancel } else { Flow::Continue }
    }

    /// A doll item that carries an inventory lists it, as a shulker box does.
    fn tooltip(&self, _: &dyn Host, event: &mut TooltipEvent<'_>) {
        if doll_for_item(&event.stack.item).is_none() {
            return;
        }
        let held = event.stack.container();
        for (_, s) in held.iter().take(TOOLTIP_STACKS) {
            let line = format!("§7{} x{}", item_label(&s.item), s.count);
            event.lines.push((Text::legacy(&line), u32::MAX));
        }
        if held.len() > TOOLTIP_STACKS {
            let line = format!("§7§oand {} more...", held.len() - TOOLTIP_STACKS);
            event.lines.push((Text::legacy(&line), u32::MAX));
        }
    }

    /// Draws the doll's own model instead of the chicken's.
    fn mob_submit(&self, host: &dyn Host, event: &mut MobSubmitEvent<'_>) -> Flow {
        if event.mob.kind != KIND {
            return Flow::Continue;
        }
        let name = event.mob.custom_name;
        let Some(doll) = name.and_then(doll_named) else {
            if name.is_some_and(|n| n.contains("Doll")) && first_time(&RENDER_SEEN, event.mob.id) {
                debug(host, &format!("render {}: named {name:?}, which is no doll name", event.mob.id));
            }
            return Flow::Continue;
        };
        // Without its texture a doll would be invisible: then it shows as the chicken it is.
        let drawn = match doll.look {
            model::Look::Base => model::draw_doll(host, event.mob, event.draw, doll.sprite, doll.held),
            model::Look::Boxes(m) => model::draw_model(host, event.mob, event.draw, doll.sprite, doll.held, m),
        };
        if first_time(&RENDER_SEEN, event.mob.id) {
            debug(host, &format!("render {} {:?}: drawn={drawn}", event.mob.id, doll.name));
            if !drawn {
                // Which sprites the atlas has: a quad with no area draws nothing to see.
                let flat = [[0.0f32; 3]; 4];
                for sprite in [
                    doll.sprite,
                    "alice_dolls:entity/shanghai",
                    "alice_dolls:item/shanghai_doll",
                    "lunatic:entity/character/alice_margatroid",
                    "entity/chicken/chicken_temperate",
                    "minecraft:entity/chicken/chicken_temperate",
                    "block/stone",
                ] {
                    let found = host.draw_quad(event.draw, sprite, flat, [0.0; 4], 0xFF_FF_FF);
                    debug(host, &format!("  atlas has {sprite}: {found}"));
                }
            }
        }
        if drawn { Flow::Cancel } else { Flow::Continue }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dolls_data_survives_saving() {
        let mut items = vec![None; INVENTORY];
        items[3] = Some(ItemStack::new("minecraft:torch", 12));
        let data = DollData {
            owner: u128::MAX - 41,
            mode: goals::RIDE,
            enabled: false,
            chat_level: 3,
            items,
        };
        assert_eq!(decode(&encode(&data).unwrap()).unwrap(), data);
        let data = DollData { owner: 7 << 64, ..DollData::default() };
        assert_eq!(decode(&encode(&data).unwrap()).unwrap(), data);
    }

    #[test]
    fn a_doll_saved_by_the_first_version_keeps_its_mode() {
        let saved = |standby: i8| -> Compound {
            [
                ("owner_high".into(), Tag::Long(0)),
                ("owner_low".into(), Tag::Long(9)),
                ("standby".into(), Tag::Byte(standby)),
            ]
            .into()
        };
        assert_eq!(decode(&saved(1)).unwrap().mode, goals::STANDBY);
        let following = decode(&saved(0)).unwrap();
        assert_eq!(following.mode, goals::FOLLOW);
        assert!(following.enabled);
        assert_eq!(following.chat_level, DEFAULT_CHAT_LEVEL);
        assert_eq!(following.items, vec![None; INVENTORY]);
    }

    #[test]
    fn a_picked_up_doll_keeps_its_inventory_in_its_item() {
        let russia = doll_named(goals::RUSSIA).unwrap();
        let mut items = vec![None; russia.slots];
        items[0] = Some(ItemStack::new("minecraft:torch", 12));
        items[35] = Some(ItemStack::new("minecraft:iron_ingot", 3));
        let stack = doll_item(russia, &items);
        assert_eq!(stack.item.as_ref(), russia.item);
        assert_eq!(stack.container().len(), 2);
        assert_eq!(items_in(&stack, russia.slots), items);
        // An empty doll's item is a plain one, which stacks.
        assert!(doll_item(russia, &vec![None; russia.slots]).extra.is_none());
        // Into fewer slots, what is past them takes the empty ones.
        let fewer = items_in(&stack, 9);
        assert_eq!(fewer.iter().flatten().count(), 2);
    }

    #[test]
    fn a_tooltip_names_items_as_the_game_does() {
        assert_eq!(item_label("minecraft:iron_ingot"), "Iron Ingot");
        assert_eq!(item_label("torch"), "Torch");
    }

    #[test]
    fn what_does_not_fit_is_left_over() {
        let mut items = vec![Some(ItemStack::new("minecraft:torch", 60)), None];
        assert_eq!(room_for(&items, &ItemStack::new("minecraft:torch", 1), 64), 68);
        let left = store_some(&mut items, &ItemStack::new("minecraft:torch", 80), 64);
        assert_eq!(left, 12);
        assert_eq!(items[1].as_ref().map(|s| s.count), Some(64));
    }

    #[test]
    fn every_doll_has_its_own_name_and_item_and_whole_rows() {
        for (i, a) in DOLLS.iter().enumerate() {
            assert!(a.slots % 9 == 0 && a.slots <= 54, "{}", a.name);
            for b in &DOLLS[i + 1..] {
                assert_ne!(a.name, b.name);
                assert_ne!(a.item, b.item);
            }
        }
        assert_eq!(health_of(doll_named(goals::GOLIATH).unwrap()), 60.0);
        assert_eq!(health_of(doll_named(goals::REIMU).unwrap()), 30.0);
    }

    #[test]
    fn each_doll_is_found_by_its_item_and_its_name() {
        for doll in &DOLLS {
            assert_eq!(doll_for_item(doll.item), Some(doll));
            assert_eq!(doll_named(doll.name), Some(doll));
        }
        assert_eq!(doll_named("Shanghai"), None);
    }

    #[test]
    fn the_modes_go_round_in_the_originals_order() {
        assert_eq!(next_mode(goals::STANDBY), goals::PATROL);
        assert_eq!(next_mode(goals::PATROL), goals::FOLLOW);
        assert_eq!(next_mode(goals::FOLLOW), goals::RIDE);
        assert_eq!(next_mode(goals::RIDE), goals::STANDBY);
    }

    #[test]
    fn a_doll_is_summoned_quiet_named_and_with_its_health() {
        let nbt = doll_nbt(&DOLLS[1]);
        assert!(matches!(nbt.get(&"Silent".into()), Some(Tag::Byte(1))));
        assert!(matches!(nbt.get(&"CustomName".into()), Some(Tag::String(_))));
        assert!(matches!(nbt.get(&"Health".into()), Some(Tag::Float(b)) if *b == 30.0f32.to_bits()));
    }
}
