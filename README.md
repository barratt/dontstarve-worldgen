# dsworldgen

Generate **Don't Starve** and **Don't Starve Together** worlds from a seed
without running the game, and search seed ranges for the world you want.
It runs in your browser or as a command-line tool.

You need the games installed: the generator runs the games' own
world-generation scripts, read from your copy. No game files are included in
this repository. The web page is built locally from your own installs.

## What it generates

Given a seed and a world type, `dsworldgen` produces the world the game would
make for that seed. That is the save data the game writes when a new world
is created:

- **the map**: every ground tile (grass, forest, marsh, rocky, savanna,
  ocean, …), the rooms ("nodes") the map is built from, and the roads;
- **the tasks**: groups of themed rooms such as *Speak to the king* (the Pig
  King's area) or *Squeltch* (marsh), and which rooms each contains;
- **every object placed at generation**: trees, rocks, ponds, beefalo herds,
  the Pig King, Walrus Camps, Touchstones, Wormholes, boss spawners, the Florid
  Postern, cave entrances, ruins and more, each with its world position;
- **the spawn point**: where the player starts.

The output comes in two forms:

1. **The savedata** (`--format savedata`). This is the game's own world file,
   byte for byte. It's useful for checking the generator, or for tools that
   read the game's format.
2. **A summary** (the default). This is a readable JSON document of what
   matters when choosing a world, and it is what searches filter on. Here is
   an excerpt for Reign of Giants seed 1001:

```json
{
  "seed": 1001,
  "dlc": "rog",
  "level_id": "SURVIVAL_DEFAULT",
  "level_type": "survival",
  "map": { "name": "Default", "prefab": "forest", "width": 376, "height": 376,
           "nodes": 177, "roads": 14, "land_tiles": 29731 },
  "spawn": { "x": -80, "z": -100 },
  "task_names": ["Badlands", "Beeeees!", "Dig that rock", "Forest hunters", "…"],
  "tasks": [{ "name": "Badlands", "rooms": 14, "room_types": ["Badlands", "BuzzardyBadlands", "…"] }],
  "prefabs": { "beefalo": 6, "pigking": 1, "walrus_camp": 4, "wormhole": 10, "…": "…" },
  "nearest": {
    "pigking": { "distance": 384.22, "x": -341.5, "z": -381.5 },
    "resurrectionstone": { "distance": 92.65, "x": -58, "z": -10 },
    "walrus_camp": { "distance": 316.29, "x": 68.08, "z": 179.49 }
  },
  "notable": { "pigking": [{ "x": -341.5, "z": -381.5, "distance": 384.22 }], "…": "…" },
  "tiles": { "GRASS": 6681, "FOREST": 5810, "MARSH": 2854, "…": "…" },
  "generator": { "name": "ds-worldgen", "game_build": "578406", "generation_ms": 939.4,
                 "savedata_sha256": "54ef3381…" }
}
```

| Field | Meaning |
|---|---|
| `seed`, `dlc`, `level_id`, `level_type` | what was generated |
| `map` | size in tiles, number of rooms (`nodes`) and roads, land tile count |
| `spawn` | the player start in world units. DST forest: the Florid Postern; DST caves: the first cave exit. |
| `tasks`, `task_names` | the world's tasks and the room types in each |
| `prefabs` | how many of every prefab (object type) the world contains |
| `nearest` | for notable prefabs, the one nearest the spawn point, with its distance |
| `notable` | every position of each notable prefab, with its distance from spawn |
| `tiles` | ground tile counts by tile name |
| `generator` | version, generation time and a hash of the savedata |

Notable prefabs include the Pig King, Walrus Camps, Touchstones, Wormholes,
Chester's Eyebone, beefalo, Glommer's statue, boss spawners (Dragonfly, Bee
Queen, Antlion, Crab King, Klaus), the Moon Stone, the Oasis, the Hermit
Crab's island, the Ancient Pseudoscience Station, the Ancient Guardian,
the Ancient Gateway, Toadstool, the Archives, and the cave entrances and exits.

Distances are in world units: a ground tile is 4 units across.

## Using it in the browser

The web page searches seeds in your browser with the game data built in, so
there's nothing to install or pick. You build your own copy from your
installed games (below), then:

1. **Game.** Pick the game and world.
   - Don't Starve: the DLC (base game, Reign of Giants, Shipwrecked, Hamlet)
     and the world (survival, caves or ruins, adventure chapters,
     Shipwrecked, volcano, Hamlet).
   - Don't Starve Together: forest or caves, and the World Size setting
     (Small, Medium, Large or Huge; Large is the game's default). It
     changes the world generated, as it does in the game; it isn't a
     filter.
   - Don't Starve Together, Host: Windows (the game, or a Windows dedicated
     server) or a Linux dedicated server. They make different worlds from
     the same seed (see Accuracy), so pick the one the world is hosted on.
2. **Filters.**
   - **Add a filter:** "Add filter" lists everything the world type
     contains, with the game's icons and names. It has tabs for points of
     interest, every object, biomes (tasks), ground and map size, and a
     search box. The list comes from a few worlds generated in the
     background and grows as you search.
   - **Edit a filter:** each filter becomes a pill. Click it to choose want
     or avoid, at least / at most / exactly N, and, for points of interest,
     "within N tiles of spawn". × removes it.
   - **Search:** set a seed range and press Search. Worlds are generated in
     parallel, one per CPU core.
3. **Leaderboard.** The 50 seeds that best match your filters: most filters
   met first, then how well each is met (more than asked for, or nearer the
   spawn).
4. **All results.** Every seed searched, with how many filters it meets and
   the value behind each one. "Matches only" hides the rest, and the
   matches can be downloaded as JSON lines.
5. **Map.** Click any seed in either list to see its world. The ground is
   drawn tile by tile, and every structure and object appears as its
   minimap icon from the game, with the spawn marked ★.
   - The legend toggles each prefab; it starts with the points of interest
     and whatever your filters ask for.
   - Scroll to zoom, drag to move, and hover an icon for its name and its
     distance from spawn.
   - You can download the world's savedata.

### Building your copy

```powershell
./web/build-standalone.ps1 -Emsdk C:\path\to\emsdk `
    -Game "C:\Program Files (x86)\Steam\steamapps\common\dont_starve" `
    -Dst  "C:\Program Files (x86)\Steam\steamapps\common\Don't Starve Together"
python -m http.server -d dist 8000      # then open http://localhost:8000
```

Either `-Game` or `-Dst` can be left out.

1. `ds-worldgen pack` collects, from your installs, into one compressed
   bundle (about 23 MB for both games):
   - the Lua scripts world generation runs;
   - the names, not the contents, of the assets those scripts check for;
   - the games' minimap icons and display names, for the page.
2. The bundle is built into the WebAssembly module.
3. Each browser worker unpacks only the game it is generating, into its
   in-memory filesystem.

**The full build is for local use.** `dist/` contains the games' scripts as
they are, so don't commit it or publish it. `.gitignore` excludes it, along
with `dist-public/` and `*.dswb`.

### Publishing

`./web/publish.ps1 -Emsdk … -Game … -Dst …` builds and deploys the hosted
site to GitHub Pages. It builds with `build-standalone.ps1 -Public`, which
differs from the local build:

- **Only what worldgen loads:** the bundle keeps just the scripts world
  generation loads, about 1,100 of the 10,000.
  `ds-worldgen trace` finds them by generating every world type; loading
  doesn't depend on the seed.
- **Stripped bytecode:** those scripts are compiled to Lua bytecode for the
  32-bit WebAssembly build, with debug information removed (no comments,
  names or line numbers).
- **Art:** the minimap icons, display names and logos are included.

The site never enters the repository's history. It goes to a temporary
release, the Pages workflow deploys it from there, and the release is
deleted. Use `-NoBuild` to redeploy the existing `dist-public/`.

The page must be served over HTTP (it can't run as a `file://` page,
because browsers don't start Web Workers from local files). It was tested in
Edge, and should work in any current browser.

Two related pieces:

- The page in `web/` is the same page built without game data. That copy
  is what the Pages workflow (`.github/workflows/pages.yml`) publishes, and
  it only explains how to build your own.
- `ds-worldgen unpack --bundle FILE --output DIR` lays a bundle out as
  installs, which `generate` and `search` accept like the real ones.

Speed in the browser:

| World type | Time per world |
|---|---|
| Don't Starve survival or caves | about 1 s |
| Shipwrecked | 2–6 s |
| Hamlet | 10–25 s |
| Don't Starve Together | 5–12 s |

## Using the command line

```sh
cargo build --release      # target/release/ds-worldgen
```

```powershell
$ds  = "C:/Program Files (x86)/Steam/steamapps/common/dont_starve"
$dst = "C:/Program Files (x86)/Steam/steamapps/common/Don't Starve Together"

# One world: the summary (default) or the game's savedata
ds-worldgen generate --game $ds --seed 1001 --dlc rog
ds-worldgen generate --game $ds --seed 1001 --format savedata --output 1001.lua
ds-worldgen generate --game $ds --seed 2002 --level cave --current-level 2   # ruins
ds-worldgen generate --dst $dst --seed 123456                                 # DST forest
ds-worldgen generate --dst $dst --seed 123456 --preset DST_CAVE               # DST caves

# Search a seed range on all cores; matching summaries as JSON lines
ds-worldgen search --game $ds --seeds 1..10000 --dlc rog `
    --where 'prefabs.walrus_camp >= 2 && nearest.pigking.distance < 250' --output hits.jsonl
ds-worldgen search --dst $dst --seeds 1..5000 --where 'nearest.pigking.distance < 200'

# Summarise a savedata file
ds-worldgen summarise --savedata 1001.lua --game $ds
```

World options:

- **Don't Starve.**
  - `--dlc base|rog|sw|hamlet` (default `rog`).
  - `--level survival|shipwrecked|volcano|porkland|cave|adventure` (default:
    the DLC's normal world).
  - `--current-level N` (caves 1, ruins 2; adventure chapter 1–5).
  - `--choices '{ … }'`: world settings as a Lua table, e.g.
    `--choices '{ preset = "SURVIVAL_DEFAULT_PLUS" }'`.
- **Don't Starve Together.**
  - `--preset SURVIVAL_TOGETHER|DST_CAVE`: the default settings, as the game
    produces them.
  - `--world-size small|medium|large|huge`: the game's World Size setting
    (default `large`, which the game stores as `default`).
  - `--gen-parameters FILE`: any world-generation parameter string the game
    passes to its generator.
  - `--platform windows|linux`: the build that generates the world (default
    `windows`: the game, or a Windows dedicated server; `linux`: a Linux
    dedicated server).

A Don't Starve seed is the number stored as `meta.seed` in a save. The same
seed gives a different world for a different DLC, world type or settings,
exactly as in the game.

## Filters

The command line's `--where` takes conditions in this language (the web page
builds the same kinds of conditions with its filter picker):

- **Conditions:** `path op value`, `has path` or `!has path`, joined with
  `&&` and `||` (`&&` binds tighter).
- **Paths:** the dotted summary fields shown above, e.g. `prefabs.beefalo`,
  `nearest.pigking.distance`, `map.land_tiles`, `tiles.MARSH`.
- **Operators:** `>= <= > < == !=`, plus `contains` for lists:
  `task_names contains "Befriend the pigs"`.
- **Missing values:** `prefabs.*` counts are 0 when the prefab is absent;
  any other missing path makes its condition false.

```text
nearest.pigking.distance < 150
prefabs.walrus_camp >= 3 && nearest.resurrectionstone.distance < 100
has nearest.dragonfly_spawner && prefabs.beefalo >= 20
```

## Accuracy

Every world type was checked by generating worlds in the games and in
`dsworldgen` from the same seeds. Each comparison is of the complete savedata,
byte for byte. The only fields left out are the record of enabled mods and
DST's per-session identifier, which comes from a timer rather than the seed.
The native and WebAssembly builds both match on every world below:

| Game / world type | Seeds compared | Result |
|---|---|---|
| DS base game, Reign of Giants (survival) | 24 | identical |
| DS Shipwrecked (survival, shipwrecked, volcano) | 19 | identical |
| DS Hamlet (forest, porkland) | 6 | identical |
| DS adventure chapters 1–3 | 4 | identical |
| DS caves and ruins | 12 | identical |
| DST forest | 6 | identical |
| DST caves | 3 | identical |
| DST forest, Linux dedicated server (incl. small and huge) | 8 | identical |
| DST caves, Linux dedicated server (incl. huge) | 3 | identical |

Seeds the game itself fails to generate fail the same way here.

**Windows and Linux make different DST worlds from the same seed.** Both
builds draw from the same random stream, but Klei's `pcg32_rand` reduces each
number modulo the C runtime's `RAND_MAX`, which is 32767 on Windows and
2147483647 on Linux. Every shuffle the native generator does (picking where
set pieces go, and the points that rooms are filled from) comes out
differently, and so does the world from the first set piece on. The Linux
build also prints numbers the glibc way (`1e-05`, rounded once). Choose the
host on the page, or `--platform linux` on the command line. Worlds hosted
from a Mac have not been compared.

The reference builds are DS 578406 (its final Steam build) and DST 756039.
Script changes in a DST update are picked up automatically, because the
scripts are read from your install. Changes to DST's native world generator
may break the match.

## How it works

The generator does three things to reproduce the game exactly:

1. **Runs the game's own scripts.** It rebuilds the game engine's
   world-generation Lua environment and runs the game's own
   `worldgen_main.lua`, unmodified, on an embedded Lua 5.1.4.
2. **Reimplements the engine in Rust.** The part of the engine that the
   scripts call, `WorldSim` (map layout, Voronoi regions, tiles,
   connectivity, roads, mazes, the cave automaton), is rewritten in Rust to
   reproduce the game's results exactly, down to float precision and every
   random number drawn.
3. **Matches the game's numerics on every platform.**
   - Random numbers: the Visual C++ 2008 `rand()` stream for DS, PCG32 for
     DST (reduced by the Windows or Linux `RAND_MAX`, see Accuracy).
   - Number formatting: the Visual C++ 2008 runtime's, reimplemented, for
     example `1e+020` and its rounding rules; glibc's for DST on Linux.
   - Maths functions: musl's (`vendor/musl-math`), which give the games'
     values.

   The native and web builds therefore compute identical results.

Limits:

- **Untested maze code:** maze types 1 and 2 are not implemented, and no
  shipped script uses them. A few maze and connection modes never occur in
  any checked world, so they are implemented but untested.
- **Vanilla worlds only:** mods that change world generation are not loaded.
- **Platforms:** the command line was built and checked on Windows x64. The
  code is portable C and Rust, but other platforms are untested. The web
  page was tested in Edge.

## Building

- **Command line:** Rust (stable) and a C/C++ compiler. On Windows that's the
  Visual Studio C++ build tools; elsewhere, clang or gcc. Then run
  `cargo build --release`.
- **Web module:** the [Emscripten SDK](https://emscripten.org) and
  `rustup target add wasm32-unknown-emscripten`, then
  `./web/build.ps1 -Emsdk <path to emsdk>`. This writes
  `web/ds-worldgen-web.js` and `web/ds_worldgen_web.wasm`.

## Community

Questions, bug reports and seed finds are welcome on the
[MapsNotIncluded Discord](https://discord.gg/3vhCpp6PNq).
For Oxygen Not Included seeds, see [MapsNotIncluded](https://mapsnotincluded.org).

## Licence

GNU Affero General Public License v3.0 or later (see `LICENSE`).

Vendored code keeps its own licence:

- Lua 5.1.4 (`vendor/lua-5.1.4`): MIT;
- Boost.Polygon headers (`vendor/boost_1_53_voronoi`): Boost Software
  License;
- musl maths (`vendor/musl-math`): MIT.

Don't Starve and Don't Starve Together are trademarks of Klei Entertainment.
This project is not affiliated with or endorsed by Klei.
