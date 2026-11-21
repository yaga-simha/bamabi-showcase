#![forbid(unsafe_code)]

//! Interactive REPL command parser & executor for Example 11 (`11_runtime_chain_studio`).
//!
//! Runs identically in interactive `winit` 3D mode (via background stdin thread -> main-thread
//! exclusive `&mut StudioEngine`) and in `--headless` / `--script` CLI modes.

use crate::components::{ChainGate, Formation, SimClock};
use crate::engine::{
    BUILTIN_MODS, StudioEngine, build_all_studio_mods, builtin_mod_binary_path,
    resolve_mod_input_path,
};
use bamabi::prelude::*;
use bamabi_host_sys::schedule::ScheduleSetMode;

/// Control action returned by REPL command execution so the host loop can pause/resume/quit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplAction {
    Continue,
    Pause,
    Resume,
    Quit,
}

/// Outcome of executing a single REPL line.
pub struct ReplOutcome {
    pub output: String,
    pub action: ReplAction,
}

impl ReplOutcome {
    fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            action: ReplAction::Continue,
        }
    }

    fn with_action(output: impl Into<String>, action: ReplAction) -> Self {
        Self {
            output: output.into(),
            action,
        }
    }
}

/// Splits a command line respecting single and double quotes (so Windows/macOS paths with spaces work).
fn split_shell_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;

    for ch in line.chars() {
        match quote {
            Some(q) if ch == q => {
                quote = None;
            }
            Some(_) => cur.push(ch),
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        tokens.push(core::mem::take(&mut cur));
                    }
                }
                _ => cur.push(ch),
            },
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// Executes one REPL command line against `engine`.
pub fn execute_repl_line(engine: &mut StudioEngine, raw_line: &str) -> ReplOutcome {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return ReplOutcome::ok("");
    }

    let tokens = split_shell_tokens(trimmed);
    if tokens.is_empty() {
        return ReplOutcome::ok("");
    }

    let cmd = tokens[0].to_ascii_lowercase();
    let args = &tokens[1..];

    match cmd.as_str() {
        "help" | "?" => ReplOutcome::ok(help_text()),
        "quit" | "exit" | "q" => {
            ReplOutcome::with_action("Exiting Runtime Chain Studio...", ReplAction::Quit)
        }
        "pause" => ReplOutcome::with_action(
            "Simulation PAUSED (use `step [N]`, `run <N>`, or `resume`).",
            ReplAction::Pause,
        ),
        "resume" | "play" => ReplOutcome::with_action("Simulation RUNNING.", ReplAction::Resume),
        "catalog" | "available" => ReplOutcome::ok(format_catalog()),
        "build-mods" | "build" => match build_all_studio_mods() {
            Ok(msg) => ReplOutcome::ok(msg),
            Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
        },
        "load" => cmd_load(engine, args),
        "load-all" => cmd_load_all(engine),
        "unload" => cmd_unload(engine, args),
        "reload" => cmd_reload(engine, args),
        "mods" => ReplOutcome::ok(format_loaded_mods(engine)),
        "systems" | "sys" => ReplOutcome::ok(format_systems(engine)),
        "chain" => cmd_chain(engine, trimmed, args),
        "solo" => cmd_solo(engine, args),
        "enable" => cmd_toggle_system(engine, args, true),
        "disable" | "mute" => cmd_toggle_system(engine, args, false),
        "preset" | "presets" => cmd_preset(engine, args),
        "reset" => cmd_reset(engine, args),
        "step" => cmd_step(engine, args),
        "run" => cmd_run(engine, args),
        "count" | "entities" | "spawn" => cmd_count(engine, args),
        "formation" | "layout" => cmd_formation(engine, args),
        "speed" => cmd_speed(engine, args),
        "amp" | "amplitude" => cmd_amplitude(engine, args),
        "perf" | "stats" => cmd_perf(engine, args),
        "bench" => cmd_bench(engine, args),
        "budget" => cmd_budget(engine, args),
        "reenable" | "unquarantine" => cmd_reenable(engine, args),
        "events" | "log" => ReplOutcome::ok(format_events(engine)),
        other => ReplOutcome::ok(format!(
            "Unknown command `{other}`. Type `help` to see all commands."
        )),
    }
}

