//! Finite CPU-only rendering of the actual app components, without a window or devices.
//! The shared 0.33 bridge transports only numeric meshes,
//! textures and clipping to a cached CPU rasterizer; all UI/layout/fonts are egui 0.37.
use super::*;
use crate::raster::{meshes, textures};
use anyhow::{Context, Result, ensure};
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use screen_recorder::{
    audio::{Kind, Packet, TrackWriter},
    project::{Cursor, ProjectWriter},
};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

struct Scene {
    name: &'static str,
    tab: Tab,
    width: u32,
    height: u32,
    scale: f32,
    error: bool,
}
const SCENES: &[Scene] = &[
    Scene {
        name: "record",
        tab: Tab::Record,
        width: 1120,
        height: 800,
        scale: 1.0,
        error: false,
    },
    Scene {
        name: "edit",
        tab: Tab::Edit,
        width: 1120,
        height: 800,
        scale: 1.0,
        error: false,
    },
    Scene {
        name: "settings",
        tab: Tab::Settings,
        width: 1120,
        height: 800,
        scale: 1.0,
        error: false,
    },
    Scene {
        name: "record-error",
        tab: Tab::Record,
        width: 880,
        height: 650,
        scale: 1.5,
        error: true,
    },
    Scene {
        name: "edit-narrow",
        tab: Tab::Edit,
        width: 880,
        height: 650,
        scale: 1.0,
        error: false,
    },
    Scene {
        name: "settings-scale",
        tab: Tab::Settings,
        width: 880,
        height: 720,
        scale: 1.5,
        error: false,
    },
];

fn fixture(root: &Path) -> Result<Arc<Project>> {
    let path = root.join("fixture.srec");
    let mut writer = ProjectWriter::create(&path, 640, 360, 15)?;
    for i in 0..16 {
        let image = image::RgbImage::from_fn(640, 360, |x, y| {
            let c = if !(36..604).contains(&x) || !(28..332).contains(&y) {
                [231, 235, 242]
            } else if y < 58 {
                [247, 249, 252]
            } else if x < 158 {
                [239, 243, 249]
            } else if (182..554).contains(&x)
                && ((86..101).contains(&y) || (130..141).contains(&y) || (172..183).contains(&y))
            {
                [202, 211, 226]
            } else if (182..322).contains(&x) && (230..268).contains(&y) {
                [49, 84, 220]
            } else {
                [255, 255, 255]
            };
            image::Rgb(c)
        });
        writer.append(
            &image,
            i * 500_000,
            Cursor {
                x: 0.5,
                y: 0.52,
                visible: true,
                click: i == 6,
            },
        )?;
    }
    writer.finish()?;
    for kind in [Kind::Microphone, Kind::System] {
        let mut audio = TrackWriter::create(&kind.path(&path))?;
        audio.append(&Packet {
            start: 0,
            samples: vec![[0.0; 2]; 4800],
        })?;
        audio.finish()?;
    }
    Ok(Arc::new(Project::open(&path)?))
}

pub(super) fn native_fixture(ctx: &egui::Context, root: &Path) -> Result<RecorderApp> {
    app(ctx, root, fixture(root)?, &SCENES[1])
}

