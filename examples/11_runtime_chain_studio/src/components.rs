//! Host-side `#[repr(C)]` `bamabi` `ABI 0.3.0` ECS components, resources, and messages
//! for Example 11: Runtime Mod Chain Studio (`winit` + `wgpu` + REPL).

use bamabi::ModComponent;
use bevy_ecs::component::Component;
use bevy_ecs::message::Message;
use bevy_ecs::prelude::Resource;
use std::collections::HashSet;

/// 3D world transform + primitive shape selector (`studio::Transform3D`, 40 bytes).
#[repr(C)]
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, ModComponent)]
#[component(summary_tick)]
#[mod_component(name = "studio::Transform3D")]
pub struct Transform3D {
    pub px: f32,
    pub py: f32,
    pub pz: f32,
    pub rx: f32,
    pub ry: f32,
    pub rz: f32,
    pub sx: f32,
    pub sy: f32,
    pub sz: f32,
    /// `0` = crystal/octahedron, `1` = cube, `2` = monolith prism.
    pub shape: u32,
}

/// 3D linear & angular velocity + per-entity phase (`studio::Motion3D`, 32 bytes).
#[repr(C)]
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, ModComponent)]
#[component(summary_tick)]
#[mod_component(name = "studio::Motion3D")]
pub struct Motion3D {
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    pub avx: f32,
    pub avy: f32,
    pub avz: f32,
    pub speed: f32,
    pub phase: f32,
}

/// Color, emissive glow, working hue, and inter-system chain channels (`studio::VisualStyle`, 32 bytes).
#[repr(C)]
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, ModComponent)]
#[component(summary_tick)]
#[mod_component(name = "studio::VisualStyle")]
pub struct VisualStyle {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    pub emissive: f32,
    pub hue: f32,
    pub energy: f32,
    pub pulse: f32,
}

/// Deterministic actor metadata & home anchor (`studio::ActorMeta`, 24 bytes).
#[repr(C)]
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, ModComponent)]
#[component(summary_tick)]
#[mod_component(name = "studio::ActorMeta")]
pub struct ActorMeta {
    pub index: u32,
    pub group: u32,
    pub home_x: f32,
    pub home_y: f32,
    pub home_z: f32,
    /// Bit 0 (`1`): temporary spark entity spawned by mods.
    pub flags: u32,
}

/// Host-driven simulation clock & global parameters (`studio::SimClock`, 32 bytes).
#[repr(C)]
#[derive(Resource, Debug, Clone, Copy, PartialEq, ModComponent)]
#[mod_component(name = "studio::SimClock")]
pub struct SimClock {
    pub tick: u64,
    pub dt: f32,
    pub elapsed: f32,
    pub speed_scale: f32,
    pub amplitude: f32,
    pub active_chain_len: u32,
    pub actor_count: u32,
}

impl Default for SimClock {
    fn default() -> Self {
        Self {
            tick: 0,
            dt: 1.0 / 60.0,
            elapsed: 0.0,
            speed_scale: 1.0,
            amplitude: 1.0,
            active_chain_len: 0,
            actor_count: 0,
        }
    }
}

/// Buffered POD Bevy `Message` for shockwave / ripple pub-sub (`studio::PulseEvent`, 24 bytes).
#[repr(C)]
#[derive(Message, Debug, Clone, Copy, PartialEq, Default, ModComponent)]
#[mod_component(name = "studio::PulseEvent")]
pub struct PulseEvent {
    pub source_index: u32,
    pub kind: u32,
    pub intensity: f32,
    pub ox: f32,
    pub oy: f32,
    pub oz: f32,
}

/// Tag component for mod-spawned spark entities (`studio::SparkTag`, 4 bytes).
#[repr(C)]
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, ModComponent)]
#[mod_component(name = "studio::SparkTag")]
pub struct SparkTag {
    pub ttl: f32,
}

/// Host-private marker tagging primary resettable studio actors (distinct from mod-spawned sparks).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct StudioActor;

/// Deterministic 3D initial formation layouts for repeatable reset & run cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Formation {
    #[default]
    Grid,
    Ring,
    Helix,
    Sphere,
}

impl Formation {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Ring => "ring",
            Self::Helix => "helix",
            Self::Sphere => "sphere",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "grid" | "lattice" => Some(Self::Grid),
            "ring" | "rings" | "circle" => Some(Self::Ring),
            "helix" | "spiral" | "dna" => Some(Self::Helix),
            "sphere" | "orb" | "fib" => Some(Self::Sphere),
            _ => None,
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Grid => Self::Ring,
            Self::Ring => Self::Helix,
            Self::Helix => Self::Sphere,
            Self::Sphere => Self::Grid,
        }
    }
}

/// Canonical `(mod_name, system_name)` identifier for runtime gating & chaining.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SystemIdKey {
    pub mod_name: String,
    pub system_name: String,
}

impl SystemIdKey {
    pub fn new(mod_name: impl Into<String>, system_name: impl Into<String>) -> Self {
        Self {
            mod_name: mod_name.into(),
            system_name: system_name.into(),
        }
    }
}

/// Read-only `Resource` consulted by Bevy `run_if` conditions attached to every discovered
/// guest system's `ModNamedSystemSet`. Because `Res<ChainGate>` is read-only, non-conflicting
/// guest systems still execute in parallel under Bevy's multi-threaded executor!
#[derive(Resource, Debug, Clone)]
pub struct ChainGate {
    /// When `true` and `chained` is non-empty, ONLY systems present in `chained` execute.
    pub solo_chain: bool,
    /// Systems currently mounted in the active runtime chain.
    pub chained: HashSet<SystemIdKey>,
    /// Explicitly muted systems (skipped even when `solo_chain == false`).
    pub disabled: HashSet<SystemIdKey>,
}

impl Default for ChainGate {
    fn default() -> Self {
        Self {
            solo_chain: true,
            chained: HashSet::new(),
            disabled: HashSet::new(),
        }
    }
}

impl ChainGate {
    #[inline]
    pub fn is_enabled(&self, key: &SystemIdKey) -> bool {
        if self.disabled.contains(key) {
            return false;
        }
        if self.solo_chain && !self.chained.is_empty() {
            return self.chained.contains(key);
        }
        true
    }
}
