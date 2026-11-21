#![forbid(unsafe_code)]

//! `mod_spin` — Rotation & gyroscopic tumbling guest mod (`SpinYaw`, `Tumble3D`).
//!
//! Showcases:
//! - Multiple independent chainable systems in one `.cdylib`
//! - `Res<SimClock>` singleton resource reads across the C-ABI
//! - `GuestPlugin::save_state` / `restore_state` preserving accumulated phase across live `reload_mod`

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use studio_abi::{Motion3D, SimClock, Transform3D, VisualStyle, wrap_angle};

#[derive(Default)]
pub struct SpinMod {
    /// Accumulated spin phase bits (`f32::to_bits`), persisted across live hot-reloads!
    accumulated_phase: Arc<AtomicU64>,
}

impl GuestPlugin for SpinMod {
    const NAME: &'static CStr = c"mod_spin";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        let phase_a = Arc::clone(&self.accumulated_phase);
        let res1 = app
            .system(c"SpinYaw")
            .query_typed::<(&mut Transform3D, &Motion3D, &mut VisualStyle)>()
            .resource_read::<SimClock>()
            .schedule(ScheduleId::UPDATE)
            .build(move |ctx| {
                let clock = ctx
                    .component_resource::<SimClock>(0)
                    .copied()
                    .unwrap_or(SimClock {
                        dt: 1.0 / 60.0,
                        speed_scale: 1.0,
                        amplitude: 1.0,
                        ..Default::default()
                    });
                let dt = clock.dt * clock.speed_scale;
                let step = phase_a.fetch_add(1, Ordering::Relaxed);
                let global_twist = (step as f32) * 0.015;

                ctx.single_query::<(&mut Transform3D, &Motion3D, &mut VisualStyle)>()
                    .for_each(|(transforms, motions, styles)| {
                        for ((xf, mot), vis) in transforms
                            .iter_mut()
                            .zip(motions.iter())
                            .zip(styles.iter_mut())
                        {
                            // Spin rate couples to upstream energy & pulse!
                            let omega =
                                (1.8 + vis.energy * 2.4 + vis.pulse * 3.0) * clock.amplitude;
                            xf.ry = wrap_angle(xf.ry + omega * dt + mot.avy * dt);
                            // Feed a fraction of spin back into hue & subtle energy so downstream
                            // color/scale systems react to SpinYaw's ordering!
                            vis.hue = (vis.hue + omega * dt * 0.04 + global_twist * 0.0005)
                                .rem_euclid(1.0);
                            vis.energy = (vis.energy + 0.012 * clock.amplitude).min(2.0);
                        }
                    })
            });
        if !res1.is_ok() {
            return res1;
        }

        app.system(c"Tumble3D")
            .query_typed::<(&mut Transform3D, &Motion3D, &mut VisualStyle)>()
            .resource_read::<SimClock>()
            .schedule(ScheduleId::UPDATE)
            .build(|ctx| {
                let clock = ctx
                    .component_resource::<SimClock>(0)
                    .copied()
                    .unwrap_or(SimClock {
                        dt: 1.0 / 60.0,
                        speed_scale: 1.0,
                        amplitude: 1.0,
                        ..Default::default()
                    });
                let dt = clock.dt * clock.speed_scale;
                let t = clock.elapsed * clock.speed_scale;

                ctx.single_query::<(&mut Transform3D, &Motion3D, &mut VisualStyle)>()
                    .for_each(|(transforms, motions, styles)| {
                        for ((xf, mot), vis) in transforms
                            .iter_mut()
                            .zip(motions.iter())
                            .zip(styles.iter_mut())
                        {
                            let boost =
                                (1.0 + xf.py.abs() * 0.35 + vis.pulse * 1.5) * clock.amplitude;
                            xf.rx = wrap_angle(
                                xf.rx + (1.3 * (t + mot.phase).cos() + 0.9) * dt * boost,
                            );
                            xf.rz = wrap_angle(
                                xf.rz + (1.1 * (t * 0.8 + mot.phase).sin() + 0.7) * dt * boost,
                            );
                            vis.emissive = (vis.emissive + 0.04 * boost.min(2.0)).min(2.0);
                        }
                    })
            })
    }

    fn save_state(&self) -> Result<Vec<u8>, FfiResult> {
        let v = self.accumulated_phase.load(Ordering::Relaxed);
        Ok(v.to_le_bytes().to_vec())
    }

    fn restore_state(&mut self, bytes: &[u8]) -> FfiResult {
        if bytes.len() == 8 {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(bytes);
            self.accumulated_phase
                .store(u64::from_le_bytes(arr), Ordering::Relaxed);
        }
        FfiResult::OK
    }
}

export_mod!(plugin: SpinMod);
