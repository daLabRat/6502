use emu_common::FrameBuffer;
use crate::snapshot::{AudioChannelSnapshot, TiaSnapshot};

/// TIA (Television Interface Adaptor) - the heart of the Atari 2600.
/// No framebuffer in hardware - "races the beam" to generate display.
/// We emulate at the scanline/pixel level.

/// NTSC: 228 color clocks per scanline, 262 scanlines per frame.
pub const CLOCKS_PER_SCANLINE: u16 = 228;
pub const VISIBLE_WIDTH: u32 = 160;
pub const VISIBLE_HEIGHT: u32 = 192;
pub const SCREEN_WIDTH: u32 = 160;
pub const SCREEN_HEIGHT: u32 = 192;

/// NTSC color clock rate.
const TIA_CLOCK_RATE: f64 = 3_579_545.0;
/// Color clocks per audio tick (half a scanline).
const AUDIO_CLOCK_DIVISOR: u16 = 114;

/// NTSC TIA color palette (128 colors).
static TIA_PALETTE: [u32; 128] = [
    0x000000, 0x404040, 0x6C6C6C, 0x909090, 0xB0B0B0, 0xC8C8C8, 0xDCDCDC, 0xF4F4F4,
    0x004444, 0x106464, 0x248484, 0x34A0A0, 0x40B8B8, 0x50D0D0, 0x5CE8E8, 0x68FCFC,
    0x002870, 0x144484, 0x285C98, 0x3C78AC, 0x4C8CBC, 0x5CA0CC, 0x68B4DC, 0x78C8EC,
    0x001884, 0x183498, 0x3050AC, 0x4868C0, 0x5C80D0, 0x7094E0, 0x80A8EC, 0x94BCFC,
    0x000088, 0x20209C, 0x3C3CB0, 0x5858C0, 0x7070D0, 0x8888E0, 0xA0A0EC, 0xB4B4FC,
    0x5C0078, 0x74208C, 0x883CA0, 0x9C58B0, 0xB070C0, 0xC084D0, 0xD09CDC, 0xE0B0EC,
    0x780048, 0x902060, 0xA43C78, 0xB8588C, 0xCC70A0, 0xDC84B4, 0xEC9CC4, 0xFCB0D4,
    0x840014, 0x982030, 0xAC3C4C, 0xC05868, 0xD07080, 0xE08894, 0xECA0A8, 0xFCB4BC,
    0x880000, 0x9C2020, 0xB03C3C, 0xC05858, 0xD07070, 0xE08888, 0xECA0A0, 0xFCB4B4,
    0x7C1800, 0x903C18, 0xA45C34, 0xB8784C, 0xCC8C68, 0xDCA080, 0xECB494, 0xFCC8AC,
    0x682800, 0x804818, 0x946834, 0xA8844C, 0xBC9C68, 0xCCB080, 0xDCC494, 0xECD8AC,
    0x504000, 0x6C5C18, 0x887C34, 0xA0984C, 0xB4B068, 0xC8C480, 0xDCD894, 0xECECAC,
    0x343800, 0x505C18, 0x6C7C34, 0x849C4C, 0x9CB468, 0xB0CC80, 0xC4E094, 0xD4F4AC,
    0x183C00, 0x306018, 0x4C8434, 0x68A44C, 0x80C068, 0x94D880, 0xA8EC94, 0xBCFCAC,
    0x003C00, 0x205C20, 0x407C40, 0x5C9C5C, 0x74B874, 0x8CD08C, 0xA4E4A4, 0xB8FCB8,
    0x003814, 0x1C5C34, 0x387C50, 0x50986C, 0x68B484, 0x7CCC9C, 0x90E4B0, 0xA4FCC4,
];

/// NUSIZ copy offsets for each mode (bits 0-2).
fn copy_offsets(mode: u8) -> &'static [u8] {
    match mode & 0x07 {
        0b000 => &[0],              // one copy
        0b001 => &[0, 16],          // two copies, close
        0b010 => &[0, 32],          // two copies, medium
        0b011 => &[0, 16, 32],      // three copies, close
        0b100 => &[0],              // one copy, double-size
        0b101 => &[0, 64],          // two copies, wide
        0b110 => &[0, 32, 64],      // three copies, medium
        0b111 => &[0],              // one copy, quad-size
        _ => &[0],
    }
}

