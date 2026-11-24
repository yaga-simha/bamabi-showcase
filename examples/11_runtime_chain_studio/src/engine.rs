#![forbid(unsafe_code)]

//! Core ECS modding engine, runtime system discovery, `ModScheduleSetRewirePass` hot-rewiring,
//! deterministic scene reset, budget watchdog control, and ECS-only performance telemetry.

use crate::components::{
    ActorMeta, ChainGate, Formation, Motion3D, PulseEvent, SimClock, SparkTag, StudioActor,
    SystemIdKey, Transform3D, VisualStyle,
};
use bamabi::prelude::*;
use bamabi_host_sys::schedule::{
    ModScheduleSetRegistry, ScheduleSetConfig, ScheduleSetMode, install_mod_schedule_set_pass,
    rewire_mod_schedule_sets_in_world,
};
use bevy_app::{App, PreUpdate, TaskPoolPlugin, Update};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::schedule::graph::Direction;
use bevy_ecs::schedule::{IntoScheduleConfigs, NodeId, Schedules, SystemSet};
use bevy_ecs::system::ResMut;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

/// Maximum rolling ECS tick samples retained for live percentile statistics.
const PERF_WINDOW_LEN: usize = 120;
/// Maximum lifecycle/fault/budget events kept in the studio event log.
const EVENT_LOG_CAP: usize = 64;

/// Catalog entry for one of the 11 bundled multi-language studio mods.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinModSpec {
    pub short_name: &'static str,
    pub binary_stem: &'static str,
    pub language: &'static str,
    pub systems: &'static str,
    pub summary: &'static str,
}

/// Complete catalog of bundled studio mods (9 Rust + 1 ISO C99 + 1 ISO C++17).
pub const BUILTIN_MODS: &[BuiltinModSpec] = &[
    BuiltinModSpec {
        short_name: "spin",
        binary_stem: "mod_spin",
        language: "Rust",
        systems: "SpinYaw, Tumble3D",
        summary: "3D yaw/tumble rotation + save_state/restore_state across hot-reload",
    },
    BuiltinModSpec {
        short_name: "color",
        binary_stem: "mod_color",
        language: "Rust",
        systems: "RainbowHue, EnergyHeatmap",
        summary: "Order-sensitive prismatic rainbow & velocity/energy thermal shader",
    },
    BuiltinModSpec {
        short_name: "scale",
        binary_stem: "mod_scale",
        language: "Rust",
        systems: "PulseScale, SquashStretch",
        summary: "Harmonic 3D breathing & volume-preserving elastic squash-stretch",
    },
    BuiltinModSpec {
        short_name: "motion",
        binary_stem: "mod_motion",
        language: "Rust",
        systems: "WaveLift, IntegrateMove",
        summary: "Traveling 3D surface wave lift & velocity integration with arena bounds",
    },
    BuiltinModSpec {
        short_name: "vortex",
        binary_stem: "mod_vortex",
        language: "Rust (par_chunks)",
        systems: "SpiralVortex, AttractCenter",
        summary: "Parallel ComputeTaskPool par_chunks spiral vortex & figure-8 attractor",
    },
    BuiltinModSpec {
        short_name: "ripple",
        binary_stem: "mod_ripple",
        language: "Rust (Messages)",
        systems: "EmitShockwave, ReactShockwave",
        summary: "Buffered FFI Messages<PulseEvent> shockwave emitter & radial wavefront",
    },
    BuiltinModSpec {
        short_name: "aura",
        binary_stem: "mod_aura",
        language: "Rust (DynComp+Cmds)",
        systems: "AuraCharge, SpawnSparks",
        summary: "Dynamic cross-mod component AuraChargeData + ModScoped CommandArena sparks",
    },
    BuiltinModSpec {
        short_name: "aura_link",
        binary_stem: "mod_aura_link",
        language: "Rust (Cross-Mod)",
        systems: "AuraSynergy",
        summary: "Discovers & mutates mod_aura::AuraChargeData across mods via UUID",
    },
    BuiltinModSpec {
        short_name: "c99_orbit",
        binary_stem: "mod_c99_orbit",
        language: "ISO C99",
        systems: "C99PlanetaryOrbit",
        summary: "Pure C99 zero-serialization SoA tilted multi-ring Keplerian orbits",
    },
    BuiltinModSpec {
        short_name: "cpp_gravity",
        binary_stem: "mod_cpp_gravity",
        language: "ISO C++17",
        systems: "CppBounceGravity",
        summary: "Pure C++17 RAII + try/catch exception-contained 3D bounce gravity",
    },
    BuiltinModSpec {
        short_name: "chaos",
        binary_stem: "mod_chaos",
        language: "Rust (Fault/Stress)",
        systems: "PanicFaultDemo, HeavyLoadStress",
        summary: "Intentional panic quarantine test & heavy CPU p99 budget-watchdog load",
    },
];

/// Platform-native shared library prefix & extension.
pub fn dylib_naming() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("", "dll")
    } else if cfg!(target_os = "macos") {
        ("lib", "dylib")
    } else {
        ("lib", "so")
    }
}

/// Discovers the directory holding `examples/11_runtime_chain_studio/mods`.
pub fn studio_mods_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let direct = manifest_dir.join("mods");
    if direct.is_dir() {
        return direct;
    }
    let rel = PathBuf::from("examples/11_runtime_chain_studio/mods");
    if rel.is_dir() {
        return rel;
    }
    PathBuf::from("mods")
}

/// Returns the expected shared library path for a bundled mod stem (e.g. `"mod_spin"`).
pub fn builtin_mod_binary_path(stem: &str) -> PathBuf {
    let (prefix, ext) = dylib_naming();
    studio_mods_dir()
        .join("target")
        .join("release")
        .join(format!("{prefix}{stem}.{ext}"))
}

/// Resolves or stages `bamabi.h` so C99 and C++17 studio mods can always compile standalone.
fn resolve_or_stage_include_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("BAM_INCLUDE_DIR")
        && !dir.trim().is_empty()
    {
        let p = PathBuf::from(dir);
        if p.join("bamabi.h").is_file() {
            return Ok(p);
        }
    }

    let mods_dir = studio_mods_dir();
    for ancestor in mods_dir.ancestors() {
        let sib = ancestor.join("bamabi-wire").join("include");
        if sib.join("bamabi.h").is_file() {
            return Ok(sib);
        }
        let target_inc = ancestor.join("target").join("include");
        if target_inc.join("bamabi.h").is_file() {
            return Ok(target_inc);
        }
    }

    let staged = mods_dir.join("target").join("include");
    std::fs::create_dir_all(&staged)
        .map_err(|e| format!("failed to create {}: {e}", staged.display()))?;
    let header = bamabi_sdk_gen::render_c99_header()?;
    std::fs::write(staged.join("bamabi.h"), header)
        .map_err(|e| format!("failed to write staged bamabi.h: {e}"))?;
    Ok(staged)
}

