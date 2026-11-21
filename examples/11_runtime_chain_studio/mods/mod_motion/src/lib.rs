#![forbid(unsafe_code)]

//! `mod_motion` — 3D wave kinematics & velocity integration (`WaveLift`, `IntegrateMove`).

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use studio_abi::{ActorMeta, Motion3D, SimClock, Transform3D, VisualStyle};

const ARENA_BOUND: f32 = 13.5;

#[derive(Default)]
pub struct MotionMod;

impl GuestPlugin for MotionMod {
    const NAME: &'static CStr = c"mod_motion";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        let res1 = app
            .system(c"WaveLift")
            .query_typed::<(
                &mut Transform3D,
                &mut Motion3D,
                &mut VisualStyle,
                &ActorMeta,
            )>()
            .resource_read::<SimClock>()
            .schedule(ScheduleId::UPDATE)
            .build(|ctx| {
                let clock = ctx
                    .component_resource::<SimClock>(0)
                    .copied()
                    .unwrap_or(SimClock {
                        dt: 1.0 / 60.0,
                        elapsed: 0.0,
                        speed_scale: 1.0,
                        amplitude: 1.0,
                        ..Default::default()
                    });
                let t = clock.elapsed * clock.speed_scale;

                ctx.single_query::<(
                    &mut Transform3D,
                    &mut Motion3D,
                    &mut VisualStyle,
                    &ActorMeta,
                )>()
                .for_each(|(transforms, motions, styles, metas)| {
                    for (((xf, mot), vis), meta) in transforms
                        .iter_mut()
                        .zip(motions.iter_mut())
                        .zip(styles.iter_mut())
                        .zip(metas.iter())
                    {
                        if (meta.flags & 1) != 0 {
                            continue;
                        }
                        let radial = (xf.px * xf.px + xf.pz * xf.pz).sqrt();
                        let wave = (radial * 0.72 - t * 3.1 + mot.phase * 0.25).sin();
                        let cross = (xf.px * 0.45 + t * 1.9).cos() * (xf.pz * 0.45 - t * 1.4).sin();
                        let target_y =
                            (1.6 + (wave * 1.45 + cross * 0.75) * clock.amplitude).max(0.35);
                        let dy = target_y - xf.py;
                        mot.vy = dy * 6.5;
                        xf.py += dy * 0.28;
                        let crest = ((wave + 1.0) * 0.5).clamp(0.0, 1.0);
                        vis.energy = (vis.energy * 0.7 + crest * 1.25).clamp(0.0, 2.0);
                    }
                })
            });
        if !res1.is_ok() {
            return res1;
        }

        app.system(c"IntegrateMove")
            .query_typed::<(&mut Transform3D, &mut Motion3D, &ActorMeta)>()
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
                let dt = (clock.dt * clock.speed_scale).clamp(0.0, 0.05);

                ctx.single_query::<(&mut Transform3D, &mut Motion3D, &ActorMeta)>()
                    .for_each(|(transforms, motions, metas)| {
                        for ((xf, mot), meta) in transforms
                            .iter_mut()
                            .zip(motions.iter_mut())
                            .zip(metas.iter())
                        {
                            xf.px += mot.vx * dt;
                            xf.py += mot.vy * dt;
                            xf.pz += mot.vz * dt;

                            // Gentle velocity damping so chained impulses stay controllable
                            let damp = if (meta.flags & 1) != 0 { 0.992 } else { 0.975 };
                            mot.vx *= damp;
                            mot.vy *= damp;
                            mot.vz *= damp;

                            // Soft arena boundary reflection
                            let r_sq = xf.px * xf.px + xf.pz * xf.pz;
                            if r_sq > ARENA_BOUND * ARENA_BOUND {
                                let r = r_sq.sqrt();
                                let nx = xf.px / r;
                                let nz = xf.pz / r;
                                xf.px = nx * ARENA_BOUND;
                                xf.pz = nz * ARENA_BOUND;
                                let dot = mot.vx * nx + mot.vz * nz;
                                if dot > 0.0 {
                                    mot.vx -= 1.75 * dot * nx;
                                    mot.vz -= 1.75 * dot * nz;
                                }
                            }
                            if xf.py < 0.35 {
                                xf.py = 0.35;
                                if mot.vy < 0.0 {
                                    mot.vy = -mot.vy * 0.72;
                                }
                            } else if xf.py > 10.5 {
                                xf.py = 10.5;
                                if mot.vy > 0.0 {
                                    mot.vy = -mot.vy * 0.65;
                                }
                            }
                        }
                    })
            })
    }
}

export_mod!(plugin: MotionMod);
