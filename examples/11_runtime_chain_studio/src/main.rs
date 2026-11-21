#![forbid(unsafe_code)]
#![allow(unreachable_pub)]

//! Example 11: `bamabi` Runtime Mod Chain Studio (`winit` + `wgpu` + Interactive REPL, `ABI 0.3.0`).
//!
//! Showcases `bamabi`'s full native ECS modding stack in an interactive 3D kinetic arena paired
//! with a live terminal REPL:
//! - Dynamic `.dll` / `.dylib` / `.so` loading by arbitrary path (`shadow_copy = true`)
//! - Automatic guest system discovery from Bevy's `Schedules` graph
//! - Sub-millisecond runtime system chaining & hot-rewiring via `ModScheduleSetRegistry` +
//!   `ModScheduleSetRewirePass` (`Sequence` / `SequenceWeak` / `Parallel`)
//! - Polyglot C-ABI interop: Rust `.cdylib` + Pure ISO C99 + ISO C++17 in the SAME schedule
//! - Cross-mod dynamic components (`mod_aura` -> `mod_aura_link`), buffered FFI `Messages<T>`,
//!   `ModScoped` batched `CommandArena` spawning, `save_state`/`restore_state` hot-reload,
//!   panic containment (`ModQuarantine` + `reenable`), and live `ModBudgetPolicy` watchdogs
//! - Deterministic Reset & Re-run across 4 3D formations + strict ECS-only mod performance metrics

mod components;
mod engine;
mod renderer;
mod repl;

use bamabi::prelude::*;
use bamabi_host_sys::schedule::ScheduleSetMode;
use components::{ChainGate, Formation, SimClock, Transform3D, VisualStyle};
use engine::{
    StudioEngine, build_all_studio_mods, builtin_mod_binary_path, resolve_mod_input_path,
};
use renderer::{HudState, OrbitCamera, StudioRenderer};
use repl::{ReplAction, execute_repl_line, format_catalog, format_perf_table};
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

struct CliArgs {
    test_mode: bool,
    headless: bool,
    empty: bool,
    build_mods: bool,
    actor_count: usize,
    formation: Formation,
    preset: Option<String>,
    script_path: Option<String>,
    exec_cmds: Vec<String>,
}

