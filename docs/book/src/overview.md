# Overview & Threat Model (`ABI 0.3.0`)

**`bamabi`** (sole canonical host facade; the legacy `bevy_abi_mod` compatibility shim was removed in the federated 5-repo split) is an **asymmetric, zero-serialization `extern "C"` modding framework** for **Bevy ECS (`v0.20.1`)**. It lets a Bevy application load, execute, budget, quarantine, persist, and hot-unload native dynamic libraries (`.so`, `.dylib`, `.dll`) compiled with **different Rust compiler versions**, **different optimization flags (`-O0` vs. `--release`)**, or **non-Rust languages (ISO C99, C++17/20, Zig)** without sharing Rust ABI assumptions or serializing ECS tables.

---

## Why Asymmetric C-ABI Instead of Rust `dylib`s?

Standard Rust `crate-type = ["dylib"]` plugins share internal `#[repr(Rust)]` layouts, `TypeId` hashes, thread-local destructors, and heap allocators:
- **Compiler Lock-In:** Any difference in `rustc` commit, `Cargo.lock`, or `-C opt-level` scrambles `StableCrateId` and struct field order (`ABI_LOCKED`).
- **`dlclose` `SIGSEGV` Hazards:** Unmapping a coupled Rust `dylib` leaves `TypeId` drop vtables and TLS destructors pointing into unmapped pages.
- **Allocator Crossover:** Freeing a guest `String` or panic payload on the host's allocator (for example Windows per-DLL `HeapAlloc` or `mimalloc`) corrupts process heaps.

`bamabi` solves all three by placing a strict **`#[repr(C)]` wire boundary (`ABI 0.3.0`, `0x0003_0000`)** between host and guest:
1. **Complete 73/73 Live `BamHostApi` Handshake:** Every core and extended iterator, resource, message, command, checkpoint/transaction, observer, typed relationship, ancestor cascade, symbol table, named object, decoupled schedule set, schema migration, diagnostic, and registration entry point lives in a `struct_size`-extensible `Option<extern "C" fn(...)>` table (`592` B on 64-bit / `300` B on 32-bit; backwards-compatible `MIN_V0_2_SIZE` and `MIN_V0_3_SIZE = 552` B / `280` B). All **73/73** slots are 100% live and implemented, with target-native and cross-target byte offsets exported in `bamabi_sys::offsets` and `include/bamabi.h`.
2. **1-Based `BamComponent(u32)` Handles:** Components and resources resolve by canonical `"<namespace>::<Name>"` + 128-bit `ComponentUuid` (keyed `SipHash-2-4-128`) once during `bamabi_init`. Per-frame hot paths perform zero string hashing.
3. **Direct SoA & Batched Sparse-Set Views:** Dense `Table` columns are borrowed directly as `&[T]` / `&mut [T]` slices (`T: AbiPod`), and `SparseSet` columns yield batched indirect row-pointer slices.
4. **First-Class Bevy Parallelism:** Each guest system registers a native Bevy `GuestSystem` with exact `FilteredAccessSet` read/write locks, allowing non-conflicting host and mod systems to run concurrently on `ComputeTaskPool` (including `.chain_weak()` schedule sets and zero-arena `par_for_each_chunk_readonly`).

---

## Federated 5-Repository `-sys` vs. `#![forbid(unsafe_code)]` Split

The federated `bamabi` stack comprises **15 library/CLI crates** across Layers 0–3 (plus `bamabi-showcase` Layer 4 with `xtask`, examples, benchmarks, fuzzers, and this book), keeping `bamabi_host` slim and headless (zero `bevy_render`, `bevy_reflect`, `bevy_scene`, `bevy_asset`, or `notify` dependencies):

