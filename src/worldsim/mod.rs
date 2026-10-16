//! The native side of world generation.
//!
//! In the game this is `WorldSimActual` (55 methods) plus the engine globals
//! `getminimumradius` and `perlin`. Implementations receive every call in
//! order; engine globals are named with a `$` prefix.

pub mod helpers;
pub mod native;
pub mod native_dst;

use crate::value::Value;

/// The per-tile methods worldgen scripts call millions of times. They get a
/// path without `Value` vectors or name lookup; results equal `call`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fast {
    GetTile,
    SetTile,
    IsTileReserved,
    IsWater,
    IsLand,
    IsShore,
    Perlin,
    // Native stand-ins for game Lua helpers (see lua_patch.rs).
    FillGroundType,
    PlaceGroundType,
    PlaceFilledGroundType,
    SquareFill,
    SimplexNoise2d,
    IsCloseToWater,
    IsCloseToLand,
    IsCloseToTileType,
    IsSurroundedByWater,
    IsSurroundedByWaterOrInvalid,
    CheckTile,
    AddShoreline,
    RemoveSingleWaterTile,
    NextTile,
    NoisePass,
    BlendPass,
    OceanBlendPass,
    GroundFillPass,
    SquareFillPass,
    WaterlinePass,
    NextTileEq,
    OceanAddShoreline,
    OceanFillGroundType,
    OceanPlaceGroundType,
    OceanPlaceFilledGroundType,
    OceanSquareFill,
    OceanIsCloseToGroup,
    OceanIsCloseToImpassable,
    OceanIsSurroundedByWaterOrInvalid,
}

impl Fast {
    pub fn of(method: &str) -> Option<Fast> {
        Some(match method {
            "GetTile" => Fast::GetTile,
            "SetTile" => Fast::SetTile,
            "IsTileReserved" => Fast::IsTileReserved,
            "IsWater" => Fast::IsWater,
            "IsLand" => Fast::IsLand,
            "IsShore" => Fast::IsShore,
            "$perlin" => Fast::Perlin,
            "DswFillGroundType" => Fast::FillGroundType,
            "DswPlaceGroundType" => Fast::PlaceGroundType,
            "DswPlaceFilledGroundType" => Fast::PlaceFilledGroundType,
            "DswSquareFill" => Fast::SquareFill,
            "DswSimplexNoise2d" => Fast::SimplexNoise2d,
            "DswIsCloseToWater" => Fast::IsCloseToWater,
            "DswIsCloseToLand" => Fast::IsCloseToLand,
            "DswIsCloseToTileType" => Fast::IsCloseToTileType,
            "DswIsSurroundedByWater" => Fast::IsSurroundedByWater,
            "DswIsSurroundedByWaterOrInvalid" => Fast::IsSurroundedByWaterOrInvalid,
            "DswCheckTile" => Fast::CheckTile,
            "DswAddShoreline" => Fast::AddShoreline,
            "DswRemoveSingleWaterTile" => Fast::RemoveSingleWaterTile,
            "DswNextTile" => Fast::NextTile,
            "DswNoisePass" => Fast::NoisePass,
            "DswBlendPass" => Fast::BlendPass,
            "DswOceanBlendPass" => Fast::OceanBlendPass,
            "DswGroundFillPass" => Fast::GroundFillPass,
            "DswSquareFillPass" => Fast::SquareFillPass,
            "DswWaterlinePass" => Fast::WaterlinePass,
            "DswNextTileEq" => Fast::NextTileEq,
            "DswOceanAddShoreline" => Fast::OceanAddShoreline,
            "DswOceanFillGroundType" => Fast::OceanFillGroundType,
            "DswOceanPlaceGroundType" => Fast::OceanPlaceGroundType,
            "DswOceanPlaceFilledGroundType" => Fast::OceanPlaceFilledGroundType,
            "DswOceanSquareFill" => Fast::OceanSquareFill,
            "DswOceanIsCloseToGroup" => Fast::OceanIsCloseToGroup,
            "DswOceanIsCloseToImpassable" => Fast::OceanIsCloseToImpassable,
            "DswOceanIsSurroundedByWaterOrInvalid" => Fast::OceanIsSurroundedByWaterOrInvalid,
            _ => return None,
        })
    }
}

/// The result of a `Fast` call.
pub enum FastOut {
    Num(f64),
    Bool(bool),
    Nothing,
    Num2(f64, f64),
}

pub trait WorldSim {
    fn call(&mut self, method: &str, args: &[Value]) -> Result<Vec<Value>, String>;

    /// A `Fast` method with all-number arguments (missing ones read as 0,
    /// as in `call`). `None` means "use `call`", which every implementation
    /// may answer; the default does.
    fn fast(&mut self, _method: Fast, _args: &[f64]) -> Option<FastOut> {
        None
    }

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
