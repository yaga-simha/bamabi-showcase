#![forbid(unsafe_code)]

//! `mod_vortex` — High-throughput vortex & orbital attraction systems (`SpiralVortex`, `AttractCenter`).
//!
//! Showcases `bamabi`'s parallel multi-threaded chunk execution (`ctx.par_chunks`) across Bevy's
//! `ComputeTaskPool` alongside zero-serialization SoA column slices.

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use studio_abi::{ActorMeta, Motion3D, SimClock, Transform3D, VisualStyle};

#[derive(Default)]
pub struct VortexMod;

impl GuestPlugin for VortexMod {
    const NAME: &'static CStr = c"mod_vortex";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        // 1. SpiralVortex — parallel chunk processing (`par_chunks`) across ComputeTaskPool!
        let res1 = app
            .system(c"SpiralVortex")
            .query(|q| {
                q.write::<Transform3D>()
                    .write::<Motion3D>()
                    .write::<VisualStyle>()
                    .read::<ActorMeta>()
            })
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
                let dt = (clock.dt * clock.speed_scale).clamp(0.0, 0.05);
                let t = clock.elapsed * clock.speed_scale;
                let amp = clock.amplitude;

                ctx.par_chunks(move |mut chunk| {
                    let n = chunk.len();
                    let mut start = 0usize;
                    let mut energies = [0.0_f32; 64];
                    while start < n {
                        let len = (n - start).min(64);
                        {
                            let Some((transforms, motions)) =
                                chunk.component_write_2::<Transform3D, Motion3D>(0, 1)
                            else {
                                return;
                            };
                            for (k, out_e) in energies[..len].iter_mut().enumerate() {
                                let i = start + k;
                                let xf = &mut transforms[i];
                                let mot = &mut motions[i];

                                let r_sq = xf.px * xf.px + xf.pz * xf.pz;
                                let r = r_sq.sqrt().max(0.45);
                                let nx = xf.px / r;
                                let nz = xf.pz / r;
                                // Tangential direction (-nz, nx)
                                let tx = -nz;
                                let tz = nx;

                                let swirl = (3.8 / (0.65 + r * 0.22)) * amp;
                                let target_r = 2.6 + (r - 2.6).clamp(-2.0, 6.0) * 0.96;
                                let radial_pull = (target_r - r) * 1.45;

                                mot.vx = mot.vx * 0.85 + (tx * swirl + nx * radial_pull) * 0.35;
                                mot.vz = mot.vz * 0.85 + (tz * swirl + nz * radial_pull) * 0.35;
                                xf.px += mot.vx * dt;
                                xf.pz += mot.vz * dt;

                                let funnel_y =
                                    0.75 + (r * 0.38) + (t * 2.2 + mot.phase).sin() * 0.55 * amp;
                                let dy = funnel_y - xf.py;
                                mot.vy = dy * 2.6;
                                xf.py = (xf.py + mot.vy * dt).clamp(0.35, 9.5);

                                *out_e = (swirl * 0.24).clamp(0.1, 1.8);
                            }
                        }
                        {
                            let Some(styles) = chunk.component_mut::<VisualStyle>(2) else {
                                return;
                            };
                            for (k, &intensity) in energies[..len].iter().enumerate() {
                                let vis = &mut styles[start + k];
                                vis.energy = (vis.energy * 0.75 + intensity * 0.45).clamp(0.0, 2.0);
                            }
                        }
                        start += len;
                    }
                })
            });
        if !res1.is_ok() {
            return res1;
        }

        // 2. AttractCenter — spring attraction toward a roaming figure-8 attractor
        app.system(c"AttractCenter")
            .query_typed::<(&mut Transform3D, &mut Motion3D, &mut VisualStyle)>()
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
                let dt = (clock.dt * clock.speed_scale).clamp(0.0, 0.05);
                let t = clock.elapsed * clock.speed_scale;
                let focus_x = (t * 0.85).cos() * 4.5 * clock.amplitude;
                let focus_y = 2.4 + (t * 1.3).sin() * 1.2 * clock.amplitude;
                let focus_z = (t * 1.7).sin() * 3.2 * clock.amplitude;

                ctx.single_query::<(&mut Transform3D, &mut Motion3D, &mut VisualStyle)>()
                    .for_each(|(transforms, motions, styles)| {
                        for ((xf, mot), vis) in transforms
                            .iter_mut()
                            .zip(motions.iter_mut())
                            .zip(styles.iter_mut())
                        {
                            let dx = focus_x - xf.px;
                            let dy = focus_y - xf.py;
                            let dz = focus_z - xf.pz;
                            let dist = (dx * dx + dy * dy + dz * dz).sqrt().max(0.5);
                            let pull = (4.8 * clock.amplitude) / (1.0 + dist * 0.22);
                            // Repel slightly if too close to prevent clumping into a singularity
                            let sign = if dist < 1.8 { -0.8 } else { 1.0 };
                            mot.vx += (dx / dist) * pull * sign * dt;
                            mot.vy += (dy / dist) * pull * sign * dt;
                            mot.vz += (dz / dist) * pull * sign * dt;
                            vis.energy = (vis.energy + (1.2 / (1.0 + dist * 0.4)) * dt * 2.5)
                                .clamp(0.0, 2.0);
                        }
                    })
            })
    }
}

export_mod!(plugin: VortexMod);
