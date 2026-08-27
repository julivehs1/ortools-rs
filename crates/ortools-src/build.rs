//! Make a usable OR-Tools tree appear on disk, then publish its paths.
//!
//! Resolution order (first hit wins):
//!
//!   1. `ORTOOLS_ROOT`            — an existing install; we touch nothing
//!   2. `<target-dir>/<name>/`    — cache from a previous run
//!   3. feature `source`          — build from upstream sources with CMake
//!   4. otherwise                 — download a prebuilt tarball, verified by SHA256
//!
//! Unlike cadrum (which inspired this design) every downloaded artifact is
//! checksum-pinned in `PREBUILT_SHA256` below. A tarball whose digest is not
//! listed is refused, not warned about.

use std::env;
use std::path::{Path, PathBuf};

/// Version and revision, kept in a plain file so CI can read the same values
/// without re-implementing this file's naming rules. See `version.txt`.
const VERSION_FILE: &str = include_str!("version.txt");

/// Look up a `key = value` line in [`VERSION_FILE`].
fn version_field(key: &str) -> &'static str {
    VERSION_FILE
        .lines()
        .filter_map(|l| l.split('#').next())
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim())
        .unwrap_or_else(|| panic!("version.txt has no `{key}` entry"))
}

/// SHA256 of each published prebuilt tarball, keyed by target triple.
///
/// Empty until the first release is cut. An entry MUST be added in the same
/// commit that uploads a tarball — an unlisted target falls through to the
/// "no prebuilt" error rather than downloading something unverified.
const PREBUILT_SHA256: &[(&str, &str)] = &[
    // ("x86_64-unknown-linux-gnu", "…"),
    // ("aarch64-unknown-linux-gnu", "…"),
    // ("x86_64-apple-darwin",       "…"),
    // ("aarch64-apple-darwin",      "…"),
    // ("x86_64-pc-windows-msvc",    "…"),
];

/// OR-Tools libraries a dependent crate must link, in dependency order.
///
/// Published as `DEP_ORTOOLS_LIBS`. The actual `rustc-link-lib` directives are
/// emitted by the crate that includes OR-Tools headers, not here: this crate's
/// rlib carries no referenced symbols, so rustc drops it from the link and
/// would drop any directives with it.
///
/// CP-SAT only needs `ortools` itself plus abseil/protobuf/re2/zlib. The
/// solver-specific archives (SCIP, SoPlex, COIN-OR, HiGHS) are deliberately
/// absent: they roughly triple the binary and CP-SAT does not use them.
const ORTOOLS_LIBS: &[&str] = &["ortools", "protobuf"];

/// Optional link manifest inside a tarball: one library name per line, in link
/// order, `#` for comments.
///
/// Our own prebuilts ship one, because a fully static OR-Tools needs abseil's
/// several dozen archives named in dependency order and hardcoding that list
/// here would put it out of sync with the tarball the moment either changes.
/// A foreign tree (`ORTOOLS_ROOT`) has no manifest and falls back to
/// [`ORTOOLS_LIBS`], which is right for the usual shared-library install.
const LINK_MANIFEST: &str = "cpsat-libs.txt";

fn main() {
    println!("cargo:rerun-if-env-changed=ORTOOLS_ROOT");
    println!("cargo:rerun-if-env-changed=ORTOOLS_PREBUILT_URL");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=version.txt");

    println!(
        "cargo:rustc-env=ORTOOLS_VERSION={}",
        version_field("ortools_version")
    );

    if env::var("DOCS_RS").is_ok() {
        // docs.rs has no network and no C++ toolchain. Publish placeholder
        // metadata so the crate still documents.
        println!("cargo:rustc-env=ORTOOLS_RESOLVED_ROOT=");
        println!("cargo:root=");
        println!("cargo:include=");
        println!("cargo:lib=");
        println!("cargo:libs=");
        return;
    }

    let target = env::var("TARGET").unwrap();
    let root = resolve(&target);
    let (include, lib) = probe(&root).unwrap_or_else(|| {
        panic!(
            "OR-Tools tree at {} has no usable include/ and lib/",
            root.display()
        )
    });

    // Consumed by dependent build scripts as DEP_ORTOOLS_{ROOT,INCLUDE,LIB}.
    println!("cargo:root={}", root.display());
    println!("cargo:include={}", include.display());
    println!("cargo:lib={}", lib.display());
    println!("cargo:rustc-env=ORTOOLS_RESOLVED_ROOT={}", root.display());
    println!("cargo:libs={}", link_libs(&root).join(","));

    // Record the artifact name where CI can read it, so the packaging step
    // never has to re-derive this file's naming rules in shell.
    let _ = std::fs::write(
        target_dir(&target).join("ortools-artifact-name.txt"),
        artifact_name(Some(&target)),
    );
}

