use crate::cpu::Cpu6502;
use emu_common::Bus;

/// Simple flat 64KB RAM bus for testing.
struct TestBus {
    ram: [u8; 65536],
    nmi_pending: bool,
    irq_line: bool,
}

impl TestBus {
    fn new() -> Self {
        Self {
            ram: [0; 65536],
            nmi_pending: false,
            irq_line: false,
        }
    }
}

impl Bus for TestBus {
    fn read(&mut self, addr: u16) -> u8 { self.ram[addr as usize] }
    fn write(&mut self, addr: u16, val: u8) { self.ram[addr as usize] = val; }
    fn peek(&self, addr: u16) -> u8 { self.ram[addr as usize] }
    fn tick(&mut self, _cycles: u8) {}
    fn poll_nmi(&mut self) -> bool {
        let pending = self.nmi_pending;
        self.nmi_pending = false;
        pending
    }
    fn poll_irq(&mut self) -> bool { self.irq_line }
}

fn make_cpu() -> Cpu6502<TestBus> {
    let mut cpu = Cpu6502::new(TestBus::new());
    // Set reset vector to $0400
    cpu.bus.ram[0xFFFC] = 0x00;
    cpu.bus.ram[0xFFFD] = 0x04;
    cpu.reset();
    cpu
}

fn load_program(cpu: &mut Cpu6502<TestBus>, addr: u16, program: &[u8]) {
    for (i, &byte) in program.iter().enumerate() {
        cpu.bus.ram[addr as usize + i] = byte;
    }
}

#[test]
fn test_lda_immediate() {
    let mut cpu = make_cpu();
    load_program(&mut cpu, 0x0400, &[0xA9, 0x42]); // LDA #$42
    cpu.step();
    assert_eq!(cpu.a, 0x42);
    assert!(!cpu.p.contains(crate::flags::StatusFlags::ZERO));
    assert!(!cpu.p.contains(crate::flags::StatusFlags::NEGATIVE));
}

#[test]
fn test_lda_zero_flag() {
    let mut cpu = make_cpu();
    load_program(&mut cpu, 0x0400, &[0xA9, 0x00]); // LDA #$00
    cpu.step();
    assert_eq!(cpu.a, 0);
    assert!(cpu.p.contains(crate::flags::StatusFlags::ZERO));
}

#[test]
fn test_lda_negative_flag() {
    let mut cpu = make_cpu();
    load_program(&mut cpu, 0x0400, &[0xA9, 0x80]); // LDA #$80
    cpu.step();
    assert!(cpu.p.contains(crate::flags::StatusFlags::NEGATIVE));
}

#[test]
fn test_sta_zero_page() {
    let mut cpu = make_cpu();
    cpu.a = 0x55;
    load_program(&mut cpu, 0x0400, &[0x85, 0x10]); // STA $10
    cpu.step();
    assert_eq!(cpu.bus.ram[0x10], 0x55);
}

#[test]
fn test_adc_simple() {
    let mut cpu = make_cpu();
    cpu.a = 0x10;
    cpu.p.remove(crate::flags::StatusFlags::CARRY);
    load_program(&mut cpu, 0x0400, &[0x69, 0x20]); // ADC #$20
    cpu.step();
    assert_eq!(cpu.a, 0x30);
    assert!(!cpu.p.contains(crate::flags::StatusFlags::CARRY));
}

#[test]
fn test_adc_carry() {
    let mut cpu = make_cpu();
    cpu.a = 0xFF;
    cpu.p.remove(crate::flags::StatusFlags::CARRY);
    load_program(&mut cpu, 0x0400, &[0x69, 0x01]); // ADC #$01
    cpu.step();
    assert_eq!(cpu.a, 0x00);
    assert!(cpu.p.contains(crate::flags::StatusFlags::CARRY));
    assert!(cpu.p.contains(crate::flags::StatusFlags::ZERO));
}

#[test]
fn test_adc_overflow() {
    let mut cpu = make_cpu();
    cpu.a = 0x50;
    cpu.p.remove(crate::flags::StatusFlags::CARRY);
    load_program(&mut cpu, 0x0400, &[0x69, 0x50]); // ADC #$50
    cpu.step();
    assert_eq!(cpu.a, 0xA0);
    assert!(cpu.p.contains(crate::flags::StatusFlags::OVERFLOW));
    assert!(cpu.p.contains(crate::flags::StatusFlags::NEGATIVE));
}

#[test]
fn test_sbc_simple() {
    let mut cpu = make_cpu();
    cpu.a = 0x50;
    cpu.p.insert(crate::flags::StatusFlags::CARRY); // no borrow
    load_program(&mut cpu, 0x0400, &[0xE9, 0x10]); // SBC #$10
    cpu.step();
    assert_eq!(cpu.a, 0x40);
    assert!(cpu.p.contains(crate::flags::StatusFlags::CARRY)); // no borrow
}

