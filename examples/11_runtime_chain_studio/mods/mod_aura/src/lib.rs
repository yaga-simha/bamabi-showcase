#![forbid(unsafe_code)]

//! `mod_aura` — Runtime dynamic cross-mod component (`mod_aura::AuraChargeData`) &
//! batched `CommandArena` `ModScoped` entity spawning (`AuraCharge`, `SpawnSparks`).
//!
//! Showcases:
//! 1. Registering a dynamic `AbiPod` component unknown to the host at compile time (`AuraChargeData`).
//! 2. Attaching it to live host entities via deferred `GuestCommands::insert`.
//! 3. Spawning and despawning `ModScoped` floating 3D energy spark entities via `GuestCommands::spawn`
//!    (automatically bulk-despawned by `bamabi` when `mod_aura` is unloaded!).

use bamabi_guest::prelude::*;
use core::ffi::CStr;
use studio_abi::{ActorMeta, Motion3D, SimClock, SparkTag, Transform3D, VisualStyle, hsv_to_rgb};

/// Runtime dynamic component (`16` bytes) introduced exclusively by `mod_aura`.
/// `mod_aura_link` discovers and mutates this component across the C-ABI!
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
pub struct AuraMod;

impl GuestPlugin for AuraMod {
    const NAME: &'static CStr = c"mod_aura";
    const VERSION: &'static CStr = c"0.3.0";