/// Name of the release tag, tarball and cache directory.
///
/// One naming authority for all three, so the download URL and the directory
/// the extracted tree lands in cannot drift apart.
///
/// - `artifact_name(None)`      → `ortools-9_15-rev1`
/// - `artifact_name(Some(t))`   → `ortools-9_15-rev1-x86_64_unknown_linux_gnu`
fn artifact_name(target: Option<&str>) -> String {
    let v = version_field("ortools_version")
        .trim_start_matches('v')
        .replace('.', "_");
    let mut name = format!("ortools-{v}-{}", version_field("build_revision"));
    if let Some(target) = target {
        name.push('-');
        name.push_str(&target.replace('-', "_"));
    }
    name
}

fn resolve(target: &str) -> PathBuf {
    if let Ok(root) = env::var("ORTOOLS_ROOT") {
        let root = absolutize(PathBuf::from(root));
        assert!(
            probe(&root).is_some(),
            "ORTOOLS_ROOT={} contains no include/ortools/sat/cp_model.h",
            root.display()
        );
        return root;
    }

    let cache = target_dir(target).join(artifact_name(Some(target)));
    if probe(&cache).is_some() {
        return cache;
    }

    if cfg!(feature = "source") {
        return build_from_source(&cache, target);
    }
    download_prebuilt(&cache, target)
}

/// Cargo's target directory, derived from `OUT_DIR`.
///
/// The cache lives here rather than in `OUT_DIR` so a `cargo clean -p` or a
/// version bump of this crate does not throw away a ~200 MB download.
///
///   `<target-dir>/<profile>/build/<pkg>-<hash>/out`           (host build)
///   `<target-dir>/<triple>/<profile>/build/<pkg>-<hash>/out`  (with --target)
fn target_dir(target: &str) -> PathBuf {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let above = out.ancestors().nth(4).expect("unexpected OUT_DIR layout");
    if above.file_name().is_some_and(|n| n == target) {
        above.parent().unwrap().to_path_buf()
    } else {
        above.to_path_buf()
    }
}

/// Libraries a dependent crate must link, in order.
fn link_libs(root: &Path) -> Vec<String> {
    let manifest = root.join(LINK_MANIFEST);
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return ORTOOLS_LIBS.iter().map(|s| s.to_string()).collect();
    };
    println!("cargo:rerun-if-changed={}", manifest.display());
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn absolutize(p: PathBuf) -> PathBuf {
    if p.is_relative() {
        env::current_dir().unwrap().join(p)
    } else {
        p
    }
}

/// Return `(include_dir, lib_dir)` if `root` looks like an OR-Tools install.
///
/// Both halves are anchored on content rather than on a directory existing.
/// A half-extracted tarball is then treated as a miss and re-fetched, and —
/// the case that actually bit — a tree carrying both `lib/` and `lib64/` picks
/// the one holding libraries. Red Hat derivatives install into `lib64` and
/// leave a `lib/` holding only CMake package files; Debian uses `lib`.
fn probe(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let include = root.join("include");
    if !include.join("ortools/sat/cp_model.h").exists() {
        return None;
    }
    let lib = [root.join("lib64"), root.join("lib")]
        .into_iter()
        .find(|p| holds_libraries(p))?;
    Some((include, lib))
}

/// Whether `dir` directly contains something a linker would accept.
fn holds_libraries(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.file_name().to_str().is_some_and(|n| {
            [".a", ".lib", ".so", ".dylib"]
                .iter()
                .any(|x| n.contains(x))
        })
    })
}

