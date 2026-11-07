//! Alice's Dolls for the eltanschauung/horizon fork: a port of the 1.7.10 mod by iwa_yukinoshita.
//!
//! Alice's dolls: Shanghai fights and steals, Horai finds and mines ore, Ooedo blows itself up
//! next to monsters. Right-click it empty-handed to go Standby, Patrol, Follow, Rideon and back to
//! Standby; with a Doll Core to switch its AI on and off, or to take over someone else's doll; with
//! a book to change how much it says. Hit it with a Doll Core, or kill it, to get the item back.
//!
//! The fork has no living mod entity types, so a doll is a silent chicken with a custom name:
//! this mod draws it, and `goals.rs` drives it. A chicken is close to the doll's size and falls
//! slowly, as the doll did. This crate saves each doll's state and hands it to the goals each tick.

mod model;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use horizon_client::mobs::alice_doll_goals as goals;
use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, AttackEntityEvent, Compound,
    EntityTickListenedEvent, EntityTickPostEvent, Flow, Holder, Hooks, Host, InteractEntityEvent,
    ItemStack, LootDropsEvent, MobSubmitEvent, Tag, Text, UseItemOnEvent,
};

// The kind every doll is underneath.
const KIND: &str = "minecraft:chicken";
const DOLL_CORE: &str = "alice_dolls:doll_core";

/// One of the dolls: the name that makes a chicken this doll, its item, its texture, and what it
/// holds (`getHeldItem`).
#[derive(Debug, PartialEq, Eq)]
pub struct Doll {
    name: &'static str,
    item: &'static str,
    sprite: &'static str,
    held: &'static str,
}

const DOLLS: [Doll; 3] = [
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
        name: goals::OOEDO,
        item: "alice_dolls:ooedo_doll",
        sprite: "alice_dolls:entity/ooedo",
        held: "minecraft:tnt",
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
const HEALTH: f32 = 8.0;
/// The original's starting state bits, 528: chat level 2.
const DEFAULT_CHAT_LEVEL: u8 = 2;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Mod)
}

struct Mod;

/// What is saved with a doll beyond the game's own data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DollData {
    /// The player's UUID.
    owner: u128,
    mode: u8,
    /// Work AI on or off.
    enabled: bool,
    chat_level: u8,
}

impl Default for DollData {
    fn default() -> Self {
        Self { owner: 0, mode: goals::STANDBY, enabled: true, chat_level: DEFAULT_CHAT_LEVEL }
    }
}

