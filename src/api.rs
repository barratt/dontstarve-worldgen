//! Generating worlds: the interface shared by the CLI and the web build.
//!
//! A [`Generator`] is prepared once per game install and world type, then
//! generates any number of seeds. Each world comes back as the game's
//! savedata, and [`Generator::summary`] turns it into the JSON summary.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};

use crate::env::{self, Config};
use crate::platform::Platform;
use crate::summary;
use crate::worldsim::native::Native;
use crate::worldsim::native_dst::NativeDst;

/// DST GEN_PARAMETERS captured from the game, one per preset.
pub const DST_PRESETS: &[(&str, &str)] = &[
    ("SURVIVAL_TOGETHER", include_str!("../data/dst/SURVIVAL_TOGETHER.gen_parameters.json")),
    ("DST_CAVE", include_str!("../data/dst/DST_CAVE.gen_parameters.json")),
];

/// Which game and world to generate.
#[derive(Clone, Debug, Default)]
pub struct WorldOptions {
    /// DS: base, rog, sw or hamlet (default rog). Ignored for DST.
    pub dlc: Option<String>,
    /// DS: survival, shipwrecked, volcano, porkland, cave or adventure
    /// (default: the DLC's normal world).
    pub level: Option<String>,
    /// DS: cave depth (1 caves, 2 ruins) or adventure level 1..5.
    pub current_level: Option<u32>,
    /// DS: world-gen choices as a Lua table constructor.
    pub choices: Option<String>,
    /// DST: a built-in preset name (default SURVIVAL_TOGETHER).
    pub preset: Option<String>,
    /// DST: a GEN_PARAMETERS string, instead of a preset.
    pub gen_parameters: Option<String>,
    /// DST: the World Size setting, small, medium, large or huge (default
    /// large). Replaces the preset's `world_size` override.
    pub world_size: Option<String>,
    /// DST: which build to reproduce, windows (the game, default) or linux
    /// (the Linux dedicated server), see [`crate::platform`].
    pub platform: Option<String>,
}

/// The game's World Size choices and the value each writes into the
/// worldgen overrides (customize.lua: "Large" is stored as "default").
pub const DST_WORLD_SIZES: &[(&str, &str)] = &[("small", "small"), ("medium", "medium"), ("large", "default"), ("huge", "huge")];

/// Sets `overrides.world_size` in a DST GEN_PARAMETERS string, keeping the
/// rest of it byte for byte.
fn with_world_size(gen_parameters: &str, size: &str) -> Result<String, String> {
    let value = DST_WORLD_SIZES
        .iter()
        .find(|(name, value)| name.eq_ignore_ascii_case(size) || value.eq_ignore_ascii_case(size))
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("unknown world size {size} (small, medium, large, huge)"))?;
    let key = "\"world_size\":\"";
    let start = gen_parameters.find(key).ok_or("GEN_PARAMETERS has no world_size override")? + key.len();
    let end = start + gen_parameters[start..].find('"').ok_or("unterminated world_size")?;
    Ok(format!("{}{value}{}", &gen_parameters[..start], &gen_parameters[end..]))
}

/// One generated world: the game's savedata, the wall time and (DST) the
/// tile id names.
pub struct World {
    pub seed: u32,
    pub savedata: Vec<u8>,
    pub ms: f64,
    dst_tile_names: Option<BTreeMap<u16, String>>,
    /// DS: GROUND names from the worldgen state.
    ds_ground_names: Option<BTreeMap<u8, String>>,
}

pub struct Generator {
    /// The dont_starve install, or the Don't Starve Together install.
    game: PathBuf,
    dst: bool,
    dlc: String,
    gen_parameters: String,
    ground_names: BTreeMap<u8, String>,
    platform: Platform,
}

/// The level type a new world of each DLC normally uses.
fn default_level(dlc: &str) -> &'static str {
    match dlc {
        "sw" => "shipwrecked",
        "hamlet" => "porkland",
        _ => "survival",
    }
}