fn download_prebuilt(dest: &Path, target: &str) -> PathBuf {
    let name = artifact_name(Some(target));
    let expected = PREBUILT_SHA256
        .iter()
        .find(|(t, _)| *t == target)
        .map(|(_, sha)| *sha)
        .unwrap_or_else(|| {
            panic!(
                "\nNo prebuilt OR-Tools published for target `{target}`.\n\n\
                 Either point at an existing install:\n\
                 \n    ORTOOLS_ROOT=/path/to/ortools cargo build\n\n\
                 or build OR-Tools from source (slow, needs CMake + git):\n\
                 \n    cargo build --features ortools-src/source\n"
            )
        });

    let url = env::var("ORTOOLS_PREBUILT_URL").unwrap_or_else(|_| {
        format!(
            "https://github.com/julivehs1/ortools-rs/releases/download/{}/{}.tar.gz",
            artifact_name(None),
            name
        )
    });

    println!("cargo:warning=downloading prebuilt OR-Tools from {url}");
    let bytes = fetch(&url);
    verify(&bytes, expected, &url);

    let parent = dest.parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    let gz = libflate::gzip::Decoder::new(&bytes[..]).expect("tarball is not gzip");
    tar::Archive::new(gz)
        .unpack(parent)
        .expect("tar unpack failed");

    let extracted = parent.join(&name);
    if extracted != dest {
        let _ = std::fs::remove_dir_all(dest);
        std::fs::rename(&extracted, dest).expect("failed to move extracted tree into place");
    }
    dest.to_path_buf()
}

fn fetch(url: &str) -> Vec<u8> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    }
    let resp = minreq::get(url)
        .send()
        .unwrap_or_else(|e| panic!("GET {url}: {e}"));
    assert!(
        (200..300).contains(&resp.status_code),
        "GET {url}: HTTP {}",
        resp.status_code
    );
    resp.into_bytes()
}

/// Refuse anything whose digest does not match. A mismatch is a hard error,
/// never a warning — a silently wrong solver binary is worse than no build.
fn verify(bytes: &[u8], expected: &str, url: &str) {
    use sha2::{Digest, Sha256};
    let actual = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(
        actual, expected,
        "\nchecksum mismatch for {url}\n  expected {expected}\n  actual   {actual}\n"
    );
}

#[cfg(not(feature = "source"))]
fn build_from_source(_dest: &Path, _target: &str) -> PathBuf {
    unreachable!("guarded by cfg!(feature = \"source\")")
}
/// Build OR-Tools from upstream sources with CMake.
///
/// This is also the recipe CI runs to produce the published tarballs, so there
/// is one definition of how an OR-Tools tree gets built. A prebuilt is nothing
/// more than the output of this function, tarred up.
#[cfg(feature = "source")]
fn build_from_source(dest: &Path, target: &str) -> PathBuf {
    let src = fetch_sources(dest);
    force_static_deps(&src);

    println!("cargo:warning=building OR-Tools from source — expect 30-90 minutes");

    let mut cfg = cmake::Config::new(&src);
    cfg.profile("Release")
        .out_dir(dest)
        // OR-Tools defaults this to ON. The point of this crate is that the
        // consumer ends up with one self-contained binary.
        .define("BUILD_SHARED_LIBS", "OFF")
        // Fetch and build abseil/protobuf/re2/zlib rather than expecting them
        // on the system — that is what makes the result portable across distros.
        .define("BUILD_DEPS", "ON")
        .define("BUILD_CXX", "ON")
        .define("BUILD_PYTHON", "OFF")
        .define("BUILD_JAVA", "OFF")
        .define("BUILD_DOTNET", "OFF")
        // CP-SAT uses none of the MIP solvers. Leaving them in roughly triples
        // the tarball and drags in Boost, COIN-OR and SCIP for nothing.
        .define("USE_SCIP", "OFF")
        .define("USE_COINOR", "OFF")
        .define("USE_HIGHS", "OFF")
        .define("USE_PDLP", "OFF")
        .define("USE_GLPK", "OFF")
        .define("USE_GUROBI", "OFF")
        .define("USE_XPRESS", "OFF")
        .define("USE_CPLEX", "OFF")
        .define("BUILD_MATH_OPT", "OFF")
        .define("BUILD_FLATZINC", "OFF")
        .define("BUILD_SAMPLES", "OFF")
        .define("BUILD_EXAMPLES", "OFF")
        .define("BUILD_TESTING", "OFF")
        // Pinned, not inherited: abseil's ABI depends on the C++ standard, so
        // this must match what the shim in `cpsat` is compiled with.
        .define("CMAKE_CXX_STANDARD", "17")
        .define("CMAKE_CXX_STANDARD_REQUIRED", "ON")
        // Keep protobuf/abseil symbols out of the consumer's dynamic symbol
        // table; see the matching --exclude-libs in cpsat's build script.
        .define("CMAKE_CXX_VISIBILITY_PRESET", "hidden")
        .define("CMAKE_VISIBILITY_INLINES_HIDDEN", "ON")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");

    if target.ends_with("msvc") {
        cfg.cxxflag("/utf-8");
    }

    let built = cfg.build();
    println!("cargo:warning=OR-Tools installed to {}", built.display());

    prune(dest);
    write_link_manifest(dest);
    dest.to_path_buf()
}

