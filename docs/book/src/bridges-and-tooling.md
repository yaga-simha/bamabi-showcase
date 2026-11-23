# Bridges, Render, Reflect, Trust & `cargo-bam`

Beyond the slim headless ECS core (`bamabi_host`), the workspace ships four dedicated `#![forbid(unsafe_code)]` companion crates (`bamabi_bridges`, `bamabi_render`, `bamabi_reflect`, `bamabi_watcher`), pure-Rust cryptographic verification (`bamabi_trust`), `mod.toml` DAG resolution (`bamabi_manifest`), and the standalone `cargo-bam` CLI.

See [`examples/10_bevy20_full_companion`](../../../examples/10_bevy20_full_companion) for a complete runnable showcase wiring these companion capabilities together.

---

## 1. Developer CLI (`cargo-bam`)

Install `cargo-bam` from [`bamabi-tools`](https://github.com/yaga-simha/bamabi-tools):

```bash
cargo install --git https://github.com/yaga-simha/bamabi-tools.git --branch main cargo-bam
# Or from a local federated sibling checkout:
cargo install --path ../bamabi-tools/crates/cargo-bam
```

Subcommands:
- **`cargo bam new <name> --lang <rust|c99|cpp17|zig>`:** Scaffolds a standalone guest mod project (with `mod.toml` and build configuration) ready for host loading.
- **`cargo bam build [--dir <dir>] [--release] [--out-dir <dir>] [--sign-key <key>]`:** Compiles Rust (`cargo`), ISO C99, C++17, or Zig (`zig build`) guests, optionally staging the binary, `mod.toml`, and detached `.sig` sidecar under `<out-dir>`.
- **`cargo bam pack [--dir <dir>] [--release] [--out <file.bampack>] [--key <key>] [--assets-dir <dir>]`:** Builds (unless `--skip-build`) and emits a deterministic canonical `.bampack` archive containing the shared library, `mod.toml`, and optional scoped `assets/` (rejecting symlinks & traversal), optionally Ed25519-signed for `bamabi_trust`.
- **`cargo bam watch [--dir <dir>] [--release] [--out-dir <watch_dir>] [--key <key>] [--poll-ms <ms>]`:** Pure-`std` debounced source watcher for Rust, C99, C++17, and Zig guests that incrementally rebuilds, signs, and atomically stages outputs into `--out-dir` for `ModHotReloadPlugin`.
- **`cargo bam export-sdk --out <dir>`** *(alias: `cargo bam header`)*: Writes `bamabi.h` (C99), `bamabi.hpp` (C++17), and `bamabi.zig` (Zig) with `BAM_EXPORT`, `BAM_RESTRICT`, and all 73 `BAM_HOST_API_OFFSET_*` macros.
- **`cargo bam check-mods <dir>`:** Parses every `mod.toml` under `<dir>`, validates SemVer compatibility against host ABI `0.3.0`, and prints the resolved topological load order (or cycle/version errors).
- **`cargo bam doctor [--dir <dir>] [--require-c]`:** Probes host ABI `0.3.0` (`73`-slot `BamHostApi`), Rust target triple, MSVC `cl.exe` (`INCLUDE`/`LIB`/`vswhere.exe`), Clang, GCC, Zig (`0.14`–`0.16`), embedded SDK header parity, and optional `mod.toml` DAG health.
- **`cargo bam keygen` / `sign` / `verify`:** Generates OpenSSH Ed25519 keypairs, creates detached `SSHSIG` `.sig` sidecars, and verifies loose shared libraries as well as `.bampack` archives directly.

---

## 2. Manifests (`mod.toml`) & Dependency DAG Loading

Multi-mod directories use `mod.toml` manifests parsed by zero-Bevy `#![forbid(unsafe_code)]` crate `bamabi_manifest`:

```toml
[mod]
id = "combat_ai"
name = "Combat AI"
version = "0.3.0"
abi_version = "^0.3.0"
language = "rust"
entry = "target/release/libcombat_ai.dylib"

[dependencies]
core_stats = "^1.0"

[optional_dependencies]
hud_markers = ">=0.1"
```

Call `app.load_mods_dir("mods")` (or `app.load_mods_dir_with("mods", &options)` / `world.load_mods_dir(...)`) to resolve all `mod.toml` manifests in Kahn topological order, verify `abi_version` compatibility against `ABI 0.3.0`, and prevent unloading mods still required by active dependents.

---

## 3. Cryptographic Verification (`signature-verification`)

Enabling `signature-verification` on `bamabi` or `bamabi_host` wires the pure-Rust `#![forbid(unsafe_code)]` engine in `bamabi_trust` into the pre-`dlopen` load path:

- **Trusted Keys, CAs, & Revocation Lists (`ModTrustPolicy`):** Configure via `AbiModPlugin::with_trust_policy(policy)` or `set_mod_trust_policy(...)`. Supports pinned Ed25519 leaf keys, OpenSSH Root CA certificates, principal / `BamManifest::name` binding, direct revocations, binary `SSHKRL` (`PROTOCOL.krl`) lists, and authority-scoped signed KRLs (`DEFAULT_KRL_SIGNER_PRINCIPAL = "krl-signer"`, `add_krl_signer_openssh` / `add_krl_signer_file`) with monotonic anti-rollback versions.
- **Supported Payloads & Hardened Extraction:** Validates detached OpenSSH `SSHSIG` `.sig` sidecars, embedded `.bam_sig` sections, and canonical `.bampack` archives (`VerifiedBamPack::extract_to_dir` / `extract_to_dir_with_limits` enforcing entry/depth/byte limits and rejecting traversal/symlinks) **before** mapping native machine code (`dlopen` / `LoadLibraryW`), preventing untrusted static constructors from running. `bamabi_manifest` likewise validates mod identifiers, relative entry paths, and rejects symlinked paths.
- **Audit Component & Runtime Eviction:** Loaded mods carry a `VerifiedModSignature` component (signer fingerprint, CA fingerprint, key ID, principals, serial, namespace, binary SHA-256) on their `LoadedModInfo` entity. After updating a KRL or trust policy at runtime, call `app.evict_revoked_mods()` / `world.evict_revoked_mods()` to unload revoked live mods cleanly.

---

## 4. Curated Bridge Presets & Optional Host Features (`bamabi`)

Enable a curated preset (`preset-2d`, `preset-3d`, `preset-ui`, `preset-full`) or pick individual granular features on canonical **`bamabi`**:

| Preset Feature | Bundle Plugin | Included Bridge Features |
|---|---|---|
| `preset-2d` | `ModPreset2dPlugin` | `bridge-std-types`, `bridge-asset`, `bridge-state`, `bridge-picking`, `bridge-audio`, `bridge-camera-2d`, `bridge-ui` |
| `preset-3d` | `ModPreset3dPlugin` | `preset-2d`, `bridge-light`, `bridge-camera-controller`, `bridge-bsn` |
| `preset-ui` | `ModPresetUiPlugin` | `bridge-std-types`, `bridge-asset`, `bridge-picking`, `bridge-ui`, `bridge-feathers` |
| `preset-full` | `ModPresetFullPlugin` | `preset-3d`, `preset-ui`, `bridge-solari` |

| Feature | Crate | Primary Plugins / Types | What It Enables |
|---|---|---|---|
| `strict-ffi-checks` | `bamabi_host` | Scoped handle poisoning | Debug/audit poisoning of `BamLoadCtx`, `BamSystemCtx`, `BamIter`, and `BamCommands` headers on return |
| `diagnostics` | `bamabi_host` | `bevy_diagnostic` metrics | Per-mod and aggregate `cpu_time_us`, `p99_time_us`, `fault_count`, `commands_queued`, `budget_breach_count`, `spawns_queued`, `arena_bytes_peak` |
| `signature-verification` | `bamabi_trust` | `ModTrustPolicy`, `VerifiedModSignature` | Pre-load OpenSSH Ed25519 `SSHSIG`, CA cert, authority-scoped `SSHKRL`, `.bam_sig`, and hardened `.bampack` verification |
| `hot-reload-watcher` | `bamabi_watcher` | `ModHotReloadPlugin` | Debounced recursive filesystem watcher with cascading SemVer DAG reloads/unloads |
| `bridge-std-types` | `bamabi_bridges` | `StdTypesBridgePlugin`, `ModInputExtBridgePlugin` | `Transform`, `GlobalTransform`, `Visibility`, `Name`, `Time<*>`, `ButtonInput<KeyCode\|MouseButton>`, `Disabled`, `FfiMouseMotion`/`FfiMouseScroll` |
| `bridge-state` | `bamabi_bridges` | `ModStateExt`, `ModStateSlab` | Unbounded generation-indexed dynamic states (`States`, `SubStates`, read-only `ComputedStates`), transition schedules, `in_state` sets |
| `bridge-asset` | `bamabi_bridges` | `ModAssetPlugin`, `AssetOverlayPolicy` | Sandboxed `mod://<namespace>/...` `AssetSource`, refcounted `FfiAssetHandle`, and opt-in `mod://overlay/<rel_path>` (`LoadOrderPriority`/`TopologicalOverlay`/`ExplicitPriority`) with live reload |
| `bridge-bsn` | `bamabi_bridges` | `ModBsnPlugin` | Bevy `0.20` `bsn!` scenes, `ScenePatch`, `cmd_spawn_scene`, and `Ready` observers |
| `bridge-picking` | `bamabi_bridges` | `ModPickingBridgePlugin`, `ModInputExtBridgePlugin` | All 7 Bevy `0.20` flattened `Pointer*` observers with `PointerTraversal` bubbling, bidirectional `FfiEntityCursor`, and `Gamepad` + rumble |
| `bridge-audio` | `bamabi_bridges` | `ModAudioBridgePlugin` | `FfiAudioPlaybackSettings`, `FfiAudioPlayerSpec` (`AudioPlayer<AudioSource>`), and `FfiSpatialAudioSinkControl` (`SpatialAudioSink`/`AudioSink`) |
| `bridge-camera-2d` | `bamabi_bridges` | `FfiCamera2d`, `ModCameraLightBridgePlugin` | Lightweight `Camera2d` bridge without pulling `bevy_pbr` or `bevy_light` |
| `bridge-light` | `bamabi_bridges` | `ModCameraLightBridgePlugin` | `Camera2d`, `Camera3d` (`Tonemapping`, `DebandDither`, `Exposure`, `Hdr`, `ScreenSpaceTransmission`), and point/spot/directional lights (implies `bridge-camera-2d`) |
| `bridge-camera-controller` | `bamabi_bridges` | `FfiPanOrbitCamera` | Bevy `0.20` `PanOrbitCamera` controller parameter sync preserving active motion |
| `bridge-solari` | `bamabi_bridges` | `FfiSolariLighting` | Bevy `0.20` `SolariLighting` realtime global illumination parameter sync |
| `bridge-ui` | `bamabi_bridges` | `ModUiBridgePlugin`, `ModUiRegistry` | Retained UI (`Val::Em`/`Rem`, `FixedNode`, elliptical `BorderRadius`, split `EditableText`/`TextInput`, `InlineBox`/`InlineImage`, headless `TabList`/`ValueChange`) with automatic unload teardown |
| `bridge-feathers` | `bamabi_bridges` | `FfiThemeContext`, `FfiFeathers*` | Real `bevy_feathers` BSN-hydrated controls (`ColorInput`, `Select`/`Dropdown`, `ListView`, `NumberInput`, `ThemeContext`; implies `bridge-ui`) |
| `sub-app-extract` | `bamabi_render` | `ModSubAppPlugin<L>`, `ModWeslMaterialPlugin`, `ModSpriteMaterialPlugin`, `ModMeshShaderPlugin`, `ModRenderRecoveryPlugin` | Cross-world `SubApp` extraction, `.wesl` shaders + `AlignedVec` `ShaderBuffer`, 2D `SpriteMaterial`/`ExtendedMaterial2d` & unified `Mesh2d`, 3D `StandardMaterial`, opt-in mesh-shader pipelines, and bounded `DeviceLost` recovery (`RenderErrorPolicy::Recover` + `RenderStartup` cache reset) |
| `bridge-reflect` | `bamabi_reflect` | `ModReflectPlugin`, `ModSaveGamePlugin`, `ModTelemetryPlugin` | Witness + declarative `BamFieldDesc` `ReflectFromPtr` & BRP + `bevy-settings` TOML; versioned JSON/`BAMSAVE1` persistent save archives (`StrictRequireMods`/`StripMissingModComponents`/`MigrateOnLoad` + Bevy `HandleSerializeProcessor`/`HandleDeserializeProcessor` & `EphemeralHandleBehavior`); and `bam/*` BRP + 64-byte Unix/loopback telemetry streaming |