fn cmd_load(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    if args.is_empty() {
        return ReplOutcome::ok(
            "Usage: load <path/to/mod.dll|.dylib|.so | @short_name> [...]\n\
             Tip: run `catalog` to see exact shared-library paths on disk.",
        );
    }
    let mut lines = Vec::new();
    for raw in args {
        match resolve_mod_input_path(raw) {
            Ok(path) => match engine.load_mod_path(&path) {
                Ok((mod_id, systems)) => {
                    let sys_list: Vec<String> =
                        systems.iter().map(|s| s.system_name.clone()).collect();
                    lines.push(format!(
                        "Loaded `{}` as {mod_id:?} in {:.1} us -> discovered {} system(s): [{}]",
                        path.display(),
                        engine.last_load_us,
                        systems.len(),
                        sys_list.join(", ")
                    ));
                }
                Err(err) => lines.push(format!("ERROR: {err}")),
            },
            Err(err) => lines.push(format!("ERROR: {err}")),
        }
    }
    ReplOutcome::ok(lines.join("\n"))
}

fn cmd_load_all(engine: &mut StudioEngine) -> ReplOutcome {
    let mut loaded = 0usize;
    let mut skipped = 0usize;
    let mut errs = Vec::new();
    for spec in BUILTIN_MODS {
        if spec.short_name == "chaos" {
            continue;
        }
        if engine.resolve_loaded_mod(spec.binary_stem).is_ok() {
            skipped += 1;
            continue;
        }
        match resolve_mod_input_path(spec.binary_stem) {
            Ok(p) => match engine.load_mod_path(&p) {
                Ok(_) => loaded += 1,
                Err(e) => errs.push(format!("{}: {e}", spec.binary_stem)),
            },
            Err(e) => errs.push(format!("{}: {e}", spec.binary_stem)),
        }
    }
    let mut msg = format!(
        "Loaded {loaded} studio mods ({skipped} already loaded). Total discovered systems: {}.",
        engine.discovered.len()
    );
    if !errs.is_empty() {
        msg.push_str("\nErrors:\n  ");
        msg.push_str(&errs.join("\n  "));
    }
    msg.push_str("\nRun `systems` to list all systems or `chain WaveLift -> SpinYaw -> RainbowHue -> SquashStretch`.");
    ReplOutcome::ok(msg)
}

fn cmd_unload(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let Some(target) = args.first() else {
        return ReplOutcome::ok("Usage: unload <mod_name | mod_id | all>");
    };
    if target.eq_ignore_ascii_case("all") {
        let ids: Vec<ModId> = engine
            .loaded_mod_summaries()
            .into_iter()
            .map(|m| m.id)
            .collect();
        if ids.is_empty() {
            return ReplOutcome::ok("No mods are currently loaded.");
        }
        let mut count = 0usize;
        for id in ids.into_iter().rev() {
            if engine.unload_mod_by_id(id).is_ok() {
                count += 1;
            }
        }
        return ReplOutcome::ok(format!("Unloaded {count} mod(s) cleanly."));
    }
    match engine
        .resolve_loaded_mod(target)
        .and_then(|id| engine.unload_mod_by_id(id))
    {
        Ok(msg) => ReplOutcome::ok(msg),
        Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
    }
}

fn cmd_reload(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let Some(target) = args.first() else {
        return ReplOutcome::ok("Usage: reload <mod_name | mod_id>");
    };
    match engine
        .resolve_loaded_mod(target)
        .and_then(|id| engine.reload_mod_by_id(id))
    {
        Ok(msg) => ReplOutcome::ok(msg),
        Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
    }
}

