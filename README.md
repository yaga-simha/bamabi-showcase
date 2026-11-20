# `bamabi-showcase`

Layer 4 (**runnable examples, benchmarks, fuzzing, integration tests, mdBook developer guide, and `xtask` automation**) for the federated **`bamabi`** (`ABI 0.3.0`, `v0.3.0`, Bevy `0.20.1`) native modding framework.

Refactored from the original [`bevy_mod_abi`](https://github.com/yaga-simha/bevy_mod_abi) monorepo into a strictly acyclic 5-repository architecture, `bamabi-showcase` exercises all four lower layers (`bamabi-wire`, `bamabi-tools`, `bamabi-core`, and `bamabi`) end-to-end across Rust, ISO C99, C++17, and Zig guest mods.

## Developer Guide & Benchmarks

- **Developer Guide (mdBook):** [`docs/book/src/SUMMARY.md`](docs/book/src/SUMMARY.md) — build locally with `mdbook build docs/book` (outputs to `target/book`).
- **Performance Benchmarks & Gates:** [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) — 9-scenario throughput baselines, cross-language permutation results, and deterministic regression gates.

## Progressive Examples (`examples/01..10`)

| # | Example | Highlights |
|---|---|---|
| 01 | [`01_minimal_handshake`](examples/01_minimal_handshake/README.md) | Minimal host + Rust `.cdylib` guest handshake and component mutation |
| 02 | [`02_multi_system_pipeline`](examples/02_multi_system_pipeline/README.md) | Multi-system ordering, `SystemSet`s, resources, and deferred commands |
| 03 | [`03_cross_language_interop`](examples/03_cross_language_interop/README.md) | Simultaneous ISO C99 + Rust guest mods operating on shared Bevy components |
| 04 | [`04_visual_game`](examples/04_visual_game/README.md) | 2D visual game with live hot-swappable gravity, rainbow, and vortex mods |
| 05 | [`05_cute_3d_winit`](examples/05_cute_3d_winit/README.md) | 3D interactive `winit` + `wgpu` scene with Rust and C99 mods |
| 06 | [`06_cross_mod_components`](examples/06_cross_mod_components/README.md) | Dynamic cross-mod component registration and inter-mod queries |
| 07 | [`07_cpp17_guest`](examples/07_cpp17_guest/README.md) | ISO C++17 RAII guest mod with exception containment and multi-World isolation |
| 08 | [`08_zig_guest`](examples/08_zig_guest/README.md) | Zig guest mod (`@cImport` / `addTranslateC`) with error-union `BamResult` mapping |
| 09 | [`09_bql_wire_parity`](examples/09_bql_wire_parity/README.md) | 500-frame bit-identical C99 vs. Safe Rust `bevy_bql` wire parity + frame-250 v1→v2 column migration |
| 10 | [`10_bevy20_full_companion`](examples/10_bevy20_full_companion/README.md) | Full Bevy `0.20.1` companion stack (presets, `.wesl` materials, `SubApp`, BRP/reflect, savegames, telemetry) |

## Repository Contents

- `examples/01..10/` — Progressive host + guest mod examples (Rust, C99, C++17, Zig).
- `benches/` — Criterion chunk throughput, cross-language permutation benchmarks, and deterministic gates.
- `fuzz/` — LibFuzzer targets (`fuzz_sshsig_and_krl`, `fuzz_v03_relations`, `fuzz_bampack_manifest`, etc.).
- `tests/` — Integration contract suite (`bql_wire_contract_test.rs`; host lifecycle tests live in `bamabi-core/tests/` and `xtask/tests/`).
- `docs/book/` & `docs/BENCHMARKS.md` — mdBook developer guide and benchmark documentation.
- `xtask/`, `scripts/`, `Makefile` — Automation CLI and thin `make` wrapper targets.

## Prerequisites & Standalone vs. Local Overlay Workflow

**Standalone by default:** `[workspace.dependencies]` and all excluded sub-crates (`benches/*`, `examples/*/mods/*`, `fuzz/`) resolve Layer 0–3 crates directly from GitHub (`https://github.com/yaga-simha/bamabi-{wire,tools,core}.git` and `https://github.com/yaga-simha/bamabi.git`, `branch = "main"`). No sibling checkouts are required.

- **Toolchains:** Rust (`rust-toolchain.toml`, workspace MSRV `1.95`, guest mod MSRV `1.87`), a C99/C++17 compiler (`cc`/`c++`, `gcc`/`g++`, `clang`/`clang++`, or MSVC `cl.exe`), **Zig `0.16.0`** (for `examples/08_zig_guest`), and **`mdbook` `0.4.48+`** (for `docs/book`).
- **Linux system libraries** (for `example_05_cute_3d_winit` and `example_10_bevy20_full_companion` / `--workspace`):
  ```bash
  sudo apt-get install -y \
    libasound2-dev libudev-dev libx11-dev libxi-dev libxkbcommon-dev \
    libwayland-dev libegl-dev libxcursor-dev libxrandr-dev libxinerama-dev pkg-config
  ```
- **Local multi-repo sibling development (optional):** To override GitHub git dependencies with local sibling checkouts under a common parent directory without modifying any tracked `Cargo.toml` or `Cargo.lock`, use [`bamabi-dev-overlay`](https://github.com/yaga-simha/bamabi-dev-overlay.git):
  ```bash
  python3 ../bamabi-dev-overlay/overlay.py enable   # or --partial
  # ... develop across sibling bamabi-* repos ...
  python3 ../bamabi-dev-overlay/overlay.py disable  # restore standalone state before committing
  python3 ../bamabi-dev-overlay/sync_check.py --strict-git
  ```

### Local & CI Verification Commands

```bash
# List all Makefile / xtask shortcuts
make help

# Stage standalone C99 headers into target/include/ (or check ../bamabi-wire/include if present)
cargo xtask header --check --test-c99
cargo xtask gen-component-headers --check

# Build guest fixtures and run workspace + doctest + integration suites
cargo xtask fixtures --examples
BAM_FIXTURE_DIR="$PWD/target/fixtures" cargo test --workspace
cargo test --doc --all-features
cargo test --test bql_wire_contract_test
BAM_FIXTURE_DIR="$PWD/target/fixtures" cargo test -p xtask

# Run examples (headless smoke flags shown for interactive examples 04, 05, 10)
make example-1
make example-4-test
make example-5-test
cargo run -p example_07_cpp17_guest
cargo run -p example_08_zig_guest
cargo run -p example_09_bql_wire_parity
cargo run -p example_10_bevy20_full_companion -- --headless

# Static audits, fuzz clippy, Zig build, benchmarks & mdBook
typos
cargo deny --all-features check advisories licenses bans sources
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --manifest-path fuzz/Cargo.toml --bins -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
(cd examples/08_zig_guest/zig_mod && zig build -Doptimize=ReleaseFast)
cargo bench --manifest-path benches/deterministic_gate/Cargo.toml
mdbook build docs/book
```

## Cross-Repo Dependencies & Federated Repositories

| Layer | Repository | GitHub URL (`branch = "main"`) | Role |
|---|---|---|---|
| **0** | `bamabi-wire` | `https://github.com/yaga-simha/bamabi-wire.git` | Zero-Bevy ABI wire contract (`spec.md`), derives, `include/bamabi.h`, and Rust guest SDK |
| **1** | `bamabi-tools` | `https://github.com/yaga-simha/bamabi-tools.git` | Zero-Bevy Ed25519 trust (`bamabi_trust`), manifest DAG (`bamabi_manifest`), and `cargo-bam` CLI |
| **2** | `bamabi-core` | `https://github.com/yaga-simha/bamabi-core.git` | Headless `bevy_ecs` + `bevy_app` host runtime (`bamabi_host_sys`, `bamabi_host`, `bamabi_watcher`) and `docs/ARCHITECTURE.md` |
| **3** | `bamabi` | `https://github.com/yaga-simha/bamabi.git` | Canonical Bevy `0.20.1` host library facade (`bamabi`, `bamabi_bridges`, `bamabi_render`, `bamabi_reflect`) |
| **4** | **`bamabi-showcase`** *(this repo)* | `https://github.com/yaga-simha/bamabi-showcase.git` | **Top-level integration repository**: examples (`01`–`10`), benchmarks, fuzzing, wire-contract tests, mdBook guide, and `xtask` |
| — | `bamabi-dev-overlay` | `https://github.com/yaga-simha/bamabi-dev-overlay.git` | Multi-repo `sync_check.py` verifier and local Cargo `[patch]` sibling `overlay.py` manager |

## License

Dual-licensed under `MIT OR Apache-2.0`.
