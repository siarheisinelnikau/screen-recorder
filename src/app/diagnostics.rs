//! Explicit, finite GUI diagnostics. Only the test's own window is minimized/restored.
use super::*;
use crate::{host::Application, render_diagnostics as host};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, atomic::AtomicU64},
    time::{Duration, Instant},
};

#[derive(Default)]
struct State {
    stop: AtomicBool,
    ui: AtomicU64,
    logic: AtomicU64,
    input: AtomicU64,
    motion: AtomicU64,
    focus: AtomicU64,
    isolation_failed: AtomicBool,
    report: Mutex<Value>,
}
#[derive(Clone)]
struct Sample {
    host: host::Snapshot,
    ui: u64,
    logic: u64,
    input: u64,
    motion: u64,
    focus: u64,
    cpu_ns: u64,
    at: Instant,
}
fn cpu_ns() -> Result<u64> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::FILETIME,
            System::Threading::{GetCurrentProcess, GetProcessTimes},
        };
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        let [created, exited, kernel, user] = &mut times;
        ensure!(
            GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) != 0,
            "Cannot read diagnostic process CPU time"
        );
        let n = |v: FILETIME| ((v.dwHighDateTime as u64) << 32) | v.dwLowDateTime as u64;
        Ok((n(*kernel) + n(*user)) * 100)
    }
    #[cfg(not(windows))]
    anyhow::bail!("GUI diagnostic CPU sampling currently supports Windows only")
}
fn sample(s: &State) -> Result<Sample> {
    Ok(Sample {
        host: host::snapshot(),
        ui: s.ui.load(Ordering::Relaxed),
        logic: s.logic.load(Ordering::Relaxed),
        input: s.input.load(Ordering::Relaxed),
        motion: s.motion.load(Ordering::Relaxed),
        focus: s.focus.load(Ordering::Relaxed),
        cpu_ns: cpu_ns()?,
        at: Instant::now(),
    })
}
fn observe_input(s: &State, ctx: &egui::Context) {
    // Count only event kinds: never retain coordinates, key values, text or clipboard data.
    let (total, motion, focus) = ctx.input(|i| {
        (
            i.events.len(),
            i.events
                .iter()
                .filter(|e| matches!(e, egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_)))
                .count(),
            i.events
                .iter()
                .filter(|e| matches!(e, egui::Event::WindowFocused(_)))
                .count(),
        )
    });
    s.input.fetch_add(total as u64, Ordering::Relaxed);
    s.motion.fetch_add(motion as u64, Ordering::Relaxed);
    s.focus.fetch_add(focus as u64, Ordering::Relaxed);
}
fn observe_viewport(s: &State, ctx: &egui::Context) {
    let viewport = ctx.input(|i| {
        let v = i.viewport();
        json!({"minimized":v.minimized,"occluded":v.occluded,"focused":v.focused,"native_pixels_per_point":v.native_pixels_per_point,"inner_rect":v.inner_rect.map(|r| [r.width(),r.height()])})
    });
    s.report.lock().unwrap()["last_viewport"] = viewport;
}
fn delta(a: &Sample, b: &Sample) -> Value {
    let seconds = b.at.duration_since(a.at).as_secs_f64();
    let cpu = (b.cpu_ns - a.cpu_ns) as f64 / 1e9 / seconds * 100.0;
    let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
    json!({"elapsed_seconds":seconds,"cpu_percent_one_core":cpu,"cpu_percent_machine_normalized":cpu/processors as f64,"logical_processors":processors,
        "ui_calls":b.ui-a.ui,"logic_calls":b.logic-a.logic,"input_events":b.input-a.input,"motion_events":b.motion-a.motion,"focus_events":b.focus-a.focus,
        "native_frames":b.host.frame.count-a.host.frame.count,"layouts":b.host.layout.count-a.host.layout.count,"paints":b.host.paint.count-a.host.paint.count,"presents":b.host.present.count-a.host.present.count,
        "paint_us":b.host.paint.total_us-a.host.paint.total_us,"present_us":b.host.present.total_us-a.host.present.total_us,
        "present_errors":b.host.present.errors-a.host.present.errors})
}
fn timings(t: &host::Timings) -> Value {
    let mut samples = t.samples_us.clone();
    samples.sort_unstable();
    let at = |percent: usize| {
        samples
            .get(samples.len().saturating_sub(1) * percent / 100)
            .copied()
    };
    json!({"count":t.count,"errors":t.errors,"total_us":t.total_us,"max_us":t.max_us,"median_us":at(50),"p95_us":at(95),"retained_samples":samples.len()})
}
fn wait(s: &State, milliseconds: u64) -> Result<()> {
    let end = Instant::now() + Duration::from_millis(milliseconds);
    while Instant::now() < end {
        ensure!(
            !s.stop.load(Ordering::Relaxed),
            "Diagnostic was closed before completion"
        );
        std::thread::sleep(
            end.saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        );
    }
    Ok(())
}
fn phase(
    s: &State,
    name: &str,
    minimized: bool,
    deadline: Instant,
    read_minimized: impl Fn() -> Option<bool>,
) -> Result<Value> {
    let mut start = sample(s)?;
    let mut previous = start.clone();
    let mut intervals = Vec::new();
    let mut high_cpu = 0;
    while intervals.len() < 5 {
        ensure!(
            Instant::now() < deadline,
            "No clean five-second idle sample within the diagnostic budget in {name}"
        );
        wait(s, 1000)?;
        let current = sample(s)?;
        let interval = delta(&previous, &current);
        let observed = read_minimized();
        s.report.lock().unwrap()["latest_interval"] =
            json!({"phase":name,"observed_minimized":observed,"sample":interval});
        ensure!(
            observed == Some(minimized),
            "Unexpected minimized state in {name}: {observed:?}"
        );
        if !minimized {
            ensure!(
                s.report.lock().unwrap()["last_viewport"]["occluded"] != true,
                "Visible sample was occluded in {name}"
            );
        }
        ensure!(
            !s.isolation_failed.load(Ordering::Relaxed),
            "Unexpected background work in isolated diagnostics"
        );
        // Input legitimately invalidates the window. Keep that evidence, then obtain
        // a new contiguous clean interval; never classify it as an idle redraw loop.
        if interval["input_events"].as_u64().unwrap() > 0 {
            s.report.lock().unwrap()["interrupted_intervals"]
                .as_array_mut().unwrap()
                .push(json!({"phase":name,"sample":interval,"reason":"Application input events interrupted the idle sample"}));
            wait(s, 500)?;
            start = sample(s)?;
            previous = start.clone();
            intervals.clear();
            high_cpu = 0;
            continue;
        }
        let cpu = interval["cpu_percent_one_core"].as_f64().unwrap();
        high_cpu = if cpu > 5.0 { high_cpu + 1 } else { 0 };
        ensure!(
            high_cpu < 2,
            "Sustained idle CPU exceeded the 5% one-core investigation trigger in {name}"
        );
        ensure!(
            interval["paints"].as_u64().unwrap() <= 10
                && interval["presents"].as_u64().unwrap() <= 10,
            "Unexplained continuous rendering in {name}"
        );
        intervals.push(interval);
        previous = current;
    }
    let total = delta(&start, &previous);
    let quiet = total["native_frames"] == 0
        && total["ui_calls"] == 0
        && total["layouts"] == 0
        && total["paints"] == 0
        && total["presents"] == 0;
    let viewport = s.report.lock().unwrap()["last_viewport"].clone();
    let result = json!({"name":name,"expected_minimized":minimized,"observed_minimized":read_minimized(),"last_viewport":viewport,"quiet":quiet,"total":total,"intervals":intervals});
    s.report.lock().unwrap()["phases"]
        .as_array_mut()
        .unwrap()
        .push(result.clone());
    Ok(result)
}

