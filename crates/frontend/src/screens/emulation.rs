use egui::{ColorImage, TextureHandle, TextureOptions};
use emu_common::FrameBuffer;

/// Render the emulation screen: display the framebuffer as a texture.
pub fn render(
    ui: &mut egui::Ui,
    texture: &mut Option<TextureHandle>,
    framebuffer: &FrameBuffer,
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
        let aspect = framebuffer.width as f32 / framebuffer.height as f32;

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