#[test]
fn test_inx_dey() {
    let mut cpu = make_cpu();
    cpu.x = 0xFF;
    cpu.y = 0x01;
    load_program(&mut cpu, 0x0400, &[0xE8, 0x88]); // INX, DEY
    cpu.step();
    assert_eq!(cpu.x, 0x00);
    assert!(cpu.p.contains(crate::flags::StatusFlags::ZERO));
    cpu.step();
    assert_eq!(cpu.y, 0x00);
    assert!(cpu.p.contains(crate::flags::StatusFlags::ZERO));
}

#[test]
fn test_jmp_absolute() {
    let mut cpu = make_cpu();
    load_program(&mut cpu, 0x0400, &[0x4C, 0x00, 0x08]); // JMP $0800
    cpu.step();
    assert_eq!(cpu.pc, 0x0800);
}

#[test]
fn test_jmp_indirect_page_wrap_bug() {
    let mut cpu = make_cpu();
    // JMP ($02FF) - should exhibit the 6502 page-wrap bug
    load_program(&mut cpu, 0x0400, &[0x6C, 0xFF, 0x02]);
    cpu.bus.ram[0x02FF] = 0x34;
    cpu.bus.ram[0x0200] = 0x12; // Bug: reads from $0200 instead of $0300
    cpu.step();
    assert_eq!(cpu.pc, 0x1234);
}

#[test]
fn test_jsr_rts() {
    let mut cpu = make_cpu();
    // JSR $0500, then at $0500: LDA #$42, RTS
    load_program(&mut cpu, 0x0400, &[0x20, 0x00, 0x05]); // JSR $0500
    load_program(&mut cpu, 0x0500, &[0xA9, 0x42, 0x60]); // LDA #$42, RTS
    cpu.step(); // JSR
    assert_eq!(cpu.pc, 0x0500);
    cpu.step(); // LDA
    assert_eq!(cpu.a, 0x42);
    cpu.step(); // RTS
    assert_eq!(cpu.pc, 0x0403);
}

#[test]
fn test_push_pull() {
    let mut cpu = make_cpu();
    cpu.a = 0x77;
    load_program(&mut cpu, 0x0400, &[0x48, 0xA9, 0x00, 0x68]); // PHA, LDA #$00, PLA
    cpu.step(); // PHA
    cpu.step(); // LDA #$00
    assert_eq!(cpu.a, 0x00);
    cpu.step(); // PLA
    assert_eq!(cpu.a, 0x77);
}

#[test]
fn test_branch_taken_same_page() {
    let mut cpu = make_cpu();
    load_program(&mut cpu, 0x0400, &[0xA9, 0x00, 0xF0, 0x02, 0xEA, 0xEA, 0xA9, 0x42]);
    // LDA #$00, BEQ +2, NOP, NOP, LDA #$42
    cpu.step(); // LDA #$00
    let cycles = cpu.step(); // BEQ (taken, same page)
    assert_eq!(cpu.pc, 0x0406);
    assert_eq!(cycles, 3); // 2 base + 1 taken
}

#[test]
fn test_branch_not_taken() {
    let mut cpu = make_cpu();
    load_program(&mut cpu, 0x0400, &[0xA9, 0x01, 0xF0, 0x02, 0xA9, 0x42]);
    // LDA #$01, BEQ +2, LDA #$42
    cpu.step(); // LDA #$01
    let cycles = cpu.step(); // BEQ (not taken)
    assert_eq!(cpu.pc, 0x0404);
    assert_eq!(cycles, 2); // 2 base only
    cpu.step(); // LDA #$42
    assert_eq!(cpu.a, 0x42);
}

#[test]
fn test_cmp() {
    let mut cpu = make_cpu();
    cpu.a = 0x40;
    load_program(&mut cpu, 0x0400, &[0xC9, 0x40]); // CMP #$40
    cpu.step();
    assert!(cpu.p.contains(crate::flags::StatusFlags::ZERO));
    assert!(cpu.p.contains(crate::flags::StatusFlags::CARRY));
}

#[test]
fn test_bit() {
    let mut cpu = make_cpu();
    cpu.a = 0x0F;
    cpu.bus.ram[0x10] = 0xC0;
    load_program(&mut cpu, 0x0400, &[0x24, 0x10]); // BIT $10
    cpu.step();
    assert!(cpu.p.contains(crate::flags::StatusFlags::ZERO)); // 0x0F & 0xC0 == 0
    assert!(cpu.p.contains(crate::flags::StatusFlags::NEGATIVE)); // bit 7 of $C0
    assert!(cpu.p.contains(crate::flags::StatusFlags::OVERFLOW)); // bit 6 of $C0
}

#[test]
fn test_asl_accumulator() {
    let mut cpu = make_cpu();
    cpu.a = 0x81;
    load_program(&mut cpu, 0x0400, &[0x0A]); // ASL A
    cpu.step();
    assert_eq!(cpu.a, 0x02);
    assert!(cpu.p.contains(crate::flags::StatusFlags::CARRY));
}