fn cmd_chain(engine: &mut StudioEngine, raw_line: &str, args: &[String]) -> ReplOutcome {
    if args.is_empty() {
        let solo = engine.app.world().resource::<ChainGate>().solo_chain;
        return ReplOutcome::ok(format!(
            "Active Chain ({:?}, solo={}):\n  {}\n  Last schedule rewire: {:.1} us",
            engine.chain_mode,
            if solo { "on" } else { "off" },
            engine.chain_display(),
            engine.last_rewire_us
        ));
    }

    match args[0].to_ascii_lowercase().as_str() {
        "clear" => match engine.set_chain(Vec::new()) {
            Ok(us) => ReplOutcome::ok(format!(
                "Cleared runtime chain (rewired in {us:.1} us). All enabled loaded systems now run freely."
            )),
            Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
        },
        "reverse" => {
            let mut next = engine.chain.clone();
            next.reverse();
            match engine.set_chain(next) {
                Ok(us) => ReplOutcome::ok(format!(
                    "Reversed chain in {us:.1} us:\n  {}",
                    engine.chain_display()
                )),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            }
        }
        "mode" => {
            let Some(m) = args.get(1) else {
                return ReplOutcome::ok(format!(
                    "Current mode: {:?}\nUsage: chain mode <sequence | weak | parallel>",
                    engine.chain_mode
                ));
            };
            let parsed = match m.to_ascii_lowercase().as_str() {
                "seq" | "sequence" | "strict" => ScheduleSetMode::Sequence,
                "weak" | "sequence_weak" | "sequence-weak" => ScheduleSetMode::SequenceWeak,
                "par" | "parallel" => ScheduleSetMode::Parallel,
                other => {
                    return ReplOutcome::ok(format!(
                        "Unknown mode `{other}`. Expected: sequence, weak, or parallel."
                    ));
                }
            };
            match engine.set_chain_mode(parsed) {
                Ok(us) => ReplOutcome::ok(format!(
                    "Switched chain mode to {parsed:?} (rewired in {us:.1} us):\n  {}",
                    engine.chain_display()
                )),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            }
        }
        "add" | "push" | "append" => {
            if args.len() < 2 {
                return ReplOutcome::ok("Usage: chain add <system> [...]");
            }
            let mut next = engine.chain.clone();
            for tok in &args[1..] {
                match engine.resolve_discovered_system(tok) {
                    Ok(sys) => {
                        if !next.iter().any(|x| x.key() == sys.key()) {
                            next.push(sys);
                        }
                    }
                    Err(err) => return ReplOutcome::ok(format!("ERROR: {err}")),
                }
            }
            match engine.set_chain(next) {
                Ok(us) => ReplOutcome::ok(format!(
                    "Updated chain in {us:.1} us:\n  {}",
                    engine.chain_display()
                )),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            }
        }
        "insert" => {
            if args.len() < 3 {
                return ReplOutcome::ok("Usage: chain insert <index> <system>");
            }
            let Ok(idx) = args[1].parse::<usize>() else {
                return ReplOutcome::ok(format!("Invalid index `{}`", args[1]));
            };
            let sys = match engine.resolve_discovered_system(&args[2]) {
                Ok(s) => s,
                Err(err) => return ReplOutcome::ok(format!("ERROR: {err}")),
            };
            let mut next = engine.chain.clone();
            next.retain(|x| x.key() != sys.key());
            let clamped = idx.min(next.len());
            next.insert(clamped, sys);
            match engine.set_chain(next) {
                Ok(us) => ReplOutcome::ok(format!(
                    "Inserted at #{clamped} (rewired in {us:.1} us):\n  {}",
                    engine.chain_display()
                )),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            }
        }
        "remove" | "rm" | "del" => {
            let Some(target) = args.get(1) else {
                return ReplOutcome::ok("Usage: chain remove <index | system_name>");
            };
            let mut next = engine.chain.clone();
            if let Ok(idx) = target.parse::<usize>() {
                if idx >= next.len() {
                    return ReplOutcome::ok(format!(
                        "Index {idx} out of range (chain has {} elements).",
                        next.len()
                    ));
                }
                next.remove(idx);
            } else {
                let sys = match engine.resolve_discovered_system(target) {
                    Ok(s) => s,
                    Err(err) => return ReplOutcome::ok(format!("ERROR: {err}")),
                };
                next.retain(|x| x.key() != sys.key());
            }
            match engine.set_chain(next) {
                Ok(us) => ReplOutcome::ok(format!(
                    "Removed from chain (rewired in {us:.1} us):\n  {}",
                    engine.chain_display()
                )),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            }
        }
        "swap" => {
            if args.len() < 3 {
                return ReplOutcome::ok("Usage: chain swap <idx_a> <idx_b>");
            }
            let (Ok(a), Ok(b)) = (args[1].parse::<usize>(), args[2].parse::<usize>()) else {
                return ReplOutcome::ok("Usage: chain swap <idx_a> <idx_b> (0-based indices)");
            };
            let mut next = engine.chain.clone();
            if a >= next.len() || b >= next.len() {
                return ReplOutcome::ok(format!(
                    "Indices ({a}, {b}) out of range (chain length: {}).",
                    next.len()
                ));
            }
            next.swap(a, b);
            match engine.set_chain(next) {
                Ok(us) => ReplOutcome::ok(format!(
                    "Swapped #{a} <-> #{b} (rewired in {us:.1} us):\n  {}",
                    engine.chain_display()
                )),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            }
        }
        _ => {
            // Parse full chain specification: `chain A -> B -> C` or `chain A, B, C` or `chain A B C`
            let rest = raw_line
                .trim()
                .strip_prefix("chain")
                .or_else(|| raw_line.trim().strip_prefix("CHAIN"))
                .unwrap_or("")
                .trim();
            let normalized = rest
                .replace("->", " ")
                .replace("~>", " ")
                .replace("||", " ")
                .replace(',', " ");
            let parts: Vec<&str> = normalized.split_whitespace().collect();
            if parts.is_empty() {
                return ReplOutcome::ok("No systems provided.");
            }
            let mut resolved = Vec::with_capacity(parts.len());
            for part in parts {
                match engine.resolve_discovered_system(part) {
                    Ok(sys) => {
                        if !resolved
                            .iter()
                            .any(|x: &crate::engine::DiscoveredSystem| x.key() == sys.key())
                        {
                            resolved.push(sys);
                        }
                    }
                    Err(err) => return ReplOutcome::ok(format!("ERROR: {err}")),
                }
            }
            match engine.set_chain(resolved) {
                Ok(us) => ReplOutcome::ok(format!(
                    "Rewired runtime chain ({:?}) in {us:.1} us:\n  {}",
                    engine.chain_mode,
                    engine.chain_display()
                )),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            }
        }
    }
}

