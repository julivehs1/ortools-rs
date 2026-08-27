//! Generate Rust types for the CP-SAT protos, then compile the C++ shim.
//!
//! The OR-Tools tree itself is provided by `ortools-src`, which hands its
//! paths over through cargo's `links` metadata.

use std::env;
use std::path::PathBuf;

const PROTOS: &[&str] = &[
    "proto/ortools/sat/cp_model.proto",
    "proto/ortools/sat/sat_parameters.proto",
];

fn main() {
    for p in PROTOS {
        println!("cargo:rerun-if-changed={p}");
    }
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:rerun-if-env-changed=PROTOC");

    // prost-build needs a protoc and does not bundle one. Use the vendored
    // binary unless PROTOC says otherwise, so a contributor's machine, CI and a
    // consumer's build all parse the protos with the same compiler instead of
    // whatever happens to be on PATH — and so a consumer needs nothing
    // installed, which is the whole point of this crate.
    if env::var_os("PROTOC").is_none() {
        let protoc = protoc_bin_vendored::protoc_bin_path()
            .expect("no vendored protoc for this platform — set PROTOC to one");
        // Safe here: build scripts are single-threaded at this point.
        unsafe { env::set_var("PROTOC", protoc) };
    }
    prost_build::compile_protos(PROTOS, &["proto/"]).expect("failed to compile CP-SAT protos");

    if env::var("DOCS_RS").is_ok() {
        return;
    }

    let include = PathBuf::from(
        env::var("DEP_ORTOOLS_INCLUDE").expect("ortools-src did not publish DEP_ORTOOLS_INCLUDE"),
    );

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("src/shim.cpp")
        // Pinned, not inherited: abseil's ABI depends on the C++ standard, so
        // the shim must be compiled exactly as OR-Tools itself was.
        .std("c++17")
        // Keep protobuf/abseil symbols out of the dynamic symbol table; see the
        // matching --exclude-libs in ortools-src.
        .flag_if_supported("-fvisibility=hidden")
        .flag_if_supported("-fvisibility-inlines-hidden")
        // OR-Tools' export macros are defined only while OR-Tools itself is
        // being built. A consumer of the installed headers has to blank them
        // out, or every generated protobuf symbol parses as a type name.
        .define("OR_PROTO_DLL", "")
        .define("OR_DLL", "");

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        build.flag("/utf-8");
        // Treat OR-Tools and abseil as external so their headers do not drown
        // the log in warnings we cannot act on. -Wall still covers shim.cpp.
        build.flag("/external:W0");
        build.flag(format!("/external:I{}", include.display()));
    } else {
        build
            .flag("-isystem")
            .flag(include.to_str().expect("non-UTF-8 include path"));
    }

    build.compile("cpsat_shim");

    link(&target_env);
}

/// Emit the link directives for OR-Tools itself.
///
/// These live here rather than in `ortools-src` because cargo attaches a build
/// script's link flags to that script's own crate, and `ortools-src` exports no
/// referenced symbols — rustc drops its rlib from the link, directives and all.
fn link(target_env: &str) {
    let lib_dir = env::var("DEP_ORTOOLS_LIB").expect("ortools-src did not publish DEP_ORTOOLS_LIB");
    println!("cargo:rustc-link-search=native={lib_dir}");

    let libs: Vec<String> = env::var("DEP_ORTOOLS_LIBS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();

    // Emitted twice on purpose.
    //
    // A fully static OR-Tools has genuine cycles among abseil's archives, which
    // a single left-to-right pass cannot resolve. The usual answer is
    // `-Wl,--start-group`, but that would have to travel as a link *argument* —
    // and `cargo:rustc-link-arg` applies only to the emitting package's own
    // targets, never to a downstream binary. `cargo:rustc-link-lib` is the only
    // directive that propagates, so the set is repeated instead: a second pass
    // resolves what the first left open, at the cost of one extra archive scan.
    let passes = if libs.len() > 1 { 2 } else { 1 };
    for _ in 0..passes {
        for l in &libs {
            println!("cargo:rustc-link-lib={l}");
        }
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Everything below is link *arguments*, which — see above — reach this
    // crate's own tests and examples but not a consumer's binary. That is
    // acceptable only because none of it is load-bearing for the shipped
    // configuration:
    //
    //   rpath          only matters for a shared ORTOOLS_ROOT. The published
    //                  prebuilts are static archives with nothing to find at
    //                  run time. A consumer pointing ORTOOLS_ROOT at a shared
    //                  install needs LD_LIBRARY_PATH; the README says so.
    //   --exclude-libs the real defence against colliding protobuf symbols is
    //                  hidden visibility baked into the objects at compile
    //                  time (-fvisibility=hidden here, CMAKE_CXX_VISIBILITY_
    //                  PRESET in ortools-src). That does travel. This flag only
    //                  hardens cpsat's own test binaries on top.
    if matches!(target_os.as_str(), "linux" | "macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_dir}");
    }
    if target_os == "linux" {
        println!("cargo:rustc-link-arg=-Wl,--exclude-libs,ALL");
    }

    // C++ runtime. Deliberately the system one on glibc Linux: that is what
    // lets one manylinux-built tarball serve every distribution.
    let cpp = match () {
        _ if target_env == "msvc" => return,
        _ if target_os == "macos" => "c++",
        _ => "stdc++",
    };
    println!("cargo:rustc-link-lib={cpp}");
}