/// Player pixel width for each NUSIZ mode.
fn player_width(mode: u8) -> u8 {
    match mode & 0x07 {
        0b100 => 2, // double-size
        0b111 => 4, // quad-size
        _ => 1,     // normal
    }
}

/// Decode HMOVE signed nibble: high nibble of byte as 4-bit two's complement.
fn decode_hm(val: u8) -> i8 {
    (val as i8) >> 4 // arithmetic right shift sign-extends
}

// ── Audio ──────────────────────────────────────────────────────────

/// One TIA audio channel.
struct AudioChannel {
    // Registers
    audc: u8,   // Control / waveform select (0-15)
    audf: u8,   // Frequency divider (0-31)
    audv: u8,   // Volume (0-15)

    // Internal state
    freq_counter: u8,

    // Polynomial shift registers
    poly4: u8,   // 4-bit LFSR, period 15
    poly5: u8,   // 5-bit LFSR, period 31
    poly9: u16,  // 9-bit LFSR, period 511

    // Division counter for modes that need sub-division
    div_counter: u8,

    // Current output bit
    output: bool,
}

impl AudioChannel {
    fn new() -> Self {
        Self {
            audc: 0, audf: 0, audv: 0,
            freq_counter: 0,
            poly4: 0x0F, poly5: 0x1F, poly9: 0x1FF,
            div_counter: 0,
            output: false,
        }
    }

    /// Step one audio clock (~31.4 kHz). Called every 114 color clocks.
    fn step(&mut self) {
        if self.freq_counter == 0 {
            self.freq_counter = self.audf;
            self.clock_waveform();
        } else {
            self.freq_counter -= 1;
        }
    }

    /// Clock the waveform generator (frequency divider expired).
    fn clock_waveform(&mut self) {
        // Advance polynomial counters
        let p4_bit = ((self.poly4 >> 1) ^ self.poly4) & 1;
        self.poly4 = ((self.poly4 >> 1) | (p4_bit << 3)) & 0x0F;

        let p5_bit = ((self.poly5 >> 2) ^ self.poly5) & 1;
        self.poly5 = ((self.poly5 >> 1) | (p5_bit << 4)) & 0x1F;

        let p9_bit = (((self.poly9 >> 4) ^ self.poly9) & 1) as u16;
        self.poly9 = ((self.poly9 >> 1) | (p9_bit << 8)) & 0x1FF;

        match self.audc & 0x0F {
            0x00 | 0x0B => {
                // Constant 1
                self.output = true;
            }
            0x01 => {
                // 4-bit poly
                self.output = self.poly4 & 1 != 0;
            }
            0x02 => {
                // Div-by-31 then 4-bit poly
                // Only update output every 31 waveform clocks
                self.div_counter = self.div_counter.wrapping_add(1);
                if self.div_counter >= 31 { self.div_counter = 0; }
                if self.div_counter == 0 {
                    self.output = self.poly4 & 1 != 0;
                }
            }
            0x03 => {
                // 5-bit poly clocks 4-bit poly
                if self.poly5 & 1 != 0 {
                    self.output = self.poly4 & 1 != 0;
                }
            }
            0x04 | 0x05 => {
                // Pure tone (divide by 2)
                self.output = !self.output;
            }
            0x06 | 0x0A => {
                // Divide by 31
                self.div_counter = self.div_counter.wrapping_add(1);
                if self.div_counter >= 31 { self.div_counter = 0; }
                if self.div_counter == 0 {
                    self.output = !self.output;
                }
            }
            0x07 => {
                // 5-bit poly clocks divide-by-2
                if self.poly5 & 1 != 0 {
                    self.output = !self.output;
                }
            }
            0x08 => {
                // 9-bit poly
                self.output = self.poly9 & 1 != 0;
            }
            0x09 => {
                // 5-bit poly
                self.output = self.poly5 & 1 != 0;
            }
            0x0C | 0x0D => {
                // Divide by 6 (pure tone)
                self.div_counter = self.div_counter.wrapping_add(1);
                if self.div_counter >= 6 { self.div_counter = 0; }
                if self.div_counter == 0 {
                    self.output = !self.output;
                }
            }
            0x0E => {
                // Divide by 93
                self.div_counter = self.div_counter.wrapping_add(1);
                if self.div_counter >= 93 { self.div_counter = 0; }
                if self.div_counter == 0 {
                    self.output = !self.output;
                }
            }
            0x0F => {
                // 5-bit poly clocks divide-by-6
                if self.poly5 & 1 != 0 {
                    self.div_counter = self.div_counter.wrapping_add(1);
                    if self.div_counter >= 6 { self.div_counter = 0; }
                    if self.div_counter == 0 {
                        self.output = !self.output;
                    }
                }
            }
            _ => {}
        }
    }