fn cmd_solo(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let mut gate = engine.app.world_mut().resource_mut::<ChainGate>();
    if let Some(arg) = args.first() {
        match arg.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => gate.solo_chain = true,
            "off" | "false" | "0" => gate.solo_chain = false,
            _ => return ReplOutcome::ok("Usage: solo <on | off>"),
        }
    } else {
        gate.solo_chain = !gate.solo_chain;
    }
    let state = if gate.solo_chain {
        "ON (only systems in active chain execute)"
    } else {
        "OFF (all enabled loaded systems execute; chained subset is strictly ordered)"
    };
    ReplOutcome::ok(format!("Chain solo mode: {state}"))
}

fn cmd_toggle_system(engine: &mut StudioEngine, args: &[String], enable: bool) -> ReplOutcome {
    let Some(tok) = args.first() else {
        return ReplOutcome::ok(if enable {
            "Usage: enable <system_name>"
        } else {
            "Usage: disable <system_name>"
        });
    };
    let sys = match engine.resolve_discovered_system(tok) {
        Ok(s) => s,
        Err(err) => return ReplOutcome::ok(format!("ERROR: {err}")),
    };
    let mut gate = engine.app.world_mut().resource_mut::<ChainGate>();
    if enable {
        gate.disabled.remove(&sys.key());
        ReplOutcome::ok(format!("Enabled `{}`.", sys.qualified()))
    } else {
        gate.disabled.insert(sys.key());
        ReplOutcome::ok(format!(
            "Disabled `{}` (skipped by Bevy run_if gate).",
            sys.qualified()
        ))
    }
}

fn cmd_preset(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let Some(name) = args.first() else {
        return ReplOutcome::ok(
            "Available presets:\n  \
             preset showcase   — WaveLift -> SpinYaw -> RainbowHue -> EnergyHeatmap -> SquashStretch\n  \
             preset tornado    — par_chunks SpiralVortex -> Tumble3D -> EnergyHeatmap -> PulseScale\n  \
             preset polyglot   — C99PlanetaryOrbit -> CppBounceGravity -> SpinYaw -> RainbowHue -> SquashStretch\n  \
             preset shockwave  — Buffered Messages<PulseEvent> shockwave ring chain\n  \
             preset aura       — Cross-mod dynamic AuraChargeData + ModScoped CommandArena sparks\n  \
             preset chaos      — Panic containment & live ModQuarantine recovery demo\n  \
             preset stress     — Parallel high-throughput benchmark preset",
        );
    };
    if name.eq_ignore_ascii_case("list") {
        return cmd_preset(engine, &[]);
    }
    match engine.apply_preset(name) {
        Ok(msg) => ReplOutcome::ok(msg),
        Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
    }
}

fn cmd_reset(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    if let Some(first) = args.first() {
        if first.eq_ignore_ascii_case("all") || first.eq_ignore_ascii_case("hard") {
            return match engine.hard_reset() {
                Ok(msg) => ReplOutcome::ok(msg),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            };
        }
        if let Some(formation) = Formation::parse(first) {
            return ReplOutcome::ok(engine.reset_scene(Some(formation), None));
        }
        return ReplOutcome::ok(format!(
            "Unknown reset target `{first}`. Use `reset`, `reset grid|ring|helix|sphere`, or `reset all`."
        ));
    }
    ReplOutcome::ok(engine.reset_scene(None, None))
}

