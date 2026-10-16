#[cfg(not(target_os = "emscripten"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ds_worldgen::api::{ground_names, Generator, WorldOptions};
use ds_worldgen::filter::Filter;
use ds_worldgen::summary;

const USAGE: &str = "\
ds-worldgen — Don't Starve and Don't Starve Together world generation outside the game

USAGE:
  ds-worldgen generate (--game <dont_starve dir> | --dst <DST dir>) --seed N
                       [--format summary|savedata] [--output FILE]
      Generate one world natively and print its summary JSON or savedata.
      DS:  --dlc base|rog|sw|hamlet (default rog), --level survival|shipwrecked|
           volcano|porkland|cave|adventure (default per DLC), --current-level N
           (cave 1 caves / 2 ruins, adventure 1..5), --choices '{ lua table }'.
      DST: --preset SURVIVAL_TOGETHER|DST_CAVE or --gen-parameters FILE.

  ds-worldgen search (--game <dir> | --dst <dir>) --seeds A..B [--where EXPR]
                     [--jobs N] [--output FILE]  (+ the generate world options)
      Generate a seed range on all cores; print the summaries matching EXPR
      as JSON lines, e.g. --where 'prefabs.pigking >= 1 && nearest.pigking.distance < 200'.

  ds-worldgen pack [--game <dont_starve dir>] [--dst <DST dir>] [--worldgen-only] --output FILE
      Collect what world generation reads from your installs (the Lua
      scripts, and the names of assets it checks for) into one compact data
      bundle, for a standalone web build (web/build-standalone.ps1). Built
      from your own copy of the games; not for redistribution.
      --worldgen-only keeps just the scripts world generation loads, compiled
      to stripped bytecode for the WebAssembly build.

  ds-worldgen unpack --bundle FILE --output DIR
      Lay a data bundle out as installs (DIR/ds, DIR/dst), which generate
      and search accept like the real ones.

  ds-worldgen summarise --savedata <savedata.lua> [--dlc rog] [--game <dont_starve dir>]
                        [--tile-names FILE] [--where EXPR]
      Print the summary JSON for a worldgen savedata file (e.g. one written by
      generate --format savedata). --game names DS ground tiles; DST tile ids
      are named by --tile-names (a JSON object of id -> WORLD_TILES name).
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("summarise") | Some("summarize") => summarise(&args[1..]),
        Some("generate") => generate(&args[1..]),
        Some("search") => search(&args[1..]),
        Some("pack") => pack(&args[1..]),
        Some("unpack") => unpack(&args[1..]),
        Some("trace") => trace(&args[1..]),
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // PowerShell writes UTF-8 with a BOM.
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes);
    serde_json::from_slice(bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn summarise(args: &[String]) -> Result<ExitCode, String> {
    let path = PathBuf::from(flag(args, "--savedata").ok_or("--savedata is required")?);
    let dlc = flag(args, "--dlc").unwrap_or_else(|| "rog".into());
    let filter = flag(args, "--where").map(|w| Filter::parse(&w)).transpose()?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let save = summary::parse_savedata(&bytes)?;
    let names = flag(args, "--game").map(|g| ground_names(Path::new(&g), &dlc));
    // DST: tile id names (WORLD_TILES ids are assigned when the game loads).
    let dst_names: Option<std::collections::BTreeMap<u16, String>> = flag(args, "--tile-names")
        .map(|p| read_json(Path::new(&p)).and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string())))
        .transpose()?;
    let ctx = summary::Context { dlc: &dlc, ground_names: names.as_ref(), dst_tile_names: dst_names.as_ref(), generator: serde_json::json!({ "source": path.display().to_string() }) };
    let doc = summary::summarise(&save, &ctx)?;
    if let Some(f) = &filter {
        if !f.matches(&doc) {
            return Ok(ExitCode::from(3));
        }
    }
    println!("{}", serde_json::to_string_pretty(&doc).unwrap());
    Ok(ExitCode::SUCCESS)
}

/// The world options shared by generate and search.
fn generator(args: &[String]) -> Result<Generator, String> {
    let gen_parameters = flag(args, "--gen-parameters")
        .map(|path| std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}")))
        .transpose()?;
    let opts = WorldOptions {
        dlc: flag(args, "--dlc"),
        level: flag(args, "--level"),
        current_level: flag(args, "--current-level").map(|v| v.parse::<u32>().map_err(|_| "bad --current-level")).transpose()?,
        choices: flag(args, "--choices"),
        preset: flag(args, "--preset"),
        gen_parameters,
    };
    if let Some(dst) = flag(args, "--dst") {
        return Generator::dst(Path::new(&dst), &opts);
    }
    let game = flag(args, "--game").ok_or("--game (the dont_starve install) or --dst (the DST install) is required")?;
    Generator::ds(Path::new(&game), &opts)
}