    /// Current output sample (0.0 or volume-scaled).
    fn sample(&self) -> f32 {
        if self.output {
            self.audv as f32 / 15.0
        } else {
            0.0
        }
    }
}

// ── TIA ────────────────────────────────────────────────────────────

pub struct Tia {
    // Playfield
    pub pf0: u8,
    pub pf1: u8,
    pub pf2: u8,
    pub pf_reflect: bool,
    pub pf_score: bool,
    pub pf_priority: bool,

    // Player graphics
    pub grp0: u8,
    pub grp1: u8,
    pub grp0_old: u8,
    pub grp1_old: u8,
    pub resp0: u8,
    pub resp1: u8,
    pub refp0: bool,
    pub refp1: bool,
    pub vdelp0: bool,
    pub vdelp1: bool,

    // Missiles
    pub enam0: bool,
    pub enam1: bool,
    pub resm0: u8,
    pub resm1: u8,
    pub resmp0: bool,
    pub resmp1: bool,

    // Ball
    pub enabl: bool,
    pub enabl_old: bool,
    pub resbl: u8,
    pub vdelbl: bool,

    // Colors
    pub colup0: u8,
    pub colup1: u8,
    pub colupf: u8,
    pub colubk: u8,

    // Sizes
    pub nusiz0: u8,
    pub nusiz1: u8,
    pub ctrlpf: u8,

    // HMOVE registers
    pub hmp0: i8,
    pub hmp1: i8,
    pub hmm0: i8,
    pub hmm1: i8,
    pub hmbl: i8,

    // HMOVE blanking
    hmove_pending: bool,
    hmove_blanking: u8,

    // Position pipeline delays (4-5 color clock delay for RESPx/RESMx/RESBL)
    resp0_delay: u8,
    resp0_pending: u8,
    resp1_delay: u8,
    resp1_pending: u8,
    resm0_delay: u8,
    resm0_pending: u8,
    resm1_delay: u8,
    resm1_pending: u8,
    resbl_delay: u8,
    resbl_pending: u8,

    // Input ports (fire buttons): true = not pressed (bit 7 high)
    pub inpt4: bool,
    pub inpt5: bool,

    // Collision latches
    pub collision: [u8; 8],

    // Timing
    pub scanline: u16,
    pub clock: u16,
    pub wsync: bool,
    pub vsync: bool,
    pub vblank: bool,

    pub frame_ready: bool,
    pub framebuffer: FrameBuffer,

    // Audio
    audio_ch: [AudioChannel; 2],
    audio_clock_counter: u16,
    sample_rate: u32,
    sample_accum: f64,
    sample_buffer: Vec<f32>,
}

