use crate::ui::Icon;
use crate::{theme, ui as controls};
mod diagnostics;
mod review;
pub(crate) use diagnostics::run as diagnose_render;
pub(crate) use review::run as render_review;
use screen_recorder::{
    capture::{self, Target},
    editor::{self, Edit, Zoom},
    project::Project,
    recorder::{self, Control},
    settings::{self, Settings},
    updater::{self, Update},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::{SystemTime, UNIX_EPOCH},
};

type JobResult<T> = Result<T, String>;
const PRIMARY_ACTION_BOUNDS: &str = "screen-recorder-primary-action-bounds";
const BODY_SCROLL_METRICS: &str = "screen-recorder-body-scroll-metrics";
fn remember_primary(response: &egui::Response) {
    response
        .ctx
        .data_mut(|data| data.insert_temp(egui::Id::unique(PRIMARY_ACTION_BOUNDS), response.rect));
}
enum Event {
    Sources(JobResult<Vec<Target>>),
    Loaded(JobResult<(Arc<Project>, Edit)>),
    Preview(u64, JobResult<egui::ColorImage>),
    Recorded(PathBuf, JobResult<()>),
    Exported(JobResult<()>),
    Progress(String),
    Update(JobResult<Option<Update>>),
    Downloaded(JobResult<PathBuf>),
}
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Record,
    Edit,
    Settings,
}
#[derive(Clone, Copy, PartialEq)]
enum Busy {
    Idle,
    Sources,
    Load,
    Preview,
    Recording,
    Export,
}

pub struct RecorderApp {
    isolated: bool,
    owner: isize,
    settings: Settings,
    settings_path: PathBuf,
    tab: Tab,
    sources: Vec<Target>,
    source_mode: usize,
    selected_source: usize,
    region: (i32, i32, u32, u32),
    recording_name: String,
    open_path: String,
    export_path: String,
    export_mp4: bool,
    project: Option<Arc<Project>>,
    edit: Option<Edit>,
    playhead: f64,
    preview: Option<egui::TextureHandle>,
    preview_dirty: bool,
    preview_generation: u64,
    status: String,
    error: Option<String>,
    busy: Busy,
    worker: Option<JoinHandle<()>>,
    control: Option<Arc<Control>>,
    cancel: Arc<AtomicBool>,
    tx: SyncSender<Event>,
    rx: Receiver<Event>,
    update_worker: Option<JoinHandle<()>>,
    update_busy: bool,
    update: Option<Update>,
    update_message: String,
    installing: bool,
    closing: bool,
}