fn generate(args: &[String]) -> Result<ExitCode, String> {
    let gen = generator(args)?;
    let seed: u32 = flag(args, "--seed").ok_or("--seed is required")?.parse().map_err(|_| "--seed must be an integer 0..4294967295")?;
    let format = flag(args, "--format").unwrap_or_else(|| "summary".into());
    let world = gen.generate(seed)?;
    let bytes = match format.as_str() {
        "summary" => serde_json::to_vec_pretty(&gen.summary(&world)?).unwrap(),
        "savedata" => world.savedata,
        other => return Err(format!("unknown --format {other} (summary, savedata)")),
    };
    match flag(args, "--output") {
        Some(path) => std::fs::write(&path, &bytes).map_err(|e| format!("{path}: {e}"))?,
        None => {
            use std::io::Write;
            std::io::stdout().write_all(&bytes).map_err(|e| e.to_string())?;
            println!();
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn search(args: &[String]) -> Result<ExitCode, String> {
    let gen = std::sync::Arc::new(generator(args)?);
    let range = flag(args, "--seeds").ok_or("--seeds A..B is required")?;
    let (a, b) = range.split_once("..").ok_or("--seeds must look like 1000..2000")?;
    let (a, b): (u32, u32) = (a.parse().map_err(|_| "bad --seeds start")?, b.parse().map_err(|_| "bad --seeds end")?);
    if b < a {
        return Err("--seeds end is before start".into());
    }
    let filter = std::sync::Arc::new(flag(args, "--where").map(|w| Filter::parse(&w)).transpose()?);
    let jobs: usize = flag(args, "--jobs").map(|j| j.parse().map_err(|_| "bad --jobs")).transpose()?
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
    let mut out: Box<dyn std::io::Write> = match flag(args, "--output") {
        Some(path) => Box::new(std::fs::File::create(&path).map_err(|e| format!("{path}: {e}"))?),
        None => Box::new(std::io::stdout()),
    };
    let next = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(a as u64));
    let (tx, rx) = std::sync::mpsc::channel::<(u32, Result<Option<serde_json::Value>, String>)>();
    let mut workers = Vec::new();
    for _ in 0..jobs.max(1) {
        let (gen, filter, next, tx) = (gen.clone(), filter.clone(), next.clone(), tx.clone());
        workers.push(std::thread::spawn(move || loop {
            let seed = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if seed > b as u64 {
                break;
            }
            let seed = seed as u32;
            let result = gen.generate(seed).and_then(|world| gen.summary(&world)).map(|doc| {
                filter.as_ref().as_ref().map_or(true, |f| f.matches(&doc)).then_some(doc)
            });
            if tx.send((seed, result)).is_err() {
                break;
            }
        }));
    }
    drop(tx);
    let (mut matched, mut failed, mut total) = (0u64, 0u64, 0u64);
    for (seed, result) in rx {
        total += 1;
        match result {
            Ok(Some(doc)) => {
                matched += 1;
                writeln!(out, "{}", serde_json::to_string(&doc).unwrap()).map_err(|e| e.to_string())?;
            }
            Ok(None) => {}
            Err(e) => {
                failed += 1;
                eprintln!("seed {seed}: {e}");
            }
        }
    }
    for w in workers {
        let _ = w.join();
    }
    eprintln!("{total} seeds, {matched} matched, {failed} failed");
    Ok(if failed > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

fn pack(args: &[String]) -> Result<ExitCode, String> {
    let output = flag(args, "--output").ok_or("--output FILE is required")?;
    let mut bundle = ds_worldgen::pack::Bundle::default();
    let (game, dst) = (flag(args, "--game"), flag(args, "--dst"));
    if game.is_none() && dst.is_none() {
        return Err("pass --game <dont_starve dir> and/or --dst <DST dir>".into());
    }
    if let Some(game) = &game {
        bundle.add_ds(Path::new(game))?;
    }
    if let Some(dst) = &dst {
        bundle.add_dst(Path::new(dst))?;
    }
    bundle.add_brand(game.as_deref().map(Path::new), dst.as_deref().map(Path::new));
    if args.iter().any(|a| a == "--worldgen-only") {
        bundle.worldgen_only(game.as_deref().map(Path::new), dst.as_deref().map(Path::new), |line| eprintln!("{line}"))?;
    }
    let bytes = bundle.encode();
    std::fs::write(&output, &bytes).map_err(|e| format!("{output}: {e}"))?;
    let files = bundle.entries.values().filter(|d| d.is_some()).count();
    let raw: usize = bundle.entries.values().flatten().map(Vec::len).sum();
    eprintln!("{output}: {files} scripts ({:.1} MB) and {} asset names, {:.1} MB compressed",
        raw as f64 / 1e6, bundle.entries.len() - files, bytes.len() as f64 / 1e6);
    Ok(ExitCode::SUCCESS)
}

fn unpack(args: &[String]) -> Result<ExitCode, String> {
    let file = flag(args, "--bundle").ok_or("--bundle FILE is required")?;
    let output = PathBuf::from(flag(args, "--output").ok_or("--output DIR is required")?);
    let bytes = std::fs::read(&file).map_err(|e| format!("{file}: {e}"))?;
    let bundle = ds_worldgen::pack::Bundle::decode(&bytes, None)?;
    ds_worldgen::pack::unpack(&bundle, &output)?;
    eprintln!("unpacked to {} (ds: {}, dst: {})", output.display(), bundle.has_ds(), bundle.has_dst());
    Ok(ExitCode::SUCCESS)
}

fn trace(args: &[String]) -> Result<ExitCode, String> {
    let seeds: u32 = flag(args, "--seeds").map(|v| v.parse().map_err(|_| "bad --seeds")).transpose()?.unwrap_or(5);
    let (game, dst) = (flag(args, "--game"), flag(args, "--dst"));
    let files = ds_worldgen::pack::trace_worldgen(game.as_deref().map(Path::new), dst.as_deref().map(Path::new), seeds, |line| eprintln!("{line}"))?;
    let text: String = files.iter().map(|f| format!("{f}
")).collect();
    match flag(args, "--output") {
        Some(path) => std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?,
        None => print!("{text}"),
    }
    eprintln!("{} files", files.len());
    Ok(ExitCode::SUCCESS)
}
