# Alice's Dolls for Horizon

A work-in-progress port of Alice's Dolls (1.7.10-kog3, by iwa_yukinoshita) to the
[eltanschauung/horizon](https://github.com/eltanschauung/horizon) fork of Horizon.

The original Java source is in the version folders of this repository. This folder is the port:

- `content-fork/` is the content mod (`alice_dolls.hmod`): the items, recipes, names and textures.
- `native/` is the native mod (`alice_dolls.hnmod`): `api/` saves each doll's owner and mode and
  draws its model, and the mixin files (`goals.rs` and the hooks) give the dolls their jobs.

## Dolls

| Doll | Crafted with | What it does |
|---|---|---|
| Shanghai | golden sword | Fights hostile mobs, shears sheep and steals from animals, reports the mobs around it |
| Horai | golden pickaxe | Reports the ore within 3 blocks and digs its way to it |
| Ooedo | TNT | On patrol, walks up to the nearest hostile mob and explodes |

Each recipe is the item in the top left, wool, and a Doll Core in the middle. Right-click a doll
empty-handed to switch modes: Standby, Patrol, Follow, Rideon. Right-click with a Doll Core to turn
its AI on or off, or with a book to change how much it says. Hit it with a Doll Core to pick it up.

## How it works on the fork

The fork cannot add new living mob types, so each doll is a silent chicken with the doll's name,
drawn with the doll's own model. A cancellable hook on `Mob::voice` keeps it quiet.

Not ported yet: the doll inventory, item pickup, chunk loading, the other dolls, and Ooedo's quarry.

## Building

Built and installed by the Horizon launcher (`native-build`) from the two mod files. The source is
pinned to fork revision `8d5195563cab0ad1e7e7c8d488f45bc6e9f20db8`.

The original mod has no licence. Do not redistribute this port without the author's permission.
