//! Alice's Dolls for the eltanschauung/horizon fork: a port of the 1.7.10 mod by iwa_yukinoshita.
//!
//! Alice's dolls: Shanghai fights and steals, Horai finds and mines ore, Ooedo blows itself up
//! next to monsters, Germany chops trees, London finds spawners. Right-click one empty-handed to go
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
    ItemStack, LootDropsEvent, MenuClickedEvent, MenuClosedEvent, MobSubmitEvent, Tag, Text,
    UseItemOnEvent,
};

// The kind every doll is underneath.
const KIND: &str = "minecraft:chicken";
const DOLL_CORE: &str = "alice_dolls:doll_core";
/// `DollBase.getSizeInventory`: one row.
const INVENTORY: usize = 9;

/// One of the dolls: the name that makes a chicken this doll, its item, its texture, and what it
/// holds (`getHeldItem`).
#[derive(Debug, PartialEq, Eq)]
pub struct Doll {
    name: &'static str,
    item: &'static str,
    sprite: &'static str,
    held: &'static str,
}

const DOLLS: [Doll; 6] = [
    Doll {
        name: goals::SHANGHAI,
        item: "alice_dolls:shanghai_doll",
        sprite: "alice_dolls:entity/shanghai",
        held: "minecraft:golden_sword",
    },
    Doll {
        name: goals::HORAI,
        item: "alice_dolls:horai_doll",
        sprite: "alice_dolls:entity/horai",
        held: "minecraft:golden_pickaxe",
    },
    Doll {
        name: goals::DIAMOND_HORAI,
        item: "alice_dolls:diamond_horai_doll",
        sprite: "alice_dolls:entity/horai",
        held: "minecraft:diamond_pickaxe",
    },
    Doll {
        name: goals::OOEDO,
        item: "alice_dolls:ooedo_doll",
        sprite: "alice_dolls:entity/ooedo",
        held: "minecraft:tnt",
    },
    Doll {
        name: goals::GERMANY,
        item: "alice_dolls:germany_doll",
        sprite: "alice_dolls:entity/germany",
        held: "minecraft:golden_axe",
    },
    Doll {
        name: goals::LONDON,
        item: "alice_dolls:london_doll",
        sprite: "alice_dolls:entity/london",
        held: "minecraft:ender_eye",
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

/// Half as much again as a player's 20: `goals.rs` keeps every doll at this.
const HEALTH: f32 = 30.0;
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

/// A gone doll's item and everything it carried, for its owner.
fn belongings(k: &Known) -> Vec<ItemStack> {
    let mut all = vec![ItemStack::new(k.doll.item, 1)];
    all.extend(k.items.iter().flatten().cloned());
    all
}

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
        ("base".into(), Tag::Double(f64::from(HEALTH).to_bits())),
    ]
    .into();
    [
        ("CustomName".into(), Tag::String(doll.name.into())),
        ("Silent".into(), Tag::Byte(1)),
        ("PersistenceRequired".into(), Tag::Byte(1)),
        // A doll lays no eggs: about three years of play until the first.
        ("EggLayTime".into(), Tag::Int(i32::MAX)),
        ("Health".into(), Tag::Float(HEALTH.to_bits())),
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

/// Puts `stack` into `items` whole, onto matching stacks first, then into empty slots. False, and
/// nothing changed, when it does not all fit.
fn store(items: &mut [Option<ItemStack>], stack: &ItemStack, max: u32) -> bool {
    let mut room = 0;
    for slot in items.iter() {
        room += match slot {
            None => max,
            Some(s) if same_kind(s, stack) => max.saturating_sub(s.count),
            Some(_) => 0,
        };
    }
    if room < stack.count {
        return false;
    }
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
    true
}

/// `native_menu_inventory_slot`: the player's inventory slot a click below the doll's row landed
/// on (the three rows of the inventory, then the hotbar).
fn inventory_slot(slot: i32) -> Option<usize> {
    let below = usize::try_from(slot).ok()?.checked_sub(INVENTORY)?;
    match below {
        0..27 => Some(below + 9),
        27..36 => Some(below - 27),
        _ => None,
    }
}

impl Mod {
    fn open_inventory(&self, host: &mut dyn Host, player: u128, id: u32, data: &DollData) {
        let name = doll_of(host, id).map_or("Doll", |d| d.name);
        let window = host.open_menu(player, 1, Text::literal(name), data.items.clone());
        debug(host, &format!("inventory {id}: open_menu -> {window:?}"));
        if let Some(window) = window
            && let Ok(mut open) = OPEN.lock()
        {
            open.insert(player, (id, window));
        }
    }
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
        let summoned = host.summon(&player.dimension, KIND, at, doll_nbt(doll));
        debug(host, &format!("  {item}: summon in {} at {at:?} -> {summoned:?}", player.dimension));
        let Some(id) = summoned else {
            let text = format!("{} : there is no room here", doll.name);
            host.action_bar(player.uuid, Text::literal(text));
            return Flow::Cancel;
        };
        let data = DollData { owner: player.uuid, ..DollData::default() };
        remember(id, doll, &data);
        tell_state(id, &data);
        let _ = DOLL.set(host, Holder::Entity(id), data);
        if player.game_mode != 1 {
            host.take_items(doll.item, 1);
        }
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
                let k = Known { doll, owner: player.uuid, items: data.items };
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
                self.open_inventory(host, player.uuid, id, &data);
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

    /// A click in a doll's inventory: a doll slot hands its stack to the player, an inventory slot
    /// moves that whole stack into the doll if it fits. The menu itself moves nothing.
    fn menu_clicked(&self, host: &mut dyn Host, event: &mut MenuClickedEvent) {
        let Some((id, window)) = OPEN.lock().ok().and_then(|open| open.get(&event.uuid).copied())
        else {
            return;
        };
        debug(host, &format!("inventory {id}: click window {} (open {window}) slot {}", event.window, event.slot));
        if window != event.window {
            return;
        }
        let holder = Holder::Entity(id);
        if !DOLL.has(host, holder).unwrap_or(false) {
            host.close_menu(event.uuid);
            return;
        }
        let mut data = DOLL.get(host, holder).unwrap_or_default();
        if data.owner != event.uuid {
            return;
        }
        if let Ok(slot) = usize::try_from(event.slot)
            && slot < INVENTORY
        {
            // Out of the doll, into the player's inventory; what does not fit stays.
            if let Some(stack) = data.items[slot].take() {
                data.items[slot] = host.give_stack(event.uuid, stack);
            }
        } else if let Some(inv) = inventory_slot(event.slot) {
            // Into the doll: only a whole stack, and only once the doll has made room for it.
            let Some(stack) = host.player_stacks(event.uuid).and_then(|s| s.get(inv).cloned().flatten())
            else {
                return;
            };
            let max = host.max_stack_size(&stack.item).unwrap_or(64);
            let mut items = data.items.clone();
            if !store(&mut items, &stack, max) {
                return;
            }
            let Some(name) = host.server_players().into_iter().find(|p| p.uuid == event.uuid).map(|p| p.name)
            else {
                return;
            };
            let ran = host.run_command(&format!("item replace entity {name} container.{inv} with minecraft:air"));
            debug(host, &format!("  put {} x{} from slot {inv}: command ran={ran}", stack.item, stack.count));
            if !ran {
                return;
            }
            data.items = items;
        } else {
            return;
        }
        host.set_menu_items(event.uuid, data.items.clone());
        if let Some(doll) = doll_of(host, id) {
            remember(id, doll, &data);
        }
        let _ = DOLL.set(host, holder, data);
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
        let k = Known { doll, owner: player.uuid, items: data.items };
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
        // An Ooedo that blew itself up: it and what it carried go back to its owner.
        for from in goals::take_spent() {
            if let Some(k) = forget(from) {
                give_to_owner(host, k.owner, belongings(&k));
            }
        }
        if event.kind != KIND {
            return;
        }
        let id = event.entity;
        let holder = Holder::Entity(id);
        if !DOLL.has(host, holder).unwrap_or(false) {
            return;
        }
        let data = DOLL.get(host, holder).unwrap_or_default();
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
            remember(id, doll, &data);
            // The first version stood by with NoAI, which would stop the goals now.
            if let Some(entity) = host.entity(id) {
                host.entity_merge_nbt(entity, [("NoAI".into(), Tag::Byte(0))].into());
            }
        }
        tell_state(id, &data);
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
        let drawn = model::draw_doll(host, event.mob, event.draw, doll.sprite, doll.held);
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
    fn a_stack_goes_in_whole_or_not_at_all() {
        let mut items = vec![None; INVENTORY];
        items[0] = Some(ItemStack::new("minecraft:torch", 60));
        assert!(store(&mut items, &ItemStack::new("minecraft:torch", 10), 64));
        assert_eq!(items[0].as_ref().map(|s| s.count), Some(64));
        assert_eq!(items[1].as_ref().map(|s| s.count), Some(6));
        let mut full = vec![Some(ItemStack::new("minecraft:dirt", 64)); INVENTORY];
        let before = full.clone();
        assert!(!store(&mut full, &ItemStack::new("minecraft:torch", 1), 64));
        assert_eq!(full, before);
    }

    #[test]
    fn clicks_below_the_row_land_on_the_inventory_then_the_hotbar() {
        assert_eq!(inventory_slot(3), None);
        assert_eq!(inventory_slot(9), Some(9));
        assert_eq!(inventory_slot(35), Some(35));
        assert_eq!(inventory_slot(36), Some(0));
        assert_eq!(inventory_slot(44), Some(8));
        assert_eq!(inventory_slot(45), None);
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
