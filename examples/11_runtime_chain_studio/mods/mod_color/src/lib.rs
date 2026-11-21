#![forbid(unsafe_code)]

//! `mod_color` — Order-sensitive color & emissive shading systems (`RainbowHue`, `EnergyHeatmap`).
//!
//! Notice how chaining `RainbowHue -> EnergyHeatmap` blends a velocity/energy thermal overlay
//! over the rainbow, whereas `EnergyHeatmap -> RainbowHue` shifts the thermal colors through
//! a spinning prismatic hue spectrum!

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use studio_abi::{Motion3D, SimClock, Transform3D, VisualStyle, hsv_to_rgb};

#[derive(Default)]
pub struct ColorMod;

impl GuestPlugin for ColorMod {
    const NAME: &'static CStr = c"mod_color";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        let res1 = app
            .system(c"RainbowHue")
            .query_typed::<(&Transform3D, &Motion3D, &mut VisualStyle)>()
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
                let t = clock.elapsed * clock.speed_scale;

                ctx.single_query::<(&Transform3D, &Motion3D, &mut VisualStyle)>()
                    .for_each(|(transforms, motions, styles)| {
                        for ((xf, mot), vis) in
                            transforms.iter().zip(motions.iter()).zip(styles.iter_mut())
                        {
                            let spatial =
                                (xf.px * 0.045 + xf.pz * 0.045 + xf.py * 0.08) + (xf.ry * 0.08);
                            let h = (vis.hue * 0.35 + spatial + t * 0.16 + mot.phase * 0.05)
                                .rem_euclid(1.0);
                            let sat = (0.78 + vis.pulse * 0.22).clamp(0.4, 1.0);
                            let val = (0.82 + vis.energy * 0.18).clamp(0.45, 1.0);
                            let (r, g, b) = hsv_to_rgb(h, sat, val);
                            vis.hue = h;
                            vis.r = r;
                            vis.g = g;
                            vis.b = b;
                            vis.a = 1.0;
                            vis.emissive = (0.25 + vis.pulse * 0.65).clamp(0.15, 1.8);
                        }
                    })
            });
        if !res1.is_ok() {
            return res1;
        }

        app.system(c"EnergyHeatmap")
            .query_typed::<(&Transform3D, &Motion3D, &mut VisualStyle)>()
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

                ctx.single_query::<(&Transform3D, &Motion3D, &mut VisualStyle)>()
                    .for_each(|(transforms, motions, styles)| {
                        for ((xf, mot), vis) in
                            transforms.iter().zip(motions.iter()).zip(styles.iter_mut())
                        {
                            let speed =
                                (mot.vx * mot.vx + mot.vy * mot.vy + mot.vz * mot.vz).sqrt();
                            let heat = ((speed * 0.14
                                + xf.py.abs() * 0.18
                                + vis.energy * 0.45
                                + vis.pulse * 0.7)
                                * clock.amplitude)
                                .clamp(0.0, 1.0);
                            // Deep sapphire -> neon cyan -> hot magenta -> solar gold
                            let (tr, tg, tb) = if heat < 0.33 {
                                let k = heat / 0.33;
                                (0.10 + 0.06 * k, 0.28 + 0.62 * k, 0.78 + 0.22 * k)
                            } else if heat < 0.66 {
                                let k = (heat - 0.33) / 0.33;
                                (0.16 + 0.80 * k, 0.90 - 0.56 * k, 1.00 - 0.18 * k)
                            } else {
                                let k = (heat - 0.66) / 0.34;
                                (0.96 + 0.04 * k, 0.34 + 0.58 * k, 0.82 - 0.64 * k)
                            };
                            // Blend 75% heatmap with 25% incoming color so upstream color mods stay visible
                            vis.r = vis.r * 0.25 + tr * 0.75;
                            vis.g = vis.g * 0.25 + tg * 0.75;
                            vis.b = vis.b * 0.25 + tb * 0.75;
                            vis.a = 1.0;
                            vis.emissive = (0.20 + heat * 1.35).min(2.0);
                            vis.energy = (vis.energy * 0.92 + heat * 0.22).clamp(0.0, 2.0);
                        }
                    })
            })
    }
}

export_mod!(plugin: ColorMod);
