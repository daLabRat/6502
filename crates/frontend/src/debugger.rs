//! Debugger — separate OS window via egui multi-viewport.

use std::collections::HashSet;
use egui::{Color32, FontId, RichText, Ui};
use emu_common::{CpuDebugState, DebugSection, SystemEmulator};

// ── State ────────────────────────────────────────────────────────────────────

pub struct DebuggerState {
    pub open:        bool,
    pub paused:      bool,
    pub step:        bool,      // consumed by the main update loop
    pub breakpoints: HashSet<u16>,
    pub mem_page:    u16,
    bp_input:        String,
    active_tab:      Tab,
}

#[derive(PartialEq)]
enum Tab { Cpu, Memory, System }

impl Default for DebuggerState {
    fn default() -> Self {
        Self {
            open:        false,
            paused:      false,
            step:        false,
            breakpoints: HashSet::new(),
            mem_page:    0,
            bp_input:    String::new(),
            active_tab:  Tab::Cpu,
        }
    }
}

impl DebuggerState {
    pub fn check_breakpoint(&mut self, pc: u16) {
        if self.breakpoints.contains(&pc) {
            self.paused = true;
        }
    }
}

// ── Snapshot (read-only data extracted from the emulator each frame) ──────────

pub struct DebugSnapshot {
    pub cpu:       CpuDebugState,
    pub disasm:    Vec<(u16, String, bool)>,
    pub mem_page:  u16,
    pub mem_bytes: Box<[u8; 256]>,
    pub panels:    Vec<DebugSection>,
}

impl DebugSnapshot {
    pub fn collect(sys: &dyn SystemEmulator, state: &DebuggerState) -> Self {
        let cpu    = sys.cpu_state();
        let disasm = disassemble_around(sys, cpu.pc, 8, 12);
        let page   = state.mem_page;
        let mem_bytes = Box::new(std::array::from_fn(|i| {
            sys.peek_memory(page.wrapping_add(i as u16))
        }));
        let panels = sys.system_debug_panels();
        Self { cpu, disasm, mem_page: page, mem_bytes, panels }
    }
}

// ── Entry point called from app.rs ───────────────────────────────────────────

/// Open the debugger in its own OS window.
/// Call this AFTER extracting `snap` from the system (avoids borrow conflicts).
pub fn show(ctx: &egui::Context, state: &mut DebuggerState, snap: &DebugSnapshot) {
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("debugger"),
        egui::ViewportBuilder::default()
            .with_title("Debugger")
            .with_inner_size([780.0, 540.0])
            .with_min_inner_size([500.0, 300.0]),
        |vp_ctx, _class| {
            egui::CentralPanel::default().show(vp_ctx, |ui| {
                render_controls(ui, state, snap);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut state.active_tab, Tab::Cpu,    "CPU");
                    ui.selectable_value(&mut state.active_tab, Tab::Memory, "Memory");
                    ui.selectable_value(&mut state.active_tab, Tab::System, "System");
                });
                ui.separator();
                match state.active_tab {
                    Tab::Cpu    => render_cpu_tab(ui, state, snap),
                    Tab::Memory => render_memory_tab(ui, state, snap),
                    Tab::System => render_system_tab(ui, snap),
                }
            });

            // Close button on the OS window title bar
            if vp_ctx.input(|i| i.viewport().close_requested()) {
                state.open = false;
            }
        },
    );
}

// ── Panels ───────────────────────────────────────────────────────────────────

fn render_controls(ui: &mut Ui, state: &mut DebuggerState, snap: &DebugSnapshot) {
    ui.horizontal(|ui| {
        let label = if state.paused { "▶ Run" } else { "⏸ Pause" };
        if ui.button(label).clicked() {
            state.paused = !state.paused;
        }
        ui.add_enabled_ui(state.paused, |ui| {
            if ui.button("⏭ Step").clicked() {
                state.step = true;
            }
        });
        ui.separator();
        let cpu = &snap.cpu;
        ui.label(RichText::new(format!(
            "PC:{:04X}  A:{:02X}  X:{:02X}  Y:{:02X}  SP:{:02X}  {}  cyc:{}",
            cpu.pc, cpu.a, cpu.x, cpu.y, cpu.sp,
            flags_str(cpu.flags),
            cpu.cycles,
        )).font(FontId::monospace(12.0)));
    });
}