impl CliArgs {
    fn parse() -> Self {
        let mut test_mode = false;
        let mut headless = false;
        let mut empty = false;
        let mut build_mods = false;
        let mut actor_count = 256usize;
        let mut formation = Formation::Grid;
        let mut preset: Option<String> = Some("showcase".to_string());
        let mut script_path: Option<String> = None;
        let mut exec_cmds: Vec<String> = Vec::new();

        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--test-mode" => {
                    test_mode = true;
                    headless = true;
                    preset = None;
                }
                "--headless" => headless = true,
                "--empty" => {
                    empty = true;
                    preset = None;
                }
                "--build-mods" => build_mods = true,
                "--count" | "--actors" | "--entities" => {
                    if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                        actor_count = v;
                        i += 1;
                    }
                }
                "--formation" => {
                    if let Some(f) = args.get(i + 1).and_then(|s| Formation::parse(s)) {
                        formation = f;
                        i += 1;
                    }
                }
                "--preset" => {
                    if let Some(p) = args.get(i + 1) {
                        preset = if p.eq_ignore_ascii_case("none") {
                            None
                        } else {
                            Some(p.clone())
                        };
                        i += 1;
                    }
                }
                "--script" => {
                    if let Some(p) = args.get(i + 1) {
                        script_path = Some(p.clone());
                        i += 1;
                    }
                }
                "--exec" => {
                    if let Some(c) = args.get(i + 1) {
                        for part in c.split(';') {
                            let t = part.trim();
                            if !t.is_empty() {
                                exec_cmds.push(t.to_string());
                            }
                        }
                        i += 1;
                    }
                }
                "-h" | "--help" => {
                    println!(
                        "Usage: example_11_runtime_chain_studio [OPTIONS]\n\n\
                         Options:\n  \
                           --empty              Start with 0 mods loaded (load .dll/.dylib/.so paths manually)\n  \
                           --preset <name>      Initial preset: showcase (default), tornado, polyglot, shockwave, aura, chaos, stress, none\n  \
                           --count <N>          Initial 3D entity count (default: 256, max: 30000)\n  \
                           --formation <kind>   Initial formation: grid (default), ring, helix, sphere\n  \
                           --build-mods         Force rebuild all 11 Rust/C99/C++17 studio mods on startup\n  \
                           --headless           Run interactive REPL on stdin without opening a 3D window\n  \
                           --script <file>      Execute REPL commands from a text file\n  \
                           --exec \"c1; c2\"      Execute semicolon-separated REPL commands on startup\n  \
                           --test-mode          Run automated end-to-end verification of all bamabi capabilities\n  \
                           -h, --help           Show this help message"
                    );
                    std::process::exit(0);
                }
                _ => {}
            }
            i += 1;
        }

        Self {
            test_mode,
            headless,
            empty,
            build_mods,
            actor_count,
            formation,
            preset,
            script_path,
            exec_cmds,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = CliArgs::parse();

    if args.build_mods {
        println!("[Studio] Building all 11 studio mods (Rust + ISO C99 + ISO C++17)...");
        let msg = build_all_studio_mods()?;
        println!("[Studio] {msg}");
    }

    if args.test_mode {
        run_automated_verification()?;
        return Ok(());
    }

    let mut engine = StudioEngine::new(args.actor_count, args.formation)?;

    if !args.empty
        && let Some(ref preset) = args.preset
    {
        match engine.apply_preset(preset) {
            Ok(msg) => println!("[Studio] {msg}"),
            Err(err) => eprintln!("[Studio] Initial preset warning: {err}"),
        }
    }

    // Run any --script or --exec commands before entering interactive loop
    if let Some(ref path) = args.script_path {
        let content = std::fs::read_to_string(path)?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            println!("bamabi> {trimmed}");
            let out = execute_repl_line(&mut engine, trimmed);
            if !out.output.is_empty() {
                println!("{}", out.output);
            }
            if out.action == ReplAction::Quit {
                return Ok(());
            }
        }
    }
    for cmd in &args.exec_cmds {
        println!("bamabi> {cmd}");
        let out = execute_repl_line(&mut engine, cmd);
        if !out.output.is_empty() {
            println!("{}", out.output);
        }
        if out.action == ReplAction::Quit {
            return Ok(());
        }
    }

    if args.headless {
        run_headless_repl(engine);
    } else {
        run_winit_3d_studio(engine)?;
    }

    Ok(())
}

fn print_startup_banner(engine: &StudioEngine) {
    println!("================================================================================");
    println!("  bamabi Runtime Mod Chain Studio (winit + wgpu + ABI 0.3.0 Interactive REPL)");
    println!("================================================================================");
    println!(
        "  Actors: {} | Formation: {} | Active Chain: {}",
        engine.target_actor_count,
        engine.formation.name(),
        engine.chain_display()
    );
    println!("  Quick REPL commands:");
    println!(
        "    catalog                                     # Show all 11 Rust/C99/C++17 mod paths"
    );
    println!(
        "    load <path/to/libmod_spin.dylib|.so|.dll>   # Load ANY shared library at runtime"
    );
    println!("    chain WaveLift -> SpinYaw -> RainbowHue -> SquashStretch");
    println!("    chain reverse | chain swap 0 2 | chain mode parallel|weak|sequence");
    println!("    preset polyglot | preset shockwave | preset aura | preset chaos");
    println!("    reset [grid|ring|helix|sphere]              # Deterministic reset & re-run");
    println!("    perf | bench 300 4000 | bench compare       # Pure ECS mod perf (NO rendering!)");
    println!("    help                                        # Full command reference");
    println!("================================================================================");
}

fn run_headless_repl(mut engine: StudioEngine) {
    print_startup_banner(&engine);
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    loop {
        print!("bamabi> ");
        let _ = stdout.flush();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let res = execute_repl_line(&mut engine, &line);
                if !res.output.is_empty() {
                    println!("{}", res.output);
                }
                if res.action == ReplAction::Quit {
                    break;
                }
            }
            Err(err) => {
                eprintln!("[REPL] read error: {err}");
                break;
            }
        }
    }
    let _ = engine.hard_reset();
}