fn result<T>(value: anyhow::Result<T>) -> JobResult<T> {
    value.map_err(|e| format!("{e:#}"))
}
fn send(tx: &SyncSender<Event>, ctx: &egui::Context, event: Event) {
    if tx.send(event).is_ok() {
        ctx.request_repaint();
    }
}
fn progress(tx: &SyncSender<Event>, ctx: &egui::Context, text: String) {
    let _ = tx.try_send(Event::Progress(text));
    ctx.request_repaint();
}
fn recording_name() -> String {
    format!(
        "Recording-{}.srec",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}

impl RecorderApp {
    pub fn new(ctx: &egui::Context, owner: isize) -> Self {
        theme::apply(ctx);
        let settings_path = settings::directory().join("settings.json");
        let (mut settings, error) = match Settings::load(&settings_path) {
            Ok(s) => (s, None),
            Err(e) => (
                Settings {
                    startup_updates: false,
                    ..Default::default()
                },
                Some(format!(
                    "Settings could not be loaded; startup checks disabled for this session: {e:#}"
                )),
            ),
        };
        if settings.output_folder.is_empty() {
            settings.output_folder = std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("Videos")
                .join("ScreenRecorder")
                .to_string_lossy()
                .into_owned();
        }
        let (tx, rx) = mpsc::sync_channel(8);
        let update_error_path = settings::directory().join("update-error.txt");
        let previous_update_error = std::fs::metadata(&update_error_path)
            .ok()
            .filter(|m| m.len() < 64 * 1024)
            .and_then(|_| std::fs::read_to_string(&update_error_path).ok());
        let mut app = Self {
            isolated: false,
            owner,
            settings,
            settings_path,
            tab: Tab::Record,
            sources: Vec::new(),
            source_mode: 0,
            selected_source: 0,
            region: (100, 100, 1280, 720),
            recording_name: recording_name(),
            open_path: String::new(),
            export_path: String::new(),
            export_mp4: true,
            project: None,
            edit: None,
            playhead: 0.0,
            preview: None,
            preview_dirty: false,
            preview_generation: 0,
            status: "Ready to record".into(),
            error: error.or(previous_update_error.map(|e| format!("Previous update failed: {e}"))),
            busy: Busy::Idle,
            worker: None,
            control: None,
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            update_worker: None,
            update_busy: false,
            update: None,
            update_message: "No check performed".into(),
            installing: false,
            closing: false,
        };
        if app.settings.startup_updates {
            app.check_updates(ctx);
        }
        app
    }
    fn can_start(&self) -> bool {
        self.busy == Busy::Idle && !self.installing && !self.closing
    }
    fn mark_preview(&mut self, ctx: &egui::Context) {
        self.preview_generation += 1;
        self.preview_dirty = true;
        ctx.request_repaint();
    }
    fn refresh_sources(&mut self, ctx: &egui::Context) {
        if self.isolated {
            return;
        }
        self.busy = Busy::Sources;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.worker = Some(std::thread::spawn(move || {
            send(&tx, &ctx, Event::Sources(result(capture::targets())))
        }));
    }
    fn selected_target(&self) -> Option<Target> {
        if self.source_mode == 2 {
            let (left, top, width, height) = self.region;
            return Some(Target::Region {
                left,
                top,
                width,
                height,
            });
        }
        self.sources
            .iter()
            .filter(|t| {
                matches!(
                    (self.source_mode, t),
                    (0, Target::Display { .. }) | (1, Target::Window { .. })
                )
            })
            .nth(self.selected_source)
            .cloned()
    }
    fn start_recording(&mut self, ctx: &egui::Context) {
        if self.isolated {
            return;
        }
        let Some(target) = self.selected_target() else {
            self.error =
                Some("Choose a source. Use Refresh sources to list displays and windows.".into());
            return;
        };
        let name = PathBuf::from(self.recording_name.trim());
        if name.components().count() != 1 || name.extension().is_none_or(|x| x != "srec") {
            self.error = Some("Recording name must be a single filename ending in .srec".into());
            return;
        }
        let folder = PathBuf::from(self.settings.output_folder.trim());
        if !folder.is_absolute() {
            self.error = Some("Choose an absolute output folder path".into());
            return;
        }
        let path = folder.join(name);
        let fps = self.settings.fps;
        let audio = screen_recorder::audio::Options {
            microphone: self.settings.microphone,
            system: self.settings.system_audio,
        };
        let (control, wake) = Control::channel();
        self.control = Some(control.clone());
        self.busy = Busy::Recording;
        self.error = None;
        self.status = "RECORDING · preparing capture · Ctrl+Shift+F10 stops while recording".into();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.worker = Some(std::thread::spawn(move || {
            let work = (|| -> anyhow::Result<()> {
                std::fs::create_dir_all(folder)?;
                #[cfg(windows)]
                {
                    let mut source = capture::Source::new(target, fps)?;
                    recorder::record_with_audio(
                        &mut source,
                        &path,
                        fps,
                        &control,
                        &wake,
                        audio,
                        |frames, seconds| {
                            progress(
                                &tx,
                                &ctx,
                                format!(
                                    "Recorded {seconds:.1} s · {frames} frames · Ctrl+Shift+F10 stops"
                                ),
                            );
                        },
                    )?;
                }
                #[cfg(not(windows))]
                {
                    let _ = (target, fps, control, wake, audio);
                    anyhow::bail!("Capture is available on Windows only");
                }
                Ok(())
            })();
            send(&tx, &ctx, Event::Recorded(path, result(work)));
        }));
    }
    fn load_project(&mut self, ctx: &egui::Context) {
        if self.isolated {
            return;
        }
        let path = PathBuf::from(self.open_path.trim());
        self.busy = Busy::Load;
        self.status = "Opening recording…".into();
        self.error = None;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.worker = Some(std::thread::spawn(move || {
            let work = (|| -> anyhow::Result<_> {
                let p = Project::open(&path)?;
                let edit = Edit::load(&p)?;
                Ok((Arc::new(p), edit))
            })();
            send(&tx, &ctx, Event::Loaded(result(work)));
        }));
    }
    fn request_preview(&mut self, ctx: &egui::Context) {
        if self.isolated {
            return;
        }
        let (Some(project), Some(edit)) = (self.project.clone(), self.edit.clone()) else {
            return;
        };
        self.preview_dirty = false;
        self.busy = Busy::Preview;
        let time = self.playhead;
        let generation = self.preview_generation;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.worker = Some(std::thread::spawn(move || {
            let work = (|| -> anyhow::Result<_> {
                edit.validate(&project)?;
                let frame = project.read_frame(project.index_at(time))?;
                let width = project.width.min(720);
                let height = (project.height as f64 * width as f64 / project.width as f64)
                    .round()
                    .max(1.0) as u32;
                let image = editor::render(
                    &project,
                    &edit,
                    &editor::automatic_zooms(&project),
                    &frame,
                    time,
                    width,
                    height,
                );
                Ok(egui::ColorImage::from_rgb(
                    [width as usize, height as usize],
                    image.as_raw(),
                ))
            })();
            send(&tx, &ctx, Event::Preview(generation, result(work)));
        }));
    }
    fn start_export(&mut self, ctx: &egui::Context) {
        if self.isolated {
            return;
        }
        let (Some(project), Some(edit)) = (self.project.clone(), self.edit.clone()) else {
            return;
        };
        let path = PathBuf::from(self.export_path.trim());
        let extension = if self.export_mp4 { "mp4" } else { "avi" };
        if path.extension().is_none_or(|x| x != extension) || !path.is_absolute() {
            self.error = Some(format!(
                "Choose an absolute .{extension} path for the selected export format"
            ));
            return;
        }
        self.busy = Busy::Export;
        self.error = None;
        self.cancel.store(false, Ordering::Relaxed);
        let cancel = self.cancel.clone();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.status = "Exporting video…".into();
        self.worker = Some(std::thread::spawn(move || {
            let last = std::cell::Cell::new(std::time::Instant::now());
            let report = |value| {
                if last.get().elapsed().as_millis() >= 250 || value >= 1.0 {
                    progress(
                        &tx,
                        &ctx,
                        format!("Exporting video · {:.0}%", value * 100.0),
                    );
                    last.set(std::time::Instant::now());
                }
            };
            let work = if path.extension().is_some_and(|x| x == "mp4") {
                #[cfg(windows)]
                {
                    screen_recorder::mp4::export(&project, &edit, &path, &cancel, report)
                }
                #[cfg(not(windows))]
                {
                    Err(anyhow::anyhow!(
                        "MP4 export requires Windows Media Foundation"
                    ))
                }
            } else {
                editor::export(&project, &edit, &path, &cancel, report)
            };
            send(&tx, &ctx, Event::Exported(result(work)));
        }));
    }
    fn check_updates(&mut self, ctx: &egui::Context) {
        if self.isolated {
            return;
        }
        if self.update_busy {
            return;
        }
        self.update_busy = true;
        self.update_message = "Checking for a stable release…".into();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.update_worker = Some(std::thread::spawn(move || {
            send(&tx, &ctx, Event::Update(result(updater::check())))
        }));
    }
    fn install_update(&mut self, ctx: &egui::Context) {
        if self.isolated {
            return;
        }
        if self.update_busy || !self.can_start() {
            return;
        }
        let Some(update) = self.update.clone() else {
            return;
        };
        self.update_busy = true;
        self.installing = true;
        self.cancel.store(false, Ordering::Relaxed);
        let cancel = self.cancel.clone();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.update_message = "Downloading update…".into();
        self.update_worker = Some(std::thread::spawn(move || {
            let last = std::cell::Cell::new(std::time::Instant::now());
            let work = updater::download(&update, &cancel, |value| {
                if last.get().elapsed().as_millis() >= 250 || value >= 1.0 {
                    progress(
                        &tx,
                        &ctx,
                        format!("Downloading update · {:.0}%", value * 100.0),
                    );
                    last.set(std::time::Instant::now());
                }
            });
            send(&tx, &ctx, Event::Downloaded(result(work)));
        }));
    }

    fn process_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Progress(text) => self.status = text,
                Event::Sources(value) => {
                    self.busy = Busy::Idle;
                    match value {
                        Ok(s) => {
                            self.sources = s;
                            self.selected_source = 0;
                            self.status = "Sources refreshed · choose one before recording".into();
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Event::Loaded(value) => {
                    self.busy = Busy::Idle;
                    match value {
                        Ok((project, edit)) => {
                            self.export_path = project
                                .path
                                .with_file_name(format!(
                                    "{}-edited.mp4",
                                    project
                                        .path
                                        .file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                ))
                                .to_string_lossy()
                                .into_owned();
                            self.status = if project.recovered_tail {
                                "Recovered complete frames; incomplete trailing data was ignored"
                                    .into()
                            } else {
                                format!(
                                    "Loaded {} frames · {:.2} s",
                                    project.frames.len(),
                                    project.duration()
                                )
                            };
                            self.project = Some(project);
                            self.export_mp4 = true;
                            self.edit = Some(edit);
                            self.playhead = 0.0;
                            self.preview = None;
                            self.tab = Tab::Edit;
                            self.mark_preview(ctx);
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Event::Preview(generation, value) => {
                    self.busy = Busy::Idle;
                    match value {
                        Ok(image) if generation == self.preview_generation => {
                            self.preview = Some(ctx.load_texture(
                                "edited preview",
                                image,
                                egui::TextureOptions::LINEAR,
                            ))
                        }
                        Ok(_) => {}
                        Err(e) => self.error = Some(e),
                    }
                }
                Event::Recorded(path, value) => {
                    self.busy = Busy::Idle;
                    self.control = None;
                    self.recording_name = recording_name();
                    self.open_path = path.to_string_lossy().into_owned();
                    match value {
                        Ok(()) => {
                            self.status = format!("Recording saved: {}", path.display());
                            if !self.closing {
                                self.load_project(ctx);
                            }
                        }
                        Err(e) => {
                            self.error = Some(format!(
                                "{e}\nAny complete frames can be opened from {}",
                                path.display()
                            ));
                            self.status =
                                "Recording stopped with an error; original project retained".into();
                        }
                    }
                }
                Event::Exported(value) => {
                    self.busy = Busy::Idle;
                    match value {
                        Ok(()) => self.status = format!("Video exported: {}", self.export_path),
                        Err(e) => self.error = Some(e),
                    }
                }
                Event::Update(value) => {
                    self.update_busy = false;
                    match value {
                        Ok(Some(update)) => {
                            self.update_message =
                                format!("Version {} is available", update.version);
                            self.update = Some(update);
                        }
                        Ok(None) => {
                            self.update_message = "You have the latest stable version".into();
                            self.update = None;
                        }
                        Err(e) => self.update_message = e,
                    }
                }
                Event::Downloaded(value) => {
                    self.update_busy = false;
                    self.installing = false;
                    match value {
                        Ok(_) if self.closing || self.cancel.load(Ordering::Relaxed) => {
                            self.status =
                                "Update download cancelled; current installation preserved".into();
                        }
                        Ok(staging) => match updater::launch_install(&staging) {
                            Ok(()) => {
                                self.status = "Installing update and restarting…".into();
                                self.closing = true;
                            }
                            Err(e) => self.error = Some(format!("{e:#}")),
                        },
                        Err(e) => {
                            self.update_message = e;
                            self.status = "Update stopped; current installation preserved".into();
                        }
                    }
                }
            }
        }
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let _ = self.worker.take().unwrap().join();
        }
        if self.update_worker.as_ref().is_some_and(|w| w.is_finished()) {
            let _ = self.update_worker.take().unwrap().join();
        }
        if self.closing {
            if let Some(control) = &self.control {
                control.stop();
            }
            if self.busy == Busy::Export || self.installing {
                self.cancel.store(true, Ordering::Relaxed);
            }
            if self.busy == Busy::Idle && !self.installing {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if self.preview_dirty
            && self.busy == Busy::Idle
            && !ctx.input(|i| i.pointer.any_down())
        {
            self.request_preview(ctx);
        }
    }

    fn record_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("New recording");
        ui.label(
            egui::RichText::new("Choose your canvas. Add sound if you need it.")
                .color(theme::SECONDARY),
        );
        ui.add_space(12.0);
        let editable = self.can_start();
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong("What would you like to record?");
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (index, label, icon) in [
                        (0, "Display", Icon::Display),
                        (1, "Window", Icon::Window),
                        (2, "Region", Icon::Region),
                    ] {
                        if controls::choice(ui, icon, label, self.source_mode == index, 124.)
                            .clicked()
                        {
                            self.source_mode = index;
                            self.selected_source = 0;
                        }
                    }
                    if ui.button("Refresh sources").clicked() {
                        self.refresh_sources(ui.ctx());
                    }
                });
                if self.source_mode == 2 {
                    ui.horizontal_wrapped(|ui| {
                        for (label, n) in
                            [("Left", &mut self.region.0), ("Top", &mut self.region.1)]
                        {
                            ui.label(label);
                            ui.add(egui::DragValue::new(n).range(-32768..=32768));
                        }
                        ui.label("Width");
                        ui.add(egui::DragValue::new(&mut self.region.2).range(64..=4096));
                        ui.label("Height");
                        ui.add(egui::DragValue::new(&mut self.region.3).range(64..=2160));
                    });
                    ui.small("Physical pixels; negative positions are supported.");
                } else {
                    egui::ComboBox::from_id_salt("source")
                        .width(ui.available_width())
                        .selected_text(
                            self.selected_target()
                                .map_or("Refresh to find a display or window".into(), |t| {
                                    t.label()
                                }),
                        )
                        .show_ui(ui, |ui| {
                            for (index, t) in self
                                .sources
                                .iter()
                                .filter(|t| {
                                    matches!(
                                        (self.source_mode, t),
                                        (0, Target::Display { .. }) | (1, Target::Window { .. })
                                    )
                                })
                                .enumerate()
                            {
                                ui.selectable_value(&mut self.selected_source, index, t.label());
                            }
                        });
                }
                if self.source_mode == 1 {
                    ui.small(
                        "Keep the selected window visible. Resizing or minimizing ends recording.",
                    );
                }
            });
        });
        ui.add_space(8.);
        ui.strong("Sound");
        ui.add_enabled_ui(editable, |ui| {
            ui.columns(2, |columns| {
                controls::toggle(
                    &mut columns[0],
                    &mut self.settings.microphone,
                    Icon::Mic,
                    "Microphone",
                    "Your default input",
                );
                controls::toggle(
                    &mut columns[1],
                    &mut self.settings.system_audio,
                    Icon::Sound,
                    "System sound",
                    "Audio playing on your PC",
                );
            });
            ui.small(if self.settings.microphone || self.settings.system_audio {
                "Sound starts only when you press Record."
            } else {
                "Sound is off. Your recording will be silent."
            });
        });
        ui.add_space(8.);
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                controls::section(ui, Icon::Folder, "Save to");
                if ui.small_button("Choose folder…").clicked() && !self.isolated {
                    match crate::dialogs::choose(self.owner, crate::dialogs::Kind::Folder) {
                        Ok(Some(path)) => self.settings.output_folder = path,
                        Ok(None) => {}
                        Err(e) => self.error = Some(format!("{e:#}")),
                    }
                }
            });
            ui.add_enabled_ui(editable, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.output_folder)
                        .desired_width(f32::INFINITY),
                );
                ui.collapsing("Filename and quality", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Filename");
                        ui.text_edit_singleline(&mut self.recording_name);
                        ui.label("Frames / sec");
                        ui.add(egui::DragValue::new(&mut self.settings.fps).range(5..=30));
                    });
                });
            });
        });
        ui.add_space(8.);
    }

    fn record_controls_ui(&mut self, ui: &mut egui::Ui) {
        let editable = self.can_start();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let start = ui
                .add_enabled_ui(editable, |ui| {
                    controls::action(ui, Icon::Record, "Start recording", 190.)
                })
                .inner;
            remember_primary(&start);
            if ui
                .add_enabled(editable, egui::Button::new("Open recording…"))
                .clicked()
                && !self.isolated
            {
                match crate::dialogs::choose(self.owner, crate::dialogs::Kind::Project) {
                    Ok(Some(path)) => {
                        self.open_path = path;
                        self.load_project(ui.ctx());
                    }
                    Ok(None) => {}
                    Err(e) => self.error = Some(format!("{e:#}")),
                }
            }
            if start.clicked() {
                self.start_recording(ui.ctx());
            }
            if let Some(control) = &self.control {
                let paused = control.is_paused();
                if ui
                    .add_enabled(
                        control.has_frames() && !control.is_stopped(),
                        egui::Button::new(if paused { "Resume" } else { "Pause" }),
                    )
                    .clicked()
                {
                    control.pause(!paused);
                }
                if ui
                    .add_enabled(!control.is_stopped(), egui::Button::new("Stop and save"))
                    .clicked()
                {
                    control.stop();
                    self.status = "Finalizing recording…".into();
                }
            }
        });
        ui.small("Ctrl+Shift+F10 stops recording. Pause excludes time from video and sound.");
    }

    fn edit_ui(&mut self, ui: &mut egui::Ui) {
        let (Some(project), Some(mut edit)) = (self.project.clone(), self.edit.take()) else {
            ui.heading("Your recording, with a clearer focus");
            ui.label("Record a clip or open a .srec project from the Record tab.");
            return;
        };
        let can_edit = matches!(self.busy, Busy::Idle | Busy::Preview) && !self.installing;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.heading("Edit tutorial");
            ui.label(format!(
                "{} × {} · {} fps · {:.2} s",
                project.width,
                project.height,
                project.fps,
                project.duration()
            ));
        });
        ui.columns(2, |columns| {
            let ui = &mut columns[0];
            theme::panel(ui).show(ui, |ui| {
                ui.strong("Preview");
                if let Some(texture) = &self.preview {
                    ui.add(
                        egui::Image::new(texture)
                            .fit_to_exact_size(egui::vec2(
                                ui.available_width(),
                                ui.available_width() * project.height as f32 / project.width as f32,
                            ))
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Scrub the timeline to inspect the edited output");
                } else {
                    ui.add_space(100.0);
                    ui.label("Preparing preview…");
                    ui.add_space(100.0);
                }
                changed |= ui
                    .add_enabled_ui(can_edit, |ui| {
                        theme::slider(
                            ui,
                            egui::Slider::new(&mut self.playhead, 0.0..=project.duration())
                                .text("seconds")
                                .fixed_decimals(2),
                        )
                    })
                    .inner
                    .changed();
                ui.small("Drag the timeline to preview your edits.");
            });
            let ui = &mut columns[1];
            ui.add_enabled_ui(can_edit, |ui| {
                ui.strong("Trim range");
                ui.horizontal_wrapped(|ui| {
                    ui.label("In");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut edit.trim_start)
                                .speed(0.05)
                                .range(0.0..=project.duration()),
                        )
                        .changed();
                    ui.label("Out");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut edit.trim_end)
                                .speed(0.05)
                                .range(0.0..=project.duration()),
                        )
                        .changed();
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("In at playhead").clicked() {
                        edit.trim_start = self.playhead;
                        changed = true;
                    }
                    if ui.small_button("Out at playhead").clicked() {
                        edit.trim_end = self.playhead;
                        changed = true;
                    }
                });
                ui.separator();
                ui.strong("Choose a style");
                let clean =
                    edit.show_cursor && !edit.highlight && !edit.clicks && !edit.automatic_zoom;
                let guided =
                    edit.show_cursor && edit.highlight && edit.clicks && !edit.automatic_zoom;
                let focus =
                    edit.show_cursor && edit.highlight && edit.clicks && edit.automatic_zoom;
                let width = (ui.available_width() - 16.) / 3.;
                ui.horizontal(|ui| {
                    for (label, icon, selected, highlight, zoom) in [
                        ("Clean", Icon::Cursor, clean, false, false),
                        ("Guided", Icon::Click, guided, true, false),
                        ("Focus", Icon::Zoom, focus, true, true),
                    ] {
                        if controls::choice(ui, icon, label, selected, width).clicked() && !selected
                        {
                            edit.show_cursor = true;
                            edit.highlight = highlight;
                            edit.clicks = highlight;
                            edit.automatic_zoom = zoom;
                            changed = true;
                        }
                    }
                });
                ui.small(if focus {
                    "Cursor emphasis with automatic zoom."
                } else if guided {
                    "Highlight the cursor and emphasize each click."
                } else if clean {
                    "Just your recording and a clean cursor."
                } else {
                    "Custom cursor and zoom settings."
                });
                ui.collapsing("Customize effects", |ui| {
                    changed |= controls::toggle(
                        ui,
                        &mut edit.show_cursor,
                        Icon::Cursor,
                        "Tutorial cursor",
                        "A clear, consistent pointer",
                    )
                    .changed();
                    changed |= controls::toggle(
                        ui,
                        &mut edit.highlight,
                        Icon::Cursor,
                        "Cursor highlight",
                        "Keep the pointer easy to follow",
                    )
                    .changed();
                    changed |= controls::toggle(
                        ui,
                        &mut edit.clicks,
                        Icon::Click,
                        "Click emphasis",
                        "Show where each click happens",
                    )
                    .changed();
                    changed |= controls::toggle(
                        ui,
                        &mut edit.automatic_zoom,
                        Icon::Zoom,
                        "Auto zoom",
                        "Focus smoothly around clicks",
                    )
                    .changed();
                });
                ui.separator();
                ui.strong("Sound");
                if !self.export_mp4 {
                    ui.small("AVI exports are silent. Choose MP4 below to include sound.");
                } else if project.audio_tracks.is_empty() {
                    ui.small("This recording has no audio tracks.");
                } else {
                    controls::toggle(
                        ui,
                        &mut edit.include_audio,
                        Icon::Sound,
                        "Recorded sound",
                        "Include sound in your video",
                    );
                    if edit.include_audio {
                        ui.collapsing("Sound levels", |ui| {
                            for track in &project.audio_tracks {
                                match track.kind {
                                    screen_recorder::audio::Kind::Microphone => {
                                        theme::slider(
                                            ui,
                                            egui::Slider::new(&mut edit.microphone_gain, 0.0..=2.0)
                                                .text("Microphone"),
                                        );
                                    }
                                    screen_recorder::audio::Kind::System => {
                                        theme::slider(
                                            ui,
                                            egui::Slider::new(&mut edit.system_gain, 0.0..=2.0)
                                                .text("System sound"),
                                        );
                                    }
                                }
                            }
                            ui.small(
                                "Two tracks mix at half gain each. Preview does not play sound.",
                            );
                        });
                    }
                }
                ui.collapsing("Edit zoom timing", |ui| {
                    if ui.button("Add zoom at playhead").clicked()
                        && self.playhead < project.duration()
                        && edit.zooms.len() < 2000
                    {
                        let cursor = project.frames[project.index_at(self.playhead)].cursor;
                        edit.zooms.push(Zoom {
                            start: self.playhead,
                            end: (self.playhead + 2.0).min(project.duration()),
                            x: cursor.x,
                            y: cursor.y,
                            scale: 1.8,
                        });
                        changed = true;
                    }
                    egui::ScrollArea::vertical()
                        .max_height(175.0)
                        .id_salt("zooms")
                        .show(ui, |ui| {
                            let mut remove = None;
                            for (i, zoom) in edit.zooms.iter_mut().enumerate() {
                                ui.push_id(i, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.strong(format!("Zoom {}", i + 1));
                                        if ui.small_button("Remove").clicked() {
                                            remove = Some(i);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("In/out");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut zoom.start)
                                                    .speed(0.05)
                                                    .range(0.0..=project.duration()),
                                            )
                                            .changed();
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut zoom.end)
                                                    .speed(0.05)
                                                    .range(0.0..=project.duration()),
                                            )
                                            .changed();
                                        ui.label("×");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut zoom.scale)
                                                    .speed(0.05)
                                                    .range(1.0..=3.0),
                                            )
                                            .changed();
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Focus X/Y");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut zoom.x)
                                                    .speed(0.01)
                                                    .range(0.0..=1.0),
                                            )
                                            .changed();
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut zoom.y)
                                                    .speed(0.01)
                                                    .range(0.0..=1.0),
                                            )
                                            .changed();
                                    });
                                });
                            }
                            if let Some(index) = remove {
                                edit.zooms.remove(index);
                                changed = true;
                            }
                        });
                });
            });
        });
        self.edit = Some(edit);
        if changed {
            self.mark_preview(ui.ctx());
        }
    }

    fn export_controls_ui(&mut self, ui: &mut egui::Ui) {
        if self.project.is_none() {
            return;
        }
        let can_edit = matches!(self.busy, Busy::Idle | Busy::Preview) && !self.installing;
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Export");
            let old = self.export_mp4;
            ui.add_enabled_ui(can_edit, |ui| {
                ui.selectable_value(&mut self.export_mp4, true, "MP4 video")
                    .on_hover_text("Video with recorded sound; odd dimensions pad to even pixels");
                ui.selectable_value(&mut self.export_mp4, false, "AVI · no sound")
                    .on_hover_text("Silent video, limited to 2 GB per export");
            });
            if old != self.export_mp4 {
                self.export_path = PathBuf::from(&self.export_path)
                    .with_extension(if self.export_mp4 { "mp4" } else { "avi" })
                    .to_string_lossy()
                    .into_owned();
            }
        });
        ui.add_enabled(
            can_edit,
            egui::TextEdit::singleline(&mut self.export_path).desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            let export = ui
                .add_enabled_ui(self.can_start(), |ui| {
                    controls::action(ui, Icon::Export, "Export video", 174.)
                })
                .inner;
            remember_primary(&export);
            if export.clicked() {
                self.start_export(ui.ctx());
            }
            if ui
                .add_enabled(can_edit, egui::Button::new("Save edit decisions"))
                .on_hover_text(
                    "Save trim, zoom and mix for the next session without changing source media",
                )
                .clicked()
                && let (Some(project), Some(edit)) = (&self.project, &self.edit)
            {
                match edit.save(project) {
                    Ok(()) => self.status = "Edit decisions saved beside the project".into(),
                    Err(e) => self.error = Some(format!("{e:#}")),
                }
            }
            if self.busy == Busy::Export && ui.button("Cancel export").clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        });
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.label(
            egui::RichText::new(format!("ScreenRecorder {}", updater::VERSION))
                .color(theme::SECONDARY),
        );
        ui.add_space(12.);
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong("Recording defaults");
            ui.horizontal(|ui| {
                ui.label("Save recordings to");
                if ui.small_button("Choose folder…").clicked() && !self.isolated {
                    match crate::dialogs::choose(self.owner, crate::dialogs::Kind::Folder) {
                        Ok(Some(path)) => self.settings.output_folder = path,
                        Ok(None) => {}
                        Err(e) => self.error = Some(format!("{e:#}")),
                    }
                }
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.output_folder)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.label("Frames per second");
                ui.add(egui::DragValue::new(&mut self.settings.fps).range(5..=30));
            });
            ui.columns(2, |columns| {
                controls::toggle(
                    &mut columns[0],
                    &mut self.settings.microphone,
                    Icon::Mic,
                    "Microphone",
                    "For new recordings",
                );
                controls::toggle(
                    &mut columns[1],
                    &mut self.settings.system_audio,
                    Icon::Sound,
                    "System sound",
                    "For new recordings",
                );
            });
            if ui.button("Save defaults").clicked() {
                match self.settings.save(&self.settings_path) {
                    Ok(()) => self.status = "Defaults saved".into(),
                    Err(e) => self.error = Some(format!("{e:#}")),
                }
            }
        });
        ui.add_space(8.);
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong("Keep up to date");
            if controls::toggle(
                ui,
                &mut self.settings.startup_updates,
                Icon::Settings,
                "Check on startup",
                "Downloads always need your approval",
            )
            .changed()
                && let Err(e) = self.settings.save(&self.settings_path)
            {
                self.error = Some(format!("Preference could not be saved: {e:#}"));
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!self.update_busy, egui::Button::new("Check for updates"))
                    .clicked()
                {
                    self.check_updates(ui.ctx());
                }
                ui.label(&self.update_message);
            });
            if let Some(update) = &self.update {
                ui.colored_label(
                    theme::SUCCESS,
                    format!("Version {} is available", update.version),
                );
                if ui
                    .add_enabled(
                        self.can_start() && !self.update_busy,
                        egui::Button::new("Download and install (restarts app)"),
                    )
                    .clicked()
                {
                    self.install_update(ui.ctx());
                }
            }
            if self.installing && ui.button("Cancel download").clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        });
        ui.add_space(8.);
        ui.collapsing("About and recording support",|ui| {
            ui.label("Records visible displays, windows or regions. Exports MP4 with sound or silent AVI.");
            ui.small("No webcam or hidden-window capture. Keep the selected window visible.");
            ui.small(updater::REPOSITORY);
            ui.small(format!("Settings: {}",self.settings_path.display()));
        });
    }
}

