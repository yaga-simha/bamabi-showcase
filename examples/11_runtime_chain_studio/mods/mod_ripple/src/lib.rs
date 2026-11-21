#![forbid(unsafe_code)]

//! `mod_ripple` — Buffered FFI `Messages<PulseEvent>` pub-sub (`EmitShockwave`, `ReactShockwave`).
//!
//! Showcases `bamabi`'s zero-serialization `Messages<M>` FFI streams:
//! - `EmitShockwave` publishes periodic radial `PulseEvent` messages.
//! - `ReactShockwave` drains unread `PulseEvent`s and propagates radial shockwave rings,
//!   spiking `VisualStyle.pulse`, `emissive`, and vertical impulse!
//!
//! Notice: chaining `EmitShockwave -> ReactShockwave` reacts on the SAME tick; reversing to
//! `ReactShockwave -> EmitShockwave` introduces a 1-frame buffered pipeline delay!

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use studio_abi::{Motion3D, PulseEvent, SimClock, Transform3D, VisualStyle};

#[derive(Default)]
pub struct RippleMod;

impl GuestPlugin for RippleMod {
    const NAME: &'static CStr = c"mod_ripple";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        let mut last_emit_tick: u64 = 0;
        let res1 = app
            .system(c"EmitShockwave")
            .resource_read::<SimClock>()
            .message_write::<PulseEvent>()
            .schedule(ScheduleId::UPDATE)
            .build(move |ctx| {
                let clock = ctx
                    .component_resource::<SimClock>(0)
                    .copied()
                    .unwrap_or_default();
                if clock.tick == 0 || clock.tick < last_emit_tick {
                    last_emit_tick = 0;
                }
                // Emit a new shockwave pulse every 35 ticks
                if clock.tick.saturating_sub(last_emit_tick) >= 35 || clock.tick == 1 {
                    last_emit_tick = clock.tick;
                    let angle = clock.elapsed * 0.9;
                    let r = 4.2 * clock.amplitude.clamp(0.5, 1.8);
                    let ev = PulseEvent {
                        source_index: (clock.tick % 997) as u32,
                        kind: ((clock.tick / 35) % 2) as u32,
                        intensity: (1.35 * clock.amplitude).clamp(0.4, 2.5),
                        ox: angle.cos() * r,
                        oy: 1.2,
                        oz: angle.sin() * r,
                    };
                    return ctx.send_message::<PulseEvent>(1, &ev);
                }
                FfiResult::OK
            });
        if !res1.is_ok() {
            return res1;
        }

        // Expanding ring wavefront state owned by ReactShockwave closure
        let mut active_wave: Option<(PulseEvent, f32)> = None;
        app.system(c"ReactShockwave")
            .query_typed::<(&mut Transform3D, &mut Motion3D, &mut VisualStyle)>()
            .resource_read::<SimClock>()
            .message_read::<PulseEvent>()
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
                if clock.tick <= 1 {
                    active_wave = None;
                }

                if let Ok(events) = ctx.read_messages::<PulseEvent>(1)
                    && let Some(latest) = events.last().copied()
                {
                    active_wave = Some((latest, 0.25));
                }

                let Some((ev, ref mut radius)) = active_wave else {
                    return FfiResult::OK;
                };
                *radius += clock.dt * clock.speed_scale * 11.5;
                let wave_r = *radius;
                let band_width = 1.65;

                let res = ctx
                    .single_query::<(&mut Transform3D, &mut Motion3D, &mut VisualStyle)>()
                    .for_each(|(transforms, motions, styles)| {
                        for ((xf, mot), vis) in transforms
                            .iter_mut()
                            .zip(motions.iter_mut())
                            .zip(styles.iter_mut())
                        {
                            let dx = xf.px - ev.ox;
                            let dz = xf.pz - ev.oz;
                            let dist = (dx * dx + dz * dz).sqrt();
                            let delta = (dist - wave_r).abs();
                            if delta < band_width {
                                let factor = (1.0 - delta / band_width) * ev.intensity;
                                vis.pulse = (vis.pulse + factor * 0.9).min(2.0);
                                vis.energy = (vis.energy + factor * 0.55).min(2.0);
                                vis.emissive = (vis.emissive + factor * 0.85).min(2.0);
                                if ev.kind == 0 {
                                    mot.vy += factor * 2.4;
                                } else {
                                    let inv = 1.0 / dist.max(0.4);
                                    mot.vx += dx * inv * factor * 1.4;
                                    mot.vz += dz * inv * factor * 1.4;
                                    mot.vy += factor * 1.8;
                                }
                            }
                        }
                    });

                if wave_r > 22.0 {
                    active_wave = None;
                }
                res
            })
    }
}

export_mod!(plugin: RippleMod);
