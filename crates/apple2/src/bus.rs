use emu_common::Bus;
use crate::disk_ii::DiskII;
use crate::memory::Memory;
use crate::soft_switch::SoftSwitches;
use crate::keyboard::Keyboard;
use crate::speaker::Speaker;
use crate::video;
use emu_common::FrameBuffer;

/// Apple II bus - routes addresses to appropriate hardware.
pub struct Apple2Bus {
    pub memory: Memory,
    pub switches: SoftSwitches,
    pub keyboard: Keyboard,
    pub speaker: Speaker,
    pub disk_ii: DiskII,
    pub framebuffer: FrameBuffer,

    cycle_count: u64,
    cycles_per_frame: u64,
    frame_ready: bool,
    frame_count: u32,
}

impl Apple2Bus {
    pub fn new() -> Self {
        Self {
            memory: Memory::new(),
            switches: SoftSwitches::new(),
            keyboard: Keyboard::new(),
            speaker: Speaker::new(),
            disk_ii: DiskII::new(),
            framebuffer: FrameBuffer::new(video::DISPLAY_WIDTH, video::DISPLAY_HEIGHT),
            cycle_count: 0,
            cycles_per_frame: 17030, // ~60fps at 1.023 MHz
            frame_ready: false,
            frame_count: 0,
        }
    }

    pub fn is_frame_ready(&mut self) -> bool {
        let ready = self.frame_ready;
        self.frame_ready = false;
        ready
    }

    fn render_frame(&mut self) {
        self.frame_count = self.frame_count.wrapping_add(1);
        // Flash cursor at ~1.9Hz (16 frames on, 16 frames off at 60fps)
        let flash_on = (self.frame_count / 16) & 1 == 0;
        video::render(&mut self.framebuffer, &self.memory, &self.switches, flash_on);
    }

    /// Read with IIe banking: select main or auxiliary RAM based on soft switches.
    fn banked_read(&self, addr: u16) -> u8 {
        let use_aux = match addr {
            0x0000..=0x01FF => self.switches.altzp,
            0x0200..=0x03FF => self.switches.ramrd,
            0x0400..=0x07FF => {
                if self.switches.store80 {
                    self.switches.page2
                } else {
                    self.switches.ramrd
                }
            }
            0x0800..=0x1FFF => self.switches.ramrd,
            0x2000..=0x3FFF => {
                if self.switches.store80 && self.switches.hires {
                    self.switches.page2
                } else {
                    self.switches.ramrd
                }
            }
            0x4000..=0xBFFF => self.switches.ramrd,
            _ => false,
        };

        if use_aux {
            self.memory.aux_ram[addr as usize]
        } else {
            self.memory.ram[addr as usize]
        }
    }

    /// Write with IIe banking: select main or auxiliary RAM based on soft switches.
    fn banked_write(&mut self, addr: u16, val: u8) {
        let use_aux = match addr {
            0x0000..=0x01FF => self.switches.altzp,
            0x0200..=0x03FF => self.switches.ramwrt,
            0x0400..=0x07FF => {
                if self.switches.store80 {
                    self.switches.page2
                } else {
                    self.switches.ramwrt
                }
            }
            0x0800..=0x1FFF => self.switches.ramwrt,
            0x2000..=0x3FFF => {
                if self.switches.store80 && self.switches.hires {
                    self.switches.page2
                } else {
                    self.switches.ramwrt
                }
            }
            0x4000..=0xBFFF => self.switches.ramwrt,
            _ => false,
        };

        if use_aux {
            self.memory.aux_ram[addr as usize] = val;
        } else {
            self.memory.ram[addr as usize] = val;
        }
    }
}

impl Bus for Apple2Bus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0xBFFF => self.banked_read(addr),
            0xC000 => self.keyboard.read_key(),
            0xC001..=0xC00F => {
                // IIe switch writes (triggered by read too on some)
                self.switches.handle_iie(addr);
                0
            }
            0xC010 => self.keyboard.clear_strobe(),
            0xC011..=0xC01F => self.switches.read_status(addr),
            0xC030 => {
                self.speaker.toggle();
                0
            }
            0xC050..=0xC05F => {
                self.switches.handle(addr);
                0
            }
            0xC080..=0xC08F => {
                self.memory.handle_lc_switch(addr);
                0
            }
            // Disk II slot 6: I/O at $C0E0-$C0EF
            0xC0E0..=0xC0EF => self.disk_ii.io_read(addr),
            // Disk II slot 6: boot ROM at $C600-$C6FF
            0xC600..=0xC6FF => self.disk_ii.read_rom(addr),
            0xC000..=0xCFFF => 0, // Other I/O slots
            0xD000..=0xFFFF => self.memory.read(addr),
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0xBFFF => self.banked_write(addr, val),
            0xC000..=0xC00F => { self.switches.handle_iie(addr); }
            0xC010 => { self.keyboard.clear_strobe(); }
            0xC030 => { self.speaker.toggle(); }
            0xC050..=0xC05F => { self.switches.handle(addr); }
            0xC080..=0xC08F => { self.memory.handle_lc_switch(addr); }
            // Disk II slot 6: I/O at $C0E0-$C0EF
            0xC0E0..=0xC0EF => { self.disk_ii.io_write(addr, val); }
            0xC000..=0xCFFF => {} // Other I/O
            0xD000..=0xFFFF => self.memory.write(addr, val),
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0xBFFF | 0xD000..=0xFFFF => self.memory.read(addr),
            0xC600..=0xC6FF => self.disk_ii.read_rom(addr),
            _ => 0,
        }
    }

    fn tick(&mut self, cycles: u8) {
        for _ in 0..cycles {
            self.speaker.step();
        }

        self.disk_ii.step(cycles);

        self.cycle_count += cycles as u64;
        if self.cycle_count >= self.cycles_per_frame {
            self.cycle_count -= self.cycles_per_frame;
            self.render_frame();
            self.frame_ready = true;
        }
    }

    fn poll_nmi(&mut self) -> bool {
        false // Apple II has no NMI
    }

    fn poll_irq(&mut self) -> bool {
        false // No IRQ sources in basic Apple II
    }
}
