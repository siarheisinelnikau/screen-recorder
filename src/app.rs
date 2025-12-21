use eframe::egui::{self, Color32, RichText};
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
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        ctx.set_visuals(egui::Visuals::dark());
        ctx.style_mut_of(egui::Theme::Dark, |s| {
            s.spacing.item_spacing = egui::vec2(10.0, 10.0);
            s.spacing.button_padding = egui::vec2(14.0, 9.0);
            s.visuals.selection.bg_fill = Color32::from_rgb(28, 111, 119);
            s.visuals.panel_fill = Color32::from_rgb(19, 25, 33);
        });
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
            project: None,
            edit: None,
            playhead: 0.0,
            preview: None,
            preview_dirty: false,
            preview_generation: 0,
            status: "Ready · no capture is active".into(),
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
                    recorder::record(
                        &mut source,
                        &path,
                        fps,
                        &control,
                        &wake,
                        |frames, seconds| {
                            progress(
                                &tx,
                                &ctx,
                                format!(
                                    "RECORDING · {seconds:.1} s · {frames} frames · Ctrl+Shift+F10 stops"
                                ),
                            );
                        },
                    )?;
                }
                #[cfg(not(windows))]
                {
                    let _ = (target, fps, control, wake);
                    anyhow::bail!("Capture is available on Windows only");
                }
                Ok(())
            })();
            send(&tx, &ctx, Event::Recorded(path, result(work)));
        }));
    }
    fn load_project(&mut self, ctx: &egui::Context) {
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
        let (Some(project), Some(edit)) = (self.project.clone(), self.edit.clone()) else {
            return;
        };
        let path = PathBuf::from(self.export_path.trim());
        if path.extension().is_none_or(|x| x != "avi") || !path.is_absolute() {
            self.error = Some("Export destination must be an absolute .avi path".into());
            return;
        }
        self.busy = Busy::Export;
        self.error = None;
        self.cancel.store(false, Ordering::Relaxed);
        let cancel = self.cancel.clone();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.status = "Exporting AVI…".into();
        self.worker = Some(std::thread::spawn(move || {
            let last = std::cell::Cell::new(std::time::Instant::now());
            let work = editor::export(&project, &edit, &path, &cancel, |value| {
                if last.get().elapsed().as_millis() >= 250 || value >= 1.0 {
                    progress(&tx, &ctx, format!("Exporting AVI · {:.0}%", value * 100.0));
                    last.set(std::time::Instant::now());
                }
            });
            send(&tx, &ctx, Event::Exported(result(work)));
        }));
    }
    fn check_updates(&mut self, ctx: &egui::Context) {
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
                                    "{}-edited.avi",
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
                        Ok(()) => self.status = format!("AVI exported: {}", self.export_path),
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
        ui.heading("Make the next step easy to follow");
        ui.label("Choose exactly what to record. Add focus and cursor effects afterwards.");
        ui.add_space(10.0);
        let editable = self.can_start();
        egui::Frame::group(ui.style()).show(ui,|ui| {
            ui.strong("01   Capture source");
            ui.add_enabled_ui(editable,|ui| {
                ui.horizontal(|ui| {
                    for (i,label) in ["Display","Window","Region"].iter().enumerate() {
                        if ui.selectable_value(&mut self.source_mode,i,*label).changed() {self.selected_source=0;}
                    }
                    if ui.button("Refresh sources").clicked() {self.refresh_sources(ui.ctx());}
                });
                if self.source_mode==2 {
                    ui.horizontal(|ui| {
                        for (label,n) in [("Left",&mut self.region.0),("Top",&mut self.region.1)] {
                            ui.label(label);ui.add(egui::DragValue::new(n).range(-32768..=32768));
                        }
                        ui.label("Width");ui.add(egui::DragValue::new(&mut self.region.2).range(64..=4096));
                        ui.label("Height");ui.add(egui::DragValue::new(&mut self.region.3).range(64..=2160));
                    });
                    ui.small("Region coordinates use physical pixels in the virtual desktop. Negative origins are supported.");
                } else {
                    egui::ComboBox::from_id_salt("source").width(650.0)
                        .selected_text(self.selected_target().map_or("Refresh sources, then select…".into(),|t|t.label()))
                        .show_ui(ui,|ui| {
                            for (index,t) in self.sources.iter().filter(|t|matches!((self.source_mode,t),(0,Target::Display{..})|(1,Target::Window{..}))).enumerate() {
                                ui.selectable_value(&mut self.selected_source,index,t.label());
                            }
                        });
                }
                ui.small("Window mode follows its visible rectangle. Keep it unoccluded and do not resize or minimize it.");
            });
        });
        egui::Frame::group(ui.style()).show(ui,|ui| {
            ui.strong("02   Recording destination");
            ui.add_enabled_ui(editable,|ui| {
                ui.label("Output folder (created when you start recording)");
                ui.add(egui::TextEdit::singleline(&mut self.settings.output_folder).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {ui.label("Filename");ui.text_edit_singleline(&mut self.recording_name);
                    ui.label("FPS");ui.add(egui::DragValue::new(&mut self.settings.fps).range(5..=30));});
                ui.horizontal(|ui| {
                    let mut disabled=false;
                    ui.add_enabled(false,egui::Checkbox::new(&mut disabled,"Microphone (unavailable)"));
                    ui.add_enabled(false,egui::Checkbox::new(&mut disabled,"System audio (unavailable)"));
                });
                ui.small("Version 0.1 records silent video. No audio devices are opened. Sources are limited to 4096 × 2160.");
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    editable,
                    egui::Button::new(RichText::new("●  Start recording").strong())
                        .fill(Color32::from_rgb(25, 106, 115)),
                )
                .clicked()
            {
                self.start_recording(ui.ctx());
            }
            if let Some(control) = &self.control {
                let paused = control.is_paused();
                if ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
                    control.pause(!paused);
                }
                if ui.button("Stop and save").clicked() {
                    control.stop();
                    self.status = "Finalizing recording…".into();
                }
                ui.colored_label(
                    if paused {
                        Color32::YELLOW
                    } else {
                        Color32::LIGHT_RED
                    },
                    if paused {
                        "● PAUSED"
                    } else {
                        "● RECORDING"
                    },
                );
            }
        });
        ui.small("Ctrl+Shift+F10 stops an active recording. No background cursor tracking runs before Start or while paused.");
        ui.add_space(12.0);
        ui.separator();
        ui.strong("Continue an existing recording");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.open_path)
                    .hint_text("Absolute path to a .srec file")
                    .desired_width(ui.available_width() - 145.0),
            );
            if ui
                .add_enabled(editable, egui::Button::new("Open project"))
                .clicked()
            {
                self.load_project(ui.ctx());
            }
        });
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
        ui.columns(2,|columns| {
            let ui=&mut columns[0];
            egui::Frame::group(ui.style()).show(ui,|ui| {
                ui.strong("Preview");
                if let Some(texture)=&self.preview {
                    ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(ui.available_width(),ui.available_width()*project.height as f32/project.width as f32)).sense(egui::Sense::click()))
                        .on_hover_text("Scrub the timeline to inspect the edited output");
                } else {ui.add_space(100.0);ui.label("Preparing preview…");ui.add_space(100.0);}
                changed|=ui.add_enabled(can_edit,egui::Slider::new(&mut self.playhead,0.0..=project.duration()).text("seconds").fixed_decimals(2)).changed();
                ui.small("Preview and export use the same effects. Source frames remain unchanged.");
            });
            let ui=&mut columns[1];
            ui.add_enabled_ui(can_edit,|ui| {
                ui.strong("Trim range");
                ui.horizontal(|ui| {
                    ui.label("In");changed|=ui.add(egui::DragValue::new(&mut edit.trim_start).speed(0.05).range(0.0..=project.duration())).changed();
                    if ui.small_button("Set at playhead").clicked(){edit.trim_start=self.playhead;changed=true;}
                });
                ui.horizontal(|ui| {
                    ui.label("Out");changed|=ui.add(egui::DragValue::new(&mut edit.trim_end).speed(0.05).range(0.0..=project.duration())).changed();
                    if ui.small_button("Set at playhead").clicked(){edit.trim_end=self.playhead;changed=true;}
                });
                ui.separator();ui.strong("Cursor and focus");
                changed|=ui.checkbox(&mut edit.show_cursor,"Draw tutorial cursor").changed();
                changed|=ui.checkbox(&mut edit.highlight,"Highlight cursor").changed();
                changed|=ui.checkbox(&mut edit.clicks,"Emphasize clicks").changed();
                changed|=ui.checkbox(&mut edit.automatic_zoom,"Automatic zoom around clicks").changed();
                ui.small("Automatic zoom uses 1.8× with smooth entry and exit. Manual segments override it.");
                if ui.button("Add zoom at playhead").clicked() && self.playhead<project.duration() && edit.zooms.len()<2000 {
                    let cursor=project.frames[project.index_at(self.playhead)].cursor;
                    edit.zooms.push(Zoom{start:self.playhead,end:(self.playhead+2.0).min(project.duration()),x:cursor.x,y:cursor.y,scale:1.8});changed=true;
                }
                egui::ScrollArea::vertical().max_height(175.0).id_salt("zooms").show(ui,|ui| {
                    let mut remove=None;
                    for (i,zoom) in edit.zooms.iter_mut().enumerate() {
                        ui.push_id(i,|ui| {
                            ui.horizontal(|ui| {ui.strong(format!("Zoom {}",i+1));if ui.small_button("Remove").clicked(){remove=Some(i);}});
                            ui.horizontal(|ui| {
                                ui.label("In/out");changed|=ui.add(egui::DragValue::new(&mut zoom.start).speed(0.05).range(0.0..=project.duration())).changed();
                                changed|=ui.add(egui::DragValue::new(&mut zoom.end).speed(0.05).range(0.0..=project.duration())).changed();
                                ui.label("×");changed|=ui.add(egui::DragValue::new(&mut zoom.scale).speed(0.05).range(1.0..=3.0)).changed();
                            });
                            ui.horizontal(|ui| {ui.label("Focus X/Y");
                                changed|=ui.add(egui::DragValue::new(&mut zoom.x).speed(0.01).range(0.0..=1.0)).changed();
                                changed|=ui.add(egui::DragValue::new(&mut zoom.y).speed(0.01).range(0.0..=1.0)).changed();
                            });
                        });
                    }
                    if let Some(index)=remove {edit.zooms.remove(index);changed=true;}
                });
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_edit, egui::Button::new("Save edit decisions"))
                .clicked()
            {
                match edit.save(&project) {
                    Ok(()) => self.status = "Edit decisions saved beside the project".into(),
                    Err(e) => self.error = Some(format!("{e:#}")),
                }
            }
            ui.small("Trim and zoom decisions are saved separately as .edit.json.");
        });
        ui.label("AVI output (new file only)");
        ui.add_enabled(
            can_edit,
            egui::TextEdit::singleline(&mut self.export_path).desired_width(f32::INFINITY),
        );
        self.edit = Some(edit);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.can_start(), egui::Button::new("Export edited AVI"))
                .clicked()
            {
                self.start_export(ui.ctx());
            }
            if self.busy == Busy::Export && ui.button("Cancel export").clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
            ui.small("Motion JPEG · silent · original resolution · 2 GB per export");
        });
        if changed {
            self.mark_preview(ui.ctx());
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.label(format!(
            "ScreenRecorder {} · Windows x64 portable",
            updater::VERSION
        ));
        ui.add_space(8.0);
        egui::Frame::group(ui.style()).show(ui,|ui| {
            ui.strong("Updates");
            if ui.checkbox(&mut self.settings.startup_updates,"Check for updates on startup").changed()
                && let Err(e)=self.settings.save(&self.settings_path) {
                self.error=Some(format!("Startup preference could not be saved: {e:#}"));
            }
            ui.small("Only checks release metadata. Download and installation require the action below.");
            if ui.add_enabled(!self.update_busy,egui::Button::new("Check for updates")).clicked() {self.check_updates(ui.ctx());}
            ui.label(&self.update_message);
            if let Some(update)=&self.update {
                ui.colored_label(Color32::LIGHT_GREEN,format!("Available: {}",update.version));
                if ui.add_enabled(self.can_start() && !self.update_busy,egui::Button::new("Download and install (restarts app)")).clicked() {self.install_update(ui.ctx());}
            }
            if self.installing && ui.button("Cancel download").clicked() {self.cancel.store(true,Ordering::Relaxed);}
            ui.small(updater::REPOSITORY);
            ui.small("The planned repository is not yet published. An unavailable release is reported as an error, never as up to date.");
        });
        ui.add_space(10.0);
        ui.strong("Recording defaults");
        ui.label("Default output folder");
        ui.text_edit_singleline(&mut self.settings.output_folder);
        ui.horizontal(|ui| {
            ui.label("Frames per second");
            ui.add(egui::DragValue::new(&mut self.settings.fps).range(5..=30));
        });
        if ui.button("Save defaults").clicked() {
            match self.settings.save(&self.settings_path) {
                Ok(()) => self.status = "Defaults saved".into(),
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        ui.small(format!("Settings: {}", self.settings_path.display()));
        ui.separator();
        ui.strong("This first version");
        ui.label("GDI visible-area capture. Silent .srec projects and Motion JPEG AVI exports. No MP4, audio, webcam, hidden-window capture or protected content support.");
        ui.label("Capture, hardware compatibility, real UI screenshots and update replacement require manual validation before a public release.");
    }
}

impl eframe::App for RecorderApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("SCREEN / RECORDER")
                        .size(23.0)
                        .strong()
                        .color(Color32::from_rgb(122, 222, 216)),
                );
                ui.add_space(22.0);
                ui.selectable_value(&mut self.tab, Tab::Record, "Record");
                ui.selectable_value(&mut self.tab, Tab::Edit, "Edit");
                ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
                if let Some(control) = &self.control {
                    let paused = control.is_paused();
                    ui.colored_label(
                        if paused {
                            Color32::YELLOW
                        } else {
                            Color32::LIGHT_RED
                        },
                        if paused {
                            "● PAUSED"
                        } else {
                            "● RECORDING"
                        },
                    );
                    if ui.small_button("Stop").clicked() {
                        control.stop();
                    }
                }
                if self.update.is_some() {
                    ui.colored_label(Color32::LIGHT_GREEN, "Update available");
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match self.tab {
                        Tab::Record => self.record_ui(ui),
                        Tab::Edit => self.edit_ui(ui),
                        Tab::Settings => self.settings_ui(ui),
                    }
                    ui.add_space(10.0);
                    ui.separator();
                    ui.label(&self.status);
                    if let Some(error) = self.error.clone() {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.colored_label(Color32::LIGHT_RED, error);
                            if ui.small_button("Dismiss").clicked() {
                                self.error = None;
                            }
                        });
                    }
                });
        });
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(control) = &self.control {
            control.stop();
        }
        self.cancel.store(true, Ordering::Relaxed);
        // Normal close waits through logic until the recording/export worker has finalized.
    }
}