/// Compiles all 11 bundled Rust, ISO C99, and ISO C++17 studio mods into `mods/target/release/`.
pub fn build_all_studio_mods() -> Result<String, String> {
    let mods_dir = studio_mods_dir();
    let manifest = mods_dir.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(format!(
            "Studio mods Cargo.toml not found at {}",
            manifest.display()
        ));
    }

    let t0 = Instant::now();
    let status = Command::new("cargo")
        .args(["build", "--manifest-path"])
        .arg(&manifest)
        .arg("--release")
        .status()
        .map_err(|e| format!("failed to run cargo build for studio mods: {e}"))?;
    if !status.success() {
        return Err("cargo build for studio Rust mods failed".to_string());
    }

    let inc_dir = resolve_or_stage_include_dir()?;
    let out_dir = mods_dir.join("target").join("release");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("failed to create {}: {e}", out_dir.display()))?;

    // Compile C99 mod_c99_orbit
    let c_src = mods_dir.join("mod_c99_orbit").join("mod.c");
    let c_out = builtin_mod_binary_path("mod_c99_orbit");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let mut c_cmd = Command::new(&cc);
    c_cmd
        .args(["-std=c99", "-Wall", "-Wextra", "-Werror", "-O2", "-shared"])
        .arg(format!("-I{}", inc_dir.display()));
    if !cfg!(target_os = "windows") {
        c_cmd.arg("-fPIC");
    }
    c_cmd.arg(&c_src).arg("-o").arg(&c_out);
    if !cfg!(target_os = "windows") {
        c_cmd.arg("-lm");
    }
    let c_status = c_cmd
        .status()
        .map_err(|e| format!("failed to invoke C compiler `{cc}`: {e}"))?;
    if !c_status.success() {
        return Err(format!("C99 compilation failed for {}", c_src.display()));
    }

    // Compile C++17 mod_cpp_gravity
    let cpp_src = mods_dir.join("mod_cpp_gravity").join("mod.cpp");
    let cpp_out = builtin_mod_binary_path("mod_cpp_gravity");
    let cxx = std::env::var("CXX").unwrap_or_else(|_| "c++".to_string());
    let mut cpp_cmd = Command::new(&cxx);
    cpp_cmd
        .args([
            "-std=c++17",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-O2",
            "-shared",
        ])
        .arg(format!("-I{}", inc_dir.display()));
    if !cfg!(target_os = "windows") {
        cpp_cmd.arg("-fPIC");
    }
    cpp_cmd.arg(&cpp_src).arg("-o").arg(&cpp_out);
    if !cfg!(target_os = "windows") {
        cpp_cmd.arg("-lm");
    }
    let cpp_status = cpp_cmd
        .status()
        .map_err(|e| format!("failed to invoke C++ compiler `{cxx}`: {e}"))?;
    if !cpp_status.success() {
        return Err(format!(
            "C++17 compilation failed for {}",
            cpp_src.display()
        ));
    }

    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    Ok(format!(
        "Built all {} studio mods (Rust + ISO C99 + ISO C++17) in {:.0} ms -> {}",
        BUILTIN_MODS.len(),
        elapsed_ms,
        out_dir.display()
    ))
}

/// Ensures the requested binary exists; if it matches a bundled mod and is missing, builds mods once.
pub fn resolve_mod_input_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return Err("empty mod path".to_string());
    }

    // Direct filesystem path check first
    let direct = PathBuf::from(trimmed);
    if direct.is_file() {
        return Ok(direct);
    }

    // Support `@spin` or `spin` or `mod_spin` convenience resolution to the built shared library
    let normalized = trimmed.strip_prefix('@').unwrap_or(trimmed);
    if let Some(spec) = BUILTIN_MODS.iter().find(|s| {
        s.short_name.eq_ignore_ascii_case(normalized)
            || s.binary_stem.eq_ignore_ascii_case(normalized)
    }) {
        let bin = builtin_mod_binary_path(spec.binary_stem);
        if !bin.is_file() {
            build_all_studio_mods()?;
        }
        if bin.is_file() {
            return Ok(bin);
        }
    }

    Err(format!(
        "mod shared library not found at `{trimmed}`. Run `catalog` to see built library paths or `build-mods` to compile them."
    ))
}

/// Lightweight per-entity render snapshot extracted for the 3D `wgpu` renderer.
#[derive(Clone, Copy, Debug)]
pub struct ActorRenderData {
    pub transform: Transform3D,
    pub style: VisualStyle,
    pub is_spark: bool,
}

/// Metadata for a single dynamically discovered guest system belonging to a loaded mod.
#[derive(Debug, Clone)]
pub struct DiscoveredSystem {
    pub mod_id: ModId,
    pub mod_name: String,
    pub system_name: String,
    pub named_set: ModNamedSystemSet,
}

impl DiscoveredSystem {
    pub fn key(&self) -> SystemIdKey {
        SystemIdKey::new(&self.mod_name, &self.system_name)
    }

    pub fn qualified(&self) -> String {
        format!("{}::{}", self.mod_name, self.system_name)
    }
}

/// Summary of a loaded mod for HUD & REPL display.
#[derive(Debug, Clone)]
pub struct ModSummary {
    pub id: ModId,
    pub name: String,
    pub version: String,
    pub generation: u64,
    pub system_count: usize,
    pub fault_count: usize,
    pub source_path: PathBuf,
}

/// Point-in-time ECS-only performance summary (strictly excluding rendering!).
#[derive(Debug, Clone)]
pub struct EcsPerfSummary {
    pub tick: u64,
    pub actor_count: usize,
    pub spark_count: usize,
    pub ecs_tick_last_us: f64,
    pub ecs_tick_mean_us: f64,
    pub ecs_tick_p50_us: f64,
    pub ecs_tick_p99_us: f64,
    pub ecs_tick_min_us: f64,
    pub ecs_tick_max_us: f64,
    pub mod_sum_last_us: u64,
    pub mod_sum_p50_us: u64,
    pub mod_sum_p99_us: u64,
    pub aggregate_m_ents_per_sec: f64,
    pub last_rewire_us: f64,
    pub last_load_us: f64,
    pub systems: Vec<ModBudgetSnapshot>,
}

/// Host `PreUpdate` system advancing `SimClock` on every simulation tick.
fn advance_sim_clock(mut clock: ResMut<SimClock>) {
    clock.tick = clock.tick.wrapping_add(1);
    clock.elapsed += clock.dt;
}