fn render_cpu_tab(ui: &mut Ui, state: &mut DebuggerState, snap: &DebugSnapshot) {
    let cpu = &snap.cpu;

    egui::Grid::new("regs").num_columns(4).spacing([16.0, 2.0]).show(ui, |ui| {
        ui.label(mono("PC")); ui.label(mono(format!("{:04X}", cpu.pc)));
        ui.label(mono("SP")); ui.label(mono(format!("{:02X}",   cpu.sp)));
        ui.end_row();
        ui.label(mono("A"));  ui.label(mono(format!("{:02X}",   cpu.a)));
        ui.label(mono("X"));  ui.label(mono(format!("{:02X}",   cpu.x)));
        ui.end_row();
        ui.label(mono("Y"));  ui.label(mono(format!("{:02X}",   cpu.y)));
        ui.label(mono("P"));  ui.label(mono(format!("{:02X} {}", cpu.flags, flags_str(cpu.flags))));
        ui.end_row();
        ui.label(mono("CYC")); ui.label(mono(format!("{}", cpu.cycles)));
        ui.end_row();
    });

    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("disasm")
        .max_height(220.0)
        .show(ui, |ui| {
            for (addr, text, is_pc) in &snap.disasm {
                let is_bp = state.breakpoints.contains(addr);
                ui.horizontal(|ui| {
                    let dot = if is_bp { "●" } else { " " };
                    if ui.label(RichText::new(dot).color(Color32::RED).font(FontId::monospace(12.0)))
                        .clicked()
                    {
                        if is_bp { state.breakpoints.remove(addr); }
                        else      { state.breakpoints.insert(*addr); }
                    }
                    let addr_color = if *is_pc { Color32::YELLOW } else { Color32::GRAY };
                    ui.label(RichText::new(format!("{:04X}", addr))
                        .color(addr_color).font(FontId::monospace(12.0)));
                    let text_color = if *is_pc { Color32::WHITE } else { Color32::LIGHT_GRAY };
                    ui.label(RichText::new(text).color(text_color).font(FontId::monospace(12.0)));
                });
            }
        });

    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Add breakpoint:");
        let resp = ui.add(egui::TextEdit::singleline(&mut state.bp_input)
            .desired_width(60.0).font(FontId::monospace(12.0)).hint_text("FFFF"));
        if (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            || ui.button("Add").clicked()
        {
            if let Ok(addr) = u16::from_str_radix(state.bp_input.trim(), 16) {
                state.breakpoints.insert(addr);
                state.bp_input.clear();
            }
        }
        let bps: Vec<u16> = { let mut v: Vec<_> = state.breakpoints.iter().copied().collect(); v.sort(); v };
        for bp in &bps {
            if ui.small_button(format!("{:04X} ✕", bp)).clicked() {
                state.breakpoints.remove(bp);
                break;
            }
        }
    });
}

fn render_memory_tab(ui: &mut Ui, state: &mut DebuggerState, snap: &DebugSnapshot) {
    ui.horizontal(|ui| {
        ui.label("Address:");
        let resp = ui.add(egui::TextEdit::singleline(&mut state.bp_input) // reuse bp_input as addr field
            .desired_width(60.0).font(FontId::monospace(12.0)));
        if resp.changed() {
            if let Ok(v) = u16::from_str_radix(state.bp_input.trim(), 16) {
                state.mem_page = v & 0xFF00;
            }
        }
        if ui.button("◀").clicked() { state.mem_page = state.mem_page.saturating_sub(0x100); }
        if ui.button("▶").clicked() { state.mem_page = state.mem_page.saturating_add(0x100); }
        ui.label(mono(format!("Page ${:04X}", snap.mem_page)));
    });

    ui.separator();

    egui::ScrollArea::vertical().id_salt("mem").max_height(400.0).show(ui, |ui| {
        for row in 0..16u16 {
            let row_addr = snap.mem_page.wrapping_add(row * 16);
            let mut line = format!("{:04X}  ", row_addr);
            let mut ascii = String::with_capacity(16);
            for col in 0..16usize {
                let b = snap.mem_bytes[row as usize * 16 + col];
                line.push_str(&format!("{:02X} ", b));
                if col == 7 { line.push(' '); }
                ascii.push(if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' });
            }
            line.push(' ');
            line.push_str(&ascii);
            ui.label(RichText::new(line).font(FontId::monospace(11.0)).color(Color32::LIGHT_GRAY));
        }
    });
}

fn render_system_tab(ui: &mut Ui, snap: &DebugSnapshot) {
    if snap.panels.is_empty() {
        ui.label("No system-specific debug info available.");
        return;
    }
    egui::ScrollArea::vertical().id_salt("sys").show(ui, |ui| {
        for section in &snap.panels {
            ui.collapsing(&section.name, |ui| {
                egui::Grid::new(&section.name).num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
                    for (k, v) in &section.rows {
                        ui.label(RichText::new(k).color(Color32::GRAY).font(FontId::monospace(11.0)));
                        ui.label(RichText::new(v).font(FontId::monospace(11.0)));
                        ui.end_row();
                    }
                });
            });
        }
    });
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn disassemble_around(sys: &dyn SystemEmulator, pc: u16, before: usize, after: usize) -> Vec<(u16, String, bool)> {
    let scan_back = (before * 3 + 3) as u16;
    let start = pc.saturating_sub(scan_back);

    let mut lines: Vec<(u16, String)> = Vec::new();
    let mut addr = start;
    let limit = pc.saturating_add(after as u16 * 3 + 16);
    while addr <= limit {
        let (text, next) = sys.disassemble(addr);
        lines.push((addr, text));
        if next <= addr { break; }
        addr = next;
    }

    let pc_idx = match lines.iter().position(|(a, _)| *a == pc) {
        Some(i) => i,
        None => {
            lines.clear();
            addr = pc;
            for _ in 0..(before + 1 + after) {
                let (text, next) = sys.disassemble(addr);
                lines.push((addr, text));
                addr = next;
            }
            before.min(lines.len().saturating_sub(1))
        }
    };

    let start_idx = pc_idx.saturating_sub(before);
    let end_idx   = (pc_idx + 1 + after).min(lines.len());
    lines[start_idx..end_idx]
        .iter()
        .map(|(a, t)| (*a, t.clone(), *a == pc))
        .collect()
}

fn mono(s: impl Into<String>) -> RichText {
    RichText::new(s).font(FontId::monospace(12.0))
}

fn flags_str(p: u8) -> String {
    format!("{}{}{}{}{}{}{}{}",
        if p & 0x80 != 0 { 'N' } else { 'n' },
        if p & 0x40 != 0 { 'V' } else { 'v' },
        if p & 0x20 != 0 { 'U' } else { 'u' },
        if p & 0x10 != 0 { 'B' } else { 'b' },
        if p & 0x08 != 0 { 'D' } else { 'd' },
        if p & 0x04 != 0 { 'I' } else { 'i' },
        if p & 0x02 != 0 { 'Z' } else { 'z' },
        if p & 0x01 != 0 { 'C' } else { 'c' },
    )
}
