// Builds the vendored Lua 5.1.4 (unmodified apart from the DS_PARITY block in
// luaconf.h) plus the Rust glue shim.
//
// lmathlib's rand()/srand() are redirected to ds_crt_rand/ds_crt_srand,
// implemented in Rust (src/crt.rs) as the MSVCR90 LCG, so Lua's math.random
// and the native WorldSim code share one stream as they do in the game.
fn main() {
    // Emscripten: Rust links with native wasm exceptions, so C setjmp/longjmp
    // (Lua errors) and C++ must use the same mechanism.
    let emscripten = std::env::var("TARGET").map(|t| t.contains("emscripten")).unwrap_or(false);
    let wasm_eh = |b: &mut cc::Build| {
        if emscripten {
            // SIMD128 lets clang vectorise; IEEE lane arithmetic gives the
            // same results as scalar code.
            b.flag("-fwasm-exceptions").flag("-sSUPPORT_LONGJMP=wasm").flag("-msimd128");
        }
    };
    let lua = "vendor/lua-5.1.4/src";
    let core = [
        "lapi.c", "lcode.c", "ldebug.c", "ldo.c", "ldump.c", "lfunc.c", "lgc.c", "llex.c",
        "lmem.c", "lobject.c", "lopcodes.c", "lparser.c", "lstate.c", "lstring.c", "ltable.c",
        "ltm.c", "lundump.c", "lvm.c", "lzio.c", "lauxlib.c", "lbaselib.c", "ldblib.c",
        "liolib.c", "lmathlib.c", "loslib.c", "ltablib.c", "lstrlib.c", "loadlib.c", "linit.c",
    ];
    let mut build = cc::Build::new();
    build
        .include(lua)
        .define("DS_PARITY", None)
        .define("rand", "ds_crt_rand")
        .define("srand", "ds_crt_srand")
        .warnings(false);
    if build.get_compiler().is_like_msvc() {
        // Strict IEEE double semantics; no contraction into FMA.
        build.flag("/fp:precise");
    } else {
        build.flag("-ffp-contract=off");
    }
    wasm_eh(&mut build);
    for file in core {
        build.file(format!("{lua}/{file}"));
    }
    build.file("src/c/shim.c");
    build.file("src/c/dump32.c");
    build.compile("lua514");

    // musl 1.2.6 transcendental functions (vendor/musl-math), the same code
    // on every platform; symbols are prefixed dsm_ (names.h).
    let mut musl = cc::Build::new();
    musl.include("vendor/musl-math").warnings(false);
    // C libraries without <features.h> (MSVC, macOS) get a stand-in.
    let target = std::env::var("TARGET").unwrap_or_default();
    if musl.get_compiler().is_like_msvc() || target.contains("apple") {
        musl.include("vendor/musl-math/compat");
    }
    // dsm_prelude.h (system headers, then hidden and the dsm_ renames) is
    // forced into every file, as some include only system headers.
    if musl.get_compiler().is_like_msvc() {
        musl.flag("/std:c11").flag("/fp:precise").flag("/FIdsm_prelude.h");
    } else {
        musl.flag("-ffp-contract=off").flag("-fno-builtin").flag("-std=c99").flag("-include").flag("dsm_prelude.h");
    }
    wasm_eh(&mut musl);
    for entry in std::fs::read_dir("vendor/musl-math").unwrap().flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("c") {
            musl.file(p);
        }
    }
    musl.compile("dsmath");
    println!("cargo:rerun-if-changed=vendor/musl-math");

    // CRT parity shims (src/c/crt.c), compiled without the redirection.
    let mut crt = cc::Build::new();
    wasm_eh(&mut crt);
    crt.file("src/c/crt.c").warnings(false).compile("dscrt");
    println!("cargo:rerun-if-changed=src/c/crt.c");

    // Boost.Polygon 1.53 voronoi (the version the game links), behind a C ABI.
    let mut cpp = cc::Build::new();
    cpp.cpp(true).include("vendor/boost_1_53_voronoi").file("src/c/voronoi.cpp").warnings(false);
    if cpp.get_compiler().is_like_msvc() {
        cpp.flag("/EHsc").flag("/fp:precise");
    } else {
        cpp.flag("-ffp-contract=off").flag("-std=c++11");
    }
    wasm_eh(&mut cpp);
    cpp.compile("dsvoronoi");

    // Standalone web builds embed a game data bundle (ds-worldgen pack,
    // web/build-standalone.ps1) named by DSW_EMBED; other builds embed none.
    println!("cargo:rerun-if-env-changed=DSW_EMBED");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded.rs");
    let code = match std::env::var("DSW_EMBED") {
        Ok(path) if !path.is_empty() => {
            let path = std::fs::canonicalize(&path).unwrap_or_else(|e| panic!("DSW_EMBED={path}: {e}"));
            // Windows canonical paths carry a \\?\ prefix include_bytes! does not need.
            let path = path.display().to_string();
            let path = path.strip_prefix(r"\\?\").unwrap_or(&path).to_string();
            println!("cargo:rerun-if-changed={path}");
            format!("pub static EMBEDDED: &[u8] = include_bytes!({path:?});\n")
        }
        _ => "pub static EMBEDDED: &[u8] = &[];\n".to_string(),
    };
    std::fs::write(out, code).unwrap();

    println!("cargo:rerun-if-changed=vendor/lua-5.1.4/src");
    println!("cargo:rerun-if-changed=vendor/boost_1_53_voronoi");
    println!("cargo:rerun-if-changed=src/c/shim.c");
    println!("cargo:rerun-if-changed=src/c/dump32.c");
    println!("cargo:rerun-if-changed=src/c/voronoi.cpp");
}