fn cmd_step(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let n = args
        .first()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, 5000);
    let mut sum_us = 0.0;
    for _ in 0..n {
        sum_us += engine.step_ecs_tick();
    }
    let avg = sum_us / (n as f64);
    let tick = engine.app.world().resource::<SimClock>().tick;
    ReplOutcome::ok(format!(
        "Stepped {n} tick(s) -> tick={tick} | avg ECS tick: {avg:.2} us (total {:.2} ms, zero rendering)",
        sum_us / 1000.0
    ))
}

fn cmd_run(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let Some(n) = args.first().and_then(|s| s.parse::<u32>().ok()) else {
        return ReplOutcome::ok(
            "Usage: run <ticks> (runs an exact deterministic burst of ECS ticks)",
        );
    };
    let n = n.clamp(1, 20_000);
    let report = engine.run_ecs_benchmark(n, None);
    ReplOutcome::ok(report)
}

fn cmd_count(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let Some(n) = args.first().and_then(|s| s.parse::<usize>().ok()) else {
        return ReplOutcome::ok(format!(
            "Current actor count: {} (usage: count <1..30000>)",
            engine.target_actor_count
        ));
    };
    ReplOutcome::ok(engine.reset_scene(None, Some(n)))
}

fn cmd_formation(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let Some(raw) = args.first() else {
        return ReplOutcome::ok(format!(
            "Current formation: {} (usage: formation <grid | ring | helix | sphere>)",
            engine.formation.name()
        ));
    };
    let Some(f) = Formation::parse(raw) else {
        return ReplOutcome::ok(format!(
            "Unknown formation `{raw}`. Expected: grid, ring, helix, or sphere."
        ));
    };
    ReplOutcome::ok(engine.reset_scene(Some(f), None))
}

fn cmd_speed(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let mut clock = engine.app.world_mut().resource_mut::<SimClock>();
    let Some(val) = args.first().and_then(|s| s.parse::<f32>().ok()) else {
        return ReplOutcome::ok(format!(
            "Current speed_scale: {:.2} (usage: speed <0.1..5.0>)",
            clock.speed_scale
        ));
    };
    clock.speed_scale = val.clamp(0.05, 5.0);
    ReplOutcome::ok(format!(
        "Set SimClock.speed_scale = {:.2}",
        clock.speed_scale
    ))
}

fn cmd_amplitude(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    let mut clock = engine.app.world_mut().resource_mut::<SimClock>();
    let Some(val) = args.first().and_then(|s| s.parse::<f32>().ok()) else {
        return ReplOutcome::ok(format!(
            "Current amplitude: {:.2} (usage: amp <0.2..4.0>)",
            clock.amplitude
        ));
    };
    clock.amplitude = val.clamp(0.1, 4.0);
    ReplOutcome::ok(format!("Set SimClock.amplitude = {:.2}", clock.amplitude))
}

fn cmd_perf(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    if let Some(sub) = args.first() {
        match sub.to_ascii_lowercase().as_str() {
            "reset" | "clear" => {
                engine.clear_perf_window();
                return ReplOutcome::ok("Cleared rolling ECS performance window.");
            }
            "watch" => {
                let on = args
                    .get(1)
                    .map(|s| matches!(s.to_ascii_lowercase().as_str(), "on" | "1" | "true"))
                    .unwrap_or(!engine.perf_watch);
                engine.perf_watch = on;
                return ReplOutcome::ok(format!(
                    "Periodic console ECS perf watch: {}",
                    if on { "ON" } else { "OFF" }
                ));
            }
            _ => {}
        }
    }
    ReplOutcome::ok(format_perf_table(engine))
}

