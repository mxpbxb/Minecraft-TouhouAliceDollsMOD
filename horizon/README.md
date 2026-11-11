# Alice's Dolls for Horizon

A work-in-progress port of Alice's Dolls (1.7.10-kog3, by iwa_yukinoshita), with its extra dolls, to the
[eltanschauung/horizon](https://github.com/eltanschauung/horizon) fork of Horizon.

The original Java source is in the version folders of this repository. This folder is the port:

- `content-fork/` is the content mod (`alice_dolls.hmod`): the items, recipes, names and textures.
- `native/` is the native mod (`alice_dolls.hnmod`): `api/` saves each doll's owner, mode and
  inventory and draws its model, and the mixin files (`goals.rs` and the hooks) give the dolls their jobs.

## Dolls

| Doll | Crafted with | Slots | What it does |
|---|---|---|---|
| Shanghai | golden sword | 9 | Fights hostile mobs within 5 blocks (from your head too), shears sheep and steals from animals, reports the mobs around it |
| Horai | golden pickaxe | 9 | Reports the ore within 3 blocks and digs its way to it, even from your head |
| Diamond Horai | diamond pickaxe | 9 | The same as Horai for now |
| Ooedo | TNT | 9 | On patrol, walks up to the nearest hostile mob and explodes |
| Germany | golden axe | 9 | Finds a tree within 5 blocks, hops to its top and chops it down |
| London | eye of ender | 9 | Reports the spawners in the 64 layers above and below it |
| Russia | chest | 36 | On patrol or following, picks up dropped items within 16 blocks; with torches in its first slot, puts them down where it is dark |
| Grand Guignol | skeleton skull | 9 | Keeps you company (a secret doll in the original) |
| Marisa | fire charge | 18 | On patrol or following, keeps within 5 blocks of a hostile mob and shoots five stars a second |
| Reimu | paper | 18 | In any mode but Rideon, shoots a ring of eight homing amulets every four seconds |
| Aya | feather | 18 | Takes a photo every 30 seconds, and when hurt, and tells you what she caught |
| Wakasagi | tropical fish | 18 | Keeps you company (a secret doll in the original) |
| Goliath | diamond sword, iron blocks | 9 | Nearly three blocks tall, 60 health: on patrol fights hostile mobs, following fights whatever is after you, and knocks them back |
| Yuuka | sunflower | 18 | On patrol, tills dirt between farmland, plants seeds from her inventory, and harvests ripe crops, melons and pumpkins, replanting them |

Each recipe is the item in the top left, wool (Goliath: iron blocks), and a Doll Core in the middle.
Right-click a doll empty-handed to switch modes: Standby, Patrol, Follow, Rideon. Right-click with a
Doll Core to turn its AI on or off, with a book to change how much it says, or with a chest to open its
inventory. Sneak and right-click it empty-handed, or hit it with a Doll Core, to pick it up. Every doll
has 30 health, half as much again as a player (Goliath 60). A doll's item can't be burnt, blown up or
despawned, and a dead doll's item and inventory go to its owner.

Marisa's and Reimu's shots home in on the mob they were fired at and hit nothing else, so they never
hurt you, your other dolls, or anything in the way. The extras held thKaguya mod items; Aya's fan,
Yuuka's parasol and Reimu's yin-yang orb are Lunatic's, and Marisa holds a fire charge.

## How it works on the fork

The fork cannot add new living mob types, so each doll is a silent chicken with the doll's name,
drawn with the doll's own model (the extras with their own box models). A cancellable hook on
`Mob::voice` keeps it quiet.

A doll's inventory opens as a chest menu with input slots (`open_menu_with_input`): you put items in
and take them out as in any chest, and what is left in it when it closes is the doll's. Single player
runs an integrated server, which can't open a menu in the middle of the player's own click, so the
menu opens on the next tick. Mob goals can't see dropped items, so the api crate finds and takes
Russia's. Wakasagi swims at a dolphin's pace in water.

Not ported yet: Germany's sapling planting, chunk loading, Ooedo's quarry, London's stronghold
direction and spawner mob names, and Aya's real screenshots.

## Building

Built and installed by the Horizon launcher (`native-build`) from the two mod files. The launcher
always builds the newest Horizon main; this was last checked against
`f5087c6b6577d6441180b42ad4e47ad1e9b06c18`.

The original mod has no licence. Do not redistribute this port without the author's permission.
