# Example 11: `bamabi` Runtime Mod Chain Studio (`winit` + `wgpu` + Interactive REPL)

An extensive, repeatable 3D kinetic studio showcasing **`bamabi` (`ABI 0.3.0`, Bevy `0.20.1`)** across **11 independently compiled native shared libraries** (9 Rust `.cdylib`, 1 strict ISO C99, 1 strict ISO C++17) and **18 runtime-chainable ECS systems**.

## Capabilities Showcased

- **Runtime `.dll` / `.dylib` / `.so` Loading by Path (`shadow_copy = true`)**: Load ANY compiled mod from disk at runtime (`load /abs/or/rel/path/to/libmod_spin.dylib`), safe across macOS Apple Silicon code-signing, Linux inode caching, and Windows file locking.
- **Automatic Guest System Discovery**: Discovers every `GuestSystem` registered by ANY loaded library directly from Bevy's `Schedules` graph (`ModSystemSet::Mod(mod_id)`), with zero host hardcoding.
- **Sub-Millisecond Runtime System Chaining (`ModScheduleSetRewirePass`)**: Chain, insert, remove, swap, reverse, or switch execution modes (`sequence` / `weak` / `parallel`) live from the REPL using `ModScheduleSetRegistry` + `rewire_mod_schedule_sets_in_world` without re-initializing guest kernels.
- **Polyglot C-ABI Interop**: Chain ISO C99 (`C99PlanetaryOrbit`), ISO C++17 (`CppBounceGravity`), and Rust systems together in the exact same Bevy `Update` schedule.
- **Parallel Multi-Threaded Chunk Execution**: `mod_vortex::SpiralVortex` uses `ctx.par_chunks` across Bevy's `ComputeTaskPool`.
- **Buffered FFI `Messages<PulseEvent>`**: `mod_ripple`'s `EmitShockwave` and `ReactShockwave` communicate via zero-serialization Bevy message buffers.
- **Cross-Mod Dynamic Components + `ModScoped` `CommandArena` Spawning**: `mod_aura` registers `mod_aura::AuraChargeData` at runtime (zero host compile-time knowledge) and spawns `ModScoped` 3D spark particles; `mod_aura_link` discovers and mutates `AuraChargeData` by canonical name + 128-bit UUID; unloading `mod_aura` automatically purges all `ModScoped` sparks.
- **Hot-Reload with State Preservation**: `reload mod_spin` invokes `GuestPlugin::save_state` -> `restore_state` and rebinds the active chain to the new generation seamlessly.
- **Fault Isolation, Quarantine & Live Budget Watchdogs**: `mod_chaos::PanicFaultDemo` intentionally panics on run #5, is isolated and quarantined via `ModErrorPolicy::Quarantine` without crashing the 3D window, and recovers live via `reenable`. `ModBudgetPolicy` enforces live `p99` frame-time, spawn-rate, and `CommandArena` caps.
- **Repeatable Deterministic Reset & Run**: Reset the 3D arena (`reset [grid|ring|helix|sphere]`) as many times as you want while preserving loaded mods and your chain, or `reset all` to start from a blank slate.
- **Strict ECS-Only Mod Performance Telemetry (NO Rendering!)**: Measures `app.update()` wall-clock latency (`mean`/`p50`/`p99`/`min`/`max` µs), per-system `ModBudgetTelemetry` (`p50`/`p99`/`last` µs, matched entities, tables, chunks, queued commands, `CommandArena` peak bytes), aggregate throughput (`M entity-ops/sec`), and schedule rewire latency (`µs`).

---

## Quick Start

```bash
# Build all 11 Rust/C99/C++17 mods and launch the 3D winit + wgpu window + live terminal REPL
make studio
# or directly:
cargo run -p example_11_runtime_chain_studio --release -- --build-mods

# Start completely empty so you can load every .dylib/.so/.dll manually by path
cargo run -p example_11_runtime_chain_studio --release -- --empty

# Run headless interactive REPL (pure ECS + benchmarking, no GPU window)
cargo run -p example_11_runtime_chain_studio --release -- --headless

# Run full automated end-to-end verification (all 9 checks)
make example-11-test
```

---

## Bundled Native Mod Library (`11` Shared Libraries, `18` Systems)

| Shortcut | Shared Library Stem | Language | Registered Systems | Highlights |
|---|---|---|---|---|
| `@spin` | `mod_spin` | Rust (`cdylib`) | `SpinYaw`, `Tumble3D` | 3D rotation + `save_state`/`restore_state` hot-reload |
| `@color` | `mod_color` | Rust (`cdylib`) | `RainbowHue`, `EnergyHeatmap` | Order-sensitive rainbow & velocity/energy heatmap |
| `@scale` | `mod_scale` | Rust (`cdylib`) | `PulseScale`, `SquashStretch` | Harmonic pulse & volume-preserving squash-stretch |
| `@motion` | `mod_motion` | Rust (`cdylib`) | `WaveLift`, `IntegrateMove` | Traveling 3D surface wave & damped velocity integration |
| `@vortex` | `mod_vortex` | Rust (`par_chunks`) | `SpiralVortex`, `AttractCenter` | Parallel `ComputeTaskPool` vortex & roaming attractor |
| `@ripple` | `mod_ripple` | Rust (`Messages`) | `EmitShockwave`, `ReactShockwave` | Buffered FFI `Messages<PulseEvent>` radial shockwaves |
| `@aura` | `mod_aura` | Rust (`DynComp+Cmds`) | `AuraCharge`, `SpawnSparks` | Runtime cross-mod component + `ModScoped` sparks |
| `@aura_link` | `mod_aura_link` | Rust (`Cross-Mod`) | `AuraSynergy` | Resolves & mutates `mod_aura::AuraChargeData` by UUID |
| `@c99_orbit` | `mod_c99_orbit` | **Pure ISO C99** | `C99PlanetaryOrbit` | Multi-ring tilted Keplerian orbits over raw SoA columns |
| `@cpp_gravity` | `mod_cpp_gravity` | **ISO C++17** | `CppBounceGravity` | RAII + `try/catch` exception-contained 3D bounce gravity |
| `@chaos` | `mod_chaos` | Rust (`Fault/Stress`) | `PanicFaultDemo`, `HeavyLoadStress` | Intentional panic quarantine & p99 watchdog stress load |

---

## Example REPL Session

```text
bamabi> catalog
bamabi> load examples/11_runtime_chain_studio/mods/target/release/libmod_c99_orbit.dylib
bamabi> load examples/11_runtime_chain_studio/mods/target/release/libmod_cpp_gravity.dylib
bamabi> load examples/11_runtime_chain_studio/mods/target/release/libmod_spin.dylib
bamabi> load examples/11_runtime_chain_studio/mods/target/release/libmod_color.dylib
bamabi> load examples/11_runtime_chain_studio/mods/target/release/libmod_scale.dylib

bamabi> systems
bamabi> chain C99PlanetaryOrbit -> CppBounceGravity -> SpinYaw -> RainbowHue -> SquashStretch
bamabi> perf

# Reset deterministically and reverse or tweak the chain to compare behavior & ECS perf!
bamabi> reset helix
bamabi> chain swap 2 4
bamabi> chain mode parallel
bamabi> bench compare 200
bamabi> count 4000
bamabi> bench 300

# Live hot-reload with state preservation:
bamabi> reload mod_spin

# Fault isolation & live recovery:
bamabi> preset chaos
bamabi> events
bamabi> reenable mod_chaos PanicFaultDemo
```