fn cmd_bench(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    if args
        .first()
        .is_some_and(|s| s.eq_ignore_ascii_case("compare"))
    {
        let ticks = args
            .get(1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(200)
            .clamp(10, 10_000);
        let original_mode = engine.chain_mode;
        let mut out = format!(
            "=== Schedule Mode ECS Comparison ({ticks} ticks, {} actors, ZERO rendering) ===\n",
            engine.target_actor_count
        );
        for mode in [
            ScheduleSetMode::Sequence,
            ScheduleSetMode::SequenceWeak,
            ScheduleSetMode::Parallel,
        ] {
            if let Err(err) = engine.set_chain_mode(mode) {
                out.push_str(&format!("  {mode:?}: ERROR ({err})\n"));
                continue;
            }
            let _ = engine.run_ecs_benchmark(ticks, None);
            let s = engine.perf_summary();
            out.push_str(&format!(
                "  {:<13} | rewire={:>6.1} us | ecs_mean={:>7.2} us | ecs_p50={:>7.2} us | ecs_p99={:>7.2} us | mod_p50={:>4} us | {:>6.2} M ents/s\n",
                format!("{mode:?}"),
                s.last_rewire_us,
                s.ecs_tick_mean_us,
                s.ecs_tick_p50_us,
                s.ecs_tick_p99_us,
                s.mod_sum_p50_us,
                s.aggregate_m_ents_per_sec,
            ));
        }
        let _ = engine.set_chain_mode(original_mode);
        return ReplOutcome::ok(out);
    }

    let ticks = args
        .first()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(250);
    let entities = args.get(1).and_then(|s| s.parse::<usize>().ok());
    ReplOutcome::ok(engine.run_ecs_benchmark(ticks, entities))
}

fn cmd_budget(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    if args.is_empty() || args[0].eq_ignore_ascii_case("show") {
        let p = engine.budget_policy();
        return ReplOutcome::ok(format!(
            "Active ModBudgetPolicy:\n  \
             max_p99_system_micros: {:?}\n  \
             max_spawns_per_frame:  {:?}\n  \
             max_arena_bytes:       {:?}\n  \
             window_size:           {}\n  \
             max_consecutive:       {}",
            p.max_p99_system_micros,
            p.max_spawns_per_frame,
            p.max_arena_bytes_per_system,
            p.window_size,
            p.max_consecutive_breaches
        ));
    }

    let mut policy = engine.budget_policy();
    match args[0].to_ascii_lowercase().as_str() {
        "off" | "unlimited" | "clear" => {
            engine.set_budget_policy(ModBudgetPolicy::unlimited());
            ReplOutcome::ok("Reset ModBudgetPolicy to unlimited.")
        }
        "p99" => {
            let Some(us) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
                return ReplOutcome::ok("Usage: budget p99 <microseconds>");
            };
            policy.max_p99_system_micros = Some(us);
            policy.window_size = 16;
            policy.max_consecutive_breaches = 3;
            engine.set_budget_policy(policy);
            ReplOutcome::ok(format!(
                "Set p99 frame-time watchdog = {us} us (auto-quarantine after 3 consecutive breaches)."
            ))
        }
        "spawns" => {
            let Some(n) = args.get(1).and_then(|s| s.parse::<usize>().ok()) else {
                return ReplOutcome::ok("Usage: budget spawns <max_spawns_per_frame>");
            };
            policy.max_spawns_per_frame = Some(n);
            engine.set_budget_policy(policy);
            ReplOutcome::ok(format!("Set max_spawns_per_frame = {n}."))
        }
        "arena" => {
            let Some(bytes) = args.get(1).and_then(|s| s.parse::<usize>().ok()) else {
                return ReplOutcome::ok("Usage: budget arena <max_bytes>");
            };
            policy.max_arena_bytes_per_system = Some(bytes);
            engine.set_budget_policy(policy);
            ReplOutcome::ok(format!("Set max_arena_bytes_per_system = {bytes} B."))
        }
        _ => ReplOutcome::ok("Usage: budget [show | p99 <us> | spawns <n> | arena <bytes> | off]"),
    }
}

fn cmd_reenable(engine: &mut StudioEngine, args: &[String]) -> ReplOutcome {
    if args.len() == 1 {
        // Allow `reenable PanicFaultDemo` shorthand!
        if let Ok(sys) = engine.resolve_discovered_system(&args[0]) {
            return match engine.reenable_system(&sys.mod_name, &sys.system_name) {
                Ok(msg) => ReplOutcome::ok(msg),
                Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
            };
        }
    }
    if args.len() < 2 {
        return ReplOutcome::ok(
            "Usage: reenable <mod_name> <system_name>  (or `reenable <system_name>`)",
        );
    }
    match engine.reenable_system(&args[0], &args[1]) {
        Ok(msg) => ReplOutcome::ok(msg),
        Err(err) => ReplOutcome::ok(format!("ERROR: {err}")),
    }
}

