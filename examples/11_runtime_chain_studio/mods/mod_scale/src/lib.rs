#![forbid(unsafe_code)]

//! `mod_scale` — 3D scaling & elastic squash-and-stretch systems (`PulseScale`, `SquashStretch`).

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use studio_abi::{Motion3D, SimClock, Transform3D, VisualStyle};

#[derive(Default)]
pub struct ScaleMod;

impl GuestPlugin for ScaleMod {
    const NAME: &'static CStr = c"mod_scale";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        let res1 = app
            .system(c"PulseScale")
            .query_typed::<(&mut Transform3D, &Motion3D, &mut VisualStyle)>()
            .resource_read::<SimClock>()
            .schedule(ScheduleId::UPDATE)
            .build(|ctx| {
                let clock = ctx
                    .component_resource::<SimClock>(0)
                    .copied()
                    .unwrap_or(SimClock {
                        elapsed: 0.0,
                        speed_scale: 1.0,
                        amplitude: 1.0,
                        ..Default::default()
                    });
                let t = clock.elapsed * clock.speed_scale;

                ctx.single_query::<(&mut Transform3D, &Motion3D, &mut VisualStyle)>()
                    .for_each(|(transforms, motions, styles)| {
                        for ((xf, mot), vis) in transforms
                            .iter_mut()
                            .zip(motions.iter())
                            .zip(styles.iter_mut())
                        {
                            let wave = (t * 3.4 + mot.phase + (xf.px + xf.pz) * 0.25).sin();
                            let s = (0.85
                                + 0.35 * clock.amplitude * wave
                                + vis.pulse * 0.55
                                + vis.energy * 0.18)
                                .clamp(0.35, 2.4);
                            xf.sx = s;
                            xf.sy = s;
                            xf.sz = s;
                            // Decay transient pulse gently after consuming it
                            vis.pulse = (vis.pulse * 0.94).max(0.0);
                        }
                    })
            });
        if !res1.is_ok() {
            return res1;
        }

        app.system(c"SquashStretch")
            .query_typed::<(&mut Transform3D, &Motion3D, &VisualStyle)>()
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

                ctx.single_query::<(&mut Transform3D, &Motion3D, &VisualStyle)>()
                    .for_each(|(transforms, motions, styles)| {
                        for ((xf, mot), vis) in
                            transforms.iter_mut().zip(motions.iter()).zip(styles.iter())
                        {
                            // Volume-preserving elastic deformation driven by vertical motion + pulse!
                            let stretch = (1.0
                                + mot.vy * 0.14 * clock.amplitude
                                + vis.pulse * 0.45 * clock.amplitude)
                                .clamp(0.55, 2.15);
                            let inv = 1.0 / stretch.sqrt();
                            xf.sy = (xf.sy * 0.45 + stretch * 0.55).clamp(0.45, 2.4);
                            xf.sx = (xf.sx * 0.45 + inv * 0.55).clamp(0.45, 2.2);
                            xf.sz = (xf.sz * 0.45 + inv * 0.55).clamp(0.45, 2.2);
                        }
                    })
            })
    }
}

export_mod!(plugin: ScaleMod);
