//! Numeric adapter shared by the production CPU renderer and finite render review.
use anyhow::Result;
use std::sync::Arc;
fn id(id: egui::TextureId) -> egui_compat::TextureId {
    match id {
        egui::TextureId::Managed(n) => egui_compat::TextureId::Managed(n),
        egui::TextureId::User(n) => egui_compat::TextureId::User(n),
    }
}
fn color(c: egui::Color32) -> egui_compat::Color32 {
    let [r, g, b, a] = c.to_array();
    egui_compat::Color32::from_rgba_premultiplied(r, g, b, a)
}
fn filter(f: egui::TextureFilter) -> egui_compat::TextureFilter {
    match f {
        egui::TextureFilter::Nearest => egui_compat::TextureFilter::Nearest,
        egui::TextureFilter::Linear => egui_compat::TextureFilter::Linear,
    }
}
pub(crate) fn textures(delta: &egui::TexturesDelta) -> egui_compat::TexturesDelta {
    egui_compat::TexturesDelta {
        set: delta
            .set
            .iter()
            .flat_map(|(texture, changes)| changes.iter().map(move |change| (texture, change)))
            .map(|(texture, change)| {
                let egui::ImageData::Color(image) = &change.image;
                let options = egui_compat::TextureOptions {
                    magnification: filter(change.options.magnification),
                    minification: filter(change.options.minification),
                    wrap_mode: match change.options.wrap_mode {
                        egui::TextureWrapMode::ClampToEdge => {
                            egui_compat::TextureWrapMode::ClampToEdge
                        }
                        egui::TextureWrapMode::Repeat => egui_compat::TextureWrapMode::Repeat,
                        egui::TextureWrapMode::MirroredRepeat => {
                            egui_compat::TextureWrapMode::MirroredRepeat
                        }
                    },
                    mipmap_mode: change.options.mipmap_mode.map(filter),
                };
                (
                    id(*texture),
                    egui_compat::epaint::ImageDelta {
                        image: egui_compat::ImageData::Color(Arc::new(
                            egui_compat::ColorImage::new(
                                image.size,
                                image.pixels.iter().copied().map(color).collect(),
                            ),
                        )),
                        options,
                        pos: change.pos,
                    },
                )
            })
            .collect(),
        free: delta.free.iter().copied().map(id).collect(),
    }
}
pub(crate) fn meshes(
    primitives: Vec<egui::ClippedPrimitive>,
) -> Result<Vec<egui_compat::ClippedPrimitive>> {
    primitives
        .into_iter()
        .map(|p| {
            let egui::epaint::Primitive::Mesh(mesh) = p.primitive else {
                anyhow::bail!("CPU renderer cannot render a GPU callback");
            };
            Ok(egui_compat::ClippedPrimitive {
                clip_rect: egui_compat::Rect::from_min_max(
                    egui_compat::pos2(p.clip_rect.min.x, p.clip_rect.min.y),
                    egui_compat::pos2(p.clip_rect.max.x, p.clip_rect.max.y),
                ),
                primitive: egui_compat::epaint::Primitive::Mesh(egui_compat::Mesh {
                    indices: mesh.indices,
                    vertices: mesh
                        .vertices
                        .into_iter()
                        .map(|v| egui_compat::epaint::Vertex {
                            pos: egui_compat::pos2(v.pos.x, v.pos.y),
                            uv: egui_compat::pos2(v.uv.x, v.uv.y),
                            color: color(v.color),
                        })
                        .collect(),
                    texture_id: id(mesh.texture_id),
                }),
            })
        })
        .collect()
}
