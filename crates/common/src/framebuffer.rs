/// RGBA8 framebuffer - the bridge between emulation and display.
/// Each system renders into this; only the frontend knows about GPU textures.
pub struct FrameBuffer {
    pub width: u32,
    pub height: u32,
    /// Pixel data in RGBA8 format, row-major, top-left origin.
    /// Length = width * height * 4.
    pub pixels: Vec<u8>,
}

impl FrameBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    /// Set a single pixel. Coordinates are bounds-checked in debug mode.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8) {
        debug_assert!(x < self.width && y < self.height);
        let offset = ((y * self.width + x) * 4) as usize;
        self.pixels[offset] = r;
        self.pixels[offset + 1] = g;
        self.pixels[offset + 2] = b;
        self.pixels[offset + 3] = 255;
    }

    /// Set a pixel from a packed 0x00RRGGBB u32.
    #[inline]
    pub fn set_pixel_rgb(&mut self, x: u32, y: u32, rgb: u32) {
        let r = ((rgb >> 16) & 0xFF) as u8;
        let g = ((rgb >> 8) & 0xFF) as u8;
        let b = (rgb & 0xFF) as u8;
        self.set_pixel(x, y, r, g, b);
    }
}