impl Tia {
    pub fn new() -> Self {
        Self {
            pf0: 0, pf1: 0, pf2: 0,
            pf_reflect: false, pf_score: false, pf_priority: false,
            grp0: 0, grp1: 0, grp0_old: 0, grp1_old: 0,
            resp0: 0, resp1: 0,
            refp0: false, refp1: false,
            vdelp0: false, vdelp1: false,
            enam0: false, enam1: false,
            resm0: 0, resm1: 0,
            resmp0: false, resmp1: false,
            enabl: false, enabl_old: false, resbl: 0, vdelbl: false,
            colup0: 0, colup1: 0, colupf: 0, colubk: 0,
            nusiz0: 0, nusiz1: 0, ctrlpf: 0,
            hmp0: 0, hmp1: 0, hmm0: 0, hmm1: 0, hmbl: 0,
            hmove_pending: false,
            hmove_blanking: 0,
            resp0_delay: 0, resp0_pending: 0,
            resp1_delay: 0, resp1_pending: 0,
            resm0_delay: 0, resm0_pending: 0,
            resm1_delay: 0, resm1_pending: 0,
            resbl_delay: 0, resbl_pending: 0,
            inpt4: true, inpt5: true,
            collision: [0; 8],
            scanline: 0, clock: 0,
            wsync: false, vsync: false, vblank: false,
            frame_ready: false,
            framebuffer: FrameBuffer::new(SCREEN_WIDTH, SCREEN_HEIGHT),
            audio_ch: [AudioChannel::new(), AudioChannel::new()],
            audio_clock_counter: 0,
            sample_rate: 44100,
            sample_accum: 0.0,
            sample_buffer: Vec::with_capacity(1024),
        }
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = rate;
    }

    /// Drain audio samples into the output buffer. Returns number of samples written.
    pub fn drain_samples(&mut self, out: &mut [f32]) -> usize {
        let n = self.sample_buffer.len().min(out.len());
        out[..n].copy_from_slice(&self.sample_buffer[..n]);
        self.sample_buffer.drain(..n);
        n
    }