fn encode(data: &DollData) -> Result<Compound, AttachmentError> {
    Ok([
        ("owner_high".into(), Tag::Long((data.owner >> 64) as i64)),
        ("owner_low".into(), Tag::Long(data.owner as u64 as i64)),
        ("mode".into(), Tag::Byte(data.mode as i8)),
        ("enabled".into(), Tag::Byte(i8::from(data.enabled))),
        ("chat_level".into(), Tag::Byte(data.chat_level as i8)),
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
fn say(host: &mut dyn Host, id: u32, data: DollData, level: u8, text: &str) {
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

static DOLL: AttachmentType<DollData> = AttachmentType {
    namespace: "alice_dolls",
    name: "doll",
    default: DollData::default,
    codec: Some(AttachmentCodec { encode, decode }),
    copy_on_death: false,
    sync: false,
};

/// The dolls seen since the game started: a dying mob's drops name only its id.
static KNOWN: LazyLock<Mutex<HashMap<u32, &'static Doll>>> = LazyLock::new(Default::default);

fn remember(id: u32, doll: &'static Doll) {
    if let Ok(mut known) = KNOWN.lock() {
        known.insert(id, doll);
    }
}

fn known(id: u32) -> Option<&'static Doll> {
    KNOWN.lock().ok()?.get(&id).copied()
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

impl Hooks for Mod {
    /// Places a doll from its item.
    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        let Some(doll) = doll_for_item(&event.stack.item) else {
            return Flow::Continue;
        };
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        let at = [
            f64::from(event.pos[0] + event.face[0]) + 0.5,
            f64::from(event.pos[1] + event.face[1]),
            f64::from(event.pos[2] + event.face[2]) + 0.5,
        ];
        let Some(id) = host.summon(&player.dimension, KIND, at, doll_nbt(doll)) else {
            let text = format!("{} : there is no room here", doll.name);
            host.action_bar(player.uuid, Text::literal(text));
            return Flow::Cancel;
        };
        remember(id, doll);
        let data = DollData { owner: player.uuid, ..DollData::default() };
        let _ = DOLL.set(host, Holder::Entity(id), data);
        goals::set_state(id, data.mode, data.enabled);
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
            let _ = DOLL.set(host, holder, data);
            goals::set_state(id, data.mode, data.enabled);
            if let Some(at) = host.entity(id).and_then(|e| host.entity_position(e)) {
                host.play_local_sound("minecraft:item.flintandsteel.use", at, 1.0, 1.0, 0);
            }
            say(host, id, data, 2, "Activation successful!");
            return Flow::Cancel;
        }
        match held {
            // The original let a name tag rename a doll; here the name is what makes it one.
            Some("minecraft:name_tag") => {}
            Some(DOLL_CORE) => {
                data.enabled = !data.enabled;
                let text = if data.enabled { "Work AI ON" } else { "Work AI OFF" };
                say(host, id, data, 1, text);
                click(host, id);
            }
            Some("minecraft:book") => {
                data.chat_level = (data.chat_level + 1) % 4;
                let text = format!("{name} : setChatLevel({})", data.chat_level);
                host.tell(data.owner, Text::literal(text));
                click(host, id);
            }
            Some("minecraft:chest") => {
                host.action_bar(player.uuid, Text::literal(format!("{name} : no inventory yet")));
            }
            _ => {
                data.mode = next_mode(data.mode);
                say(host, id, data, 1, mode_name(data.mode));
                click(host, id);
            }
        }
        let _ = DOLL.set(host, holder, data);
        goals::set_state(id, data.mode, data.enabled);
        Flow::Cancel
    }

    /// `attackEntityFrom`: a blow with a Doll Core in hand picks the doll straight up.
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
        host.remove_mob(&player.dimension, id);
        host.give_items(doll.item, 1);
        Flow::Cancel
    }

    fn entity_tick_listened(&self, _: &mut dyn Host, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }

    /// Tells the goals each doll's state, and passes on what the dolls said.
    fn entity_tick_post(&self, host: &mut dyn Host, event: &mut EntityTickPostEvent<'_>) {
        if event.kind != KIND {
            return;
        }
        let id = event.entity;
        let holder = Holder::Entity(id);
        if !DOLL.has(host, holder).unwrap_or(false) {
            return;
        }
        if known(id).is_none() {
            let Some(doll) = doll_of(host, id) else { return };
            remember(id, doll);
            // The first version stood by with NoAI, which would stop the goals now.
            if let Some(entity) = host.entity(id) {
                host.entity_merge_nbt(entity, [("NoAI".into(), Tag::Byte(0))].into());
            }
        }
        let data = DOLL.get(host, holder).unwrap_or_default();
        goals::set_state(id, data.mode, data.enabled);
        for (from, level, text) in goals::take_messages() {
            let data = DOLL.get(host, Holder::Entity(from)).unwrap_or_default();
            say(host, from, data, level, &text);
        }
    }

    /// A doll drops itself, not feathers and raw chicken.
    fn loot_drops(&self, _: &mut dyn Host, event: &mut LootDropsEvent<'_>) {
        let Some((id, KIND)) = event.context.entity else {
            return;
        };
        if let Some(doll) = known(id) {
            event.drops.clear();
            event.drops.push(ItemStack::new(doll.item, 1));
        }
    }

    /// Draws the doll's own model instead of the chicken's.
    fn mob_submit(&self, host: &dyn Host, event: &mut MobSubmitEvent<'_>) -> Flow {
        if event.mob.kind != KIND {
            return Flow::Continue;
        }
        let Some(doll) = event.mob.custom_name.and_then(doll_named) else {
            return Flow::Continue;
        };
        model::draw_doll(host, event.mob, event.draw, doll.sprite, doll.held);
        Flow::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dolls_data_survives_saving() {
        let data = DollData { owner: u128::MAX - 41, mode: goals::RIDE, enabled: false, chat_level: 3 };
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
        assert!(matches!(nbt.get(&"Health".into()), Some(Tag::Float(b)) if *b == 8.0f32.to_bits()));
    }
}