    fn build(&mut self, app: &mut GuestApp<'_>) -> FfiResult {
        // Register runtime dynamic component (zero host compile-time knowledge!)
        let aura_h = match app.register_component::<AuraChargeData>() {
            Ok(h) => h,
            Err(err) => return err,
        };
        let xf_h = match app.resolve_component::<Transform3D>() {
            Ok(h) => h,
            Err(err) => return err,
        };
        let mot_h = match app.resolve_component::<Motion3D>() {
            Ok(h) => h,
            Err(err) => return err,
        };
        let vis_h = match app.resolve_component::<VisualStyle>() {
            Ok(h) => h,
            Err(err) => return err,
        };
        let meta_h = match app.resolve_component::<ActorMeta>() {
            Ok(h) => h,
            Err(err) => return err,
        };
        let spark_h = match app.resolve_component::<SparkTag>() {
            Ok(h) => h,
            Err(err) => return err,
        };

        // System 1: AuraCharge — attaches `AuraChargeData` to actors lacking it and charges aura!
        let res1 = app
            .system(c"AuraCharge")
            .query(|q| {
                q.read::<ActorMeta>()
                    .read::<Transform3D>()
                    .without_handle(aura_h)
            })
            .query(|q| {
                q.write_handle(aura_h)
                    .write::<VisualStyle>()
                    .read::<Transform3D>()
            })
            .resource_read::<SimClock>()
            .schedule(ScheduleId::UPDATE)
            .build(move |ctx| {
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

                // Attach `AuraChargeData` to up to 128 un-instrumented actors per tick
                let mut to_attach = Vec::new();
                if let Some(q0) = ctx.query_mut(0) {
                    while let Some(chunk) = q0.next_chunk_view() {
                        let entities = chunk.entities();
                        let Some(metas) = chunk.component::<ActorMeta>(0) else {
                            return FfiResult::TYPE_MISMATCH;
                        };
                        for (&entity, meta) in entities.iter().zip(metas.iter()) {
                            if (meta.flags & 1) != 0 {
                                continue;
                            }
                            to_attach.push((entity, meta.index));
                            if to_attach.len() >= 128 {
                                break;
                            }
                        }
                        if to_attach.len() >= 128 {
                            break;
                        }
                    }
                }
                if !to_attach.is_empty() {
                    let cmds = ctx.commands();
                    for (entity, idx) in to_attach {
                        let init = AuraChargeData {
                            charge: 0.25,
                            max_charge: 1.0,
                            harmonic: (idx as f32) * 0.37,
                            generation: 1,
                        };
                        let status = cmds.insert(entity, aura_h, &init);
                        if !status.is_ok() {
                            return status;
                        }
                    }
                }

                // Charge existing AuraChargeData columns
                let dt = clock.dt * clock.speed_scale;
                let t = clock.elapsed * clock.speed_scale;
                if let Some(q1) = ctx.query_mut(1) {
                    while let Some(mut chunk) = q1.next_chunk_view() {
                        let n = chunk.len();
                        let mut boosts = [0.0_f32; 64];
                        let mut start = 0;
                        while start < n {
                            let len = (n - start).min(64);
                            {
                                let Some(auras) = chunk.component_mut::<AuraChargeData>(0) else {
                                    return FfiResult::TYPE_MISMATCH;
                                };
                                for (k, slot) in boosts[..len].iter_mut().enumerate() {
                                    let a = &mut auras[start + k];
                                    let wave = 0.5 + 0.5 * (t * 2.4 + a.harmonic).sin();
                                    a.charge = (a.charge
                                        + dt * (0.45 + 0.75 * wave) * clock.amplitude)
                                        .min(a.max_charge);
                                    if a.charge >= a.max_charge * 0.99 {
                                        a.charge = 0.15;
                                        a.generation = a.generation.wrapping_add(1);
                                        *slot = 1.0;
                                    } else {
                                        *slot = a.charge * 0.45;
                                    }
                                }
                            }
                            {
                                let Some(styles) = chunk.component_mut::<VisualStyle>(1) else {
                                    return FfiResult::TYPE_MISMATCH;
                                };
                                for (k, &boost) in boosts[..len].iter().enumerate() {
                                    let vis = &mut styles[start + k];
                                    vis.emissive =
                                        (vis.emissive * 0.8 + boost * 0.9).clamp(0.15, 2.0);
                                    if boost >= 0.95 {
                                        vis.pulse = (vis.pulse + 0.75).min(2.0);
                                    }
                                }
                            }
                            start += len;
                        }
                    }
                }

                FfiResult::OK
            });
        if !res1.is_ok() {
            return res1;
        }

        // System 2: SpawnSparks — spawns & ages ModScoped 3D spark particles via batched CommandArena
        let mut spawn_seq: u32 = 0;
        app.system(c"SpawnSparks")
            .query_typed::<(
                FfiEntity,
                &mut Transform3D,
                &mut Motion3D,
                &mut VisualStyle,
                &mut SparkTag,
            )>()
            .resource_read::<SimClock>()
            .schedule(ScheduleId::UPDATE)
            .build(move |ctx| {
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

                let mut active_sparks = 0usize;
                let mut expired = Vec::new();
                let res = ctx
                    .single_query::<(
                        FfiEntity,
                        &mut Transform3D,
                        &mut Motion3D,
                        &mut VisualStyle,
                        &mut SparkTag,
                    )>()
                    .for_each(|(entities, transforms, motions, styles, sparks)| {
                        for i in 0..entities.len() {
                            active_sparks += 1;
                            let sp = &mut sparks[i];
                            sp.ttl -= dt;
                            if sp.ttl <= 0.0 {
                                expired.push(entities[i]);
                                continue;
                            }
                            let xf = &mut transforms[i];
                            let mot = &mut motions[i];
                            let vis = &mut styles[i];
                            xf.px += mot.vx * dt;
                            xf.py += mot.vy * dt;
                            xf.pz += mot.vz * dt;
                            mot.vy -= 2.8 * dt;
                            xf.ry += 4.5 * dt;
                            let fade = (sp.ttl / 1.8).clamp(0.1, 1.0);
                            xf.sx = 0.28 * fade;
                            xf.sy = 0.28 * fade;
                            xf.sz = 0.28 * fade;
                            vis.emissive = 1.6 * fade;
                        }
                    });
                if !res.is_ok() {
                    return res;
                }

                let cmds = ctx.commands();
                for e in expired {
                    let s = cmds.despawn(e);
                    if !s.is_ok() {
                        return s;
                    }
                }

                // Spawn fresh sparks up to 48 live sparks (respecting CommandArena & spawn caps cleanly)
                let to_spawn = 48usize.saturating_sub(active_sparks).min(4);
                for _ in 0..to_spawn {
                    spawn_seq = spawn_seq.wrapping_add(1);
                    let ang = (spawn_seq as f32) * 2.399_963 + clock.elapsed;
                    let r = 1.2 + ((spawn_seq % 7) as f32) * 0.45;
                    let hue = (ang * 0.15 + clock.elapsed * 0.1).rem_euclid(1.0);
                    let (cr, cg, cb) = hsv_to_rgb(hue, 0.9, 1.0);

                    let xf = Transform3D {
                        px: ang.cos() * r,
                        py: 0.8 + ((spawn_seq % 5) as f32) * 0.35,
                        pz: ang.sin() * r,
                        rx: 0.0,
                        ry: ang,
                        rz: 0.4,
                        sx: 0.26,
                        sy: 0.26,
                        sz: 0.26,
                        shape: 0,
                    };
                    let mot = Motion3D {
                        vx: ang.cos() * 2.4 * clock.amplitude,
                        vy: (4.2 + ((spawn_seq % 4) as f32) * 0.7) * clock.amplitude,
                        vz: ang.sin() * 2.4 * clock.amplitude,
                        avx: 2.0,
                        avy: 4.0,
                        avz: 1.5,
                        speed: 3.0,
                        phase: ang,
                    };
                    let vis = VisualStyle {
                        r: cr,
                        g: cg,
                        b: cb,
                        a: 1.0,
                        emissive: 1.75,
                        hue,
                        energy: 1.2,
                        pulse: 1.0,
                    };
                    let meta = ActorMeta {
                        index: 100_000 + spawn_seq,
                        group: 99,
                        home_x: 0.0,
                        home_y: 1.0,
                        home_z: 0.0,
                        flags: 1, // spark flag
                    };
                    let tag = SparkTag { ttl: 1.8 };

                    if let Err(err) = cmds.spawn(&[
                        SpawnEntry::new(xf_h, &xf),
                        SpawnEntry::new(mot_h, &mot),
                        SpawnEntry::new(vis_h, &vis),
                        SpawnEntry::new(meta_h, &meta),
                        SpawnEntry::new(spark_h, &tag),
                    ]) {
                        // Gracefully stop if host `ModBudgetPolicy::max_spawns_per_frame` caps us!
                        if err == FfiResult::CAPACITY_EXCEEDED {
                            break;
                        }
                        return err;
                    }
                }

                FfiResult::OK
            })
    }
}

export_mod!(plugin: AuraMod);