fn app(
    ctx: &egui::Context,
    root: &Path,
    project: Arc<Project>,
    scene: &Scene,
) -> Result<RecorderApp> {
    theme::apply(ctx);
    let mut edit = Edit::new(&project);
    edit.trim_start = 0.5;
    edit.trim_end = 6.5;
    edit.zooms.push(Zoom {
        start: 2.0,
        end: 4.5,
        x: 0.5,
        y: 0.52,
        scale: 1.8,
    });
    let time = 3.2;
    let frame = project.read_frame(project.index_at(time))?;
    let preview = editor::render(
        &project,
        &edit,
        &editor::automatic_zooms(&project),
        &frame,
        time,
        640,
        360,
    );
    let texture = ctx.load_texture(
        "edited preview",
        egui::ColorImage::from_rgb([640, 360], preview.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    let (tx, rx) = mpsc::sync_channel(8);
    Ok(RecorderApp{
        isolated:true,owner:0,
        settings:Settings{startup_updates:false,output_folder:root.join("Recordings").to_string_lossy().into_owned(),fps:15,microphone:true,system_audio:true},
        settings_path:root.join("settings.json"),tab:scene.tab,
        sources:vec![Target::Display{left:0,top:0,width:1920,height:1080,name:"Display 1 · 1920 × 1080 (synthetic fixture)".into()}],
        source_mode:0,selected_source:0,region:(100,100,1280,720),recording_name:"Tutorial.srec".into(),
        open_path:project.path.to_string_lossy().into_owned(),export_path:root.join("Tutorial-edited.mp4").to_string_lossy().into_owned(),export_mp4:true,
        project:Some(project),edit:Some(edit),playhead:time,preview:Some(texture),preview_dirty:false,preview_generation:0,
        status:if scene.error{"Recording stopped. Completed frames have been retained."}else{"Ready to record"}.into(),
        error:scene.error.then(||"Microphone could not start. Check microphone access in Windows and choose a new recording filename. Completed video and audio packets remain available.".into()),
        busy:Busy::Idle,worker:None,control:None,cancel:Arc::new(AtomicBool::new(false)),tx,rx,
        update_worker:None,update_busy:false,update:None,update_message:"No check performed".into(),installing:false,closing:false,
    })
}

pub(crate) fn run(root: &Path) -> Result<()> {
    ensure!(
        root.is_absolute(),
        "Render output directory must be absolute"
    );
    fs::create_dir(root).context(
        "Choose a new render directory with an existing parent; existing output is preserved",
    )?;
    let started = Instant::now();
    let project = fixture(root)?;
    let mut records = Vec::new();
    for scene in SCENES {
        ensure!(
            started.elapsed() < Duration::from_secs(30),
            "Offscreen review exceeded its 30-second deadline"
        );
        let ctx = egui::Context::default();
        let mut app = app(&ctx, root, project.clone(), scene)?;
        let mut renderer = EguiSoftwareRender::new(ColorFieldOrder::Rgba);
        let width = (scene.width as f32 * scene.scale).round() as usize;
        let height = (scene.height as f32 * scene.scale).round() as usize;
        let mut buffer = vec![[0u8; 4]; width * height];
        let scene_start = Instant::now();
        let mut triangles = 0;
        // Three finite passes settle font atlases and egui layout; no OS event loop exists.
        for pass in 0..3 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(scene.width as f32, scene.height as f32),
                )),
                time: Some(pass as f64 * 0.5),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(scene.scale);
            let mut output = ctx.run_ui(input, |ui| app.show(ui));
            ensure!(
                (output.pixels_per_point - scene.scale).abs() < 0.001,
                "Unexpected render scale"
            );
            let primitives = meshes(ctx.tessellate(output.shapes, output.pixels_per_point))?;
            triangles = primitives
                .iter()
                .map(|p| match &p.primitive {
                    egui_compat::epaint::Primitive::Mesh(m) => m.indices.len() / 3,
                    _ => 0,
                })
                .sum::<usize>();
            renderer.render(
                &mut BufferMutRef::new(&mut buffer, width, height),
                &primitives,
                &textures(&mut output.textures_delta),
                output.pixels_per_point,
            );
            ensure!(
                app.worker.is_none() && app.update_worker.is_none() && app.control.is_none(),
                "Review unexpectedly started background work"
            );
        }
        let raw: Vec<u8> = buffer.into_iter().flatten().collect();
        let primary =
            ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::unique(PRIMARY_ACTION_BOUNDS)));
        let body =
            ctx.data(|data| data.get_temp::<[f32; 4]>(egui::Id::unique(BODY_SCROLL_METRICS)));
        if scene.tab != Tab::Settings {
            let rect = primary.context("Primary action was not laid out")?;
            ensure!(
                egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(scene.width as f32, scene.height as f32)
                )
                .contains_rect(rect),
                "Primary action is clipped in {}",
                scene.name
            );
        }
        let image = image::RgbaImage::from_raw(width as u32, height as u32, raw)
            .context("Invalid CPU bitmap")?;
        ensure!(
            triangles > 100,
            "Review rendered an unexpectedly empty scene"
        );
        let target = root.join(format!("{}.png", scene.name));
        let temp = root.join(format!("{}.png.tmp", scene.name));
        image.save_with_format(&temp, image::ImageFormat::Png)?;
        fs::rename(temp, &target)?;
        records.push(serde_json::json!({"screen":scene.name,"logical_size":[scene.width,scene.height],"pixel_size":[width,height],"scale":scene.scale,"passes":3,"triangles":triangles,"primary_action_bounds":primary.map(|r|[r.min.x,r.min.y,r.max.x,r.max.y]),"scroll_content_and_viewport":body,"elapsed_ms":scene_start.elapsed().as_millis(),"png":target}));
    }
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"version":updater::VERSION,"mode":"offscreen CPU rendering; actual egui 0.37 application components; no window/event loop", "production_presenter":"same CPU rasterizer; softbuffer Win32 GDI presentation tested separately","fixtures":"synthetic video and silent audio only","elapsed_ms":started.elapsed().as_millis(),"scenes":records}),
        )?,
    )?;
    Ok(())
}