/// Formats the catalog of all 11 bundled shared libraries with exact on-disk paths.
pub fn format_catalog() -> String {
    let mut out = String::from(
        "=== Bundled Multi-Language Studio Mods (copy ANY path into `load <path>`) ===\n",
    );
    for spec in BUILTIN_MODS {
        let path = builtin_mod_binary_path(spec.binary_stem);
        let status = if path.is_file() {
            "BUILT"
        } else {
            "needs build-mods"
        };
        out.push_str(&format!(
            "  @{:<11} [{:<17}] ({status})\n    Path:    {}\n    Systems: {}\n    About:   {}\n",
            spec.short_name,
            spec.language,
            path.display(),
            spec.systems,
            spec.summary
        ));
    }
    out
}

fn format_loaded_mods(engine: &StudioEngine) -> String {
    let mods = engine.loaded_mod_summaries();
    if mods.is_empty() {
        return "No mods loaded. Use `load <path>` or `preset showcase`.".to_string();
    }
    let mut out = format!("=== Loaded Mods ({}) ===\n", mods.len());
    for m in mods {
        out.push_str(&format!(
            "  {:?} | {:<16} v{} (gen={}, sys={}, faults={}) — {}\n",
            m.id,
            m.name,
            m.version,
            m.generation,
            m.system_count,
            m.fault_count,
            m.source_path.display()
        ));
    }
    out
}

fn format_systems(engine: &StudioEngine) -> String {
    if engine.discovered.is_empty() {
        return "No guest systems discovered yet. Load mods with `load <path>` or `load-all`."
            .to_string();
    }
    let gate = engine.app.world().resource::<ChainGate>().clone();
    let snaps = engine
        .app
        .world()
        .resource::<ModBudgetTelemetry>()
        .system_budget_snapshots();

    let mut out = format!(
        "=== Discovered Guest Systems ({}, solo_chain={}) ===\n",
        engine.discovered.len(),
        gate.solo_chain
    );
    out.push_str(&format!(
        "  {:<35} {:<10} {:<10} {:>7} {:>7} {:>8}\n",
        "Mod::System", "ChainPos", "State", "p50 us", "p99 us", "entities"
    ));
    for d in &engine.discovered {
        let pos = engine
            .chain
            .iter()
            .position(|c| c.key() == d.key())
            .map(|i| format!("#{i}"))
            .unwrap_or_else(|| "-".to_string());
        let quarantined = engine.is_quarantined(d.mod_id, &d.system_name);
        let state = if quarantined {
            "QUARANTINE"
        } else if gate.disabled.contains(&d.key()) {
            "DISABLED"
        } else if gate.is_enabled(&d.key()) {
            "ACTIVE"
        } else {
            "STANDBY"
        };
        let (p50, p99, ents) = snaps
            .iter()
            .find(|s| s.mod_name == d.mod_name && s.system_name == d.system_name)
            .map(|s| (s.p50_us, s.p99_us, s.matched_entities))
            .unwrap_or((0, 0, 0));
        out.push_str(&format!(
            "  {:<35} {:<10} {:<10} {:>7} {:>7} {:>8}\n",
            d.qualified(),
            pos,
            state,
            p50,
            p99,
            ents
        ));
    }
    out
}

/// Formats live ECS-only mod performance metrics (explicitly excluding GPU/rendering time).
pub fn format_perf_table(engine: &mut StudioEngine) -> String {
    let s = engine.perf_summary();
    let mut out = format!(
        "=== ECS Mod Performance Telemetry (STRICTLY NO RENDERING) ===\n\
         Tick: {} | Actors: {} (+{} sparks) | Mode: {:?} | Chain: {}\n\
         ECS app.update(): last={:.2} us | mean={:.2} us | p50={:.2} us | p99={:.2} us | min={:.2} us | max={:.2} us\n\
         Mod Callback Sum: last={} us | p50={} us | p99={} us | Throughput: {:.2} M entity-ops/sec\n\
         Schedule Rewire:  {:.1} us | Last Mod Load/Reload: {:.1} us\n",
        s.tick,
        s.actor_count,
        s.spark_count,
        engine.chain_mode,
        engine.chain_display(),
        s.ecs_tick_last_us,
        s.ecs_tick_mean_us,
        s.ecs_tick_p50_us,
        s.ecs_tick_p99_us,
        s.ecs_tick_min_us,
        s.ecs_tick_max_us,
        s.mod_sum_last_us,
        s.mod_sum_p50_us,
        s.mod_sum_p99_us,
        s.aggregate_m_ents_per_sec,
        s.last_rewire_us,
        s.last_load_us,
    );
    if s.systems.is_empty() {
        out.push_str("  (no mod system executions recorded yet)\n");
    } else {
        out.push_str(&format!(
            "  {:<35} {:>6} {:>6} {:>6} {:>7} {:>5} {:>5} {:>7} {:>10}\n",
            "Mod::System", "p50", "p99", "last", "ents", "tbls", "cmds", "arenaB", "status"
        ));
        for sys in &s.systems {
            let st = if sys.quarantined {
                "QUARANTINE"
            } else if sys.breach_count > 0 {
                "BREACH"
            } else {
                "OK"
            };
            out.push_str(&format!(
                "  {:<35} {:>5}u {:>5}u {:>5}u {:>7} {:>5} {:>5} {:>7} {:>10}\n",
                format!("{}::{}", sys.mod_name, sys.system_name),
                sys.p50_us,
                sys.p99_us,
                sys.last_duration_us,
                sys.matched_entities,
                sys.matched_tables,
                sys.commands_queued,
                sys.arena_bytes_peak,
                st
            ));
        }
    }
    out
}