/// Force the vendored dependencies to build as static archives.
///
/// Three separate things in `cmake/dependencies/CMakeLists.txt` defeat
/// `-DBUILD_SHARED_LIBS=OFF`, which reaches OR-Tools' own targets and nothing
/// else:
///
///   * a plain `set(BUILD_SHARED_LIBS ON)` before anything is fetched, which
///     shadows the cache entry for that directory and every subdirectory
///     `FetchContent` adds beneath it — zlib, bzip2, abseil, protobuf, re2 and
///     eigen, all six of them;
///   * `set(protobuf_BUILD_SHARED_LIBS ON)`, which protobuf's own CMake turns
///     straight back into `BUILD_SHARED_LIBS ON` for its subtree;
///   * bzip2, whose `ENABLE_STATIC_LIB` defaults off and whose
///     `ENABLE_SHARED_LIB` defaults on. Both matter: bzip2 aliases
///     `BZip2::BZip2` onto its static target only when no shared target exists,
///     and `cmake/cpp.cmake` links that alias into `ortools`.
///
/// Upstream means all of it — Google ships one dynamically linked archive per
/// distribution. This crate ships a single static tree, so the lines have to
/// go, and patching the sources is the only lever: none of them is a cache
/// variable, and OR-Tools exposes no option to override them.
///
/// Left alone the build fails everywhere, just at different points. macOS dies
/// linking abseil's own dylibs: ld64 resolves every symbol when it links one,
/// and the hidden visibility set above leaves abseil's cross-library references
/// undefined. Linux gets further, because ELF permits a shared object with
/// unresolved symbols, and then fails at the first executable linked against
/// them — `libabsl_cord.so: undefined reference to CordRepBtree::IsFlat`, and
/// several hundred more like it.
///
/// Had it linked, the result would still have been unusable: `prune` deletes
/// every .so from the install tree for not being an archive, and
/// `write_link_manifest` records only archives, so the tarball would carry
/// `libortools.a` with no abseil, protobuf, re2 or bz2 in it at all.
#[cfg(feature = "source")]
fn force_static_deps(src: &Path) {
    // Anchored, not global: the same file sets BUILD_SHARED_LIBS again for
    // Boost and for Windows, and those occurrences must keep their own values.
    const PATCHES: &[(&str, &str)] = &[
        (
            "set(FETCHCONTENT_UPDATES_DISCONNECTED ON)\nset(BUILD_SHARED_LIBS ON)",
            "set(FETCHCONTENT_UPDATES_DISCONNECTED ON)\nset(BUILD_SHARED_LIBS OFF)",
        ),
        (
            "set(protobuf_BUILD_SHARED_LIBS ON)",
            "set(protobuf_BUILD_SHARED_LIBS OFF)",
        ),
        // bzip2 patches CMP0077 to NEW, so a plain set() ahead of its option()
        // is honoured. The static target installs as libbz2_static.a, which the
        // link manifest picks up like any other archive.
        (
            "  set(ENABLE_LIB_ONLY ON)\n  set(ENABLE_TESTS OFF)",
            "  set(ENABLE_LIB_ONLY ON)\n  set(ENABLE_TESTS OFF)\n  \
             set(ENABLE_SHARED_LIB OFF)\n  set(ENABLE_STATIC_LIB ON)",
        ),
    ];

    let path = src
        .join("cmake")
        .join("dependencies")
        .join("CMakeLists.txt");
    let mut text =
        std::fs::read_to_string(&path).expect("OR-Tools sources have no dependency CMakeLists.txt");

    for (from, to) in PATCHES {
        // The source tree is reused across runs, so this has to be idempotent.
        if text.contains(to) {
            continue;
        }
        assert!(
            text.contains(from),
            "{}: expected `{from}`. OR-Tools changed how it links its \
             dependencies — check that they still build static before dropping \
             this patch.",
            path.display()
        );
        text = text.replace(from, to);
    }

    std::fs::write(&path, text).expect("failed to patch dependency CMakeLists.txt");
}

