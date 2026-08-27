# Default recipe
default:
    @just --list

# OR-Tools release used for local development. Matches version.txt.
ortools_release := "v9.15"
ortools_build := "9.15.6755"
ortools_dir := justfile_directory() / "ortools"

# --- everyday loop -----------------------------------------------------------

# Run the test suite
test: ortools
    #!/usr/bin/env bash
    set -euo pipefail
    export ORTOOLS_ROOT="{{ortools_dir}}"
    export LD_LIBRARY_PATH="{{ortools_dir}}/lib64:{{ortools_dir}}/lib"
    export DYLD_LIBRARY_PATH="{{ortools_dir}}/lib64:{{ortools_dir}}/lib"
    cargo test --workspace

# Solve the job-shop example
jobshop: ortools
    #!/usr/bin/env bash
    set -euo pipefail
    export ORTOOLS_ROOT="{{ortools_dir}}"
    export LD_LIBRARY_PATH="{{ortools_dir}}/lib64:{{ortools_dir}}/lib"
    export DYLD_LIBRARY_PATH="{{ortools_dir}}/lib64:{{ortools_dir}}/lib"
    cargo run --example jobshop

# Build and run cpsat from outside the workspace, as a user would
consumer: ortools
    #!/usr/bin/env bash
    set -euo pipefail
    export ORTOOLS_ROOT="{{ortools_dir}}"
    export LD_LIBRARY_PATH="{{ortools_dir}}/lib64:{{ortools_dir}}/lib"
    export DYLD_LIBRARY_PATH="{{ortools_dir}}/lib64:{{ortools_dir}}/lib"
    cargo run --manifest-path tests/consumer/Cargo.toml

# Format and lint, the way CI does
check: ortools
    #!/usr/bin/env bash
    set -euo pipefail
    export ORTOOLS_ROOT="{{ortools_dir}}"
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings

# This is Google's dynamically linked, per-distribution build: fine to develop
# against, and explicitly not what this project ships. Skipped if already there.

# Fetch an OR-Tools install for local development
ortools:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -f "{{ortools_dir}}/include/ortools/sat/cp_model.h" ]; then exit 0; fi
    case "$(uname -s)-$(uname -m)" in
      Linux-x86_64)  archive="or-tools_amd64_fedora-42_cpp_v{{ortools_build}}.tar.gz" ;;
      Linux-aarch64) archive="or-tools_aarch64_AlmaLinux-8.10_cpp_v{{ortools_build}}.tar.gz" ;;
      Darwin-arm64)  archive="or-tools_arm64_macOS-26.2_cpp_v{{ortools_build}}.tar.gz" ;;
      Darwin-x86_64) archive="or-tools_x86_64_macOS-26.2_cpp_v{{ortools_build}}.tar.gz" ;;
      *) echo "no known archive for $(uname -s)-$(uname -m); set ORTOOLS_ROOT yourself" >&2; exit 1 ;;
    esac
    url="https://github.com/google/or-tools/releases/download/{{ortools_release}}/$archive"
    echo "fetching $url"
    mkdir -p "{{ortools_dir}}"
    curl -fsSL "$url" | tar xz -C "{{ortools_dir}}" --strip-components=1
    echo "OR-Tools ready at {{ortools_dir}}"

# Removes compiled Rust, but deliberately not the OR-Tools tree under target/.
# It lives there so `cargo clean -p` cannot reach it; a plain `cargo clean`
# still can, and rebuilding it from source costs 30-90 minutes.

# Remove build artifacts, keeping the OR-Tools tree
clean:
    #!/usr/bin/env bash
    set -euo pipefail
    rm -rf target/debug target/release tests/consumer/target dist
    kept=$(find target -maxdepth 1 -type d -name 'ortools-*' 2>/dev/null || true)
    if [ -n "$kept" ] || [ -d "{{ortools_dir}}" ]; then
        echo "kept:"
        du -sh $kept "{{ortools_dir}}" 2>/dev/null || true
    fi

# Remove everything, including the OR-Tools tree and the dev install
clean-all:
    #!/usr/bin/env bash
    set -euo pipefail
    rm -rf target tests/consumer/target dist "{{ortools_dir}}"
    echo 'removed everything; `just test` will fetch OR-Tools again'


# --- release plumbing --------------------------------------------------------

# For checking the recipe before spending CI minutes on five targets. NOT how
# releases are made: this host can only produce Linux, and only CI builds every
# target against the old glibc that makes the result portable.

# Build one prebuilt tarball locally, in the container CI uses
prebuilt target="x86_64-unknown-linux-gnu":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{target}}" in
      x86_64-unknown-linux-gnu)  image=quay.io/pypa/manylinux_2_28_x86_64 ;;
      aarch64-unknown-linux-gnu) image=quay.io/pypa/manylinux_2_28_aarch64 ;;
      *) echo "{{target}} cannot be built here — macOS and Windows need native runners" >&2; exit 1 ;;
    esac
    mkdir -p dist
    podman run --rm -v "{{justfile_directory()}}:/work:Z" -w /work "$image" bash -euxc '
      dnf install -y cmake ninja-build git
      curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
      . "$HOME/.cargo/env"
      cargo build --release -p ortools-src --features source
      name=$(cat target/ortools-artifact-name.txt)
      test -f "target/$name/cpsat-libs.txt"
      du -sh "target/$name"
      tar czf "dist/$name.tar.gz" -C target "$name"
    '
    ls -lh dist/

# Run after the prebuilt workflow has cut a release. Leaves the change unstaged
# so you can read it before committing — these digests are the only thing
# between a consumer and an unverified solver binary.

# Pin a published release's SHA256 digests into build.rs
digests tag="":
    #!/usr/bin/env bash
    set -euo pipefail
    tag="{{tag}}"
    if [ -z "$tag" ]; then
      field() { sed 's/#.*//' crates/ortools-src/version.txt | grep -E "^\s*$1\s*=" | head -1 | cut -d= -f2- | tr -d '[:space:]'; }
      v=$(field ortools_version); r=$(field build_revision)
      tag="ortools-${v#v}-${r}"; tag="${tag//./_}"
    fi
    repo=$(sed -n 's|^repository *= *"https://github.com/\(.*\)"|\1|p' Cargo.toml | head -1)
    url="https://github.com/$repo/releases/download/$tag/digests.txt"
    echo "reading $url"
    curl -fsSL "$url" -o /tmp/ortools-digests.txt
    python3 tools/update-digests.py /tmp/ortools-digests.txt
    # The generated rows are wider than rustfmt's limit, so normalise them here
    # rather than leaving `just check` to fail on the next run.
    cargo fmt -p ortools-src
    git --no-pager diff --stat crates/ortools-src/build.rs

# Fail if build.rs digests disagree with the published release
digests-check tag="": (digests tag)
    #!/usr/bin/env bash
    set -euo pipefail
    if ! git diff --quiet crates/ortools-src/build.rs; then
      echo "PREBUILT_SHA256 is out of date — run `just digests`" >&2
      git --no-pager diff crates/ortools-src/build.rs
      exit 1
    fi
    echo "digests match the published release"