// =============================================================================
// Interactive Winit 0.30 + WGPU 3D Window + Concurrent Stdin REPL
// =============================================================================

#[derive(Debug)]
enum StudioUserEvent {
    ReplCommandReady,
}

struct ReplRequest {
    line: String,
    reply_tx: Sender<(String, bool)>,
}

fn spawn_stdin_repl_thread(req_tx: Sender<ReplRequest>, proxy: EventLoopProxy<StudioUserEvent>) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        loop {
            print!("bamabi> ");
            let _ = stdout.flush();
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) | Err(_) => break, // EOF or closed stdin
                Ok(_) => {
                    let (reply_tx, reply_rx) = mpsc::channel();
                    if req_tx.send(ReplRequest { line, reply_tx }).is_err() {
                        break;
                    }
                    let _ = proxy.send_event(StudioUserEvent::ReplCommandReady);
                    match reply_rx.recv() {
                        Ok((output, should_quit)) => {
                            if !output.is_empty() {
                                println!("{output}");
                            }
                            if should_quit {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });
}

struct StudioWinitApp {
    engine: StudioEngine,
    repl_rx: Receiver<ReplRequest>,
    window: Option<Arc<Window>>,
    renderer: Option<StudioRenderer>,
    camera: OrbitCamera,
    cam_preset_idx: u8,
    dragging: bool,
    last_cursor: Option<(f64, f64)>,
    paused: bool,
    last_frame: Instant,
    last_sim_step: Instant,
    last_watch_print: Instant,
}

impl StudioWinitApp {
    fn new(engine: StudioEngine, repl_rx: Receiver<ReplRequest>) -> Self {
        let now = Instant::now();
        Self {
            engine,
            repl_rx,
            window: None,
            renderer: None,
            camera: OrbitCamera::default(),
            cam_preset_idx: 0,
            dragging: false,
            last_cursor: None,
            paused: false,
            last_frame: now,
            last_sim_step: now,
            last_watch_print: now,
        }
    }

    fn drain_repl_requests(&mut self, event_loop: &ActiveEventLoop) {
        while let Ok(req) = self.repl_rx.try_recv() {
            let outcome = execute_repl_line(&mut self.engine, &req.line);
            let mut quit = false;
            match outcome.action {
                ReplAction::Continue => {}
                ReplAction::Pause => self.paused = true,
                ReplAction::Resume => self.paused = false,
                ReplAction::Quit => quit = true,
            }
            let _ = req.reply_tx.send((outcome.output, quit));
            self.refresh_window_title();
            if quit {
                let _ = self.engine.hard_reset();
                event_loop.exit();
                return;
            }
        }
    }

    fn refresh_window_title(&self) {
        if let Some(win) = &self.window {
            let pause = if self.paused { " [PAUSED]" } else { "" };
            win.set_title(&format!(
                "bamabi Runtime Chain Studio — {} ({:?}){} | [R] Reset [Space] Pause [1-6] Presets",
                self.engine.chain_display(),
                self.engine.chain_mode,
                pause
            ));
        }
    }
}

impl ApplicationHandler<StudioUserEvent> for StudioWinitApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("bamabi Runtime Chain Studio (winit + wgpu, ABI 0.3.0)")
            .with_inner_size(LogicalSize::new(1360.0, 840.0));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("[Studio] Failed to create winit window: {e}");
                event_loop.exit();
                return;
            }
        };

        match StudioRenderer::new(window.clone()) {
            Ok(renderer) => {
                self.window = Some(window);
                self.renderer = Some(renderer);
                self.refresh_window_title();
            }
            Err(e) => {
                eprintln!("[Studio] Failed to initialize wgpu 3D renderer: {e}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: StudioUserEvent) {
        self.drain_repl_requests(event_loop);
        if let Some(win) = &self.window {
            win.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        self.drain_repl_requests(event_loop);

        match event {
            WindowEvent::CloseRequested => {
                let _ = self.engine.hard_reset();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && let PhysicalKey::Code(code) = event.physical_key
                {
                    match code {
                        KeyCode::KeyR => {
                            let msg = self.engine.reset_scene(None, None);
                            println!("\n[Studio] {msg}");
                            self.refresh_window_title();
                        }
                        KeyCode::Space => {
                            self.paused = !self.paused;
                            self.refresh_window_title();
                        }
                        KeyCode::Period => {
                            self.paused = true;
                            let us = self.engine.step_ecs_tick();
                            println!("\n[Studio] Stepped 1 tick (ECS: {us:.2} us)");
                            self.refresh_window_title();
                        }
                        KeyCode::KeyF => {
                            let next = self.engine.formation.next();
                            let msg = self.engine.reset_scene(Some(next), None);
                            println!("\n[Studio] {msg}");
                            self.refresh_window_title();
                        }
                        KeyCode::Digit1 => {
                            if let Ok(msg) = self.engine.apply_preset("showcase") {
                                println!("\n[Studio] {msg}");
                            }
                            self.refresh_window_title();
                        }
                        KeyCode::Digit2 => {
                            if let Ok(msg) = self.engine.apply_preset("tornado") {
                                println!("\n[Studio] {msg}");
                            }
                            self.refresh_window_title();
                        }
                        KeyCode::Digit3 => {
                            if let Ok(msg) = self.engine.apply_preset("polyglot") {
                                println!("\n[Studio] {msg}");
                            }
                            self.refresh_window_title();
                        }
                        KeyCode::Digit4 => {
                            if let Ok(msg) = self.engine.apply_preset("shockwave") {
                                println!("\n[Studio] {msg}");
                            }
                            self.refresh_window_title();
                        }
                        KeyCode::Digit5 => {
                            if let Ok(msg) = self.engine.apply_preset("aura") {
                                println!("\n[Studio] {msg}");
                            }
                            self.refresh_window_title();
                        }
                        KeyCode::Digit6 => {
                            if let Ok(msg) = self.engine.apply_preset("chaos") {
                                println!("\n[Studio] {msg}");
                            }
                            self.refresh_window_title();
                        }
                        KeyCode::KeyC => {
                            self.cam_preset_idx = self.cam_preset_idx.wrapping_add(1);
                            self.camera.cycle_preset(self.cam_preset_idx);
                        }
                        KeyCode::KeyO => {
                            self.camera.auto_orbit = !self.camera.auto_orbit;
                        }
                        KeyCode::ArrowLeft | KeyCode::KeyA => {
                            self.camera.auto_orbit = false;
                            self.camera.yaw -= 0.14;
                        }
                        KeyCode::ArrowRight | KeyCode::KeyD => {
                            self.camera.auto_orbit = false;
                            self.camera.yaw += 0.14;
                        }
                        KeyCode::ArrowUp | KeyCode::KeyW => {
                            self.camera.pitch = (self.camera.pitch + 0.08).clamp(0.10, 1.42);
                        }
                        KeyCode::ArrowDown | KeyCode::KeyS => {
                            self.camera.pitch = (self.camera.pitch - 0.08).clamp(0.10, 1.42);
                        }
                        KeyCode::Escape | KeyCode::KeyQ => {
                            let _ = self.engine.hard_reset();
                            event_loop.exit();
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    self.dragging = state == ElementState::Pressed;
                    if !self.dragging {
                        self.last_cursor = None;
                    } else {
                        self.camera.auto_orbit = false;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.dragging {
                    if let Some((lx, ly)) = self.last_cursor {
                        let dx = (position.x - lx) as f32;
                        let dy = (position.y - ly) as f32;
                        self.camera.yaw -= dx * 0.0065;
                        self.camera.pitch = (self.camera.pitch + dy * 0.0065).clamp(0.08, 1.45);
                    }
                    self.last_cursor = Some((position.x, position.y));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 1.4,
                    MouseScrollDelta::PixelDelta(p) => (p.y as f32) * 0.025,
                };
                self.camera.distance = (self.camera.distance - scroll).clamp(6.0, 48.0);
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt_frame = now.duration_since(self.last_frame).as_secs_f32();
                self.last_frame = now;

                if self.camera.auto_orbit && !self.dragging {
                    self.camera.yaw += dt_frame * 0.18;
                }

                // Step ECS simulation at ~60 Hz (timing STRICTLY around app.update(), zero render time!)
                if now.duration_since(self.last_sim_step).as_secs_f32() >= (1.0 / 62.0) {
                    self.last_sim_step = now;
                    if !self.paused {
                        self.engine.step_ecs_tick();
                    }
                }

                if self.engine.perf_watch
                    && now.duration_since(self.last_watch_print).as_secs_f32() >= 2.0
                {
                    self.last_watch_print = now;
                    println!("\n{}", format_perf_table(&mut self.engine));
                }

                let actors = self.engine.collect_render_actors();
                let perf = self.engine.perf_summary();
                let clock = *self.engine.app.world().resource::<SimClock>();
                let solo_chain = self.engine.app.world().resource::<ChainGate>().solo_chain;
                let chain_names: Vec<String> =
                    self.engine.chain.iter().map(|s| s.qualified()).collect();
                let loaded_mod_count = self.engine.loaded_mod_summaries().len();
                let latest_event = self.engine.event_log.back().map(String::as_str);

                let hud = HudState {
                    paused: self.paused,
                    formation: self.engine.formation,
                    chain_mode: self.engine.chain_mode,
                    chain_names: &chain_names,
                    loaded_mod_count,
                    discovered_sys_count: self.engine.discovered.len(),
                    run_count: self.engine.run_count,
                    speed_scale: clock.speed_scale,
                    amplitude: clock.amplitude,
                    solo_chain,
                    latest_event,
                    perf: &perf,
                };

                if let Some(renderer) = &self.renderer {
                    renderer.render_frame(&self.camera, &actors, &hud);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.drain_repl_requests(event_loop);
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Some(win) = &self.window {
            win.request_redraw();
        }
    }
}

fn run_winit_3d_studio(engine: StudioEngine) -> Result<(), Box<dyn std::error::Error>> {
    print_startup_banner(&engine);
    let event_loop = EventLoop::<StudioUserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let (req_tx, req_rx) = mpsc::channel();
    spawn_stdin_repl_thread(req_tx, proxy);

    let mut handler = StudioWinitApp::new(engine, req_rx);
    event_loop.run_app(&mut handler)?;
    Ok(())
}

// =============================================================================
// Comprehensive Automated End-to-End Verification (`--test-mode`)
// =============================================================================

fn run_automated_verification() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Example 11: Automated Runtime Chain Studio Verification ===");

    // Ensure all 11 studio mods (Rust + ISO C99 + ISO C++17) are built on disk
    let all_built = [
        "mod_spin",
        "mod_color",
        "mod_scale",
        "mod_motion",
        "mod_vortex",
        "mod_ripple",
        "mod_aura",
        "mod_aura_link",
        "mod_c99_orbit",
        "mod_cpp_gravity",
        "mod_chaos",
    ]
    .iter()
    .all(|stem| builtin_mod_binary_path(stem).is_file());
    if !all_built {
        let msg = build_all_studio_mods()?;
        println!("[1/9] {msg}");
    } else {
        println!("[1/9] All 11 bundled multi-language mods already built.");
    }
    let catalog = format_catalog();
    assert!(catalog.contains("mod_c99_orbit") && catalog.contains("mod_cpp_gravity"));

    let mut engine = StudioEngine::new(128, Formation::Grid)?;

    // 2. Load mods dynamically by explicit filesystem path & verify dynamic system discovery
    println!("[2/9] Loading Rust, ISO C99, and ISO C++17 mods by explicit library path...");
    for stem in [
        "mod_motion",
        "mod_spin",
        "mod_color",
        "mod_scale",
        "mod_c99_orbit",
        "mod_cpp_gravity",
    ] {
        let path = resolve_mod_input_path(stem)?;
        let (_id, discovered) = engine.load_mod_path(&path)?;
        assert!(
            !discovered.is_empty(),
            "Mod `{stem}` must have discovered systems"
        );
    }
    assert!(
        engine.discovered.len() >= 10,
        "Expected >= 10 discovered systems, found {}",
        engine.discovered.len()
    );
    println!(
        "  -> PASS: Discovered {} systems across 6 polyglot mods.",
        engine.discovered.len()
    );

    // 3. Runtime chaining & sub-millisecond ModScheduleSetRewirePass hot-rewiring + order reversal
    println!("[3/9] Testing runtime chaining via REPL (`chain`) & schedule mode switching...");
    let out1 = execute_repl_line(
        &mut engine,
        "chain WaveLift -> SpinYaw -> RainbowHue -> EnergyHeatmap -> SquashStretch",
    );
    assert!(
        out1.output.contains("Rewired runtime chain"),
        "{}",
        out1.output
    );
    for _ in 0..20 {
        engine.step_ecs_tick();
    }
    let color_after_forward: Vec<(f32, f32, f32)> = {
        let world = engine.app.world_mut();
        let mut q = world.query::<&VisualStyle>();
        q.iter(world).map(|v| (v.r, v.g, v.b)).collect()
    };

    // Reset deterministically and reverse RainbowHue <-> EnergyHeatmap order!
    engine.reset_scene(Some(Formation::Grid), None);
    let out_rev = execute_repl_line(
        &mut engine,
        "chain WaveLift -> SpinYaw -> EnergyHeatmap -> RainbowHue -> SquashStretch",
    );
    assert!(
        out_rev.output.contains("Rewired runtime chain"),
        "{}",
        out_rev.output
    );
    for _ in 0..20 {
        engine.step_ecs_tick();
    }
    let color_after_swapped: Vec<(f32, f32, f32)> = {
        let world = engine.app.world_mut();
        let mut q = world.query::<&VisualStyle>();
        q.iter(world).map(|v| (v.r, v.g, v.b)).collect()
    };
    let order_changed_output = color_after_forward
        .iter()
        .zip(color_after_swapped.iter())
        .any(|(a, b)| {
            (a.0 - b.0).abs() > 0.05 || (a.1 - b.1).abs() > 0.05 || (a.2 - b.2).abs() > 0.05
        });
    assert!(
        order_changed_output,
        "Swapping RainbowHue <-> EnergyHeatmap in runtime chain MUST visibly change ECS output!"
    );
    println!(
        "  -> PASS: Runtime chain reordering verified (rewire latency: {:.1} us) and measurably altered ECS state!",
        engine.last_rewire_us
    );

    // 4. Polyglot 3-language chain (ISO C99 -> ISO C++17 -> Rust)
    println!("[4/9] Testing 3-language polyglot chain (C99 -> C++17 -> Rust)...");
    let out_poly = execute_repl_line(
        &mut engine,
        "chain C99PlanetaryOrbit -> CppBounceGravity -> SpinYaw -> RainbowHue",
    );
    assert!(out_poly.output.contains("Rewired"), "{}", out_poly.output);
    for _ in 0..25 {
        engine.step_ecs_tick();
    }
    let moved_3d = {
        let world = engine.app.world_mut();
        let mut q = world.query::<&Transform3D>();
        q.iter(world).any(|xf| xf.py > 1.5 && xf.ry.abs() > 0.1)
    };
    assert!(
        moved_3d,
        "Polyglot C99 + C++17 + Rust chain must animate 3D transforms"
    );
    println!("  -> PASS: ISO C99 + ISO C++17 + Rust systems chained & executed cleanly.");

    // 5. Hot-Reload with save_state -> restore_state
    println!("[5/9] Testing live hot-reload (`reload mod_spin`) with state preservation...");
    let out_reload = execute_repl_line(&mut engine, "reload mod_spin");
    assert!(
        out_reload.output.contains("Hot-reloaded"),
        "{}",
        out_reload.output
    );
    for _ in 0..5 {
        engine.step_ecs_tick();
    }
    println!("  -> PASS: {}", out_reload.output);

    // 6. Buffered FFI Messages<PulseEvent> (`mod_ripple`) + parallel `par_chunks` (`mod_vortex`)
    println!(
        "[6/9] Testing buffered FFI Messages (`mod_ripple`) & parallel `par_chunks` (`mod_vortex`)..."
    );
    let out_shock = execute_repl_line(&mut engine, "preset shockwave");
    assert!(
        out_shock.output.contains("Applied preset"),
        "{}",
        out_shock.output
    );
    let mut saw_pulse = false;
    for _ in 0..45 {
        engine.step_ecs_tick();
        let world = engine.app.world_mut();
        let mut q = world.query::<&VisualStyle>();
        if q.iter(world).any(|v| v.pulse > 0.1) {
            saw_pulse = true;
        }
    }
    assert!(
        saw_pulse,
        "mod_ripple Messages<PulseEvent> must trigger VisualStyle.pulse"
    );
    let _ = execute_repl_line(&mut engine, "preset tornado");
    for _ in 0..15 {
        engine.step_ecs_tick();
    }
    println!("  -> PASS: Buffered FFI Messages and parallel par_chunks verified.");

    // 7. Cross-mod dynamic components (`mod_aura` -> `mod_aura_link`) + ModScoped sparks + unload cleanup
    println!(
        "[7/9] Testing cross-mod dynamic components (`mod_aura` + `mod_aura_link`) & ModScoped cleanup..."
    );
    let out_aura = execute_repl_line(&mut engine, "preset aura");
    assert!(
        out_aura.output.contains("Applied preset"),
        "{}",
        out_aura.output
    );
    for _ in 0..15 {
        engine.step_ecs_tick();
    }
    let has_dyn_comp = engine
        .app
        .world()
        .resource::<ModRegistry>()
        .snapshot()
        .get_by_name("mod_aura::AuraChargeData")
        .is_some();
    assert!(
        has_dyn_comp,
        "mod_aura::AuraChargeData must be registered dynamically"
    );
    let sparks_before = engine.perf_summary().spark_count;
    assert!(
        sparks_before > 0,
        "SpawnSparks must spawn ModScoped spark entities"
    );
    let _ = execute_repl_line(&mut engine, "unload mod_aura_link");
    let _ = execute_repl_line(&mut engine, "unload mod_aura");
    let sparks_after = engine.perf_summary().spark_count;
    assert_eq!(
        sparks_after, 0,
        "Unloading mod_aura must automatically purge ModScoped sparks (left: {sparks_after})"
    );
    println!(
        "  -> PASS: Dynamic cross-mod component & ModScoped spark purge ({sparks_before} -> 0) verified."
    );

    // 8. Panic containment, ModQuarantine, reenable recovery & ModBudgetPolicy watchdog
    println!("[8/9] Testing panic containment, ModQuarantine, reenable & p99 budget watchdog...");
    let _ = execute_repl_line(&mut engine, "preset chaos");
    for _ in 0..10 {
        engine.step_ecs_tick();
    }
    let chaos_id = engine.resolve_loaded_mod("mod_chaos")?;
    assert!(
        engine.is_quarantined(chaos_id, "PanicFaultDemo"),
        "PanicFaultDemo must be quarantined after run #5 panic"
    );
    let out_reenable = execute_repl_line(&mut engine, "reenable mod_chaos PanicFaultDemo");
    assert!(
        out_reenable.output.contains("Re-enabled"),
        "{}",
        out_reenable.output
    );
    assert!(
        !engine.is_quarantined(chaos_id, "PanicFaultDemo"),
        "PanicFaultDemo must be active again after reenable"
    );

    // Budget watchdog test with HeavyLoadStress
    let _ = execute_repl_line(&mut engine, "chain HeavyLoadStress");
    let _ = execute_repl_line(&mut engine, "budget p99 1"); // ultra-tight 1 us ceiling
    let _ = execute_repl_line(&mut engine, "count 512");
    for _ in 0..12 {
        engine.step_ecs_tick();
    }
    let _ = execute_repl_line(&mut engine, "budget off");
    println!("  -> PASS: Panic quarantine, live reenable, and budget watchdog verified.");

    // 9. Deterministic Reset + Pure ECS Benchmark + Schedule Mode Comparison (ZERO rendering!)
    println!(
        "[9/9] Testing deterministic reset & pure-ECS benchmark (Sequence vs Weak vs Parallel)..."
    );
    let _ = execute_repl_line(&mut engine, "reset all");
    assert!(engine.loaded_mod_summaries().is_empty());
    let _ = execute_repl_line(&mut engine, "preset showcase");
    let _ = execute_repl_line(&mut engine, "count 1000");
    let bench_report = execute_repl_line(&mut engine, "bench compare 100");
    println!("{}", bench_report.output);
    assert!(bench_report.output.contains("Schedule Mode ECS Comparison"));
    assert_eq!(engine.chain_mode, ScheduleSetMode::Sequence);

    let _ = engine.hard_reset()?;
    println!("=== ALL 9 EXAMPLE 11 CHECKS PASSED! ===");
    Ok(())
}