/// Computes deterministic initial `(Transform3D, Motion3D, VisualStyle, ActorMeta)` for actor `i` of `n`.
fn initial_actor_state(
    i: usize,
    n: usize,
    formation: Formation,
) -> (Transform3D, Motion3D, VisualStyle, ActorMeta) {
    let fi = i as f32;
    let fn_total = (n.max(1)) as f32;
    let frac = fi / fn_total;

    let (px, py, pz, group) = match formation {
        Formation::Grid => {
            let side = (fn_total.sqrt().ceil() as usize).max(1);
            let row = i / side;
            let col = i % side;
            let span = 14.0_f32;
            let step = if side > 1 {
                span / ((side - 1) as f32)
            } else {
                0.0
            };
            let x = (col as f32) * step - span * 0.5;
            let z = (row as f32) * step - span * 0.5;
            (x, 1.15, z, ((row + col) % 4) as u32)
        }
        Formation::Ring => {
            let rings = 4usize;
            let ring = i % rings;
            let per_ring = (n / rings).max(1);
            let slot = i / rings;
            let angle =
                (slot as f32) / (per_ring as f32) * core::f32::consts::TAU + (ring as f32) * 0.35;
            let radius = 2.4 + (ring as f32) * 2.25;
            (
                radius * angle.cos(),
                1.0 + (ring as f32) * 0.45,
                radius * angle.sin(),
                ring as u32,
            )
        }
        Formation::Helix => {
            let strand = i % 2;
            let turns = 4.5_f32;
            let angle =
                frac * turns * core::f32::consts::TAU + (strand as f32) * core::f32::consts::PI;
            let radius = 3.2 + (frac * core::f32::consts::PI).sin() * 2.8;
            let y = 0.65 + frac * 7.2;
            (radius * angle.cos(), y, radius * angle.sin(), strand as u32)
        }
        Formation::Sphere => {
            // Fibonacci sphere centered at y = 4.2
            let golden = core::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
            let y_unit = 1.0 - (fi + 0.5) / fn_total * 2.0;
            let r_xz = (1.0 - y_unit * y_unit).max(0.0).sqrt();
            let theta = golden * fi;
            let radius = 4.8;
            (
                r_xz * theta.cos() * radius,
                5.4 + y_unit * radius,
                r_xz * theta.sin() * radius,
                (i % 4) as u32,
            )
        }
    };

    let shape = (i % 3) as u32;
    let hue = (frac + (group as f32) * 0.17).rem_euclid(1.0);
    let (r, g, b) = hsv_to_rgb_host(hue, 0.72, 0.92);

    let xf = Transform3D {
        px,
        py,
        pz,
        rx: 0.0,
        ry: frac * core::f32::consts::TAU,
        rz: 0.0,
        sx: 1.0,
        sy: 1.0,
        sz: 1.0,
        shape,
    };
    let mot = Motion3D {
        vx: 0.0,
        vy: 0.0,
        vz: 0.0,
        avx: 0.0,
        avy: 0.0,
        avz: 0.0,
        speed: 1.0,
        phase: frac * core::f32::consts::TAU,
    };
    let vis = VisualStyle {
        r,
        g,
        b,
        a: 1.0,
        emissive: 0.22,
        hue,
        energy: 0.25,
        pulse: 0.0,
    };
    let meta = ActorMeta {
        index: i as u32,
        group,
        home_x: px,
        home_y: py,
        home_z: pz,
        flags: 0,
    };
    (xf, mot, vis, meta)
}

#[inline]
fn hsv_to_rgb_host(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h_wrapped = h.rem_euclid(1.0) * 6.0;
    let sector = h_wrapped.floor() as i32;
    let frac = h_wrapped - (sector as f32);
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * frac);
    let t = v * (1.0 - s * (1.0 - frac));
    match sector.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

/// Central host engine driving Bevy ECS, `bamabi` mod lifecycle, runtime schedule-set chaining,
/// scene reset/formations, budget watchdogs, and ECS-only performance telemetry.
pub struct StudioEngine {
    pub app: App,
    pub formation: Formation,
    pub target_actor_count: usize,
    pub chain_handle: BamSystemSet,
    pub chain_mode: ScheduleSetMode,
    pub chain: Vec<DiscoveredSystem>,
    pub discovered: Vec<DiscoveredSystem>,
    gated_sets: HashSet<ModNamedSystemSet>,
    ecs_tick_samples_us: VecDeque<f64>,
    ecs_tick_min_us: f64,
    ecs_tick_max_us: f64,
    pub last_rewire_us: f64,
    pub last_load_us: f64,
    pub event_log: VecDeque<String>,
    pub perf_watch: bool,
    pub run_count: u32,
}

impl StudioEngine {
    /// Creates a fully configured `bamabi` ECS studio world with `actor_count` initial 3D entities.
    pub fn new(actor_count: usize, formation: Formation) -> Result<Self, String> {
        let mut app = App::new();
        app.add_plugins(TaskPoolPlugin::default());
        app.add_plugins(
            AbiModPlugin::with_error_policy(ModErrorPolicy::Quarantine)
                .with_budget_policy(ModBudgetPolicy::unlimited()),
        );
        app.init_resource::<SimClock>();
        app.init_resource::<ChainGate>();
        app.init_resource::<ModScheduleSetRegistry>();

        app.register_mod_component::<Transform3D>(
            ComponentOptions::new()
                .with_raw_insert()
                .with_default()
                .with_summary_tick(),
        )
        .map_err(|e| format!("register Transform3D: {e}"))?;
        app.register_mod_component::<Motion3D>(
            ComponentOptions::new()
                .with_raw_insert()
                .with_default()
                .with_summary_tick(),
        )
        .map_err(|e| format!("register Motion3D: {e}"))?;
        app.register_mod_component::<VisualStyle>(
            ComponentOptions::new()
                .with_raw_insert()
                .with_default()
                .with_summary_tick(),
        )
        .map_err(|e| format!("register VisualStyle: {e}"))?;
        app.register_mod_component::<ActorMeta>(
            ComponentOptions::new()
                .with_raw_insert()
                .with_default()
                .with_summary_tick(),
        )
        .map_err(|e| format!("register ActorMeta: {e}"))?;
        app.register_mod_component::<SparkTag>(
            ComponentOptions::new().with_raw_insert().with_default(),
        )
        .map_err(|e| format!("register SparkTag: {e}"))?;
        app.register_mod_resource::<SimClock>()
            .map_err(|e| format!("register SimClock: {e}"))?;
        app.register_mod_message::<PulseEvent>()
            .map_err(|e| format!("register PulseEvent: {e}"))?;

        app.add_systems(PreUpdate, advance_sim_clock);

        // Configure master host chain container in ModScheduleSetRegistry for sub-millisecond
        // live schedule-only hot-rewiring (`ModScheduleSetRewirePass`).
        let chain_handle = app
            .world_mut()
            .resource_mut::<ModScheduleSetRegistry>()
            .configure_set(ScheduleSetConfig {
                mod_id: None,
                mod_name: Arc::from("studio_host"),
                set_name: Arc::from("runtime_chain"),
                generation: 0,
                schedule: ScheduleId::UPDATE,
                mode: ScheduleSetMode::Sequence,
                ..Default::default()
            })
            .map_err(|(code, msg)| format!("configure host chain set ({code}): {msg}"))?;

        {
            let mut schedules = app.world_mut().resource_mut::<Schedules>();
            let update_sched = schedules.entry(Update);
            install_mod_schedule_set_pass(update_sched, ScheduleId::UPDATE);
        }

        app.finish();
        app.cleanup();

        let mut engine = Self {
            app,
            formation,
            target_actor_count: actor_count.clamp(1, 30_000),
            chain_handle,
            chain_mode: ScheduleSetMode::Sequence,
            chain: Vec::new(),
            discovered: Vec::new(),
            gated_sets: HashSet::new(),
            ecs_tick_samples_us: VecDeque::with_capacity(PERF_WINDOW_LEN),
            ecs_tick_min_us: f64::MAX,
            ecs_tick_max_us: 0.0,
            last_rewire_us: 0.0,
            last_load_us: 0.0,
            event_log: VecDeque::with_capacity(EVENT_LOG_CAP),
            perf_watch: false,
            run_count: 1,
        };

        engine.spawn_initial_actors();
        Ok(engine)
    }

