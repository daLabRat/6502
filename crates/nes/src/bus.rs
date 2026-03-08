use emu_common::Bus;
use crate::ppu::Ppu;
use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::controller::Controller;

/// NES memory bus. Owns all hardware components.
/// Memory map:
/// $0000-$07FF: 2KB internal RAM (mirrored to $1FFF)
/// $2000-$2007: PPU registers (mirrored to $3FFF)
/// $4000-$4017: APU and I/O registers
/// $4018-$FFFF: Cartridge space
pub struct NesBus {
    pub ram: [u8; 2048],
    pub ppu: Ppu,
    pub apu: Apu,
    pub cartridge: Cartridge,
    pub controller1: Controller,
    pub controller2: Controller,

    // OAM DMA state
    pub(crate) oam_dma_page: u8,
    pub(crate) oam_dma_pending: bool,

    // Cycle tracking for PPU synchronization
    pub(crate) ppu_nmi_pending: bool,
}

impl NesBus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            ram: [0; 2048],
            ppu: Ppu::new(),
            apu: Apu::new(),
            cartridge,
            controller1: Controller::new(),
            controller2: Controller::new(),
            oam_dma_page: 0,
            oam_dma_pending: false,
            ppu_nmi_pending: false,
        }
    }
}

impl Bus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => {
                self.ppu.read_register(addr, self.cartridge.mapper.as_mut())
            }
            0x4015 => self.apu.read_status(),
            0x4016 => self.controller1.read(),
            0x4017 => self.controller2.read(),
            0x4000..=0x4014 | 0x4018..=0x401F => 0, // APU/IO - open bus
            0x4020..=0xFFFF => self.cartridge.mapper.cpu_read(addr),
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = val,
            0x2000..=0x3FFF => {
                self.ppu.write_register(addr, val, self.cartridge.mapper.as_mut());
            }
            0x4000..=0x4013 | 0x4015 | 0x4017 => {
                self.apu.write_register(addr, val);
            }
            0x4014 => {
                // OAM DMA
                self.oam_dma_page = val;
                self.oam_dma_pending = true;
            }
            0x4016 => {
                self.controller1.write(val);
                self.controller2.write(val);
            }
            0x4020..=0xFFFF => self.cartridge.mapper.cpu_write(addr, val),
            _ => {}
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => 0, // Can't peek PPU without side effects
            0x4020..=0xFFFF => self.cartridge.mapper.cpu_read(addr),
            _ => 0,
        }
    }

    fn tick(&mut self, cycles: u8) {
        // Handle OAM DMA if pending
        let dma_stall = if self.oam_dma_pending {
            self.oam_dma_pending = false;
            let page = (self.oam_dma_page as u16) << 8;
            let mut data = [0u8; 256];
            for i in 0..256u16 {
                data[i as usize] = self.read(page | i);
            }
            self.ppu.write_oam_dma(&data);
            // DMA takes 513 CPU cycles (256 reads + 256 writes + 1 alignment)
            513u32
        } else {
            0
        };

        // Each CPU cycle = 3 PPU cycles
        let total_cpu_cycles = cycles as u32 + dma_stall;
        let ppu_cycles = total_cpu_cycles * 3;
        for _ in 0..ppu_cycles {
            self.ppu.step(self.cartridge.mapper.as_mut());
        }

        // APU runs at CPU clock rate
        for _ in 0..total_cpu_cycles {
            self.apu.step();
            self.cartridge.mapper.cpu_tick();

            // Service DMC DMA request
            if let Some(addr) = self.apu.dmc.dma_request.take() {
                let byte = match addr {
                    0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
                    0x8000..=0xFFFF => self.cartridge.mapper.cpu_read(addr),
                    _ => 0,
                };
                self.apu.dmc.receive_dma_byte(byte);
            }
        }

        // Check PPU NMI
        if self.ppu.nmi_pending {
            self.ppu_nmi_pending = true;
            self.ppu.nmi_pending = false;
        }
    }

    fn poll_nmi(&mut self) -> bool {
        let pending = self.ppu_nmi_pending;
        self.ppu_nmi_pending = false;
        pending
    }

    fn poll_irq(&mut self) -> bool {
        self.cartridge.mapper.irq_pending() || self.apu.irq_pending()
    }
}
