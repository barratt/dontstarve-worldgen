//! Native stand-ins for the game's hottest worldgen helper functions.
//!
//! A handful of small Lua functions in the game scripts (the waterline
//! fills of DS's DLC water.lua and DST's ocean_gen.lua, the IsCloseTo* ring
//! scans, simplexnoise2d, the water layout searches, Hamlet's city and
//! border scans) run over every tile of the map: tens of millions of
//! WorldSim calls per world. When a script being loaded contains one of
//! them exactly (same header line, length and hash), it is redirected to
//! native code (worldsim::helpers, the Dsw* WorldSim methods, shim.c),
//! which reads and writes the same tiles and tables in the same order and
//! leaves every math.random call in the script. Anything else runs
//! unchanged. Edits keep the original line count, so line numbers in errors
//! are unaffected. The originals are identified by fingerprint; only their
//! first lines, and the single lines an edit rewrites, appear here.
//!
//! The table is generated from the installed scripts by a development
//! script (not part of the export); a game update that changes a function
//! simply turns its patch off.

use std::borrow::Cow;

struct Patch {
    /// The function's first line.
    header: &'static str,
    /// Byte length and FNV-1a hash of the whole function, header through
    /// the closing `end`, with `\n` line ends.
    len: usize,
    hash: u64,
    edit: Edit,
    /// Code added on the line of the function's closing `end` (after it):
    /// a marker that the function is the one recognised.
    after: &'static str,
}