impl crate::host::Application for RecorderApp {
    fn logic(&mut self, ctx: &egui::Context) {
        self.process_events(ctx);
        if ctx.input(|i| i.viewport().close_requested())
            && (self.busy != Busy::Idle || self.installing)
        {
            self.closing = true;
            if let Some(control) = &self.control {
                control.stop();
            }
            self.cancel.store(true, Ordering::Relaxed);
            self.status = "Finishing active work before closing…".into();
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.show(ui);
    }
    fn on_exit(&mut self) {
        if let Some(control) = &self.control {
            control.stop();
        }
        self.cancel.store(true, Ordering::Relaxed);
        // Normal close waits through logic until the recording/export worker has finalized.
    }
}

impl RecorderApp {
    /// Shared by the real window and the isolated offscreen review. No second UI is built.
    fn show(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BACKGROUND).inner_margin(20))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    controls::brand(ui);
                    ui.add_space(20.);
                    for (tab, label, icon) in [
                        (Tab::Record, "Record", Icon::Record),
                        (Tab::Edit, "Edit", Icon::Edit),
                        (Tab::Settings, "Settings", Icon::Settings),
                    ] {
                        if controls::nav(ui, icon, label, self.tab == tab, 112.).clicked() {
                            self.tab = tab;
                        }
                    }
                    if let Some(control) = &self.control {
                        let (label, color) = if control.is_stopped() {
                            ("FINALIZING", theme::WARNING)
                        } else if !control.has_frames() {
                            ("STARTING", theme::WARNING)
                        } else if control.is_paused() {
                            ("PAUSED", theme::WARNING)
                        } else {
                            ("RECORDING", theme::ERROR)
                        };
                        ui.colored_label(color, label);
                        if self.tab != Tab::Record
                            && ui
                                .add_enabled(!control.is_stopped(), egui::Button::new("Stop"))
                                .clicked()
                        {
                            control.stop();
                        }
                    }
                    if self.update.is_some() {
                        ui.colored_label(theme::SUCCESS, "Update available");
                    }
                });
                ui.separator();
                if let Some(error) = self.error.clone() {
                    theme::panel(ui).show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("error-message")
                            .max_height(88.0)
                            .show(ui, |ui| {
                                ui.colored_label(theme::ERROR, error);
                            });
                        if ui.small_button("Dismiss").clicked() {
                            self.error = None;
                        }
                    });
                }
                let footer_height = match self.tab {
                    Tab::Record => 132.0,
                    Tab::Edit if self.project.is_some() => 184.0,
                    _ => 42.0,
                };
                let body_height = (ui.available_height() - footer_height).max(64.0);
                let body = egui::ScrollArea::vertical()
                    .id_salt(match self.tab {
                        Tab::Record => "record-body",
                        Tab::Edit => "edit-body",
                        Tab::Settings => "settings-body",
                    })
                    .max_height(body_height)
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.tab {
                        Tab::Record => self.record_ui(ui),
                        Tab::Edit => self.edit_ui(ui),
                        Tab::Settings => self.settings_ui(ui),
                    });
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::unique(BODY_SCROLL_METRICS),
                        [
                            body.content_size.x,
                            body.content_size.y,
                            body.inner_rect.width(),
                            body.inner_rect.height(),
                        ],
                    )
                });
                match self.tab {
                    Tab::Record => self.record_controls_ui(ui),
                    Tab::Edit => self.export_controls_ui(ui),
                    Tab::Settings => {}
                }
                ui.separator();
                ui.add(egui::Label::new(&self.status).truncate())
                    .on_hover_text(&self.status);
            });
    }
}