    fn push_event(&mut self, msg: String) {
        if self.event_log.len() >= EVENT_LOG_CAP {
            self.event_log.pop_front();
        }
        self.event_log.push_back(msg);
    }

    /// Spawns `target_actor_count` primary studio actors in `self.formation`.
    fn spawn_initial_actors(&mut self) {
        let count = self.target_actor_count;
        let formation = self.formation;
        {
            let world = self.app.world_mut();
            for i in 0..count {
                let (xf, mot, vis, meta) = initial_actor_state(i, count, formation);
                world.spawn((StudioActor, xf, mot, vis, meta));
            }
            let mut clock = world.resource_mut::<SimClock>();
            clock.actor_count = count as u32;
        }
    }

    /// Resets the 3D scene deterministically (positions, velocities, rotations, scales, colors,
    /// clock, sparks, and per-run ECS perf accumulators) while keeping all loaded mods & chain!
    pub fn reset_scene(
        &mut self,
        new_formation: Option<Formation>,
        new_count: Option<usize>,
    ) -> String {
        if let Some(f) = new_formation {
            self.formation = f;
        }
        if let Some(c) = new_count {
            self.target_actor_count = c.clamp(1, 30_000);
        }

        // Despawn all existing studio actors and mod-spawned sparks
        let to_despawn: Vec<Entity> = {
            let world = self.app.world_mut();
            let mut q1 = world.query_filtered::<Entity, bevy_ecs::query::With<StudioActor>>();
            let mut list: Vec<Entity> = q1.iter(world).collect();
            let mut q2 = world.query_filtered::<Entity, bevy_ecs::query::With<SparkTag>>();
            list.extend(q2.iter(world));
            list
        };
        for e in to_despawn {
            let _ = self.app.world_mut().despawn(e);
        }

        // Reset SimClock while preserving speed_scale & amplitude
        {
            let mut clock = self.app.world_mut().resource_mut::<SimClock>();
            clock.tick = 0;
            clock.elapsed = 0.0;
            clock.active_chain_len = self.chain.len() as u32;
            clock.actor_count = self.target_actor_count as u32;
        }

        self.spawn_initial_actors();
        self.clear_perf_window();
        self.run_count = self.run_count.saturating_add(1);
        let msg = format!(
            "Reset scene (run #{}) -> {} actors in `{}` formation (loaded mods & chain preserved).",
            self.run_count,
            self.target_actor_count,
            self.formation.name()
        );
        self.push_event(msg.clone());
        msg
    }

    /// Unloads all mods, clears chain & budgets, and resets the scene cleanly.
    pub fn hard_reset(&mut self) -> Result<String, String> {
        let mod_ids: Vec<ModId> = self
            .app
            .world()
            .resource::<ModHost>()
            .loaded_mods()
            .iter()
            .map(LoadedMod::id)
            .collect();
        for id in mod_ids.into_iter().rev() {
            let _ = self.unload_mod_by_id(id);
        }
        self.chain.clear();
        self.discovered.clear();
        self.gated_sets.clear();
        {
            let mut gate = self.app.world_mut().resource_mut::<ChainGate>();
            gate.chained.clear();
            gate.disabled.clear();
            gate.solo_chain = true;
        }
        self.app
            .world_mut()
            .set_mod_budget_policy(ModBudgetPolicy::unlimited());
        self.apply_chain_rewire()?;
        let _ = self.reset_scene(None, None);
        let msg = format!(
            "Hard reset complete: all mods unloaded, chain cleared, budgets unlimited, {} actors in `{}`.",
            self.target_actor_count,
            self.formation.name()
        );
        self.push_event(msg.clone());
        Ok(msg)
    }

    /// Clears rolling ECS performance window without resetting entity state.
    pub fn clear_perf_window(&mut self) {
        self.ecs_tick_samples_us.clear();
        self.ecs_tick_min_us = f64::MAX;
        self.ecs_tick_max_us = 0.0;
    }

    /// Runs ONE pure ECS simulation tick, timing strictly `app.update()` (zero GPU/render overhead!).
    pub fn step_ecs_tick(&mut self) -> f64 {
        let t0 = Instant::now();
        self.app.update();
        let us = t0.elapsed().as_secs_f64() * 1_000_000.0;

        if self.ecs_tick_samples_us.len() >= PERF_WINDOW_LEN {
            self.ecs_tick_samples_us.pop_front();
        }
        self.ecs_tick_samples_us.push_back(us);
        self.ecs_tick_min_us = self.ecs_tick_min_us.min(us);
        self.ecs_tick_max_us = self.ecs_tick_max_us.max(us);

        self.drain_lifecycle_messages();
        us
    }

    /// Drains `ModLoaded`, `ModUnloaded`, `ModSystemFaulted`, and `ModBudgetBreach` messages.
    fn drain_lifecycle_messages(&mut self) {
        let mut new_events = Vec::new();
        {
            let world = self.app.world_mut();
            if let Some(mut msgs) = world.get_resource_mut::<Messages<ModLoaded>>() {
                for ev in msgs.drain() {
                    new_events.push(format!(
                        "[ModLoaded] {} v{} ({:?}, {} sys)",
                        ev.name, ev.version, ev.id, ev.system_count
                    ));
                }
            }
            if let Some(mut msgs) = world.get_resource_mut::<Messages<ModUnloaded>>() {
                for ev in msgs.drain() {
                    new_events.push(format!(
                        "[ModUnloaded] {} ({:?}, removed {} sys)",
                        ev.name, ev.id, ev.removed_systems
                    ));
                }
            }
            if let Some(mut msgs) = world.get_resource_mut::<Messages<ModSystemFaulted>>() {
                for ev in msgs.drain() {
                    let detail = ev.last_error.as_deref().unwrap_or("no error detail");
                    new_events.push(format!(
                        "[FAULT -> QUARANTINED] {}::{} ({}) : {}",
                        ev.mod_namespace.as_deref().unwrap_or("?"),
                        ev.system_name,
                        ev.status.name(),
                        detail
                    ));
                }
            }
            if let Some(mut msgs) = world.get_resource_mut::<Messages<ModBudgetBreach>>() {
                for ev in msgs.drain() {
                    new_events.push(format!(
                        "[BUDGET BREACH] {}::{} {:?} observed={} > limit={} (quarantined={})",
                        ev.mod_name, ev.system_name, ev.kind, ev.observed, ev.limit, ev.quarantined
                    ));
                }
            }
        }
        for line in new_events {
            self.push_event(line);
        }
    }

