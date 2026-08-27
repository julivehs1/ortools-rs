# ortools-rs

Rust bindings to Google's [CP-SAT][cpsat] constraint solver — with no system
OR-Tools install, no `protoc`, no `ORTOOLS_PREFIX`, and no `RUSTFLAGS`
incantation.

```rust
use cpsat::CpModelBuilder;

let mut m = CpModelBuilder::default();
let x = m.new_int_var(0..=10);
let y = m.new_int_var(0..=10);
m.add_le(x + y, 12);
m.maximize(x * 2 + y);

let r = m.solve();
println!("x = {}, y = {}", x.value(&r), y.value(&r));
```

## Crates

| Crate | What it does |
|---|---|
| `ortools-src` | Makes a linkable OR-Tools tree appear on disk: prebuilt download (checksum-pinned), cache, or a CMake build from source. No API. |
| `cpsat` | The solver bindings and the model builder. |

The split is deliberate. `ortools-src` is useful on its own — any crate that
needs OR-Tools C++ can depend on it instead of reinventing the download, and it
is versioned and maintained separately from whatever API sits on top.

## Why another one

There are several OR-Tools crates already. Every one of them asks you to
install OR-Tools yourself first, and the most-used one also needs a `protoc` on
your PATH and covers roughly the boolean and linear part of CP-SAT.

|  | `cpsat` | `cp_sat` | `or-tools-sys` |
|---|---|---|---|
| Zero-setup build | planned | no | no |
| Builds without a system `protoc` | **yes** | no | — |
| Intervals, `no_overlap`, `cumulative` | **yes** | no | no |
| `circuit` / `routes` | planned | no | no |
| `table`, `automaton`, `element`, `inverse`, `reservoir` | planned | no | no |
| `int_prod` / `int_div` / `int_mod` / `abs` | planned | no | no |
| Solution callbacks | planned | no | no |
| Mutable proto escape hatch | **yes** | no | — |

Scheduling is what most people come to CP-SAT for, and it is the first thing
this crate implemented. See `crates/cpsat/examples/jobshop.rs`.

## Status

Early. The API builds and solves, the test suite passes, and the job-shop
example produces provably optimal schedules. The CMake recipe is implemented
and its configure step is verified against OR-Tools 9.15 sources: every flag is
accepted, and the dependency set comes out as abseil, protobuf, re2, zlib,
bzip2 and eigen — no SCIP, no COIN-OR, no Boost.

What is **not** done:

- No prebuilt tarballs are published yet, so a build currently needs
  `ORTOOLS_ROOT`. Running `.github/workflows/prebuilt.yml` is what fixes that.
- A full source build has not been run end to end here; only configure was.
- Constraint coverage is core + scheduling. See the table above.

## CI

Two workflows, deliberately separate:

| | Runs | Does |
|---|---|---|
| `test.yml` | every push and PR | fmt, clippy, tests and the job-shop example on Linux and macOS, against Google's official OR-Tools archive |
| `prebuilt.yml` | `workflow_dispatch` only | builds the static tarballs for five targets and cuts a release |

`prebuilt.yml` is the one that makes `ORTOOLS_ROOT` unnecessary for everyone
else, and it is expected to run about as often as OR-Tools releases — two or
three times a year.

Linux is built inside a glibc 2.28 image, which is what collapses Google's
fourteen per-distribution archives into one tarball per architecture. macOS and
Windows are built on native runners, because libc++ and the MSVC STL have
stable ABIs and cross-building either invites link errors that only surface at
the consumer.

Two things the workflow does that cadrum's does not: the release tag is derived
from `crates/ortools-src/version.txt` — the same file `build.rs` reads, so the
two cannot drift — and every tarball's SHA256 is published, with a
ready-to-paste snippet in the job summary.

```sh
just test        # fetches an OR-Tools install on first run, then tests
just jobshop     # solve the scheduling example
just check       # fmt and clippy, the way CI does
just consumer    # build cpsat from outside the workspace, as a user would
just clean       # build artifacts, keeping the OR-Tools tree
```

Prefer `just clean` over `cargo clean`. The OR-Tools tree is cached inside the
cargo target directory so that `cargo clean -p` cannot reach it — but a plain
`cargo clean` still can, and rebuilding it from source costs 30-90 minutes.
`just clean-all` removes it too.

`just ortools` pulls the official C++ archive for your platform into `./ortools`
and everything else picks it up from there. To use an install you already have,
set `ORTOOLS_ROOT` and call cargo directly — that path also covers Nix, Bazel
and any build with no network access.

**If `ORTOOLS_ROOT` points at a shared-library install**, your binary also needs
`LD_LIBRARY_PATH` (or `DYLD_LIBRARY_PATH`) at run time. Cargo propagates a
build script's `rustc-link-lib` across package boundaries but not its
`rustc-link-arg`, so this crate cannot bake an rpath into your binary. The
published prebuilts are static archives and have no such requirement.

## Releasing

Tarballs are built by CI, not locally: this only works on a machine old enough
that its glibc is older than every target system's, and no single machine can
produce Linux, macOS and Windows.

1. `just prebuilt` — optional. Builds one Linux target in the same container CI
   uses, to check the recipe before spending an hour on five of them.
2. Run the `prebuilt` workflow. Its `targets` input takes `all` (the default),
   an OS group (`linux`, `macos`, `windows`), or a comma-separated list of
   triples, so a first attempt can start narrow.
3. `just digests` — reads the release and pins the digests into `build.rs`.
   Left unstaged on purpose: read the diff before committing, since these are
   the only thing between a consumer and an unverified solver binary.

A partial run uploads artifacts but publishes no release. `just digests`
replaces the whole `PREBUILT_SHA256` table, so a release missing targets would
silently unpin whatever was not rebuilt.

`just digests-check` fails if the committed digests disagree with the release.

Actions minutes are free and unmetered on public repositories. On a private one
they are not: macOS runners bill at ten times the rate and Windows at twice, so
a single full run costs roughly 1400 minutes against a 2000/month free tier.

## Prior art

The build-script design follows [cadrum][cadrum], which does the same thing for
OpenCASCADE: publish statically linkable binaries per target, resolve them at
build time, cache them in the target directory. Two deliberate departures —
every download is SHA256-pinned, and the link order comes from a manifest
inside the tarball rather than a hardcoded list.

The protobuf-across-the-boundary approach and the shape of the C++ shim are
taken from [`cp_sat`][cp_sat] (Apache-2.0), which got that part right.

[cpsat]: https://developers.google.com/optimization/cp/cp_solver
[releases]: https://github.com/google/or-tools/releases
[cadrum]: https://github.com/lzpel/cadrum
[cp_sat]: https://github.com/KardinalAI/cp_sat
