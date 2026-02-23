use egui::{ColorImage, TextureHandle, TextureOptions};
use emu_common::FrameBuffer;

/// Render the emulation screen: display the framebuffer as a texture.
/// `aspect_ratio` is the correct display aspect ratio (width/height),
/// which may differ from the framebuffer pixel ratio (e.g. 4:3 for CRT systems).
pub fn render(
    ui: &mut egui::Ui,
    texture: &mut Option<TextureHandle>,
    framebuffer: &FrameBuffer,
    aspect_ratio: f32,
) {
    let image = ColorImage::from_rgba_unmultiplied(
        [framebuffer.width as usize, framebuffer.height as usize],
        &framebuffer.pixels,
    );

    match texture {
        Some(tex) => tex.set(image, TextureOptions::NEAREST),
        None => {
            *texture = Some(ui.ctx().load_texture("emu_screen", image, TextureOptions::NEAREST));
        }
    }

    if let Some(tex) = texture {
        let available = ui.available_size();
        let aspect = aspect_ratio;

        let (w, h) = if available.x / available.y > aspect {
            (available.y * aspect, available.y)
        } else {
            (available.x, available.x / aspect)
        };

        ui.centered_and_justified(|ui| {
            ui.image(egui::load::SizedTexture::new(tex.id(), egui::vec2(w, h)));
        });
    }
}
