use crate::ui::Icon;
use crate::{theme, ui as controls};
use screen_recorder::localization::{Language, Message};
use screen_recorder::settings::Theme;
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
    Progress(Message),
    Update(JobResult<Option<Update>>),
    Downloaded(JobResult<updater::Prepared>),
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
    status: Message,
    error: Option<Message>,
    busy: Busy,
    worker: Option<JoinHandle<()>>,
    control: Option<Arc<Control>>,
    cancel: Arc<AtomicBool>,
    tx: SyncSender<Event>,
    rx: Receiver<Event>,
    update_worker: Option<JoinHandle<()>>,
    update_busy: bool,
    update: Option<Update>,
    update_message: Message,
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
fn progress(tx: &SyncSender<Event>, ctx: &egui::Context, text: Message) {
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

fn target_label(target: &Target, language: Language) -> String {
    match target {
        Target::Display {
            left,
            top,
            width,
            height,
            name,
        } => {
            // The number is generated metadata; do not translate an OS window title.
            let number = name
                .strip_prefix("Display ")
                .and_then(|v| v.split_whitespace().next())
                .unwrap_or("1");
            Message::with(
                "Display {number} · {width} × {height} at {left}, {top}",
                [
                    ("number", number.into()),
                    ("width", width.to_string()),
                    ("height", height.to_string()),
                    ("left", left.to_string()),
                    ("top", top.to_string()),
                ],
            )
            .render(language)
        }
        Target::Window { name, .. } => name.clone(),
        Target::Region {
            left,
            top,
            width,
            height,
        } => Message::with(
            "Region {left}, {top} · {width} × {height}",
            [
                ("left", left.to_string()),
                ("top", top.to_string()),
                ("width", width.to_string()),
                ("height", height.to_string()),
            ],
        )
        .render(language),
    }
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
                Some(Message::with(
                    "Settings could not be loaded; startup checks disabled for this session: {detail}",
                    [("detail", format!("{e:#}"))],
                )),
            ),
        };
        theme::choose(ctx, settings.theme);
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
            error: error.or(previous_update_error
                .map(|e| Message::with("Previous update failed: {detail}", [("detail", e)]))),
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
                                Message::with(
                                    "Recorded {seconds} s · {frames} frames · Ctrl+Shift+F10 stops",
                                    [
                                        ("seconds", format!("{seconds:.1}")),
                                        ("frames", frames.to_string()),
                                    ],
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
            self.error = Some(Message::with(
                "Choose an absolute .{extension} path for the selected export format",
                [("extension", extension.into())],
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
                        Message::with(
                            "Exporting video · {percent}%",
                            [("percent", format!("{:.0}", value * 100.0))],
                        ),
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
                        Message::with(
                            "Downloading update · {percent}%",
                            [("percent", format!("{:.0}", value * 100.0))],
                        ),
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
                        Err(e) => self.error = Some(e.into()),
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
                                Message::with(
                                    "Loaded {frames} frames · {seconds} s",
                                    [
                                        ("frames", project.frames.len().to_string()),
                                        ("seconds", format!("{:.2}", project.duration())),
                                    ],
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
                        Err(e) => self.error = Some(e.into()),
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
                        Err(e) => self.error = Some(e.into()),
                    }
                }
                Event::Recorded(path, value) => {
                    self.busy = Busy::Idle;
                    self.control = None;
                    self.recording_name = recording_name();
                    self.open_path = path.to_string_lossy().into_owned();
                    match value {
                        Ok(()) => {
                            self.status = Message::with(
                                "Recording saved: {path}",
                                [("path", path.display().to_string())],
                            );
                            if !self.closing {
                                self.load_project(ctx);
                            }
                        }
                        Err(e) => {
                            self.error = Some(Message::with(
                                "{detail}\nAny complete frames can be opened from {path}",
                                [("detail", e), ("path", path.display().to_string())],
                            ));
                            self.status =
                                "Recording stopped with an error; original project retained".into();
                        }
                    }
                }
                Event::Exported(value) => {
                    self.busy = Busy::Idle;
                    match value {
                        Ok(()) => {
                            self.status = Message::with(
                                "Video exported: {path}",
                                [("path", self.export_path.clone())],
                            )
                        }
                        Err(e) => self.error = Some(e.into()),
                    }
                }
                Event::Update(value) => {
                    self.update_busy = false;
                    match value {
                        Ok(Some(update)) => {
                            self.update_message = Message::with(
                                "Version {version} is available",
                                [("version", update.version.to_string())],
                            );
                            self.update = Some(update);
                        }
                        Ok(None) => {
                            self.update_message = "You have the latest stable version".into();
                            self.update = None;
                        }
                        Err(e) => {
                            self.update_message =
                                Message::with("Update check failed: {detail}", [("detail", e)])
                        }
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
                        Ok(mut prepared) => match updater::launch_install(&mut prepared) {
                            Ok(()) => {
                                self.status = "Installing update and restarting…".into();
                                self.closing = true;
                            }
                            Err(e) => self.error = Some(format!("{e:#}").into()),
                        },
                        Err(e) => {
                            self.update_message =
                                Message::with("Update failed: {detail}", [("detail", e)]);
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
        let l = self.settings.language;
        ui.heading(l.text("New recording"));
        ui.label(
            egui::RichText::new(l.text("Choose your canvas. Add sound if you need it."))
                .color(theme::palette(ui).secondary),
        );
        ui.add_space(12.0);
        let editable = self.can_start();
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong(l.text("What would you like to record?"));
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (index, label, icon) in [
                        (0, l.text("Display"), Icon::Display),
                        (1, l.text("Window"), Icon::Window),
                        (2, l.text("Region"), Icon::Region),
                    ] {
                        if controls::choice(ui, icon, label, self.source_mode == index, 124.)
                            .clicked()
                        {
                            self.source_mode = index;
                            self.selected_source = 0;
                        }
                    }
                    if ui.button(l.text("Refresh sources")).clicked() {
                        self.refresh_sources(ui.ctx());
                    }
                });
                if self.source_mode == 2 {
                    ui.horizontal_wrapped(|ui| {
                        for (label, n) in [
                            (l.text("Left"), &mut self.region.0),
                            (l.text("Top"), &mut self.region.1),
                        ] {
                            ui.label(label);
                            ui.add(egui::DragValue::new(n).range(-32768..=32768));
                        }
                        ui.label(l.text("Width"));
                        ui.add(egui::DragValue::new(&mut self.region.2).range(64..=4096));
                        ui.label(l.text("Height"));
                        ui.add(egui::DragValue::new(&mut self.region.3).range(64..=2160));
                    });
                    ui.small(l.text("Physical pixels; negative positions are supported."));
                } else {
                    egui::ComboBox::from_id_salt("source")
                        .width(ui.available_width())
                        .selected_text(
                            self.selected_target().map_or(
                                l.text("Refresh to find a display or window").into(),
                                |t| target_label(&t, l),
                            ),
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
                                ui.selectable_value(
                                    &mut self.selected_source,
                                    index,
                                    target_label(&t, l),
                                );
                            }
                        });
                }
                if self.source_mode == 1 {
                    ui.small(l.text(
                        "Keep the selected window visible. Resizing or minimizing ends recording.",
                    ));
                }
            });
        });
        ui.add_space(8.);
        ui.strong(l.text("Sound"));
        ui.add_enabled_ui(editable, |ui| {
            ui.columns(2, |columns| {
                controls::toggle(
                    &mut columns[0],
                    &mut self.settings.microphone,
                    Icon::Mic,
                    l.text("Microphone"),
                    l.text("Your default input"),
                );
                controls::toggle(
                    &mut columns[1],
                    &mut self.settings.system_audio,
                    Icon::Sound,
                    l.text("System sound"),
                    l.text("Audio playing on your PC"),
                );
            });
            ui.small(if self.settings.microphone || self.settings.system_audio {
                l.text("Sound starts only when you press Record.")
            } else {
                l.text("Sound is off. Your recording will be silent.")
            });
        });
        ui.add_space(8.);
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                controls::section(ui, Icon::Folder, l.text("Save to"));
                if ui.small_button(l.text("Choose folder…")).clicked() && !self.isolated {
                    match crate::dialogs::choose(self.owner, crate::dialogs::Kind::Folder, l) {
                        Ok(Some(path)) => self.settings.output_folder = path,
                        Ok(None) => {}
                        Err(e) => self.error = Some(format!("{e:#}").into()),
                    }
                }
            });
            ui.add_enabled_ui(editable, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.output_folder)
                        .desired_width(f32::INFINITY),
                );
                ui.collapsing(l.text("Filename and quality"), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(l.text("Filename"));
                        ui.text_edit_singleline(&mut self.recording_name);
                        ui.label(l.text("Frames / sec"));
                        ui.add(egui::DragValue::new(&mut self.settings.fps).range(5..=30));
                    });
                });
            });
        });
        ui.add_space(8.);
    }

    fn record_controls_ui(&mut self, ui: &mut egui::Ui) {
        let l = self.settings.language;
        let editable = self.can_start();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let start = ui
                .add_enabled_ui(editable, |ui| {
                    controls::action(ui, Icon::Record, l.text("Start recording"), 190.)
                })
                .inner;
            remember_primary(&start);
            if ui
                .add_enabled(editable, egui::Button::new(l.text("Open recording…")))
                .clicked()
                && !self.isolated
            {
                match crate::dialogs::choose(self.owner, crate::dialogs::Kind::Project, l) {
                    Ok(Some(path)) => {
                        self.open_path = path;
                        self.load_project(ui.ctx());
                    }
                    Ok(None) => {}
                    Err(e) => self.error = Some(format!("{e:#}").into()),
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
                        egui::Button::new(if paused {
                            l.text("Resume")
                        } else {
                            l.text("Pause")
                        }),
                    )
                    .clicked()
                {
                    control.pause(!paused);
                }
                if ui
                    .add_enabled(
                        !control.is_stopped(),
                        egui::Button::new(l.text("Stop and save")),
                    )
                    .clicked()
                {
                    control.stop();
                    self.status = "Finalizing recording…".into();
                }
            }
        });
        ui.small(
            l.text("Ctrl+Shift+F10 stops recording. Pause excludes time from video and sound."),
        );
    }

    fn edit_ui(&mut self, ui: &mut egui::Ui) {
        let l = self.settings.language;
        let (Some(project), Some(mut edit)) = (self.project.clone(), self.edit.take()) else {
            ui.heading(l.text("Your recording, with a clearer focus"));
            ui.label(l.text("Record a clip or open a .srec project from the Record tab."));
            return;
        };
        let can_edit = matches!(self.busy, Busy::Idle | Busy::Preview) && !self.installing;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.heading(l.text("Edit tutorial"));
            ui.label(
                Message::with(
                    "{width} × {height} · {fps} fps · {seconds} s",
                    [
                        ("width", project.width.to_string()),
                        ("height", project.height.to_string()),
                        ("fps", project.fps.to_string()),
                        ("seconds", format!("{:.2}", project.duration())),
                    ],
                )
                .render(l),
            );
        });
        ui.columns(2, |columns| {
            let ui = &mut columns[0];
            theme::panel(ui).show(ui, |ui| {
                ui.strong(l.text("Preview"));
                if let Some(texture) = &self.preview {
                    ui.add(
                        egui::Image::new(texture)
                            .fit_to_exact_size(egui::vec2(
                                ui.available_width(),
                                ui.available_width() * project.height as f32 / project.width as f32,
                            ))
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text(l.text("Scrub the timeline to inspect the edited output"));
                } else {
                    ui.add_space(100.0);
                    ui.label(l.text("Preparing preview…"));
                    ui.add_space(100.0);
                }
                changed |= ui
                    .add_enabled_ui(can_edit, |ui| {
                        theme::slider(
                            ui,
                            egui::Slider::new(&mut self.playhead, 0.0..=project.duration())
                                .text(l.text("seconds"))
                                .fixed_decimals(2),
                        )
                    })
                    .inner
                    .changed();
                ui.small(l.text("Drag the timeline to preview your edits."));
            });
            let ui = &mut columns[1];
            ui.add_enabled_ui(can_edit, |ui| {
                ui.strong(l.text("Trim range"));
                ui.horizontal_wrapped(|ui| {
                    ui.label(l.text("In"));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut edit.trim_start)
                                .speed(0.05)
                                .range(0.0..=project.duration()),
                        )
                        .changed();
                    ui.label(l.text("Out"));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut edit.trim_end)
                                .speed(0.05)
                                .range(0.0..=project.duration()),
                        )
                        .changed();
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button(l.text("In at playhead")).clicked() {
                        edit.trim_start = self.playhead;
                        changed = true;
                    }
                    if ui.small_button(l.text("Out at playhead")).clicked() {
                        edit.trim_end = self.playhead;
                        changed = true;
                    }
                });
                ui.separator();
                ui.strong(l.text("Choose a style"));
                let clean =
                    edit.show_cursor && !edit.highlight && !edit.clicks && !edit.automatic_zoom;
                let guided =
                    edit.show_cursor && edit.highlight && edit.clicks && !edit.automatic_zoom;
                let focus =
                    edit.show_cursor && edit.highlight && edit.clicks && edit.automatic_zoom;
                let width = (ui.available_width() - 16.) / 3.;
                ui.horizontal(|ui| {
                    for (label, icon, selected, highlight, zoom) in [
                        (l.text("Clean"), Icon::Cursor, clean, false, false),
                        (l.text("Guided"), Icon::Click, guided, true, false),
                        (l.text("Focus"), Icon::Zoom, focus, true, true),
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
                    l.text("Cursor emphasis with automatic zoom.")
                } else if guided {
                    l.text("Highlight the cursor and emphasize each click.")
                } else if clean {
                    l.text("Just your recording and a clean cursor.")
                } else {
                    l.text("Custom cursor and zoom settings.")
                });
                ui.collapsing(l.text("Customize effects"), |ui| {
                    changed |= controls::toggle(
                        ui,
                        &mut edit.show_cursor,
                        Icon::Cursor,
                        l.text("Tutorial cursor"),
                        l.text("A clear, consistent pointer"),
                    )
                    .changed();
                    changed |= controls::toggle(
                        ui,
                        &mut edit.highlight,
                        Icon::Cursor,
                        l.text("Cursor highlight"),
                        l.text("Keep the pointer easy to follow"),
                    )
                    .changed();
                    changed |= controls::toggle(
                        ui,
                        &mut edit.clicks,
                        Icon::Click,
                        l.text("Click emphasis"),
                        l.text("Show where each click happens"),
                    )
                    .changed();
                    changed |= controls::toggle(
                        ui,
                        &mut edit.automatic_zoom,
                        Icon::Zoom,
                        l.text("Auto zoom"),
                        l.text("Focus smoothly around clicks"),
                    )
                    .changed();
                });
                ui.separator();
                ui.strong(l.text("Sound"));
                if !self.export_mp4 {
                    ui.small(l.text("AVI exports are silent. Choose MP4 below to include sound."));
                } else if project.audio_tracks.is_empty() {
                    ui.small(l.text("This recording has no audio tracks."));
                } else {
                    controls::toggle(
                        ui,
                        &mut edit.include_audio,
                        Icon::Sound,
                        l.text("Recorded sound"),
                        l.text("Include sound in your video"),
                    );
                    if edit.include_audio {
                        ui.collapsing(l.text("Sound levels"), |ui| {
                            for track in &project.audio_tracks {
                                match track.kind {
                                    screen_recorder::audio::Kind::Microphone => {
                                        theme::slider(
                                            ui,
                                            egui::Slider::new(&mut edit.microphone_gain, 0.0..=2.0)
                                                .text(l.text("Microphone")),
                                        );
                                    }
                                    screen_recorder::audio::Kind::System => {
                                        theme::slider(
                                            ui,
                                            egui::Slider::new(&mut edit.system_gain, 0.0..=2.0)
                                                .text(l.text("System sound")),
                                        );
                                    }
                                }
                            }
                            ui.small(l.text(
                                "Two tracks mix at half gain each. Preview does not play sound.",
                            ));
                        });
                    }
                }
                ui.collapsing(l.text("Edit zoom timing"), |ui| {
                    if ui.button(l.text("Add zoom at playhead")).clicked()
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
                                        ui.strong(
                                            Message::with(
                                                "Zoom {number}",
                                                [("number", (i + 1).to_string())],
                                            )
                                            .render(l),
                                        );
                                        if ui.small_button(l.text("Remove")).clicked() {
                                            remove = Some(i);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(l.text("In/out"));
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
                                        ui.label(l.text("Focus X/Y"));
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
        let l = self.settings.language;
        if self.project.is_none() {
            return;
        }
        let can_edit = matches!(self.busy, Busy::Idle | Busy::Preview) && !self.installing;
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong(l.text("Export"));
            let old = self.export_mp4;
            ui.add_enabled_ui(can_edit, |ui| {
                ui.selectable_value(&mut self.export_mp4, true, l.text("MP4 video"))
                    .on_hover_text(
                        l.text("Video with recorded sound; odd dimensions pad to even pixels"),
                    );
                ui.selectable_value(&mut self.export_mp4, false, l.text("AVI · no sound"))
                    .on_hover_text(l.text("Silent video, limited to 2 GB per export"));
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
                    controls::action(ui, Icon::Export, l.text("Export video"), 174.)
                })
                .inner;
            remember_primary(&export);
            if export.clicked() {
                self.start_export(ui.ctx());
            }
            if ui
                .add_enabled(can_edit, egui::Button::new(l.text("Save edit decisions")))
                .on_hover_text(l.text(
                    "Save trim, zoom and mix for the next session without changing source media",
                ))
                .clicked()
                && let (Some(project), Some(edit)) = (&self.project, &self.edit)
            {
                match edit.save(project) {
                    Ok(()) => self.status = "Edit decisions saved beside the project".into(),
                    Err(e) => self.error = Some(format!("{e:#}").into()),
                }
            }
            if self.busy == Busy::Export && ui.button(l.text("Cancel export")).clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        });
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let l = self.settings.language;
        ui.heading(l.text("Settings"));
        ui.label(
            egui::RichText::new(format!("ScreenRecorder {}", updater::VERSION))
                .color(theme::palette(ui).secondary),
        );
        ui.add_space(12.);
        let before = (self.settings.language, self.settings.theme);
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong(l.text("Appearance and language"));
            ui.horizontal_wrapped(|ui| {
                ui.label(l.text("Language"));
                egui::ComboBox::from_id_salt("interface-language")
                    .selected_text(self.settings.language.native_name())
                    .show_ui(ui, |ui| {
                        for language in Language::ALL {
                            ui.selectable_value(
                                &mut self.settings.language,
                                language,
                                language.native_name(),
                            );
                        }
                    });
                ui.label(l.text("Theme"));
                ui.selectable_value(&mut self.settings.theme, Theme::Light, l.text("Light"));
                ui.selectable_value(&mut self.settings.theme, Theme::Dark, l.text("Dark"));
            });
            ui.small(l.text("Changes apply immediately and are saved automatically."));
        });
        if before != (self.settings.language, self.settings.theme) {
            theme::choose(ui.ctx(), self.settings.theme);
            ui.ctx().request_repaint();
            if !self.isolated
                && let Err(error) = self.settings.save(&self.settings_path)
            {
                self.error = Some(Message::with(
                    "Preference could not be saved: {detail}",
                    [("detail", format!("{error:#}"))],
                ));
            }
        }
        ui.add_space(8.);
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong(l.text("Recording defaults"));
            ui.horizontal(|ui| {
                ui.label(l.text("Save recordings to"));
                if ui.small_button(l.text("Choose folder…")).clicked() && !self.isolated {
                    match crate::dialogs::choose(self.owner, crate::dialogs::Kind::Folder, l) {
                        Ok(Some(path)) => self.settings.output_folder = path,
                        Ok(None) => {}
                        Err(e) => self.error = Some(format!("{e:#}").into()),
                    }
                }
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.output_folder)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.label(l.text("Frames per second"));
                ui.add(egui::DragValue::new(&mut self.settings.fps).range(5..=30));
            });
            ui.columns(2, |columns| {
                controls::toggle(
                    &mut columns[0],
                    &mut self.settings.microphone,
                    Icon::Mic,
                    l.text("Microphone"),
                    l.text("For new recordings"),
                );
                controls::toggle(
                    &mut columns[1],
                    &mut self.settings.system_audio,
                    Icon::Sound,
                    l.text("System sound"),
                    l.text("For new recordings"),
                );
            });
            if ui.button(l.text("Save defaults")).clicked() {
                match self.settings.save(&self.settings_path) {
                    Ok(()) => self.status = "Defaults saved".into(),
                    Err(e) => self.error = Some(format!("{e:#}").into()),
                }
            }
        });
        ui.add_space(8.);
        theme::panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong(l.text("Keep up to date"));
            if controls::toggle(
                ui,
                &mut self.settings.startup_updates,
                Icon::Settings,
                l.text("Check on startup"),
                l.text("Downloads always need your approval"),
            )
            .changed()
                && let Err(e) = self.settings.save(&self.settings_path)
            {
                self.error = Some(Message::with(
                    "Preference could not be saved: {detail}",
                    [("detail", format!("{e:#}"))],
                ));
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.update_busy,
                        egui::Button::new(l.text("Check for updates")),
                    )
                    .clicked()
                {
                    self.check_updates(ui.ctx());
                }
                ui.label(self.update_message.render(l));
            });
            if let Some(update) = &self.update {
                ui.colored_label(
                    theme::palette(ui).success,
                    Message::with(
                        "Version {version} is available",
                        [("version", update.version.to_string())],
                    )
                    .render(l),
                );
                if ui
                    .add_enabled(
                        self.can_start() && !self.update_busy,
                        egui::Button::new(l.text("Download and install (restarts app)")),
                    )
                    .clicked()
                {
                    self.install_update(ui.ctx());
                }
            }
            if self.installing && ui.button(l.text("Cancel download")).clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        });
        ui.add_space(8.);
        ui.collapsing(l.text("About and recording support"),|ui| {
            ui.label(l.text("Records visible displays, windows or regions. Exports MP4 with sound or silent AVI."));
            ui.small(l.text("No webcam or hidden-window capture. Keep the selected window visible."));
            ui.small(updater::REPOSITORY);
            ui.small(Message::with("Settings: {path}", [("path", self.settings_path.display().to_string())]).render(l));
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
        let l = self.settings.language;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::palette(ui).background)
                    .inner_margin(20),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    controls::brand(ui);
                    ui.add_space(20.);
                    for (tab, label, icon) in [
                        (Tab::Record, l.text("Record"), Icon::Record),
                        (Tab::Edit, l.text("Edit"), Icon::Edit),
                        (Tab::Settings, l.text("Settings"), Icon::Settings),
                    ] {
                        if controls::nav(ui, icon, label, self.tab == tab, 112.).clicked() {
                            self.tab = tab;
                        }
                    }
                    if let Some(control) = &self.control {
                        let (label, color) = if control.is_stopped() {
                            (l.text("FINALIZING"), theme::palette(ui).warning)
                        } else if !control.has_frames() {
                            (l.text("STARTING"), theme::palette(ui).warning)
                        } else if control.is_paused() {
                            (l.text("PAUSED"), theme::palette(ui).warning)
                        } else {
                            (l.text("RECORDING"), theme::palette(ui).error)
                        };
                        ui.colored_label(color, label);
                        if self.tab != Tab::Record
                            && ui
                                .add_enabled(
                                    !control.is_stopped(),
                                    egui::Button::new(l.text("Stop")),
                                )
                                .clicked()
                        {
                            control.stop();
                        }
                    }
                    if self.update.is_some() {
                        ui.colored_label(theme::palette(ui).success, l.text("Update available"));
                    }
                });
                ui.separator();
                if let Some(error) = self.error.clone() {
                    theme::panel(ui).show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("error-message")
                            .max_height(88.0)
                            .show(ui, |ui| {
                                ui.strong(l.text("Something went wrong"));
                                if matches!(error, Message::Details(_)) {
                                    ui.collapsing(l.text("Technical details"), |ui| {
                                        ui.colored_label(theme::palette(ui).error, error.render(l));
                                    });
                                } else {
                                    ui.colored_label(theme::palette(ui).error, error.render(l));
                                }
                            });
                        if ui.small_button(l.text("Dismiss")).clicked() {
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
                ui.add(egui::Label::new(self.status.render(l)).truncate())
                    .on_hover_text(self.status.render(l));
            });
    }
}