    /// Discovers all guest systems belonging to `mod_id` directly from Bevy's `Schedules` graph
    /// via each system's typed `ModNamedSystemSet` (works regardless of `bevy_ecs/debug` feature!).
    fn discover_systems_for_mod(&mut self, mod_id: ModId) -> Vec<DiscoveredSystem> {
        let (mod_name, generation) = {
            let host = self.app.world().resource::<ModHost>();
            let Some(m) = host.get_mod(mod_id) else {
                return Vec::new();
            };
            (m.name().to_string(), m.generation())
        };

        let mut system_names: Vec<String> = Vec::new();
        {
            let schedules = self.app.world().resource::<Schedules>();
            if let Some(update) = schedules.get(Update) {
                let graph = update.graph();
                let hierarchy = graph.hierarchy().graph();
                let mod_set_interned = ModSystemSet::Mod(mod_id).intern();
                if let Some(set_key) = graph.system_sets.get_key(mod_set_interned) {
                    for neighbor in
                        hierarchy.neighbors_directed(NodeId::Set(set_key), Direction::Outgoing)
                    {
                        if let NodeId::System(sys_key) = neighbor {
                            for parent in hierarchy
                                .neighbors_directed(NodeId::System(sys_key), Direction::Incoming)
                            {
                                if let NodeId::Set(parent_key) = parent
                                    && let Some(dyn_set) = graph.system_sets.get(parent_key)
                                    && let Some(named) = (dyn_set as &dyn core::any::Any)
                                        .downcast_ref::<ModNamedSystemSet>()
                                    && named.mod_name.as_ref() == mod_name
                                    && named.generation == generation
                                {
                                    let name = named.system_name.to_string();
                                    if !system_names.contains(&name) {
                                        system_names.push(name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut out = Vec::with_capacity(system_names.len());
        for sys_name in system_names {
            let named_set = ModNamedSystemSet::with_generation(
                mod_name.as_str(),
                sys_name.as_str(),
                generation,
            );
            self.attach_gate_condition(&mod_name, &sys_name, &named_set);
            out.push(DiscoveredSystem {
                mod_id,
                mod_name: mod_name.clone(),
                system_name: sys_name,
                named_set,
            });
        }
        out
    }

    /// Attaches a single lightweight `Res<ChainGate>` `run_if` condition to `named_set` once.
    fn attach_gate_condition(
        &mut self,
        mod_name: &str,
        sys_name: &str,
        named_set: &ModNamedSystemSet,
    ) {
        if !self.gated_sets.insert(named_set.clone()) {
            return;
        }
        let key = SystemIdKey::new(mod_name, sys_name);
        let set_clone = named_set.clone();
        let mut schedules = self.app.world_mut().resource_mut::<Schedules>();
        schedules.configure_sets(
            Update,
            set_clone.run_if(move |gate: bevy_ecs::system::Res<ChainGate>| gate.is_enabled(&key)),
        );
    }

    /// Loads ANY `.dll` / `.dylib` / `.so` mod dynamically from disk using `shadow_copy = true`.
    pub fn load_mod_path(&mut self, path: &Path) -> Result<(ModId, Vec<DiscoveredSystem>), String> {
        let t0 = Instant::now();
        let opts = LoadOptions::new().with_shadow_copy(true);
        let mod_id = self
            .app
            .world_mut()
            .load_mod_with(path, opts)
            .map_err(|err| format!("failed to load `{}`: {err}", path.display()))?;
        let load_us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        self.last_load_us = load_us;

        let new_systems = self.discover_systems_for_mod(mod_id);
        self.discovered.retain(|s| s.mod_id != mod_id);
        self.discovered.extend(new_systems.clone());

        // Re-apply chain pass in case newly loaded systems match or schedule needs dirty rebuild
        self.apply_chain_rewire()?;
        self.drain_lifecycle_messages();
        Ok((mod_id, new_systems))
    }

    /// Unloads a loaded mod cleanly via `world.unload_mod(mod_id)` (excising systems + `ModScoped` entities).
    pub fn unload_mod_by_id(&mut self, mod_id: ModId) -> Result<String, String> {
        let mod_name = self
            .app
            .world()
            .resource::<ModHost>()
            .get_mod(mod_id)
            .map(|m| m.name().to_string())
            .ok_or_else(|| format!("no loaded mod with {mod_id:?}"))?;

        let removed_sets: Vec<ModNamedSystemSet> = self
            .discovered
            .iter()
            .filter(|s| s.mod_id == mod_id)
            .map(|s| s.named_set.clone())
            .collect();

        self.chain.retain(|s| s.mod_id != mod_id);
        self.discovered.retain(|s| s.mod_id != mod_id);

        {
            let mut reg = self
                .app
                .world_mut()
                .resource_mut::<ModScheduleSetRegistry>();
            for ns in &removed_sets {
                reg.unmount_system_from_schedule(ScheduleId::UPDATE, ns);
            }
        }

        let t0 = Instant::now();
        self.app
            .world_mut()
            .unload_mod(mod_id)
            .map_err(|err| format!("failed to unload `{mod_name}` ({mod_id:?}): {err}"))?;
        let us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        self.last_load_us = us;

        self.apply_chain_rewire()?;
        self.drain_lifecycle_messages();
        Ok(format!(
            "Unloaded `{mod_name}` ({mod_id:?}) cleanly in {us:.1} us (systems excised & ModScoped entities purged)."
        ))
    }

    /// Hot-reloads a loaded mod in place via `world.reload_mod(mod_id)` (invoking `save_state` -> `restore_state`!).
    pub fn reload_mod_by_id(&mut self, mod_id: ModId) -> Result<String, String> {
        let old_name = self
            .app
            .world()
            .resource::<ModHost>()
            .get_mod(mod_id)
            .map(|m| m.name().to_string())
            .ok_or_else(|| format!("no loaded mod with {mod_id:?}"))?;

        // Remember chain positions occupied by this mod so we re-bind them at the new generation!
        let old_chain_positions: Vec<(usize, String)> = self
            .chain
            .iter()
            .enumerate()
            .filter(|(_, s)| s.mod_id == mod_id)
            .map(|(idx, s)| (idx, s.system_name.clone()))
            .collect();

        let t0 = Instant::now();
        let new_id = self
            .app
            .world_mut()
            .reload_mod(mod_id)
            .map_err(|err| format!("reload `{old_name}` failed: {err}"))?;
        let us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        self.last_load_us = us;

        let new_systems = self.discover_systems_for_mod(new_id);
        self.discovered.retain(|s| s.mod_id != mod_id);
        self.discovered.extend(new_systems.clone());

        // Re-bind chain entries to the reloaded generation's ModNamedSystemSet
        for (pos, sys_name) in old_chain_positions {
            if let Some(updated) = new_systems.iter().find(|s| s.system_name == sys_name)
                && pos < self.chain.len()
            {
                self.chain[pos] = updated.clone();
            }
        }
        self.chain.retain(|s| s.mod_id != mod_id);

        self.apply_chain_rewire()?;
        self.drain_lifecycle_messages();
        let new_gen = self
            .app
            .world()
            .resource::<ModHost>()
            .get_mod(new_id)
            .map(LoadedMod::generation)
            .unwrap_or(0);
        Ok(format!(
            "Hot-reloaded `{old_name}` -> {new_id:?} (gen={new_gen}, state preserved) in {us:.1} us; rewired chain in {:.1} us.",
            self.last_rewire_us
        ))
    }

    /// Resolves a user token (mod name, short name, or numeric ModId) to a loaded `ModId`.
    pub fn resolve_loaded_mod(&self, query: &str) -> Result<ModId, String> {
        let q = query.trim();
        let host = self.app.world().resource::<ModHost>();
        if let Ok(num) = q.parse::<u64>() {
            let id = ModId(num);
            if host.contains_mod(id) {
                return Ok(id);
            }
        }
        let q_lower = q.to_ascii_lowercase();
        let q_prefixed = format!("mod_{q_lower}");
        for m in host.loaded_mods() {
            let name_lower = m.name().to_ascii_lowercase();
            if name_lower == q_lower || name_lower == q_prefixed {
                return Ok(m.id());
            }
        }
        Err(format!(
            "no loaded mod matching `{query}` (run `mods` to list loaded mods)"
        ))
    }

    /// Resolves a user token (`SpinYaw`, `mod_spin::SpinYaw`, case-insensitive) to a discovered system.
    pub fn resolve_discovered_system(&self, token: &str) -> Result<DiscoveredSystem, String> {
        let t = token.trim();
        let t_lower = t.to_ascii_lowercase();

        // 1. Exact qualified match (`mod_name::system_name`)
        if let Some((m_part, s_part)) = t.split_once("::") {
            let m_low = m_part.to_ascii_lowercase();
            let s_low = s_part.to_ascii_lowercase();
            if let Some(found) = self.discovered.iter().find(|d| {
                (d.mod_name.to_ascii_lowercase() == m_low
                    || d.mod_name.to_ascii_lowercase() == format!("mod_{m_low}"))
                    && d.system_name.to_ascii_lowercase() == s_low
            }) {
                return Ok(found.clone());
            }
            return Err(format!(
                "system `{t}` not found among loaded mods (run `systems`)"
            ));
        }

        // 2. Exact case-insensitive system_name match
        let matches: Vec<&DiscoveredSystem> = self
            .discovered
            .iter()
            .filter(|d| d.system_name.to_ascii_lowercase() == t_lower)
            .collect();
        if matches.len() == 1 {
            return Ok(matches[0].clone());
        }
        if matches.len() > 1 {
            let names: Vec<String> = matches.iter().map(|d| d.qualified()).collect();
            return Err(format!(
                "ambiguous system `{t}` — disambiguate with one of: {}",
                names.join(", ")
            ));
        }

        // 3. Unique substring match on system_name
        let sub_matches: Vec<&DiscoveredSystem> = self
            .discovered
            .iter()
            .filter(|d| d.system_name.to_ascii_lowercase().contains(&t_lower))
            .collect();
        if sub_matches.len() == 1 {
            return Ok(sub_matches[0].clone());
        }
        if sub_matches.len() > 1 {
            let names: Vec<String> = sub_matches.iter().map(|d| d.qualified()).collect();
            return Err(format!(
                "ambiguous shorthand `{t}` matches: {}",
                names.join(", ")
            ));
        }

        Err(format!(
            "system `{t}` is not loaded (run `systems` to see loaded systems or `catalog` to load mods)"
        ))
    }

    /// Commits `self.chain` and `self.chain_mode` into `ModScheduleSetRegistry` + `ChainGate` and
    /// triggers sub-millisecond `rewire_mod_schedule_sets_in_world` (with automatic rollback on cycles).
    pub fn apply_chain_rewire(&mut self) -> Result<f64, String> {
        let prev_mounted = self
            .app
            .world()
            .resource::<ModScheduleSetRegistry>()
            .get(self.chain_handle)
            .map(|e| (e.mode, e.mounted_systems.clone()))
            .unwrap_or((ScheduleSetMode::Sequence, Vec::new()));

        let ordered_sets: Vec<ModNamedSystemSet> =
            self.chain.iter().map(|s| s.named_set.clone()).collect();

        {
            let mut reg = self
                .app
                .world_mut()
                .resource_mut::<ModScheduleSetRegistry>();
            reg.set_mode(self.chain_handle, self.chain_mode)
                .map_err(|e| format!("set_mode failed: {e}"))?;
            reg.reorder_mounted_systems(self.chain_handle, ordered_sets)
                .map_err(|e| format!("reorder_mounted_systems failed: {e}"))?;
        }

        let t0 = Instant::now();
        let rewire_res =
            rewire_mod_schedule_sets_in_world(self.app.world_mut(), ScheduleId::UPDATE);
        let rewire_us = t0.elapsed().as_secs_f64() * 1_000_000.0;

        if let Err(err) = rewire_res {
            // Roll back registry to previous valid graph!
            {
                let mut reg = self
                    .app
                    .world_mut()
                    .resource_mut::<ModScheduleSetRegistry>();
                let _ = reg.set_mode(self.chain_handle, prev_mounted.0);
                let _ = reg.reorder_mounted_systems(self.chain_handle, prev_mounted.1);
            }
            let _ = rewire_mod_schedule_sets_in_world(self.app.world_mut(), ScheduleId::UPDATE);
            return Err(format!(
                "Schedule rewire rejected (e.g. ordering cycle with static mod edges): {err:?}"
            ));
        }

        // Sync ChainGate & SimClock
        {
            let mut gate = self.app.world_mut().resource_mut::<ChainGate>();
            gate.chained = self.chain.iter().map(DiscoveredSystem::key).collect();
        }
        {
            let mut clock = self.app.world_mut().resource_mut::<SimClock>();
            clock.active_chain_len = self.chain.len() as u32;
        }

        self.last_rewire_us = rewire_us;
        Ok(rewire_us)
    }

    /// Replaces the entire active runtime chain and rewires Bevy's schedule in microseconds.
    pub fn set_chain(&mut self, items: Vec<DiscoveredSystem>) -> Result<f64, String> {
        let backup = self.chain.clone();
        self.chain = items;
        match self.apply_chain_rewire() {
            Ok(us) => Ok(us),
            Err(err) => {
                self.chain = backup;
                Err(err)
            }
        }
    }

    /// Sets the chain execution mode (`Sequence`, `SequenceWeak`, `Parallel`) and hot-rewires.
    pub fn set_chain_mode(&mut self, mode: ScheduleSetMode) -> Result<f64, String> {
        let prev = self.chain_mode;
        self.chain_mode = mode;
        match self.apply_chain_rewire() {
            Ok(us) => Ok(us),
            Err(err) => {
                self.chain_mode = prev;
                Err(err)
            }
        }
    }

    /// Returns a list of all currently loaded mods.
    pub fn loaded_mod_summaries(&self) -> Vec<ModSummary> {
        self.app
            .world()
            .resource::<ModHost>()
            .loaded_mods()
            .iter()
            .map(|m| ModSummary {
                id: m.id(),
                name: m.name().to_string(),
                version: m.version().to_string(),
                generation: m.generation(),
                system_count: m.system_count(),
                fault_count: m.fault_count(),
                source_path: m.source_path().to_path_buf(),
            })
            .collect()
    }

    /// Re-enables a quarantined guest system via `ModQuarantine::reenable`.
    pub fn reenable_system(&mut self, mod_query: &str, sys_name: &str) -> Result<String, String> {
        let mod_id = self.resolve_loaded_mod(mod_query)?;
        let reenabled = self
            .app
            .world()
            .resource::<ModQuarantine>()
            .reenable(mod_id, sys_name);
        if reenabled {
            let msg = format!("Re-enabled quarantined system `{sys_name}` on {mod_id:?}.");
            self.push_event(msg.clone());
            Ok(msg)
        } else {
            Err(format!(
                "system `{sys_name}` on {mod_id:?} was not quarantined or not found"
            ))
        }
    }

    /// Updates the live `ModBudgetPolicy` on the world.
    pub fn set_budget_policy(&mut self, policy: ModBudgetPolicy) {
        self.app.world_mut().set_mod_budget_policy(policy);
    }

    /// Returns the current `ModBudgetPolicy`.
    pub fn budget_policy(&self) -> ModBudgetPolicy {
        self.app
            .world()
            .get_resource::<ModBudgetPolicy>()
            .cloned()
            .unwrap_or_default()
    }

    /// Returns whether `(mod_id, system_name)` is currently quarantined.
    pub fn is_quarantined(&self, mod_id: ModId, sys_name: &str) -> bool {
        self.app
            .world()
            .resource::<ModQuarantine>()
            .is_quarantined(mod_id, sys_name)
    }

    /// Collects point-in-time ECS-only performance metrics (strictly zero rendering overhead!).
    pub fn perf_summary(&mut self) -> EcsPerfSummary {
        let clock = *self.app.world().resource::<SimClock>();
        let snapshots = self
            .app
            .world()
            .resource::<ModBudgetTelemetry>()
            .system_budget_snapshots();

        let (actor_count, spark_count) = {
            let world = self.app.world_mut();
            let mut q_actors = world.query_filtered::<(), bevy_ecs::query::With<StudioActor>>();
            let actors = q_actors.iter(world).count();
            let mut q_sparks = world.query_filtered::<(), bevy_ecs::query::With<SparkTag>>();
            let sparks = q_sparks.iter(world).count();
            (actors, sparks)
        };

        let mut sorted: Vec<f64> = self.ecs_tick_samples_us.iter().copied().collect();
        let n = sorted.len();
        let (last_us, mean_us, p50_us, p99_us, min_us, max_us) = if n == 0 {
            (0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
        } else {
            let last = *self.ecs_tick_samples_us.back().unwrap_or(&0.0);
            let mean = sorted.iter().sum::<f64>() / (n as f64);
            sorted.sort_by(|a, b| a.total_cmp(b));
            let p50_idx = (n * 50).div_ceil(100).saturating_sub(1).min(n - 1);
            let p99_idx = (n * 99).div_ceil(100).saturating_sub(1).min(n - 1);
            (
                last,
                mean,
                sorted[p50_idx],
                sorted[p99_idx],
                if self.ecs_tick_min_us.is_finite() {
                    self.ecs_tick_min_us
                } else {
                    0.0
                },
                self.ecs_tick_max_us,
            )
        };

        let mod_sum_last_us: u64 = snapshots.iter().map(|s| s.last_duration_us).sum();
        let mod_sum_p50_us: u64 = snapshots.iter().map(|s| s.p50_us).sum();
        let mod_sum_p99_us: u64 = snapshots.iter().map(|s| s.p99_us).sum();
        let total_matched_ops: u64 = snapshots.iter().map(|s| s.matched_entities).sum();

        let aggregate_m_ents_per_sec = if mean_us > 0.01 && total_matched_ops > 0 {
            (total_matched_ops as f64) / mean_us // entities / us == M entities / sec!
        } else {
            0.0
        };

        EcsPerfSummary {
            tick: clock.tick,
            actor_count,
            spark_count,
            ecs_tick_last_us: last_us,
            ecs_tick_mean_us: mean_us,
            ecs_tick_p50_us: p50_us,
            ecs_tick_p99_us: p99_us,
            ecs_tick_min_us: min_us,
            ecs_tick_max_us: max_us,
            mod_sum_last_us,
            mod_sum_p50_us,
            mod_sum_p99_us,
            aggregate_m_ents_per_sec,
            last_rewire_us: self.last_rewire_us,
            last_load_us: self.last_load_us,
            systems: snapshots,
        }
    }

    /// Runs a tight pure-ECS headless burst benchmark (ZERO rendering!) and returns a formatted report.
    pub fn run_ecs_benchmark(&mut self, ticks: u32, entity_override: Option<usize>) -> String {
        let ticks = ticks.clamp(1, 20_000);
        if let Some(count) = entity_override {
            self.reset_scene(None, Some(count));
        } else {
            self.clear_perf_window();
        }

        // Warmup 5 ticks so allocator/caches settle
        for _ in 0..5 {
            self.app.update();
        }
        self.clear_perf_window();

        let mut samples = Vec::with_capacity(ticks as usize);
        let wall_start = Instant::now();
        for _ in 0..ticks {
            let us = self.step_ecs_tick();
            samples.push(us);
        }
        let wall_ms = wall_start.elapsed().as_secs_f64() * 1000.0;

        samples.sort_by(|a, b| a.total_cmp(b));
        let n = samples.len();
        let mean_us = samples.iter().sum::<f64>() / (n as f64);
        let p50_us = samples[(n * 50).div_ceil(100).saturating_sub(1).min(n - 1)];
        let p99_us = samples[(n * 99).div_ceil(100).saturating_sub(1).min(n - 1)];
        let min_us = samples[0];
        let max_us = samples[n - 1];

        let summary = self.perf_summary();
        let total_entity_ops: u64 = summary.systems.iter().map(|s| s.matched_entities).sum();
        let throughput = if mean_us > 0.001 && total_entity_ops > 0 {
            (total_entity_ops as f64) / mean_us
        } else {
            0.0
        };

        let mut out = String::new();
        out.push_str(&format!(
            "=== Pure ECS Mod Benchmark (ZERO Rendering) ===\n\
             Ticks: {ticks} | Actors: {} (+{} sparks) | Mode: {:?} | Wall: {wall_ms:.2} ms\n\
             ECS Tick Time: mean={mean_us:.2} us | p50={p50_us:.2} us | p99={p99_us:.2} us | min={min_us:.2} us | max={max_us:.2} us\n\
             Mod Callbacks Sum: p50={} us | p99={} us | Aggregate Throughput: {throughput:.2} M entity-ops/sec\n",
            summary.actor_count,
            summary.spark_count,
            self.chain_mode,
            summary.mod_sum_p50_us,
            summary.mod_sum_p99_us,
        ));
        if !summary.systems.is_empty() {
            out.push_str(&format!(
                "  {:<35} {:>7} {:>7} {:>7} {:>8} {:>6} {:>6} {:>10}\n",
                "Mod::System", "p50 us", "p99 us", "last", "entities", "chunks", "cmds", "status"
            ));
            for s in &summary.systems {
                let qname = format!("{}::{}", s.mod_name, s.system_name);
                let st = if s.quarantined {
                    "QUARANTINED"
                } else if s.breach_count > 0 {
                    "BREACH"
                } else {
                    "OK"
                };
                out.push_str(&format!(
                    "  {:<35} {:>7} {:>7} {:>7} {:>8} {:>6} {:>6} {:>10}\n",
                    qname,
                    s.p50_us,
                    s.p99_us,
                    s.last_duration_us,
                    s.matched_entities,
                    s.chunks_yielded,
                    s.commands_queued,
                    st
                ));
            }
        }
        out
    }

    /// Extracts all 3D entities (`StudioActor` + `SparkTag`) for the `wgpu` renderer.
    pub fn collect_render_actors(&mut self) -> Vec<ActorRenderData> {
        let world = self.app.world_mut();
        let mut query = world.query::<(&Transform3D, &VisualStyle, &ActorMeta)>();
        query
            .iter(world)
            .map(|(xf, style, meta)| ActorRenderData {
                transform: *xf,
                style: *style,
                is_spark: (meta.flags & 1) != 0,
            })
            .collect()
    }

    /// Loads a curated preset (loading any missing bundled mods and wiring a vivid multi-system chain).
    pub fn apply_preset(&mut self, name: &str) -> Result<String, String> {
        let (mod_shorts, sys_names, mode, formation, desc): (
            &[&str],
            &[&str],
            ScheduleSetMode,
            Formation,
            &str,
        ) = match name.trim().to_ascii_lowercase().as_str() {
            "showcase" | "default" => (
                &["motion", "spin", "color", "scale"],
                &[
                    "WaveLift",
                    "SpinYaw",
                    "RainbowHue",
                    "EnergyHeatmap",
                    "SquashStretch",
                ],
                ScheduleSetMode::Sequence,
                Formation::Grid,
                "Classic 5-stage kinetic chain: WaveLift -> SpinYaw -> RainbowHue -> EnergyHeatmap -> SquashStretch",
            ),
            "tornado" | "vortex" => (
                &["vortex", "spin", "color", "scale"],
                &["SpiralVortex", "Tumble3D", "EnergyHeatmap", "PulseScale"],
                ScheduleSetMode::Sequence,
                Formation::Ring,
                "Parallel par_chunks SpiralVortex -> Tumble3D -> EnergyHeatmap -> PulseScale",
            ),
            "polyglot" | "multi" => (
                &["c99_orbit", "cpp_gravity", "spin", "color", "scale"],
                &[
                    "C99PlanetaryOrbit",
                    "CppBounceGravity",
                    "SpinYaw",
                    "RainbowHue",
                    "SquashStretch",
                ],
                ScheduleSetMode::Sequence,
                Formation::Helix,
                "3-Language Chain: ISO C99 -> ISO C++17 -> Rust SpinYaw -> Rust RainbowHue -> Rust SquashStretch!",
            ),
            "shockwave" | "ripple" => (
                &["ripple", "motion", "color", "scale", "spin"],
                &[
                    "EmitShockwave",
                    "ReactShockwave",
                    "IntegrateMove",
                    "RainbowHue",
                    "PulseScale",
                    "SpinYaw",
                ],
                ScheduleSetMode::Sequence,
                Formation::Grid,
                "Buffered FFI Messages<PulseEvent>: EmitShockwave -> ReactShockwave -> IntegrateMove -> RainbowHue -> PulseScale -> SpinYaw",
            ),
            "aura" | "crossmod" => (
                &["aura", "aura_link", "vortex", "spin", "color"],
                &[
                    "AuraCharge",
                    "AuraSynergy",
                    "SpawnSparks",
                    "AttractCenter",
                    "SpinYaw",
                    "RainbowHue",
                ],
                ScheduleSetMode::Sequence,
                Formation::Sphere,
                "Cross-Mod Dynamic Component + ModScoped Sparks: AuraCharge -> AuraSynergy -> SpawnSparks -> AttractCenter -> SpinYaw -> RainbowHue",
            ),
            "chaos" | "fault" => (
                &["motion", "chaos", "spin", "color"],
                &["WaveLift", "PanicFaultDemo", "SpinYaw", "RainbowHue"],
                ScheduleSetMode::Sequence,
                Formation::Grid,
                "Fault Isolation Demo: PanicFaultDemo panics on run #5 & gets quarantined while WaveLift/SpinYaw/RainbowHue keep running!",
            ),
            "stress" | "bench" => (
                &["vortex", "c99_orbit", "spin", "color", "scale"],
                &["SpiralVortex", "SpinYaw", "RainbowHue", "PulseScale"],
                ScheduleSetMode::Parallel,
                Formation::Ring,
                "High-throughput ECS benchmark preset (try `bench 300 4000` or `bench compare`)",
            ),
            other => {
                return Err(format!(
                    "unknown preset `{other}`. Available presets: showcase, tornado, polyglot, shockwave, aura, chaos, stress"
                ));
            }
        };

        // Load any required mods not already loaded
        for short in mod_shorts {
            if self.resolve_loaded_mod(short).is_err() {
                let path = resolve_mod_input_path(short)?;
                self.load_mod_path(&path)?;
            }
        }

        let mut chain_items = Vec::with_capacity(sys_names.len());
        for s in sys_names {
            chain_items.push(self.resolve_discovered_system(s)?);
        }

        self.chain_mode = mode;
        let rewire_us = self.set_chain(chain_items)?;
        self.reset_scene(Some(formation), None);
        Ok(format!(
            "Applied preset `{name}` ({desc})\n  Mode: {mode:?} | Rewired in {rewire_us:.1} us | Formation: {}",
            formation.name()
        ))
    }

    /// Formats the active chain as a crisp string (`SysA -> SysB -> SysC`).
    pub fn chain_display(&self) -> String {
        if self.chain.is_empty() {
            return "(empty — running all enabled loaded systems)".to_string();
        }
        let sep = match self.chain_mode {
            ScheduleSetMode::Sequence => " -> ",
            ScheduleSetMode::SequenceWeak => " ~> ",
            ScheduleSetMode::Parallel => " || ",
        };
        self.chain
            .iter()
            .map(|s| s.system_name.as_str())
            .collect::<Vec<_>>()
            .join(sep)
    }
}