struct DiagnosticApp {
    inner: RecorderApp,
    state: Arc<State>,
    worker: Option<JoinHandle<()>>,
    root: PathBuf,
}
impl DiagnosticApp {
    fn new(ctx: &egui::Context, window: Arc<winit::window::Window>, root: PathBuf) -> Result<Self> {
        let inner = review::native_fixture(ctx, &root)?;
        let backend = json!({"path":"egui 0.37 -> egui_software_backend 0.0.2 CPU rasterizer -> softbuffer 0.4.8 Win32 CreateDIBSection / BitBlt", "renderer":"CPU / Windows GDI", "gpu_context":false});
        let state = Arc::new(State {
            report: Mutex::new(
                json!({"version":updater::VERSION,"backend":backend,"phases":[],"interrupted_intervals":[],"isolation":"synthetic fixtures; recording/export/update dispatch disabled; no normal settings or audio endpoints","complete":false}),
            ),
            ..Default::default()
        });
        fs::write(
            root.join("backend.json"),
            serde_json::to_vec_pretty(&*state.report.lock().unwrap())?,
        )?;
        let ctx = ctx.clone();
        let shared = state.clone();
        let output = root.clone();
        let worker = std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let deadline = Instant::now() + Duration::from_secs(32);
                wait(&shared, 2000)?;
                ensure!(
                    host::snapshot().present.count > 0,
                    "The main renderer did not present a frame during warmup"
                );
                phase(&shared, "visible_idle", false, deadline, || {
                    window.is_minimized()
                })?;
                let before = sample(&shared)?;
                ctx.request_repaint();
                wait(&shared, 500)?;
                let after = sample(&shared)?;
                shared.report.lock().unwrap()["deliberate_invalidation"] = delta(&before, &after);
                ensure!(
                    after.host.present.count > before.host.present.count,
                    "Explicit repaint did not present"
                );
                phase(&shared, "return_to_idle", false, deadline, || {
                    window.is_minimized()
                })?;
                let before_minimize = sample(&shared)?;
                window.set_minimized(true);
                wait(&shared, 750)?;
                shared.report.lock().unwrap()["minimize_transition"] =
                    delta(&before_minimize, &sample(&shared)?);
                phase(&shared, "minimized_idle", true, deadline, || {
                    window.is_minimized()
                })?;
                let before_restore = sample(&shared)?;
                window.set_minimized(false);
                ctx.request_repaint();
                wait(&shared, 750)?;
                let after_restore = sample(&shared)?;
                shared.report.lock().unwrap()["restore_transition"] =
                    delta(&before_restore, &after_restore);
                ensure!(
                    after_restore.host.present.count > before_restore.host.present.count,
                    "Restore did not present a fresh frame"
                );
                phase(&shared, "restored_idle", false, deadline, || {
                    window.is_minimized()
                })?;
                Ok(())
            })();
            {
                let mut report = shared.report.lock().unwrap();
                report["complete"] = json!(result.is_ok());
                report["failure"] = result
                    .err()
                    .map_or(Value::Null, |e| json!(format!("{e:#}")));
                report["all_idle_phases_quiet"] = json!(
                    report["phases"]
                        .as_array()
                        .is_some_and(|p| p.len() == 4 && p.iter().all(|p| p["quiet"] == true))
                );
                let _ = fs::write(
                    output.join("phases.json"),
                    serde_json::to_vec_pretty(&*report).unwrap(),
                );
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint();
        });
        Ok(Self {
            inner,
            state,
            worker: Some(worker),
            root,
        })
    }
}
impl Application for DiagnosticApp {
    fn logic(&mut self, ctx: &egui::Context) {
        self.state.logic.fetch_add(1, Ordering::Relaxed);
        observe_viewport(&self.state, ctx);
        observe_input(&self.state, ctx);
        self.inner.logic(ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.state.stop.store(true, Ordering::Relaxed);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.state.ui.fetch_add(1, Ordering::Relaxed);
        observe_viewport(&self.state, ui.ctx());
        self.inner.ui(ui);
        if self.inner.worker.is_some()
            || self.inner.update_worker.is_some()
            || self.inner.control.is_some()
        {
            self.state.isolation_failed.store(true, Ordering::Relaxed);
        }
    }
    fn on_exit(&mut self) {
        self.state.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.inner.on_exit();
        let metrics = host::snapshot();
        let mut report = self.state.report.lock().unwrap();
        report["totals"] = json!({"ui_calls":self.state.ui.load(Ordering::Relaxed),"logic_calls":self.state.logic.load(Ordering::Relaxed),"input_events":self.state.input.load(Ordering::Relaxed),"frame":timings(&metrics.frame),"layout":timings(&metrics.layout),"paint":timings(&metrics.paint),"present":timings(&metrics.present)});
        report["isolation_failed"] = json!(self.state.isolation_failed.load(Ordering::Relaxed));
        let _ = fs::write(
            self.root.join("result.json"),
            serde_json::to_vec_pretty(&*report).unwrap(),
        );
    }
}
impl Drop for DiagnosticApp {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn run(root: &Path) -> Result<()> {
    ensure!(
        root.is_absolute(),
        "Diagnostic output must be an absolute new directory"
    );
    fs::create_dir(root)?;
    host::enable();
    let output = root.to_path_buf();
    crate::host::run(
        "ScreenRecorder · isolated CPU/GDI diagnostic (no capture)",
        false,
        Box::new(move |ctx, window| {
            DiagnosticApp::new(ctx, window, output).map(|app| Box::new(app) as Box<dyn Application>)
        }),
    )?;
    let report: Value = serde_json::from_slice(&fs::read(root.join("result.json"))?)?;
    ensure!(
        report["complete"] == true,
        "Incomplete diagnostic: {}",
        report["failure"]
    );
    ensure!(
        report["all_idle_phases_quiet"] == true,
        "Unexpected UI/paint/present activity in an idle phase"
    );
    ensure!(
        report["totals"]["present"]["errors"] == 0,
        "Native graphics errors were reported"
    );
    Ok(())
}