fn format_events(engine: &StudioEngine) -> String {
    if engine.event_log.is_empty() {
        return "No lifecycle/fault/budget events recorded yet.".to_string();
    }
    let mut out = String::from("=== Recent Mod Lifecycle, Fault & Budget Events ===\n");
    for line in &engine.event_log {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub fn help_text() -> String {
    "\
=== bamabi Runtime Chain Studio — REPL Commands ===
  MOD LOADING & LIFECYCLE (enter any .dll / .dylib / .so path!)
    catalog                         List all 11 bundled Rust/C99/C++17 mods & exact file paths
    build-mods                      Compile all 11 bundled mods (Rust + ISO C99 + ISO C++17)
    load <path | @short> [...]      Dynamically load shared lib(s) via shadow_copy
    load-all                        Load all 10 non-chaos studio mods at once
    reload <mod | id>               Hot-reload mod in place (save_state -> restore_state!)
    unload <mod | id | all>         Unload mod cleanly (excises systems + ModScoped entities)
    mods                            List loaded mods, generations, system counts, faults
    systems                         List every discovered system across all loaded mods

  RUNTIME SYSTEM CHAINING (`ModScheduleSetRewirePass` sub-ms hot-rewire)
    chain                           Show active execution chain, mode & rewire latency
    chain <S1> -> <S2> -> <S3> ...  Set exact runtime execution order across loaded mods!
    chain add <sys> [...]           Append system(s) to the chain
    chain insert <idx> <sys>        Insert system at 0-based position
    chain remove <idx | sys>        Remove system from chain
    chain swap <i> <j>              Swap two chain stages live
    chain reverse                   Reverse entire chain order live!
    chain clear                     Clear chain ordering constraints
    chain mode <sequence|weak|parallel>  Compare Bevy .chain() vs .chain_weak() vs .parallel()
    solo <on | off>                 Run only chained systems (on) or all enabled systems (off)
    enable <sys> / disable <sys>    Toggle individual system via Bevy run_if gate

  CURATED PRESETS & SCENE RESET / RUN
    preset [showcase|tornado|polyglot|shockwave|aura|chaos|stress]
    reset [grid|ring|helix|sphere]  Deterministic scene reset (keeps loaded mods & chain!)
    reset all                       Unload all mods, clear chain, reset budgets & scene
    pause / resume                  Pause or resume live 3D simulation
    step [N]                        Advance exactly N ECS ticks
    run <N>                         Run N deterministic ECS ticks + print perf summary
    count <N>                       Change 3D entity count (e.g. 256, 2000, 10000)
    formation <grid|ring|helix|sphere>
    speed <f32> / amp <f32>         Live-tune SimClock.speed_scale & amplitude resource

  ECS MOD PERFORMANCE, BUDGETS & FAULT RECOVERY (NO RENDERING!)
    perf [reset | watch on|off]     Inspect live per-system ECS mod latency & throughput
    bench [ticks] [entities]        Run pure-ECS burst benchmark (ZERO rendering!)
    bench compare [ticks]           Benchmark sequence vs weak vs parallel side-by-side
    budget [show|p99 <us>|spawns <n>|arena <bytes>|off]
    reenable <mod> <system>         Recover a quarantined system via ModQuarantine::reenable
    events                          View recent ModLoaded/ModUnloaded/Fault/BudgetBreach events
    quit                            Exit studio"
        .to_string()
}