| Layer / Repo | Crate | `#![forbid(unsafe_code)]` | `bevy*` Allowed | Role |
|---|---|---|---|---|
| **L0 `bamabi-wire`** | `bamabi_sys` | No (`#![no_std]`) | **Never** | Zero-dependency `ABI 0.3.0` `#[repr(C)]` wire contract & `offsets` (`bamabi-wire/include/bamabi.h` source of truth). |
| **L0 `bamabi-wire`** | `bamabi_macros` | **Yes** | **Never** | `#[derive(ModComponent, AbiPod, GuestComponent)]` zero-padding assertions, `SCHEMA_FINGERPRINT`, `BAM_FIELDS`, and VTable/C/Zig codegen. |
| **L0 `bamabi-wire`** | `bamabi_sdk_gen` | **Yes** | **Never** | Single-source C99 (`bamabi-wire/include/bamabi.h`), C++17, Zig, and component header generator (`BAM_EXPORT`, `BAM_RESTRICT`, `BAM_HOST_API_OFFSET_*`). |
| **L0 `bamabi-wire`** | `bamabi_guest_sys` | No (isolates guest `unsafe`) | **Never** | Per-World `GuestInstance`, all 73 `Host` slot wrappers, `catch_guest_*` panic guards, lock-free closure trampolines, slice/migration validators. |
| **L0 `bamabi-wire`** | `bamabi_guest` | **Yes** | **Never** | Safe Rust guest SDK (`GuestPlugin`, `GuestApp`, `#[derive(AbiPod, GuestComponent)]`, typed `query_typed`/`single_query`, `ObserverBuilder::run_if`, `GuestTransaction`, 16-term `QueryTuple`, `Or<(T0..T7)>`, `export_mod!`). |
| **L1 `bamabi-tools`** | `bamabi_trust` | **Yes** | **Never** | Pure-Rust Ed25519 OpenSSH `SSHSIG`, CA certificate, authority-scoped `SSHKRL`, `.bam_sig`, and hardened `.bampack` (`VerifiedBamPack::extract_to_dir`) verifier. |
| **L1 `bamabi-tools`** | `bamabi_manifest` | **Yes** | **Never** | `mod.toml` manifest parser, path/symlink validator, and Kahn SemVer DAG topological loader (`ModTomlManifest`, `ModDependencyGraph`). |
| **L1 `bamabi-tools`** | `cargo-bam` | **Yes** | **Never** | Standalone CLI (`new`, `export-sdk`/`header`, `build`, `pack`, `watch`, `check-mods`, `doctor`, `keygen`, `sign`, `verify`). |
| **L2 `bamabi-core`** | `bamabi_host_sys` | No (isolates host `unsafe`) | Yes (core ECS) | 73/73 `HOST_API`, sealed `ffi/mod.rs` + `ffi/validation.rs`, `HostIter`, `CascadeHierarchyCache`, `ModRelationGraph`, `ModScheduleSetRegistry`, `CommandArena`, `GuestSystem`, observer `FallbackErrorHandler`/`ErrorContext::Observer`, `DROP_PAGES`, zero-copy reload & `migrate_component_column`. |
| **L2 `bamabi-core`** | `bamabi_host` | **Yes** | Yes (**headless only**) | Slim headless Bevy `AbiModPlugin`, `ModBudgetPolicy`, `ModUnloadHooks`, `ModWorldExt` (`despawn_all_where` incl. `Allow<Disabled>`), `ModAppExt`, `ModCommandsExt`, `LoadOptions`, `ComponentOptions::with_summary_tick()`. |
| **L2 `bamabi-core`** | `bamabi_watcher` | **Yes** | Yes (`notify`) | Debounced filesystem watcher (`ModHotReloadPlugin`) with cascading DAG hot-reloads. |
| **L3 `bamabi`** | `bamabi_bridges` | **Yes** | Yes (opt-in features) | Opt-in Bevy `0.20.1` bridges (`std_types`+`Disabled`, `state`, `asset`+`AssetOverlayPolicy`, `bsn`, `picking`, `bridge-camera-2d`, `camera_light`, `audio`, `ui`+real `feathers` BSN) + presets `preset-2d`/`3d`/`ui`/`full` (`ModPreset*Plugin`). |
| **L3 `bamabi`** | `bamabi_render` | **Yes** | Yes (render) | `ModSubAppPlugin<L>`, `ModWeslMaterialPlugin`, `ModSpriteMaterialPlugin`, opt-in `ModMeshShaderPlugin`, and `ModRenderRecoveryPlugin` (`DeviceLost` recovery). |
| **L3 `bamabi`** | `bamabi_reflect` | **Yes** | Yes (reflect/remote) | `ModReflectPlugin` (witness + declarative `BamFieldDesc` `ReflectFromPtr` + BRP + TOML), `ModSaveGamePlugin` (`HandleSerializeProcessor`/`HandleDeserializeProcessor`), and `ModTelemetryPlugin` (`bam/*` BRP + 64B Unix/loopback stream). |
| **L3 `bamabi`** | `bamabi` | **Yes** | Yes (via features) | **Sole canonical host facade** re-exporting `bamabi_host`, macros, companion crates (18 granular features + 4 presets), and unified `prelude` (`cargo add bamabi`). |

See [`bamabi-wire/spec.md`](https://github.com/yaga-simha/bamabi-wire/blob/main/spec.md), [`bamabi-core/docs/ARCHITECTURE.md`](https://github.com/yaga-simha/bamabi-core/blob/main/docs/ARCHITECTURE.md), and the [`bevy_bql` Lowering Contract](./bql-lowering-contract.md) for the complete module breakdown, wire contract, and memory reclamation diagrams.

---

## Threat Model & Native Trust Boundary

> **Important:** Native dynamic libraries loaded via `dlopen` / `LoadLibraryW` execute **in-process inside the host's virtual address space with full OS privileges of the host process**.

- **Trusted Native Code Only:** `bamabi` is **not** a security sandbox against adversarial or untrusted third-party binaries. Malicious machine code in a `.dll`/`.so`/`.dylib` can read arbitrary memory or invoke arbitrary OS syscalls.
- **What `bamabi` Guarantees:** For trusted or cooperative mods (studio gameplay modules, DLCs, decoupled debug/release iteration, locally opted-in native mods), `bamabi` enforces **compiler ABI independence**, **allocator isolation**, **compile-time `AbiPod` bit-validity**, **Bevy scheduler aliasing safety**, **per-mod resource/frame budgets (`ModBudgetPolicy`)**, **guest-side panic containment (`panic = "unwind"`)**, and **deterministic `dlclose` teardown**.
- **Pre-Load Signature Verification (Optional):** Enabling `signature-verification` attaches a `ModTrustPolicy` that verifies OpenSSH Ed25519 `SSHSIG` signatures, CA certificates, `.bam_sig` / `.bampack` artifacts, and `SSHKRL` revocation lists **before** `dlopen` / `LoadLibraryW` executes any static constructors.
- **Untrusted User Mods:** Games accepting untrusted mods from arbitrary internet sources should execute untrusted scripts in a **WebAssembly (Wasm)** sandbox and reserve `bamabi` for trusted or cryptographically signed native extensions.