    /// Write a TIA register.
    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x3F {
            0x00 => { // VSYNC
                let new_vsync = val & 0x02 != 0;
                if self.vsync && !new_vsync {
                    self.scanline = 0;
                    self.frame_ready = true;
                }
                self.vsync = new_vsync;
            }
            0x01 => { // VBLANK
                self.vblank = val & 0x02 != 0;
            }
            0x02 => self.wsync = true,
            0x04 => self.nusiz0 = val,
            0x05 => self.nusiz1 = val,
            0x06 => self.colup0 = val >> 1,
            0x07 => self.colup1 = val >> 1,
            0x08 => self.colupf = val >> 1,
            0x09 => self.colubk = val >> 1,
            0x0A => {
                self.ctrlpf = val;
                self.pf_reflect = val & 0x01 != 0;
                self.pf_score = val & 0x02 != 0;
                self.pf_priority = val & 0x04 != 0;
            }
            0x0B => self.refp0 = val & 0x08 != 0,
            0x0C => self.refp1 = val & 0x08 != 0,
            0x0D => self.pf0 = val,
            0x0E => self.pf1 = val,
            0x0F => self.pf2 = val,
            0x10 => { // RESP0 — position takes effect after ~5 color clock delay
                let pos = self.clock as i16 - 68 + 5;
                self.resp0_pending = if pos < 0 { 0 } else { (pos as u8) % 160 };
                self.resp0_delay = 5;
            }
            0x11 => { // RESP1
                let pos = self.clock as i16 - 68 + 5;
                self.resp1_pending = if pos < 0 { 0 } else { (pos as u8) % 160 };
                self.resp1_delay = 5;
            }
            0x12 => { // RESM0
                let pos = self.clock as i16 - 68 + 4;
                self.resm0_pending = if pos < 0 { 0 } else { (pos as u8) % 160 };
                self.resm0_delay = 4;
            }
            0x13 => { // RESM1
                let pos = self.clock as i16 - 68 + 4;
                self.resm1_pending = if pos < 0 { 0 } else { (pos as u8) % 160 };
                self.resm1_delay = 4;
            }
            0x14 => { // RESBL
                let pos = self.clock as i16 - 68 + 4;
                self.resbl_pending = if pos < 0 { 0 } else { (pos as u8) % 160 };
                self.resbl_delay = 4;
            }
            0x15 => self.audio_ch[0].audc = val & 0x0F, // AUDC0
            0x16 => self.audio_ch[1].audc = val & 0x0F, // AUDC1
            0x17 => self.audio_ch[0].audf = val & 0x1F, // AUDF0
            0x18 => self.audio_ch[1].audf = val & 0x1F, // AUDF1
            0x19 => self.audio_ch[0].audv = val & 0x0F, // AUDV0
            0x1A => self.audio_ch[1].audv = val & 0x0F, // AUDV1
            0x1B => { // GRP0
                self.grp0_old = self.grp0;
                self.grp0 = val;
                self.grp1_old = self.grp1;
            }
            0x1C => { // GRP1
                self.grp1_old = self.grp1;
                self.grp1 = val;
                self.grp0_old = self.grp0;
                self.enabl_old = self.enabl;
            }
            0x1D => self.enam0 = val & 0x02 != 0,
            0x1E => self.enam1 = val & 0x02 != 0,
            0x1F => self.enabl = val & 0x02 != 0,
            0x20 => self.hmp0 = decode_hm(val),
            0x21 => self.hmp1 = decode_hm(val),
            0x22 => self.hmm0 = decode_hm(val),
            0x23 => self.hmm1 = decode_hm(val),
            0x24 => self.hmbl = decode_hm(val),
            0x25 => self.vdelp0 = val & 0x01 != 0,
            0x26 => self.vdelp1 = val & 0x01 != 0,
            0x27 => self.vdelbl = val & 0x01 != 0,
            0x28 => self.resmp0 = val & 0x02 != 0,
            0x29 => self.resmp1 = val & 0x02 != 0,
            0x2A => { // HMOVE
                self.resp0 = ((self.resp0 as i16 - self.hmp0 as i16).rem_euclid(160)) as u8;
                self.resp1 = ((self.resp1 as i16 - self.hmp1 as i16).rem_euclid(160)) as u8;
                self.resm0 = ((self.resm0 as i16 - self.hmm0 as i16).rem_euclid(160)) as u8;
                self.resm1 = ((self.resm1 as i16 - self.hmm1 as i16).rem_euclid(160)) as u8;
                self.resbl = ((self.resbl as i16 - self.hmbl as i16).rem_euclid(160)) as u8;
                self.hmove_pending = true;
            }
            0x2B => { // HMCLR
                self.hmp0 = 0; self.hmp1 = 0;
                self.hmm0 = 0; self.hmm1 = 0;
                self.hmbl = 0;
            }
            0x2C => self.collision = [0; 8], // CXCLR
            _ => {}
        }
    }

    /// Read a TIA register (collision detection, input).
    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x0F {
            0x00..=0x07 => self.collision[(addr & 0x07) as usize],
            0x0C => if self.inpt4 { 0x80 } else { 0x00 }, // INPT4 (P0 fire)
            0x0D => if self.inpt5 { 0x80 } else { 0x00 }, // INPT5 (P1 fire)
            _ => 0,
        }
    }

    /// Step the TIA by one color clock (3 per CPU cycle).
    pub fn step_clock(&mut self) {
        // ── Position pipeline delays ──
        if self.resp0_delay > 0 {
            self.resp0_delay -= 1;
            if self.resp0_delay == 0 { self.resp0 = self.resp0_pending; }
        }
        if self.resp1_delay > 0 {
            self.resp1_delay -= 1;
            if self.resp1_delay == 0 { self.resp1 = self.resp1_pending; }
        }
        if self.resm0_delay > 0 {
            self.resm0_delay -= 1;
            if self.resm0_delay == 0 { self.resm0 = self.resm0_pending; }
        }
        if self.resm1_delay > 0 {
            self.resm1_delay -= 1;
            if self.resm1_delay == 0 { self.resm1 = self.resm1_pending; }
        }
        if self.resbl_delay > 0 {
            self.resbl_delay -= 1;
            if self.resbl_delay == 0 { self.resbl = self.resbl_pending; }
        }

        // ── Video ──
        let visible_start = 68u16;
        if self.clock >= visible_start && self.clock < visible_start + VISIBLE_WIDTH as u16 {
            let x = (self.clock - visible_start) as u32;
            let y = self.scanline.saturating_sub(40);

            if y < VISIBLE_HEIGHT as u16 && !self.vblank {
                // HMOVE blanking: first 8 visible pixels are black after HMOVE
                if self.hmove_blanking > 0 && x < 8 {
                    self.framebuffer.set_pixel_rgb(x, y as u32, 0x000000);
                } else {
                    let color = self.get_pixel_color(x as u8);
                    let rgb = TIA_PALETTE[(color as usize) & 0x7F];
                    self.framebuffer.set_pixel_rgb(x, y as u32, rgb);
                }
            }
        }

        // Lock missiles to player center when RESMP is set
        if self.resmp0 {
            self.resm0 = (self.resp0 + 4) % 160;
        }
        if self.resmp1 {
            self.resm1 = (self.resp1 + 4) % 160;
        }

        // ── Audio ──
        self.audio_clock_counter += 1;
        if self.audio_clock_counter >= AUDIO_CLOCK_DIVISOR {
            self.audio_clock_counter = 0;
            self.audio_ch[0].step();
            self.audio_ch[1].step();
        }

        // Generate output sample at target sample rate
        if self.sample_rate > 0 {
            self.sample_accum += self.sample_rate as f64;
            if self.sample_accum >= TIA_CLOCK_RATE {
                self.sample_accum -= TIA_CLOCK_RATE;
                let s0 = self.audio_ch[0].sample();
                let s1 = self.audio_ch[1].sample();
                let mixed = (s0 + s1) * 0.5;
                self.sample_buffer.push(mixed);
            }
        }

        // ── Scanline advance ──
        self.clock += 1;
        if self.clock >= CLOCKS_PER_SCANLINE {
            self.clock = 0;
            self.scanline += 1;
            self.wsync = false;

            // Apply HMOVE blanking at start of new scanline
            if self.hmove_pending {
                self.hmove_blanking = 8;
                self.hmove_pending = false;
            } else {
                self.hmove_blanking = 0;
            }
        }
    }

    /// Effective GRP0 (considering VDEL).
    fn effective_grp0(&self) -> u8 {
        if self.vdelp0 { self.grp0_old } else { self.grp0 }
    }

    /// Effective GRP1 (considering VDEL).
    fn effective_grp1(&self) -> u8 {
        if self.vdelp1 { self.grp1_old } else { self.grp1 }
    }

    /// Effective ball enable (considering VDEL).
    fn effective_enabl(&self) -> bool {
        if self.vdelbl { self.enabl_old } else { self.enabl }
    }

    /// Determine the color of the pixel at horizontal position x (0-159).
    /// Also updates collision latches.
    fn get_pixel_color(&mut self, x: u8) -> u8 {
        let pf = self.get_playfield_bit(x);
        let bl = self.get_ball_bit(x);
        let p0 = self.get_player_bit(x, self.resp0, self.effective_grp0(), self.nusiz0, self.refp0);
        let p1 = self.get_player_bit(x, self.resp1, self.effective_grp1(), self.nusiz1, self.refp1);
        let m0 = self.get_missile_bit(x, self.resm0, self.enam0 && !self.resmp0, self.nusiz0);
        let m1 = self.get_missile_bit(x, self.resm1, self.enam1 && !self.resmp1, self.nusiz1);

        // Update collision registers (latched — stay set until CXCLR)
        if m0 && p1 { self.collision[0] |= 0x80; }
        if m0 && p0 { self.collision[0] |= 0x40; }
        if m1 && p0 { self.collision[1] |= 0x80; }
        if m1 && p1 { self.collision[1] |= 0x40; }
        if p0 && pf { self.collision[2] |= 0x80; }
        if p0 && bl { self.collision[2] |= 0x40; }
        if p1 && pf { self.collision[3] |= 0x80; }
        if p1 && bl { self.collision[3] |= 0x40; }
        if m0 && pf { self.collision[4] |= 0x80; }
        if m0 && bl { self.collision[4] |= 0x40; }
        if m1 && pf { self.collision[5] |= 0x80; }
        if m1 && bl { self.collision[5] |= 0x40; }
        if bl && pf { self.collision[6] |= 0x80; }
        if p0 && p1 { self.collision[7] |= 0x80; }
        if m0 && m1 { self.collision[7] |= 0x40; }

        // Priority resolution
        if self.pf_priority {
            if pf || bl {
                return if self.pf_score {
                    if x < 80 { self.colup0 } else { self.colup1 }
                } else {
                    self.colupf
                };
            }
            if p0 || m0 { return self.colup0; }
            if p1 || m1 { return self.colup1; }
        } else {
            if p0 || m0 { return self.colup0; }
            if p1 || m1 { return self.colup1; }
            if pf || bl {
                return if self.pf_score {
                    if x < 80 { self.colup0 } else { self.colup1 }
                } else {
                    self.colupf
                };
            }
        }

        self.colubk
    }

    fn get_playfield_bit(&self, x: u8) -> bool {
        let pf_x = if x < 80 {
            x / 4
        } else if self.pf_reflect {
            19 - (x - 80) / 4
        } else {
            (x - 80) / 4
        };

        match pf_x {
            0..=3 => (self.pf0 >> (4 + pf_x)) & 1 != 0,
            4..=11 => (self.pf1 >> (11 - pf_x)) & 1 != 0,
            12..=19 => (self.pf2 >> (pf_x - 12)) & 1 != 0,
            _ => false,
        }
    }

    fn get_player_bit(&self, x: u8, player_pos: u8, grp: u8, nusiz: u8, reflect: bool) -> bool {
        if grp == 0 { return false; }

        let pixel_w = player_width(nusiz);
        let offsets = copy_offsets(nusiz);

        for &offset in offsets {
            let pos = (player_pos as u16 + offset as u16) % 160;
            let diff = ((x as i16 - pos as i16).rem_euclid(160)) as u8;
            let total_width = 8 * pixel_w;
            if diff < total_width {
                let bit_idx = diff / pixel_w;
                let bit = if reflect {
                    bit_idx
                } else {
                    7 - bit_idx
                };
                if (grp >> bit) & 1 != 0 {
                    return true;
                }
            }
        }
        false
    }

    fn get_missile_bit(&self, x: u8, missile_pos: u8, enabled: bool, nusiz: u8) -> bool {
        if !enabled { return false; }

        let size = 1u8 << ((nusiz >> 4) & 0x03);
        let offsets = copy_offsets(nusiz);

        for &offset in offsets {
            let pos = (missile_pos as u16 + offset as u16) % 160;
            let diff = ((x as i16 - pos as i16).rem_euclid(160)) as u8;
            if diff < size {
                return true;
            }
        }
        false
    }

    fn get_ball_bit(&self, x: u8) -> bool {
        if !self.effective_enabl() { return false; }
        let size = 1u8 << ((self.ctrlpf >> 4) & 0x03);
        let diff = ((x as i16 - self.resbl as i16).rem_euclid(160)) as u8;
        diff < size
    }

    pub fn snapshot(&self) -> TiaSnapshot {
        let ch = |c: &AudioChannel| AudioChannelSnapshot {
            audc: c.audc, audf: c.audf, audv: c.audv,
            freq_counter: c.freq_counter,
            poly4: c.poly4, poly5: c.poly5, poly9: c.poly9,
            div_counter: c.div_counter, output: c.output,
        };
        TiaSnapshot {
            pf0: self.pf0, pf1: self.pf1, pf2: self.pf2,
            pf_reflect: self.pf_reflect, pf_score: self.pf_score, pf_priority: self.pf_priority,
            grp0: self.grp0, grp1: self.grp1, grp0_old: self.grp0_old, grp1_old: self.grp1_old,
            resp0: self.resp0, resp1: self.resp1,
            refp0: self.refp0, refp1: self.refp1,
            vdelp0: self.vdelp0, vdelp1: self.vdelp1,
            enam0: self.enam0, enam1: self.enam1,
            resm0: self.resm0, resm1: self.resm1,
            resmp0: self.resmp0, resmp1: self.resmp1,
            enabl: self.enabl, enabl_old: self.enabl_old, resbl: self.resbl, vdelbl: self.vdelbl,
            colup0: self.colup0, colup1: self.colup1, colupf: self.colupf, colubk: self.colubk,
            nusiz0: self.nusiz0, nusiz1: self.nusiz1, ctrlpf: self.ctrlpf,
            hmp0: self.hmp0, hmp1: self.hmp1, hmm0: self.hmm0, hmm1: self.hmm1, hmbl: self.hmbl,
            hmove_pending: self.hmove_pending, hmove_blanking: self.hmove_blanking,
            resp0_delay: self.resp0_delay, resp0_pending: self.resp0_pending,
            resp1_delay: self.resp1_delay, resp1_pending: self.resp1_pending,
            resm0_delay: self.resm0_delay, resm0_pending: self.resm0_pending,
            resm1_delay: self.resm1_delay, resm1_pending: self.resm1_pending,
            resbl_delay: self.resbl_delay, resbl_pending: self.resbl_pending,
            inpt4: self.inpt4, inpt5: self.inpt5,
            collision: self.collision,
            scanline: self.scanline, clock: self.clock,
            wsync: self.wsync, vsync: self.vsync, vblank: self.vblank, frame_ready: self.frame_ready,
            audio_clock_counter: self.audio_clock_counter,
            audio_ch: [ch(&self.audio_ch[0]), ch(&self.audio_ch[1])],
        }
    }

    pub fn restore(&mut self, s: &TiaSnapshot) {
        let rc = |c: &mut AudioChannel, sc: &AudioChannelSnapshot| {
            c.audc = sc.audc; c.audf = sc.audf; c.audv = sc.audv;
            c.freq_counter = sc.freq_counter;
            c.poly4 = sc.poly4; c.poly5 = sc.poly5; c.poly9 = sc.poly9;
            c.div_counter = sc.div_counter; c.output = sc.output;
        };
        self.pf0 = s.pf0; self.pf1 = s.pf1; self.pf2 = s.pf2;
        self.pf_reflect = s.pf_reflect; self.pf_score = s.pf_score; self.pf_priority = s.pf_priority;
        self.grp0 = s.grp0; self.grp1 = s.grp1; self.grp0_old = s.grp0_old; self.grp1_old = s.grp1_old;
        self.resp0 = s.resp0; self.resp1 = s.resp1;
        self.refp0 = s.refp0; self.refp1 = s.refp1;
        self.vdelp0 = s.vdelp0; self.vdelp1 = s.vdelp1;
        self.enam0 = s.enam0; self.enam1 = s.enam1;
        self.resm0 = s.resm0; self.resm1 = s.resm1;
        self.resmp0 = s.resmp0; self.resmp1 = s.resmp1;
        self.enabl = s.enabl; self.enabl_old = s.enabl_old; self.resbl = s.resbl; self.vdelbl = s.vdelbl;
        self.colup0 = s.colup0; self.colup1 = s.colup1; self.colupf = s.colupf; self.colubk = s.colubk;
        self.nusiz0 = s.nusiz0; self.nusiz1 = s.nusiz1; self.ctrlpf = s.ctrlpf;
        self.hmp0 = s.hmp0; self.hmp1 = s.hmp1; self.hmm0 = s.hmm0; self.hmm1 = s.hmm1; self.hmbl = s.hmbl;
        self.hmove_pending = s.hmove_pending; self.hmove_blanking = s.hmove_blanking;
        self.resp0_delay = s.resp0_delay; self.resp0_pending = s.resp0_pending;
        self.resp1_delay = s.resp1_delay; self.resp1_pending = s.resp1_pending;
        self.resm0_delay = s.resm0_delay; self.resm0_pending = s.resm0_pending;
        self.resm1_delay = s.resm1_delay; self.resm1_pending = s.resm1_pending;
        self.resbl_delay = s.resbl_delay; self.resbl_pending = s.resbl_pending;
        self.inpt4 = s.inpt4; self.inpt5 = s.inpt5;
        self.collision = s.collision;
        self.scanline = s.scanline; self.clock = s.clock;
        self.wsync = s.wsync; self.vsync = s.vsync; self.vblank = s.vblank; self.frame_ready = s.frame_ready;
        self.audio_clock_counter = s.audio_clock_counter;
        rc(&mut self.audio_ch[0], &s.audio_ch[0]);
        rc(&mut self.audio_ch[1], &s.audio_ch[1]);
    }
}
