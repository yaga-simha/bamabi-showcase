#![forbid(unsafe_code)]

//! Shared guest-side `#[repr(C)]` `bamabi` `ABI 0.3.0` component, resource, and message
//! definitions for Example 11 (`11_runtime_chain_studio`).
//!
//! Zero Bevy dependencies — depends strictly on `bamabi_guest`.

use bamabi_guest::prelude::*;
use core::ffi::CStr;

/// 3D world transform + primitive shape selector (`studio::Transform3D`, 40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, ModComponent)]
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

impl GuestComponent for Transform3D {
    const NAME: &'static CStr = c"studio::Transform3D";
    const UUID: ComponentUuid = <Self as ModComponent>::UUID;
}

/// 3D linear & angular velocity + phase state (`studio::Motion3D`, 32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, ModComponent)]
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

impl GuestComponent for Motion3D {
    const NAME: &'static CStr = c"studio::Motion3D";
    const UUID: ComponentUuid = <Self as ModComponent>::UUID;
}

/// Color, emissive glow, working hue, and inter-system chain channels (`studio::VisualStyle`, 32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, ModComponent)]
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

impl GuestComponent for VisualStyle {
    const NAME: &'static CStr = c"studio::VisualStyle";
    const UUID: ComponentUuid = <Self as ModComponent>::UUID;
}

/// Deterministic actor metadata & home anchor (`studio::ActorMeta`, 24 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, ModComponent)]
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

impl GuestComponent for ActorMeta {
    const NAME: &'static CStr = c"studio::ActorMeta";
    const UUID: ComponentUuid = <Self as ModComponent>::UUID;
}

/// Host-driven simulation clock & global parameters (`studio::SimClock`, 32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, ModComponent)]
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

impl GuestComponent for SimClock {
    const NAME: &'static CStr = c"studio::SimClock";
    const UUID: ComponentUuid = <Self as ModComponent>::UUID;
}

/// Buffered FFI message for shockwave / ripple pub-sub (`studio::PulseEvent`, 24 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, ModComponent)]
#[mod_component(name = "studio::PulseEvent")]
pub struct PulseEvent {
    pub source_index: u32,
    pub kind: u32,
    pub intensity: f32,
    pub ox: f32,
    pub oy: f32,
    pub oz: f32,
}

impl GuestComponent for PulseEvent {
    const NAME: &'static CStr = c"studio::PulseEvent";
    const UUID: ComponentUuid = <Self as ModComponent>::UUID;
}

/// Tag component for mod-spawned spark entities (`studio::SparkTag`, 4 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, ModComponent)]
#[mod_component(name = "studio::SparkTag")]
pub struct SparkTag {
    pub ttl: f32,
}

impl GuestComponent for SparkTag {
    const NAME: &'static CStr = c"studio::SparkTag";
    const UUID: ComponentUuid = <Self as ModComponent>::UUID;
}

/// Converts HSV (`h` in `[0, 1)`, `s` in `[0, 1]`, `v` in `[0, 1]`) to RGB.
#[inline]
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
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

/// Wraps an angle in radians to `[-PI, PI]` to avoid precision loss over long runs.
#[inline]
pub fn wrap_angle(a: f32) -> f32 {
    let tau = core::f32::consts::TAU;
    let mut x = a % tau;
    if x > core::f32::consts::PI {
        x -= tau;
    } else if x < -core::f32::consts::PI {
        x += tau;
    }
    x
}
