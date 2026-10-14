//! The native side of world generation.
//!
//! In the game this is `WorldSimActual` (55 methods) plus the engine globals
//! `getminimumradius` and `perlin`. Implementations receive every call in
//! order; engine globals are named with a `$` prefix.

pub mod native;
pub mod native_dst;

use crate::value::Value;

pub trait WorldSim {
    fn call(&mut self, method: &str, args: &[Value]) -> Result<Vec<Value>, String>;

    /// Called once generation returns; lets an implementation verify that
    /// it was fully consumed.
    fn finish(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// All 55 methods of the game's WorldSim userdata, from the registration
/// table at 0x006F5864.
pub const METHODS: &[&str] = &[
    "AddChild", "AddEdgePoints", "AddExternalLink", "AddIslandRegionMapping", "AddLink",
    "BlendWater", "ClearNodeLinks", "ConvertToTileMap", "DeleteSpecifiedTiles", "DetectDisconnect",
    "DrawCellLine", "DrawRoads", "ForceConnectivity", "ForceDisconectedCells", "GenerateBlendedMap",
    "GenerateBlendedNoiseMap", "GenerateVoronoiMap", "GetChildrenForSite", "GetDebugString",
    "GetEncodedMap", "GetPointsForMetaMaze", "GetPointsForSite", "GetRandomPointsForSite", "GetRoad",
    "GetSite", "GetSiteArea", "GetSiteCentroid", "GetSitePolygon", "GetTile", "GetTileCenterPoint",
    "GetTileCoordsAtPoint", "GetVisualTileAtPosition", "GetWorldSize", "GetWormholes",
    "GetWormholesExtra", "IsLand", "IsShore", "IsTileReserved", "IsWater", "LuaPrint", "MoveToBack",
    "MoveToFront", "PointInSite", "RemoveLink", "ReserveSpace", "ReserveTile", "ResetAll", "RunCA",
    "RunMaze", "SeparateIslands", "SetImpassibleTileType", "SetNodeType", "SetRoadParameters",
    "SetSiteFlags", "SetTile", "SetTileMap", "SetWorldSize", "WorldPointInPoly",
];

/// Engine globals (not WorldSim methods) that worldgen scripts call.
pub const ENGINE_FUNCTIONS: &[&str] = &["getminimumradius", "perlin"];
