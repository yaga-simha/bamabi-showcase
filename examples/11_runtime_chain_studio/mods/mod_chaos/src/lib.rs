#![forbid(unsafe_code)]

//! `mod_chaos` — Fault isolation, panic containment, quarantine & budget-watchdog stress mod
//! (`PanicFaultDemo`, `HeavyLoadStress`).
//!
//! Showcases:
//! 1. `PanicFaultDemo`: runs cleanly for 5 ticks pulsing crimson-amber, then intentionally panics!
//!    `bamabi_guest`'s FFI trampoline catches the unwind (`FfiResult::PANIC_CAUGHT`), the host
//!    quarantines ONLY `PanicFaultDemo` without crashing the 3D studio, and the user can recover
//!    via `reenable mod_chaos PanicFaultDemo`!
//! 2. `HeavyLoadStress`: performs configurable deterministic math per entity so you can test
//!    `ModBudgetPolicy::max_p99_system_micros` watchdog auto-quarantine and measure real CPU cost.

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use studio_abi::{SimClock, Transform3D, VisualStyle};

#[derive(Default)]
pub struct ChaosMod;

impl GuestPlugin for ChaosMod {
    const NAME: &'static CStr = c"mod_chaos";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        // 1. PanicFaultDemo: intentionally panics on its 5th execution to demonstrate FFI panic containment
        let mut runs: u32 = 0;
        let res1 = app
            .system(c"PanicFaultDemo")
            .query_typed::<(&mut Transform3D, &mut VisualStyle)>()
            .schedule(ScheduleId::UPDATE)
            .build(move |ctx| {
                runs = runs.wrapping_add(1);
                if runs == 5 {
                    panic!(
                        "intentional PanicFaultDemo panic on run #5 (testing bamabi panic containment & ModQuarantine)"
                    );
                }

                let flash = if runs.is_multiple_of(2) { 1.0 } else { 0.45 };
                ctx.single_query::<(&mut Transform3D, &mut VisualStyle)>()
                    .for_each(|(transforms, styles)| {
                        for (xf, vis) in transforms.iter_mut().zip(styles.iter_mut()) {
                            vis.r = 1.0;
                            vis.g = 0.22 * flash;
                            vis.b = 0.14;
                            vis.emissive = 1.5 * flash;
                            xf.rz += 0.08;
                        }
                    })
            });
        if !res1.is_ok() {
            return res1;
        }

        // 2. HeavyLoadStress: deliberate multi-iteration trig workload to test p99 frame-time watchdogs
        app.system(c"HeavyLoadStress")
            .query_typed::<(&mut Transform3D, &mut VisualStyle)>()
            .resource_read::<SimClock>()
            .schedule(ScheduleId::UPDATE)
            .build(|ctx| {
                let clock = ctx
                    .component_resource::<SimClock>(0)
                    .copied()
                    .unwrap_or(SimClock {
                        elapsed: 0.0,
                        amplitude: 1.0,
                        ..Default::default()
                    });
                let iters = (64.0 * clock.amplitude.clamp(0.5, 4.0)) as u32;

                ctx.single_query::<(&mut Transform3D, &mut VisualStyle)>()
                    .for_each(|(transforms, styles)| {
                        for (xf, vis) in transforms.iter_mut().zip(styles.iter_mut()) {
                            let mut acc = xf.px * 0.13 + xf.pz * 0.17 + clock.elapsed;
                            for k in 0..iters {
                                acc = (acc + (k as f32) * 0.017).sin().cos() + acc * 0.001;
                            }
                            let delta = acc.clamp(-0.6, 0.6);
                            xf.ry += delta * 0.05;
                            vis.emissive = (0.5 + delta.abs() * 1.4).min(2.0);
                        }
                    })
            })
    }
}

export_mod!(plugin: ChaosMod);