enum Edit {
    /// Replace the body (between the header and `end`).
    Body(&'static str),
    /// Replace whole lines of the function (each occurs once in it), to
    /// add a fast path that skips the original code it guards.
    Lines(&'static [(&'static str, &'static str)]),
}

const PATCHES: &[Patch] = &[
    // ds water.lua
    Patch {
        header: "local function fillGroundType(width, height, x, y, offset, depth, ground)",
        len: 714,
        hash: 0x6a146e54c3513828,
        edit: Edit::Body("\tWorldSim:DswFillGroundType(width, height, x, y, offset, depth, ground, GROUND.OCEAN_CORAL or -1, GROUND.MANGROVE or -1)"),
        after: "local dsw_fill = true",
    },
    // ds water.lua
    Patch {
        header: "local function placeGroundType(width, height, x, y, offx, offy, depth, ground)",
        len: 384,
        hash: 0xffcc261c616587e4,
        edit: Edit::Body("\treturn WorldSim:DswPlaceGroundType(width, height, x, y, offx, offy, depth, ground, GROUND.OCEAN_CORAL or -1, GROUND.MANGROVE or -1)"),
        after: "local dsw_place = true",
    },
    // ds water.lua
    Patch {
        header: "local function placeFilledGroundType(width, height, x, y, offx, offy, depth, ground, fillOffset, fillDepth)",
        len: 700,
        hash: 0x04efc2899ca015e2,
        edit: Edit::Body("\treturn WorldSim:DswPlaceFilledGroundType(width, height, x, y, offx, offy, depth, ground, fillOffset, fillDepth, GROUND.OCEAN_CORAL or -1, GROUND.MANGROVE or -1)"),
        after: "local dsw_pfill = true",
    },
    // ds water.lua
    Patch {
        header: "local function squareFill(width, height, x, y, radius, ground)",
        len: 343,
        hash: 0xeacebb040131b729,
        edit: Edit::Body("\tWorldSim:DswSquareFill(width, height, x, y, radius, ground, GROUND.OCEAN_CORAL or -1, GROUND.MANGROVE or -1)"),
        after: "local dsw_sq = true",
    },
    // ds/dst water.lua, ocean_gen.lua
    Patch {
        header: "local function simplexnoise2d(x, y, octaves, persistence)",
        len: 310,
        hash: 0xf9281adb285468f8,
        edit: Edit::Body("\treturn WorldSim:DswSimplexNoise2d(x, y, octaves, persistence)"),
        after: "local dsw_simplex_native = true",
    },
    // ds spawnutil.lua
    Patch {
        header: "function IsCloseToWater(x, y, radius)",
        len: 421,
        hash: 0x1eb4f81f74ed1789,
        edit: Edit::Body("\treturn WorldSim:DswIsCloseToWater(x, y, radius)"),
        after: "DSW_ICW = IsCloseToWater",
    },
    // ds spawnutil.lua
    Patch {
        header: "function IsCloseToLand(x, y, radius)",
        len: 416,
        hash: 0x19817230bd7f6e23,
        edit: Edit::Body("\treturn WorldSim:DswIsCloseToLand(x, y, radius)"),
        after: "",
    },
    // ds spawnutil.lua
    Patch {
        header: "function IsSurroundedByWater(x, y, radius)",
        len: 443,
        hash: 0x5ca9d00c092e167d,
        edit: Edit::Body("\treturn WorldSim:DswIsSurroundedByWater(x, y, radius)"),
        after: "",
    },
    // ds spawnutil.lua
    Patch {
        header: "function IsSurroundedByWaterOrInvalid(x, y, radius)",
        len: 452,
        hash: 0xb64344489f3df989,
        edit: Edit::Body("\treturn WorldSim:DswIsSurroundedByWaterOrInvalid(x, y, radius, GROUND.INVALID or 1e300)"),
        after: "DSW_ISWOI = IsSurroundedByWaterOrInvalid",
    },
    // ds water.lua
    Patch {
        header: "function RemoveSingleWaterTile(width, height)",
        len: 610,
        hash: 0xecd2ddd664587b04,
        edit: Edit::Body("\tprint(\"Removing single tiles...\")\n\tWorldSim:DswRemoveSingleWaterTile(width, height)"),
        after: "",
    },
    // ds spawnutil.lua
    Patch {
        header: "function IsCloseToTileType(x, y, radius, tile)",
        len: 390,
        hash: 0xc05f407edc774662,
        edit: Edit::Body("\treturn WorldSim:DswIsCloseToTileType(x, y, radius, tile)"),
        after: "DSW_ICTT = IsCloseToTileType",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function IsSurroundedByWaterOrInvalid(x, y, radius)",
        len: 446,
        hash: 0x271b4db181024c38,
        edit: Edit::Body("\treturn world:DswOceanIsSurroundedByWaterOrInvalid(x, y, radius, LEGACY_WORLD_TILES_OCEAN_START, LEGACY_WORLD_TILES_OCEAN_END, WORLD_TILES_OCEAN_START, WORLD_TILES_OCEAN_END, WORLD_TILES.IMPASSABLE, LEGACY_WORLD_TILES_IMPASSABLE_START, LEGACY_WORLD_TILES_IMPASSABLE_END, WORLD_TILES_IMPASSABLE_START, WORLD_TILES_IMPASSABLE_END, WORLD_TILES.INVALID)"),
        after: "local dsw_swoi = true",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function IsCloseToWater(x, y, radius)",
        len: 395,
        hash: 0x3d0d7bfc8255a8e6,
        edit: Edit::Body("\treturn world:DswOceanIsCloseToGroup(x, y, radius, LEGACY_WORLD_TILES_OCEAN_START, LEGACY_WORLD_TILES_OCEAN_END, WORLD_TILES_OCEAN_START, WORLD_TILES_OCEAN_END)"),
        after: "local dsw_ctw = true",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function IsCloseToLand(x, y, radius)",
        len: 390,
        hash: 0xdab9d4a8c6c5fc72,
        edit: Edit::Body("\treturn world:DswOceanIsCloseToGroup(x, y, radius, LEGACY_WORLD_TILES_LAND_START, LEGACY_WORLD_TILES_LAND_END, WORLD_TILES_LAND_START, WORLD_TILES_LAND_END)"),
        after: "",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function IsCloseToImpassable(x, y, radius)",
        len: 488,
        hash: 0xa767e0eb36f9e714,
        edit: Edit::Body("\treturn world:DswOceanIsCloseToImpassable(x, y, radius, WORLD_TILES.IMPASSABLE, LEGACY_WORLD_TILES_IMPASSABLE_START, LEGACY_WORLD_TILES_IMPASSABLE_END, WORLD_TILES_IMPASSABLE_START, WORLD_TILES_IMPASSABLE_END)"),
        after: "local dsw_cti = true",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function fillGroundType(width, height, x, y, offset, depth, ground)",
        len: 586,
        hash: 0x8711b3512cb2be84,
        edit: Edit::Body("\tworld:DswOceanFillGroundType(width, height, x, y, offset, depth, ground, LEGACY_WORLD_TILES_LAND_START, LEGACY_WORLD_TILES_LAND_END, WORLD_TILES_LAND_START, WORLD_TILES_LAND_END)"),
        after: "local dsw_fill = true",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function placeGroundType(width, height, x, y, offx, offy, depth, ground)",
        len: 345,
        hash: 0x5cbc6cd4ec2ed762,
        edit: Edit::Body("\treturn world:DswOceanPlaceGroundType(width, height, x, y, offx, offy, depth, ground, LEGACY_WORLD_TILES_LAND_START, LEGACY_WORLD_TILES_LAND_END, WORLD_TILES_LAND_START, WORLD_TILES_LAND_END)"),
        after: "",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function placeFilledGroundType(width, height, x, y, offx, offy, depth, ground, fillOffset, fillDepth)",
        len: 697,
        hash: 0x28551c754b1c3a85,
        edit: Edit::Body("\treturn world:DswOceanPlaceFilledGroundType(width, height, x, y, offx, offy, depth, ground, fillOffset, fillDepth, LEGACY_WORLD_TILES_LAND_START, LEGACY_WORLD_TILES_LAND_END, WORLD_TILES_LAND_START, WORLD_TILES_LAND_END)"),
        after: "",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function squareFill(width, height, x, y, radius, ground)",
        len: 337,
        hash: 0xa0e594f71ac094a3,
        edit: Edit::Body("\tworld:DswOceanSquareFill(width, height, x, y, radius, ground, LEGACY_WORLD_TILES_LAND_START, LEGACY_WORLD_TILES_LAND_END, WORLD_TILES_LAND_START, WORLD_TILES_LAND_END)"),
        after: "local dsw_sq = true",
    },
    // ds water.lua
    Patch {
        header: "function AddShoreline(width, height)",
        len: 523,
        hash: 0x9b97da7174e63919,
        edit: Edit::Lines(&[("function AddShoreline(width, height)", "function AddShoreline(width, height) if DSW_ISWOI and IsSurroundedByWaterOrInvalid == DSW_ISWOI then print(\"Adding shoreline...\") WorldSim:DswAddShoreline(width, height, GROUND.INVALID or 1e300, GROUND.MANGROVE or 1e300, GROUND.MANGROVE_SHORE or 0, GROUND.OCEAN_CORAL or 1e300, GROUND.OCEAN_CORAL_SHORE or 0, GROUND.OCEAN_SHORE or 0) return end")]),
        after: "",
    },
    // ds water.lua
    Patch {
        header: "local function PopulateWaterType(checkFn, spawnFn, entitiesOut, width, height, edge_dist, water_contents, world_gen_choices)",
        len: 3562,
        hash: 0x9a59911bdd3ca5ee,
        edit: Edit::Lines(&[("\t\t\tfor x = edge_dist, width - edge_dist - 1, 1 do", "\t\t\tlocal dsw_p = DSW_PRED[checkFn] if dsw_p == 4 then dsw_p = nil end local dsw_x = edge_dist - 1 while true do local x if dsw_p then x = WorldSim:DswNextTile(dsw_x + 1, y, width - edge_dist - 1, dsw_p, DSW_PREDV[checkFn] or 1e300) if not x then break end else x = dsw_x + 1 if x > width - edge_dist - 1 then break end end dsw_x = x"), ("\t\t\t\tif checkTile(x, y, checkFn) then", "\t\t\t\tif dsw_p or checkTile(x, y, checkFn) then")]),
        after: "",
    },
    // hamlet city_builder.lua
    Patch {
        header: "local function clearground(pt) ",
        len: 825,
        hash: 0x67a8ca0b84da1eaf,
        edit: Edit::Lines(&[("local function clearground(pt) ", "local function clearground(pt) require(\"map/levels/porkland\") if DswClearGround(entities, REQUIRED_PREFABS, pt.x, pt.z, WIDTH, HEIGHT, TILE_SCALE, 6) then return end")]),
        after: "",
    },
    // hamlet border_finder.lua
    Patch {
        header: "function makeborder(entities, topology_save, worldsim, map_width, map_height, prefab, valid_tile_types, chance)",
        len: 1796,
        hash: 0x7be5415af445df9f,
        edit: Edit::Lines(&[("                local totalnear = 0", "                local totalnear = 0 local dsw_n = DswCountNear(borders, x, z, 30*30) if dsw_n then totalnear = dsw_n end do local borders = dsw_n and DSW_EMPTY or borders"), ("                if valid and (math.random() < chance or totalnear < 2) and totalnear < 6 then                   ", "end                 if valid and (math.random() < chance or totalnear < 2) and totalnear < 6 then                   "), ("        for z= -(HEIGHT/2)*TILE_SCALE,(HEIGHT/2)*TILE_SCALE,TILE_SCALE do", "        local dsw_on = dsw_tt and worldsim == WorldSim local dsw_z = -(HEIGHT/2)*TILE_SCALE - TILE_SCALE while true do local z if dsw_on then dsw_z = WorldSim:DswBorderNext(dsw_z, (HEIGHT/2)*TILE_SCALE, TILE_SCALE, x, WIDTH, HEIGHT, valid_tile_types) if not dsw_z then break end else dsw_z = dsw_z + TILE_SCALE if not (dsw_z <= (HEIGHT/2)*TILE_SCALE) then break end end z = dsw_z"), ("            if testtile(worldsim,valid_tile_types, tilex,tilez) then", "            if dsw_on or testtile(worldsim,valid_tile_types, tilex,tilez) then"), ("                local valid = false", "                local valid = dsw_on"), ("                if not testtile(worldsim,valid_tile_types, tilex+1,tilez) then", "                if not valid and not testtile(worldsim,valid_tile_types, tilex+1,tilez) then")]),
        after: "",
    },
    // ds water.lua
    Patch {
        header: "local function checkTile(x, y, checkFn)",
        len: 147,
        hash: 0x3b97b7409067149d,
        edit: Edit::Lines(&[("local function checkTile(x, y, checkFn)", "local function checkTile(x, y, checkFn) local dsw_p = DSW_PRED[checkFn] if dsw_p and dsw_p ~= 4 then return WorldSim:DswCheckTile(x, y, dsw_p, DSW_PREDV[checkFn] or 1e300) end")]),
        after: "",
    },
    // hamlet network.lua
    Patch {
        header: "function Graph:GenerateBermudaTriangleMarkers(entities, width, height)",
        len: 1075,
        hash: 0xd5de783ee28e57b5,
        edit: Edit::Lines(&[("\tfor i = 1, numTriangles, 1 do ", "\tfor i = 1, numTriangles, 1 do  DswPred(4, function() local r = {GROUND.OCEAN_DEEP, minDistance} for k = 1, #points do r[#r + 1] = points[k].x r[#r + 1] = points[k].y end return r end, checkTriangle)")]),
        after: "",
    },
    // ds water.lua
    Patch {
        header: "local function findLayoutPositions(radius, edge_dist, checkFn, count)",
        len: 1937,
        hash: 0xbcf740ee04c4d3f8,
        edit: Edit::Lines(&[("local function findLayoutPositions(radius, edge_dist, checkFn, count)", "local function findLayoutPositions(radius, edge_dist, checkFn, count) local dsw_p = DSW_PRED[checkFn] if dsw_p then local dsw_v = DSW_PREDV[checkFn] if dsw_p == 4 then dsw_v = dsw_v() end local size = 2 * radius edge_dist = edge_dist or 0 local width, height = WorldSim:GetWorldSize() local adj_width, adj_height = width - 2 * edge_dist - size, height - 2 * edge_dist - size local start_x, start_y = math.random(0, adj_width), math.random(0, adj_height) local r = WorldSim:DswFindLayoutPositions(dsw_p, dsw_v, size, edge_dist, adj_width, adj_height, start_x, start_y, count or -1) local positions = {} for k = 1, #r, 4 do table.insert(positions, {x = r[k], y = r[k + 1], x2 = r[k + 2], y2 = r[k + 3], size = size}) end return positions end")]),
        after: "",
    },
    // ds water.lua
    Patch {
        header: "function GetRandomWaterPoints(checkFn, width, height, edge_dist, needed)",
        len: 1248,
        hash: 0x84d4c11fb1d61291,
        edit: Edit::Lines(&[("\tlocal get_points = function(points, checkFn, width, height, edge_dist, needed, inc)", "\tlocal get_points = function(points, checkFn, width, height, edge_dist, needed, inc) local dsw_p = DSW_PRED[checkFn] if dsw_p then local dsw_v = DSW_PREDV[checkFn] if dsw_p == 4 then dsw_v = dsw_v() end local adj_width, adj_height = width - 2 * edge_dist, height - 2 * edge_dist local start_x, start_y = math.random(0, adj_width), math.random(0, adj_height) local r = WorldSim:DswGetPoints(dsw_p, dsw_v, adj_width, adj_height, start_x, start_y, edge_dist, needed - #points, inc) for k = 1, #r, 2 do table.insert(points, {x = r[k], y = r[k + 1]}) end return end")]),
        after: "",
    },
    // hamlet border_finder.lua
    Patch {
        header: "local function testtile(WorldSim, valid_tile_types, x,y)",
        len: 417,
        hash: 0x83a5a19aaa44a23a,
        edit: Edit::Lines(&[]),
        after: "local dsw_tt = true",
    },
    // ds water.lua
    Patch {
        header: "local function is_waterlined(ground)",
        len: 133,
        hash: 0xc2435bdbfea953e9,
        edit: Edit::Lines(&[]),
        after: "local dsw_lined = true",
    },
    // ds water.lua
    Patch {
        header: "local function placeWaterline(width, height, x, y, offx, offy, depthShallow, depthMed)",
        len: 271,
        hash: 0x4f9134808102636e,
        edit: Edit::Lines(&[]),
        after: "local dsw_wl = true",
    },
    // ds water.lua
    Patch {
        header: "local function placeWaterlineFilled(width, height, x, y, offx, offy, depthShallow, depthMed, fillOffset, fillDepth)",
        len: 431,
        hash: 0xdb09a9463061cc77,
        edit: Edit::Lines(&[]),
        after: "local dsw_wlf = true",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function is_waterlined(tile)",
        len: 157,
        hash: 0xfcfcb88621dfe39c,
        edit: Edit::Lines(&[]),
        after: "local dsw_lined = true",
    },
    // ds mathutil.lua
    Patch {
        header: "function math.clamp(num, min, max)",
        len: 122,
        hash: 0x09db3b5fe9e975a8,
        edit: Edit::Lines(&[]),
        after: "DSW_CLAMPS[math.clamp] = true",
    },
    // ds util.lua
    Patch {
        header: "function math.clamp(input, min_val, max_val)",
        len: 181,
        hash: 0xec74cc5c59fe2a8a,
        edit: Edit::Lines(&[]),
        after: "DSW_CLAMPS[math.clamp] = true",
    },
    // dst mathutil.lua
    Patch {
        header: "function math.clamp(num, min, max)",
        len: 99,
        hash: 0x43d6aa42f093c457,
        edit: Edit::Lines(&[]),
        after: "DSW_CLAMPS[math.clamp] = true",
    },
    // ds water.lua
    Patch {
        header: "function ConvertImpassibleToWater(width, height, data)",
        len: 10041,
        hash: 0x6128972eebfc0035,
        edit: Edit::Lines(&[("\t\tlocal init_level_medium = data.init_level_medium or 0.5", "\t\tlocal init_level_medium = data.init_level_medium or 0.5 if dsw_simplex_native then WorldSim:DswNoisePass(width, height, GROUND.IMPASSABLE, 3, noise_scale_coral, offx_coral, offy_coral, noise_octave_coral, noise_persistence_coral, init_level_coral, GROUND.OCEAN_CORAL, noise_scale_water, offx_water, offy_water, noise_octave_water, noise_persistence_water, init_level_medium, GROUND.OCEAN_MEDIUM, noise_scale_grave, offx_grave, offy_grave, noise_octave_grave, noise_persistence_grave, init_level_grave, GROUND.OCEAN_SHIPGRAVEYARD, GROUND.OCEAN_DEEP) return end"), ("\t\tlocal final_level_grave = data.final_level_grave or 0.3", "\t\tlocal final_level_grave = data.final_level_grave or 0.3 if DSW_CLAMPS[math.clamp] then WorldSim:DswBlendPass(width, height, TUNING.MAPWRAPPER_WARN_RANGE + 1, TUNING.MAPWRAPPER_WARN_RANGE + 5, final_level_shallow, final_level_medium, final_level_coral, final_level_grave, GROUND.MANGROVE or 1e300, GROUND.IMPASSABLE, GROUND.OCEAN_CORAL or 1e300, GROUND.OCEAN_SHALLOW, GROUND.OCEAN_MEDIUM, GROUND.OCEAN_SHIPGRAVEYARD, GROUND.OCEAN_DEEP) return end"), ("\t\tprint(\"  Waterline...\")", "\t\tprint(\"  Waterline...\") if dsw_lined and dsw_place and dsw_wl then WorldSim:DswWaterlinePass(0, GROUND.OCEAN_CORAL or -1, GROUND.MANGROVE or -1, 0, 0, width, height, depthShallow, depthMed, GROUND.OCEAN_SHALLOW, GROUND.OCEAN_MEDIUM) return end"), ("\t\tprint(\"  Filled Waterline...\")", "\t\tprint(\"  Filled Waterline...\") if dsw_lined and dsw_place and dsw_pfill and dsw_fill and dsw_wl and dsw_wlf then WorldSim:DswWaterlinePass(0, GROUND.OCEAN_CORAL or -1, GROUND.MANGROVE or -1, 0, 0, width, height, depthShallow, depthMed, GROUND.OCEAN_SHALLOW, GROUND.OCEAN_MEDIUM, fillOffset, fillDepth) return end"), ("\t\tprint(\"  Ground fill...\")", "\t\tprint(\"  Ground fill...\") if dsw_lined and dsw_fill then WorldSim:DswGroundFillPass(0, GROUND.OCEAN_CORAL or -1, GROUND.MANGROVE or -1, 0, 0, width, height, fillTile, fillOffset, fillDepth) return end"), ("\t\tprint(\"  Square fill...\")", "\t\tprint(\"  Square fill...\") if dsw_lined and dsw_sq and DSW_ICTT and IsCloseToTileType == DSW_ICTT and IsCloseToWater == DSW_ICW then WorldSim:DswSquareFillPass(0, GROUND.OCEAN_CORAL or -1, GROUND.MANGROVE or -1, 0, 0, width, height, shallowRadius, GROUND.OCEAN_SHALLOW, GROUND.IMPASSABLE) return end")]),
        after: "",
    },
    // dst ocean_gen.lua
    Patch {
        header: "function Ocean_ConvertImpassibleToWater(width, height, data)",
        len: 8291,
        hash: 0x1b86c9618fa7cfde,
        edit: Edit::Lines(&[("\t\tlocal init_level_medium = data.init_level_medium or 0.5", "\t\tlocal init_level_medium = data.init_level_medium or 0.5 if dsw_simplex_native then world:DswNoisePass(width, height, WORLD_TILES.IMPASSABLE, 2, noise_scale_water, offx_water, offy_water, noise_octave_water, noise_persistence_water, init_level_medium, WORLD_TILES.OCEAN_SWELL, noise_scale_grave, offx_grave, offy_grave, noise_octave_grave, noise_persistence_grave, init_level_grave, WORLD_TILES.OCEAN_HAZARDOUS, WORLD_TILES.OCEAN_ROUGH) return end"), ("\t\tlocal final_level_grave = data.final_level_grave or 0.3", "\t\tlocal final_level_grave = data.final_level_grave or 0.3 if DSW_CLAMPS[math.clamp] then world:DswOceanBlendPass(width, height, OCEAN_MAPWRAPPER_WARN_RANGE + 1, OCEAN_MAPWRAPPER_WARN_RANGE + 5, final_level_shallow, final_level_medium, final_level_coral, final_level_grave, LEGACY_WORLD_TILES_OCEAN_START, LEGACY_WORLD_TILES_OCEAN_END, WORLD_TILES_OCEAN_START, WORLD_TILES_OCEAN_END, WORLD_TILES.IMPASSABLE, WORLD_TILES.OCEAN_WATERLOG or 1e300, WORLD_TILES.OCEAN_BRINEPOOL or 1e300, WORLD_TILES.OCEAN_COASTAL, WORLD_TILES.OCEAN_SWELL, WORLD_TILES.OCEAN_HAZARDOUS, WORLD_TILES.OCEAN_ROUGH) return end"), ("\t\tprint(\"[Ocean]  Ground fill...\")", "\t\tprint(\"[Ocean]  Ground fill...\") if dsw_lined and dsw_fill then world:DswGroundFillPass(1, LEGACY_WORLD_TILES_LAND_START, LEGACY_WORLD_TILES_LAND_END, WORLD_TILES_LAND_START, WORLD_TILES_LAND_END, width, height, fillTile, fillOffset, fillDepth) return end"), ("\t\tprint(\"[Ocean]  Square fill...\")", "\t\tprint(\"[Ocean]  Square fill...\") if dsw_lined and dsw_sq and dsw_cti and dsw_ctw then world:DswSquareFillPass(1, LEGACY_WORLD_TILES_LAND_START, LEGACY_WORLD_TILES_LAND_END, WORLD_TILES_LAND_START, WORLD_TILES_LAND_END, width, height, shallowRadius, WORLD_TILES.OCEAN_COASTAL, WORLD_TILES.IMPASSABLE, LEGACY_WORLD_TILES_IMPASSABLE_START, LEGACY_WORLD_TILES_IMPASSABLE_END, WORLD_TILES_IMPASSABLE_START, WORLD_TILES_IMPASSABLE_END, LEGACY_WORLD_TILES_OCEAN_START, LEGACY_WORLD_TILES_OCEAN_END, WORLD_TILES_OCEAN_START, WORLD_TILES_OCEAN_END) return end")]),
        after: "",
    },
    // hamlet bunch_spawner.lua
    Patch {
        header: "local function findEntsInRange(x,z,range) ",
        len: 340,
        hash: 0xfae26b14835798ec,
        edit: Edit::Lines(&[("local function findEntsInRange(x,z,range) ", "local function findEntsInRange(x,z,range) local dsw_e = DswEntsInRange(bunch, x, z, range*range) if dsw_e then return dsw_e end")]),
        after: "",
    },
    // hamlet bunch_spawner.lua
    Patch {
        header: "local function checkforblockingitems(x,z,range)",
        len: 577,
        hash: 0x84423da0a7add85a,
        edit: Edit::Lines(&[("local function checkforblockingitems(x,z,range)", "local function checkforblockingitems(x,z,range) local dsw_b, dsw_m = DswAnyNear(entities, BUNCH_BLOCKERS, x, z, 4*4) if dsw_b ~= nil then if dsw_m then for _, p in ipairs(dsw_m) do print(\">>> BUNCH SPAWN ERROR?\",p) end end return not dsw_b end")]),
        after: "",
    },
    // hamlet city_builder.lua
    Patch {
        header: "local function spawnSetPiece(setpiece_string,pt, city)",
        len: 4520,
        hash: 0xd55f9fb520e077fa,
        edit: Edit::Lines(&[("    local radius = math.max(#setpiece.ground,#setpiece.ground[1])/2 * 1.4", "    local radius = math.max(#setpiece.ground,#setpiece.ground[1])/2 * 1.4 local dsw_cleared = DswClearGround(entities, DSW_EMPTY, pt.x, pt.z, WIDTH, HEIGHT, TILE_SCALE, radius)"), ("    for prefab,datalist in pairs(entities) do", "    for prefab,datalist in pairs(dsw_cleared and DSW_EMPTY or entities) do")]),
        after: "",
    },
    // hamlet city_builder.lua
    Patch {
        header: "local function testTile(pt,types)",
        len: 1199,
        hash: 0x19366f077b1df78f,
        edit: Edit::Lines(&[("local function testTile(pt,types)", "local function testTile(pt,types) local dsw_t = WorldSim:DswTestTile(pt.x, pt.z, types) if dsw_t ~= nil then return dsw_t end")]),
        after: "",
    },
    // ds dumper.lua
    Patch {
        header: "local function keys(t)",
        len: 334,
        hash: 0x2f2f08c0f6afb26c,
        edit: Edit::Lines(&[("local function keys(t)", "local function keys(t) local dsw_k = DswSortedKeys(t) if dsw_k then return dsw_k end")]),
        after: "",
    },
    // dst dumper.lua
    Patch {
        header: "local function keys(t)",
        len: 378,
        hash: 0x1985301bd4704274,
        edit: Edit::Lines(&[("local function keys(t)", "local function keys(t) local dsw_k = DswSortedKeys(t) if dsw_k then return dsw_k end")]),
        after: "",
    },
    // ds forest_map.lua (all DLCs)
    Patch {
        header: "local function pickspawnprefab(items_in, ground_type)",
        len: 1089,
        hash: 0x50598952822f0854,
        edit: Edit::Lines(&[("local function pickspawnprefab(items_in, ground_type)", "local function pickspawnprefab(items_in, ground_type) local dsw_ok, dsw_k = DswPickSpawnPrefab(items_in, ground_type, ground_type ~= nil and terrain.filter or nil) if dsw_ok then return dsw_k end")]),
        after: "",
    },
    // dst forest_map.lua
    Patch {
        header: "local function pickspawnprefab(items_in, ground_type)",
        len: 1078,
        hash: 0x089dd533a8c0db06,
        edit: Edit::Lines(&[("local function pickspawnprefab(items_in, ground_type)", "local function pickspawnprefab(items_in, ground_type) local dsw_ok, dsw_k = DswPickSpawnPrefab(items_in, ground_type, ground_type ~= nil and terrain.filter or nil) if dsw_ok then return dsw_k end")]),
        after: "",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function checkTile(x, y, populating_tile, ignore_reserverd)",
        len: 173,
        hash: 0x6d88374888ca4bfe,
        edit: Edit::Lines(&[]),
        after: "local dsw_ckt = true",
    },
    // dst ocean_gen.lua
    Patch {
        header: "function GetRandomWaterPoints(populating_tile, width, height, edge_dist, needed)",
        len: 1244,
        hash: 0x9a04c97d1573eb68,
        edit: Edit::Lines(&[("\tlocal get_points = function(points,  width, height, edge_dist, needed, inc)", "\tlocal get_points = function(points,  width, height, edge_dist, needed, inc) if dsw_ckt then local adj_width, adj_height = width - 2 * edge_dist, height - 2 * edge_dist local start_x, start_y = math.random(0, adj_width), math.random(0, adj_height) local r = world:DswOceanGetPoints(adj_width, adj_height, start_x, start_y, edge_dist, needed - #points, inc, populating_tile) for k = 1, #r, 2 do table.insert(points, {x = r[k], y = r[k + 1]}) end return end")]),
        after: "",
    },
    // dst ocean_gen.lua
    Patch {
        header: "function AddShoreline(width, height)",
        len: 387,
        hash: 0x684e161f76b168dd,
        edit: Edit::Lines(&[("function AddShoreline(width, height)", "function AddShoreline(width, height) if dsw_swoi then print(\"[Ocean]  Adding shoreline...\") world:DswOceanAddShoreline(width, height, LEGACY_WORLD_TILES_OCEAN_START, LEGACY_WORLD_TILES_OCEAN_END, WORLD_TILES_OCEAN_START, WORLD_TILES_OCEAN_END, WORLD_TILES.IMPASSABLE, LEGACY_WORLD_TILES_IMPASSABLE_START, LEGACY_WORLD_TILES_IMPASSABLE_END, WORLD_TILES_IMPASSABLE_START, WORLD_TILES_IMPASSABLE_END, WORLD_TILES.INVALID, WORLD_TILES.OCEAN_BRINEPOOL or 1e300, WORLD_TILES.OCEAN_COASTAL_SHORE) return end")]),
        after: "",
    },
    // dst ocean_gen.lua
    Patch {
        header: "local function PopulateWaterType(populating_tile, spawnFn, entitiesOut, width, height, edge_dist, water_contents, world_gen_choices, min_dist_from_land, encoded_topology)",
        len: 3060,
        hash: 0x2e26b08953653370,
        edit: Edit::Lines(&[("\t\t\tfor x = edge_dist, width - edge_dist - 1, 1 do", "\t\t\tlocal dsw_on = world == WorldSim local dsw_x = edge_dist - 1 while true do local x if dsw_on then x = WorldSim:DswNextTileEq(dsw_x + 1, y, width - edge_dist - 1, populating_tile) if not x then break end else x = dsw_x + 1 if x > width - edge_dist - 1 then break end end dsw_x = x"), ("\t\t\t\tif world:GetTile(x, y) == populating_tile then", "\t\t\t\tif dsw_on or world:GetTile(x, y) == populating_tile then")]),
        after: "",
    },
    // hamlet bunch_spawner.lua
    Patch {
        header: "local function round(x)",
        len: 116,
        hash: 0x48a981b709fcc278,
        edit: Edit::Lines(&[]),
        after: "local dsw_round = true",
    },
    // hamlet bunch_spawner.lua
    Patch {
        header: "local function placeitemoffgrids(x1,z1, range, prefab, valid_tile_types, water)",
        len: 1141,
        hash: 0x8f05d6a9dc3eaf8e,
        edit: Edit::Lines(&[("        local test = true", "        local dsw_t = dsw_round and DswOffgrid(ents, x, z) local test = true if dsw_t ~= nil then test = dsw_t end"), ("        for i,ent in ipairs(ents) do", "        for i,ent in ipairs(dsw_t ~= nil and DSW_EMPTY or ents) do")]),
        after: "",
    },
    // hamlet city_builder.lua
    Patch {
        header: "local function FindTempEnts(data,x,z,range,prefabs)",
        len: 688,
        hash: 0x7d617e362a89e0ad,
        edit: Edit::Lines(&[("local function FindTempEnts(data,x,z,range,prefabs)", "local function FindTempEnts(data,x,z,range,prefabs) local dsw_e = DswFindTempEnts(data, x, z, range, prefabs) if dsw_e then return dsw_e end")]),
        after: "",
    },
    // ds dumper.lua
    Patch {
        header: "function DataDumper(value, varname, fastmode, ident)",
        len: 6458,
        hash: 0x53469fc35ef07794,
        edit: Edit::Lines(&[("function DataDumper(value, varname, fastmode, ident)", "function DataDumper(value, varname, fastmode, ident) if not fastmode then local dsw_s = DswDataDumper(value, varname, ident) if dsw_s then return dsw_s end end")]),
        after: "",
    },
    // dst dumper.lua
    Patch {
        header: "function DataDumper(value, varname, fastmode, ident)",
        len: 7878,
        hash: 0x6f96e8ffefd3a1ae,
        edit: Edit::Lines(&[("    local sim = USE_SAVEBUFFER and (WorldSim or TheSim) or nil", "    local sim = USE_SAVEBUFFER and (WorldSim or TheSim) or nil if fastmode and sim then local dsw_s = DswDataDumperFast(value, varname) if dsw_s then sim:InitSaveString() sim:AppendSaveString(dsw_s) return sim:GetSaveString() end end")]),
        after: "",
    },
];

/// Tile predicates the water layout searches are given, tagged with what
/// they test (see PRELUDE) so the native searches can evaluate them:
/// (text, kind, value expression).
const PREDICATES: &[(&str, u8, &str)] = &[
    ("function(ground) return WorldSim:IsWater(ground) and not WorldSim:IsShore(ground) end", 2, "nil"),
    ("function(ground) return WorldSim:IsWater(ground) end", 1, "nil"),
];
/// `function(ground) return ground == GROUND.<NAME> end`, kind 3.
const EQ_PREDICATE: (&str, &str) = ("function(ground) return ground == GROUND.", " end");

/// Runs before the game scripts (after shim.c's ds_register_helpers, which
/// adds DswClearGround, DswCountNear, DswEntsInRange, DswAnyNear,
/// DswSortedKeys, DswPickSpawnPrefab, DswOffgrid, DswFindTempEnts,
/// DswDataDumper and DswDataDumperFast). DSW_PRED / DSW_PREDV map a tagged
/// predicate to its kind and value; weak keys keep them out of the way of
/// the garbage collector.
pub const PRELUDE: &str = r#"
DSW_PRED = setmetatable({}, {__mode = "k"})
DSW_PREDV = setmetatable({}, {__mode = "k"})
function DswPred(kind, value, f) DSW_PRED[f] = kind DSW_PREDV[f] = value return f end
DSW_EMPTY = {}
DSW_CLAMPS = {}
"#;

/// WorldSim methods the patched scripts call (see worldsim::Fast and the
/// Dsw* methods of the native WorldSims).
pub const HELPERS: &[&str] = &[
    "DswFillGroundType",
    "DswPlaceGroundType",
    "DswPlaceFilledGroundType",
    "DswSquareFill",
    "DswSimplexNoise2d",
    "DswIsCloseToWater",
    "DswIsCloseToLand",
    "DswIsCloseToTileType",
    "DswIsSurroundedByWater",
    "DswIsSurroundedByWaterOrInvalid",
    "DswCheckTile",
    "DswAddShoreline",
    "DswRemoveSingleWaterTile",
    "DswNextTile",
    "DswNoisePass",
    "DswBlendPass",
    "DswOceanBlendPass",
    "DswTestTile",
    "DswGroundFillPass",
    "DswSquareFillPass",
    "DswWaterlinePass",
    "DswBorderNext",
    "DswNextTileEq",
    "DswOceanAddShoreline",
    "DswOceanGetPoints",
    "DswFindLayoutPositions",
    "DswGetPoints",
    "DswOceanFillGroundType",
    "DswOceanPlaceGroundType",
    "DswOceanPlaceFilledGroundType",
    "DswOceanSquareFill",
    "DswOceanIsCloseToGroup",
    "DswOceanIsCloseToImpassable",
    "DswOceanIsSurroundedByWaterOrInvalid",
];

fn fnv1a(s: &[u8]) -> u64 {
    s.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn log(what: &str) {
    if std::env::var_os("DSW_PATCH_LOG").is_some() {
        eprintln!("lua_patch: {what}");
    }
}

/// The script with every matching helper redirected (see the module docs).
/// DSW_NO_LUA_PATCH disables it.
pub fn apply(source: &[u8]) -> Cow<'_, [u8]> {
    if source.first() == Some(&0x1b) || std::env::var_os("DSW_NO_LUA_PATCH").is_some() {
        return Cow::Borrowed(source);
    }
    // Each seed loads the same ~1100 scripts; remember the outcome by hash.
    let key = (source.len(), crate::lua::source_hash(source));
    if let Some(hit) = APPLIED.with(|c| c.borrow().get(&key).cloned()) {
        return match hit {
            Some(patched) => Cow::Owned(patched.to_vec()),
            None => Cow::Borrowed(source),
        };
    }
    let out = apply_uncached(source);
    APPLIED.with(|c| c.borrow_mut().insert(key, match &out {
        Cow::Owned(v) => Some(std::rc::Rc::from(v.as_slice())),
        Cow::Borrowed(_) => None,
    }));
    out
}

thread_local! {
    static APPLIED: std::cell::RefCell<std::collections::HashMap<(usize, u64), Option<std::rc::Rc<[u8]>>>> = Default::default();
}

fn apply_uncached(source: &[u8]) -> Cow<'_, [u8]> {
    let Ok(text) = std::str::from_utf8(source) else { return Cow::Borrowed(source) };
    if !PATCHES.iter().any(|p| text.contains(p.header)) && !text.contains("function(ground) return ") {
        return Cow::Borrowed(source);
    }
    // Work on \n line ends; a file with mixed line ends is left alone.
    let crlf = text.contains("\r\n");
    let mut out = if crlf { text.replace("\r\n", "\n") } else { text.to_string() };
    if crlf && out.contains('\r') {
        return Cow::Borrowed(source);
    }
    let mut changed = false;
    for p in PATCHES {
        let starts: Vec<usize> = out.match_indices(p.header).map(|(i, _)| i).collect();
        for at in starts.into_iter().rev() {
            let Some(original) = out.get(at..at + p.len) else { continue };
            if (at > 0 && !out[..at].ends_with('\n')) || !original.ends_with("\nend") || fnv1a(original.as_bytes()) != p.hash {
                continue;
            }
            let replacement = match p.edit {
                Edit::Body(body) => {
                    let mut r = format!("{}\n{}\nend {}", p.header, body, p.after);
                    let pad = original.matches('\n').count().saturating_sub(r.matches('\n').count());
                    r.push_str(&"\n".repeat(pad));
                    r
                }
                Edit::Lines(edits) => {
                    let mut lines: Vec<&str> = original.split('\n').collect();
                    for &(line, new) in edits {
                        if let Some(l) = lines.iter().position(|l| *l == line) {
                            lines[l] = new;
                        }
                    }
                    let last = format!("end {}", p.after);
                    if !p.after.is_empty() {
                        *lines.last_mut().unwrap() = &last;
                    }
                    lines.join("\n")
                }
            };
            log(p.header);
            out.replace_range(at..at + p.len, &replacement);
            changed = true;
        }
    }
    for &(pred, kind, value) in PREDICATES {
        if out.contains(pred) {
            log(pred);
            out = out.replace(pred, &format!("DswPred({kind}, {value}, {pred})"));
            changed = true;
        }
    }
    let (eq_start, eq_end) = EQ_PREDICATE;
    let mut from = 0;
    while let Some(i) = out[from..].find(eq_start).map(|i| i + from) {
        let name_at = i + eq_start.len();
        let name_len = out[name_at..].bytes().take_while(|b| b.is_ascii_uppercase() || *b == b'_' || b.is_ascii_digit()).count();
        let name = out[name_at..name_at + name_len].to_string();
        if name_len == 0 || !out[name_at + name_len..].starts_with(eq_end) {
            from = name_at;
            continue;
        }
        let end = name_at + name_len + eq_end.len();
        let wrapped = format!("DswPred(3, GROUND.{name}, {})", &out[i..end]);
        log(&out[i..end].to_string());
        from = i + wrapped.len();
        out.replace_range(i..end, &wrapped);
        changed = true;
    }
    if !changed {
        return Cow::Borrowed(source);
    }
    Cow::Owned(if crlf { out.replace('\n', "\r\n") } else { out }.into_bytes())
}