/// GROUND names from the DLC's constants.lua (the overlay that the game uses).
pub fn ground_names(game: &Path, dlc: &str) -> BTreeMap<u8, String> {
    let vfs = crate::vfs::Vfs::new(&game.join("data"), env::overlays_for(dlc).unwrap_or_default(), None);
    vfs.resolve("scripts/constants.lua")
        .and_then(|r| std::fs::read_to_string(r.path).ok())
        .map(|src| summary::parse_ground_table(&src))
        .unwrap_or_default()
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

impl Generator {
    /// Don't Starve: `game` is the dont_starve install directory.
    pub fn ds(game: &Path, opts: &WorldOptions) -> Result<Generator, String> {
        let dlc = opts.dlc.clone().unwrap_or_else(|| "rog".into());
        let level = opts.level.clone().unwrap_or_else(|| default_level(&dlc).into());
        let gen_parameters = env::gen_parameters(&game.join("data"), &level, &dlc, opts.choices.as_deref(), opts.current_level)?;
        let ground_names = ground_names(game, &dlc);
        if platform(opts)? != Platform::Windows {
            return Err("the platform option is for Don't Starve Together only".into());
        }
        Ok(Generator { game: game.to_path_buf(), dst: false, dlc, gen_parameters, ground_names, platform: Platform::Windows })
    }

    /// Don't Starve Together: `game` is the DST (or dedicated server) install.
    pub fn dst(game: &Path, opts: &WorldOptions) -> Result<Generator, String> {
        let gen_parameters = match &opts.gen_parameters {
            Some(text) => text.trim_start_matches('\u{feff}').trim().to_string(),
            None => {
                let preset = opts.preset.clone().unwrap_or_else(|| "SURVIVAL_TOGETHER".into());
                DST_PRESETS
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&preset))
                    .map(|(_, text)| text.trim().to_string())
                    .ok_or_else(|| {
                        format!(
                            "unknown preset {preset} (built in: {}; or pass GEN_PARAMETERS)",
                            DST_PRESETS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
                        )
                    })?
            }
        };
        let gen_parameters = match &opts.world_size {
            Some(size) => with_world_size(&gen_parameters, size)?,
            None => gen_parameters,
        };
        let platform = platform(opts)?;
        Ok(Generator { game: game.to_path_buf(), dst: true, dlc: "dst".into(), gen_parameters, ground_names: Default::default(), platform })
    }

    pub fn generate(&self, seed: u32) -> Result<World, String> {
        let started = std::time::Instant::now();
        if self.dst {
            let config = crate::dst_env::DstConfig {
                data_root: self.game.join("data"),
                mods_dir: None,
                seed: seed as f64,
                gen_parameters: self.gen_parameters.clone(),
                gen_moddata: "null".into(),
                capture_log: false,
                platform: self.platform,
            };
            let out = crate::dst_env::run(&config, Rc::new(RefCell::new(NativeDst::new())))?;
            return Ok(World { seed, savedata: out.savedata, ms: elapsed_ms(started), dst_tile_names: Some(out.tile_names), ds_ground_names: None });
        }
        let config = Config {
            data_root: self.game.join("data"),
            overlays: env::overlays_for(&self.dlc)?,
            mods_dir: None,
            seed: seed as f64,
            gen_parameters: self.gen_parameters.clone(),
            gen_moddata: "null".into(),
            app_version: env::APP_VERSION.into(),
            capture_log: false,
        };
        let out = env::run(&config, Rc::new(RefCell::new(Native::new())))?;
        let names = (!out.ground_names.is_empty()).then_some(out.ground_names);
        Ok(World { seed, savedata: out.savedata, ms: elapsed_ms(started), dst_tile_names: None, ds_ground_names: names })
    }

    /// Tile id -> ground name, for decoding the savedata tile grid (DS: the
    /// GROUND table; DST: WORLD_TILES as the game assigned them).
    pub fn tile_names(&self, world: &World) -> BTreeMap<u16, String> {
        match &world.dst_tile_names {
            Some(names) => names.clone(),
            None => world.ds_ground_names.as_ref().unwrap_or(&self.ground_names).iter().map(|(k, v)| (*k as u16, v.clone())).collect(),
        }
    }

    /// The JSON summary of a world this generator produced.
    pub fn summary(&self, world: &World) -> Result<serde_json::Value, String> {
        let save = summary::parse_savedata(&world.savedata)?;
        let ctx = summary::Context {
            dlc: &self.dlc,
            ground_names: Some(world.ds_ground_names.as_ref().unwrap_or(&self.ground_names)),
            dst_tile_names: world.dst_tile_names.as_ref(),
            generator: serde_json::json!({
                "name": "ds-worldgen",
                "version": env!("CARGO_PKG_VERSION"),
                "game_build": if self.dst { crate::dst_env::APP_VERSION } else { env::APP_VERSION },
                "platform": self.platform.name(),
                "generation_ms": (world.ms * 10.0).round() / 10.0,
                "savedata_sha256": sha256(&world.savedata),
                "seed_input": world.seed,
            }),
        };
        summary::summarise(&save, &ctx)
    }
}

fn platform(opts: &WorldOptions) -> Result<Platform, String> {
    opts.platform.as_deref().map(Platform::parse).transpose().map(Option::unwrap_or_default)
}

fn elapsed_ms(started: std::time::Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