#[test]
fn test_ror_accumulator() {
    let mut cpu = make_cpu();
    cpu.a = 0x01;
    cpu.p.insert(crate::flags::StatusFlags::CARRY);
    load_program(&mut cpu, 0x0400, &[0x6A]); // ROR A
    cpu.step();
    assert_eq!(cpu.a, 0x80);
    assert!(cpu.p.contains(crate::flags::StatusFlags::CARRY));
    assert!(cpu.p.contains(crate::flags::StatusFlags::NEGATIVE));
}

#[test]
fn test_indexed_indirect() {
    let mut cpu = make_cpu();
    cpu.x = 0x04;
    cpu.bus.ram[0x24] = 0x00;
    cpu.bus.ram[0x25] = 0x03;
    cpu.bus.ram[0x0300] = 0xAB;
    load_program(&mut cpu, 0x0400, &[0xA1, 0x20]); // LDA ($20,X)
    cpu.step();
    assert_eq!(cpu.a, 0xAB);
}

#[test]
fn test_indirect_indexed() {
    let mut cpu = make_cpu();
    cpu.y = 0x10;
    cpu.bus.ram[0x86] = 0x28;
    cpu.bus.ram[0x87] = 0x40;
    cpu.bus.ram[0x4038] = 0xCD;
    load_program(&mut cpu, 0x0400, &[0xB1, 0x86]); // LDA ($86),Y
    cpu.step();
    assert_eq!(cpu.a, 0xCD);
}

#[test]
fn test_brk_and_rti() {
    let mut cpu = make_cpu();
    // Set IRQ vector to $0600
    cpu.bus.ram[0xFFFE] = 0x00;
    cpu.bus.ram[0xFFFF] = 0x06;
    // ISR at $0600: RTI
    cpu.bus.ram[0x0600] = 0x40;
    // Program: LDA #$42, BRK
    load_program(&mut cpu, 0x0400, &[0xA9, 0x42, 0x00]);
    cpu.step(); // LDA
    let old_pc = cpu.pc;
    let old_p = cpu.p;
    cpu.step(); // BRK
    assert_eq!(cpu.pc, 0x0600);
    assert!(cpu.p.contains(crate::flags::StatusFlags::IRQ_DISABLE));
    cpu.step(); // RTI
    // RTI returns to BRK + 2 (padding byte)
    assert_eq!(cpu.pc, old_pc + 2);
    // Flags restored (except B)
    assert_eq!(
        cpu.p.bits() & 0xCF, // mask out B and unused
        old_p.bits() & 0xCF
    );
}

#[test]
fn test_nmi() {
    let mut cpu = make_cpu();
    // Set NMI vector to $0700
    cpu.bus.ram[0xFFFA] = 0x00;
    cpu.bus.ram[0xFFFB] = 0x07;
    // NMI handler: RTI
    cpu.bus.ram[0x0700] = 0x40;
    // Program: NOP, NOP
    load_program(&mut cpu, 0x0400, &[0xEA, 0xEA]);
    cpu.step(); // NOP
    cpu.bus.nmi_pending = true;
    cpu.step(); // triggers NMI
    assert_eq!(cpu.pc, 0x0700);
}

#[test]
fn test_absolute_x_page_cross_penalty() {
    let mut cpu = make_cpu();
    cpu.x = 0xFF;
    cpu.bus.ram[0x1100] = 0x42;
    load_program(&mut cpu, 0x0400, &[0xBD, 0x01, 0x10]); // LDA $1001,X → $1100 (page cross)
    let cycles = cpu.step();
    assert_eq!(cpu.a, 0x42);
    assert_eq!(cycles, 5); // 4 base + 1 page cross
}

#[test]
fn test_zero_page_x_wrap() {
    let mut cpu = make_cpu();
    cpu.x = 0xFF;
    cpu.bus.ram[0x0F] = 0x77; // $10 + $FF wraps to $0F
    load_program(&mut cpu, 0x0400, &[0xB5, 0x10]); // LDA $10,X
    cpu.step();
    assert_eq!(cpu.a, 0x77);
}

/// Klaus Dormann's 6502 functional test.
/// The test binary must be placed at test_roms/6502_functional_test.bin
/// It's a 64KB flat binary that runs from $0400.
/// Success = PC trapped at $3469.
#[test]
#[ignore]
fn test_klaus_dormann_functional() {
    let rom = std::fs::read("test_roms/6502_functional_test.bin")
        .expect("Place 6502_functional_test.bin in test_roms/");
    assert_eq!(rom.len(), 65536);

    let mut bus = TestBus::new();
    bus.ram.copy_from_slice(&rom);

    let mut cpu = Cpu6502::new(bus);
    cpu.pc = 0x0400;
    cpu.bcd_enabled = true;

    let mut prev_pc = 0u16;
    let mut same_count = 0u32;

    loop {
        cpu.step();

        if cpu.pc == prev_pc {
            same_count += 1;
            if same_count > 10 {
                if cpu.pc == 0x3469 {
                    println!("Klaus Dormann test PASSED (trapped at $3469)");
                    return;
                } else {
                    panic!("CPU trapped at ${:04X} (expected $3469)", cpu.pc);
                }
            }
        } else {
            same_count = 0;
            prev_pc = cpu.pc;
        }
    }
}