/// Download and unpack the OR-Tools source tree next to `dest`, returning its path.
#[cfg(feature = "source")]
fn fetch_sources(dest: &Path) -> PathBuf {
    let src = dest.with_extension("src");
    if src.join("CMakeLists.txt").exists() {
        return src;
    }

    let url = format!(
        "https://github.com/google/or-tools/archive/refs/tags/{}.tar.gz",
        version_field("ortools_version")
    );
    println!("cargo:warning=downloading OR-Tools sources from {url}");
    let bytes = fetch(&url);

    let staging = dest.with_extension("unpack");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).unwrap();
    let gz = libflate::gzip::Decoder::new(&bytes[..]).expect("source tarball is not gzip");
    tar::Archive::new(gz)
        .unpack(&staging)
        .expect("tar unpack failed");

    // The archive holds a single `or-tools-<version>/` directory.
    let inner = std::fs::read_dir(&staging)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .expect("source tarball has no top-level directory");
    let _ = std::fs::remove_dir_all(&src);
    std::fs::rename(&inner, &src).expect("failed to move sources into place");
    let _ = std::fs::remove_dir_all(&staging);
    src
}

/// Drop everything a linker will never look at.
///
/// The install tree also carries CMake package files, pkg-config data and
/// documentation. Only `include/` and the archives in `lib/` matter, and the
/// difference is a large fraction of the tarball.
#[cfg(feature = "source")]
fn prune(root: &Path) {
    for dir in ["share", "bin", "examples", "doc"] {
        let _ = std::fs::remove_dir_all(root.join(dir));
    }
    for lib in ["lib", "lib64"] {
        let Ok(entries) = std::fs::read_dir(root.join(lib)) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_some_and(|x| x == "a" || x == "lib") {
                continue;
            }
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// Record which archives a consumer must link, so the list travels with the
/// tarball instead of being frozen into this file.
///
/// Order here is advisory only: a fully static OR-Tools has circular references
/// among abseil's archives, so the consumer wraps the set in a linker group
/// rather than relying on a topological order that does not exist.
#[cfg(feature = "source")]
fn write_link_manifest(root: &Path) {
    let (_, lib_dir) = probe(root).expect("built tree has no include/ and lib/");
    let mut libs: Vec<String> = std::fs::read_dir(&lib_dir)
        .expect("lib dir unreadable")
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = name
                .strip_suffix(".a")
                .or(name.strip_suffix(".lib"))?
                .to_string();
            Some(stem.strip_prefix("lib").unwrap_or(&stem).to_string())
        })
        .collect();
    libs.sort();
    // `ortools` first: it carries the symbols that pull in everything else.
    libs.sort_by_key(|l| l != "ortools");

    let mut out =
        String::from("# generated by ortools-src; one library per line, linked as a group\n");
    for l in &libs {
        out.push_str(l);
        out.push('\n');
    }
    std::fs::write(root.join(LINK_MANIFEST), out).expect("failed to write link manifest");
}
