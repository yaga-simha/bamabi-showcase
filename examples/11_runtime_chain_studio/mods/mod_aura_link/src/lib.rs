#![forbid(unsafe_code)]

//! `mod_aura_link` — Cross-mod dynamic component interop (`AuraSynergy`).
//!
//! Resolves `"mod_aura::AuraChargeData"` dynamically registered by `mod_aura` (ZERO host
//! compile-time registration!), converts aura charge into glowing golden resonance bursts,
//! scale flares, and upward lift.

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use studio_abi::{Motion3D, SimClock, Transform3D, VisualStyle};

/// C-ABI layout mirror of `mod_aura`'s runtime-registered `"mod_aura::AuraChargeData"`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, AbiPod)]
pub struct AuraChargeData {
    pub charge: f32,
    pub max_charge: f32,
    pub harmonic: f32,
    pub generation: u32,
}

impl GuestComponent for AuraChargeData {
    const NAME: &'static CStr = c"mod_aura::AuraChargeData";
}

#[derive(Default)]
pub struct AuraLinkMod;

impl GuestPlugin for AuraLinkMod {
    const NAME: &'static CStr = c"mod_aura_link";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        app.system(c"AuraSynergy")
            .query(|q| {
                q.write::<AuraChargeData>()
                    .write::<Transform3D>()
                    .write::<Motion3D>()
                    .write::<VisualStyle>()
            })
            .resource_read::<SimClock>()
            .schedule(ScheduleId::UPDATE)
            .build(|ctx| {
                let clock = ctx
                    .component_resource::<SimClock>(0)
                    .copied()
                    .unwrap_or(SimClock {
                        amplitude: 1.0,
                        ..Default::default()
                    });

                while let Some(mut chunk) = ctx.iter().next_chunk_view() {
                    let n = chunk.len();
                    let mut drained = [0.0_f32; 64];
                    let mut start = 0;
                    while start < n {
                        let len = (n - start).min(64);
                        {
                            let Some((auras, transforms)) =
                                chunk.component_write_2::<AuraChargeData, Transform3D>(0, 1)
                            else {
                                return FfiResult::TYPE_MISMATCH;
                            };
                            for (k, out) in drained[..len].iter_mut().enumerate() {
                                let row = start + k;
                                let aura = &mut auras[row];
                                let xf = &mut transforms[row];
                                let ratio =
                                    (aura.charge / aura.max_charge.max(0.01)).clamp(0.0, 1.0);
                                // Siphon a small share of charge when high to fuel resonance flare
                                let flare = if ratio > 0.72 {
                                    aura.charge *= 0.88;
                                    ratio * clock.amplitude
                                } else {
                                    ratio * 0.25 * clock.amplitude
                                };
                                let boost = 1.0 + flare * 0.48;
                                xf.sx = (xf.sx * 0.8 + boost * 0.2).clamp(0.4, 2.5);
                                xf.sy = (xf.sy * 0.8 + (boost * 1.18) * 0.2).clamp(0.4, 2.8);
                                xf.sz = (xf.sz * 0.8 + boost * 0.2).clamp(0.4, 2.5);
                                *out = flare;
                            }
                        }
                        {
                            let Some((motions, styles)) =
                                chunk.component_write_2::<Motion3D, VisualStyle>(2, 3)
                            else {
                                return FfiResult::TYPE_MISMATCH;
                            };
                            for (k, &flare) in drained[..len].iter().enumerate() {
                                let row = start + k;
                                let mot = &mut motions[row];
                                let vis = &mut styles[row];
                                if flare > 0.5 {
                                    mot.vy += flare * 0.35;
                                    // Shift tint toward luminous gold-cyan resonance
                                    vis.r = (vis.r * 0.75 + 0.98 * 0.25).min(1.0);
                                    vis.g = (vis.g * 0.75 + 0.86 * 0.25).min(1.0);
                                    vis.b = (vis.b * 0.75 + 0.35 * 0.25).min(1.0);
                                    vis.emissive = (vis.emissive + flare * 0.75).min(2.0);
                                    vis.energy = (vis.energy + flare * 0.35).min(2.0);
                                }
                            }
                        }
                        start += len;
                    }
                }
                FfiResult::OK
            })
    }
}

export_mod!(plugin: AuraLinkMod);
