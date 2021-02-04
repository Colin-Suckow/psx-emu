use std::{cell::RefCell, rc::Rc};

use bit_field::BitField;

use cop0::Cop0;
use instruction::{Instruction, NumberHelpers};

use crate::bus::MainBus;
use crate::timer::TimerState;
use crate::dma::DMAState;
use std::fs::File;
use std::path::Path;
use std::io::Write;

mod cop0;
mod instruction;

pub enum InterruptSource {
    VBLANK,
    GPU,
    CDROM,
    DMA,
    TMR0,
    TMR1,
    TMR2,
    Controller,
    SIO,
    SPU,
    Lightpen
}

pub enum Exception {
    IBE = 6,  //Bus error
    DBE = 7,  //Bus error Data
    AdEL = 4, //Address Error Load
    AdES = 5, //Address Error Store
    Ovf = 12, //Overflow
    Sys = 8,  //System Call
    Bp = 9,   //Breakpoint
    RI = 10,  //Reserved Instruction
    CpU = 11, //Co-processor Unusable
    TLBL = 2, //TLB Miss Load
    TLBS = 3, //TLB Miss Store
    Mod = 1,  // TLB modified
    Int = 0,  //Interrupt
}

struct LoadDelay {
    register: u8,
    value: u32,
}

pub struct R3000 {
    pub gen_registers: [u32; 32],
    pub pc: u32,
    old_pc: u32,
    hi: u32,
    lo: u32,
    pub main_bus: MainBus,
    delay_slot: u32,
    cop0: Cop0,
    load_delay: Option<LoadDelay>,
    i_mask: u32,
    pub i_status: u32,
    trace_file: File,
}

impl R3000 {
    pub fn new(bus: MainBus) -> R3000 {
        R3000 {
            gen_registers: [0; 32],
            pc: 0,
            old_pc: 0,
            hi: 0,
            lo: 0,
            main_bus: bus,
            delay_slot: 0,
            cop0: Cop0::new(),
            load_delay: None,
            i_mask: 0,
            i_status: 0,
            trace_file: File::create(Path::new("trace.txt")).unwrap(),
        }
    }
    /// Resets cpu registers to zero and sets program counter to reset vector (0xBFC00000)
    pub fn reset(&mut self) {
        //Clear registers
        for reg in self.gen_registers.iter_mut() {
            *reg = 0;
        }
        self.hi = 0;
        self.lo = 0;
        self.pc = 0xBFC00000; // Points to the bios entry point
        self.cop0
            .write_reg(12, self.cop0.read_reg(12).set_bit(23, true).clone());
    }

    /// Runs the next instruction based on the PC location. Only useful for testing because it is not at all accurate to
    /// how the cpu actually works.
    pub fn step_instruction(&mut self, timers: &mut TimerState) {

        //Check for vblank
        if self.main_bus.gpu.consume_vblank() {
            self.i_status.set_bit(0, true);
        };

        //Handle interrupts
        for i in 0..=10 {
            if self.cop0.interrupt_enabled() && self.i_status.get_bit(i) && self.i_mask.get_bit(i) {
                // println!("IMASK = {:#}", self.i_mask);
                //println!("Firing exception for irq {}", i);
                self.fire_exception(Exception::Int);
            }
        }

        //Execute delayed load
        if let Some(load) = self.load_delay.take() {
            self.write_reg(load.register, load.value);
        }

        let instruction = self.main_bus.read_word(self.pc);
        self.old_pc = self.pc;
        self.pc += 4;

        //println!("Executing {:#X} (FUNCT {:#X}) at {:#X} (FULL {:#X})", instruction.opcode(), instruction.funct(), self.old_pc, instruction);
        //self.trace_file.write(format!("{:08x}: {:08x}\n", self.old_pc, instruction).as_bytes());
        
        self.execute_instruction(instruction, timers);

        //Execute branch delay operation
        if self.delay_slot != 0 {
            let delay_instruction = self.main_bus.read_word(self.delay_slot);
            //println!("DS executing {:#X} (FUNCT {:#X}) at {:#X}",delay_instruction.opcode(), delay_instruction.funct(), self.old_pc + 4);
            //self.trace_file.write(format!("{:08x}: {:08x}\n", self.delay_slot, delay_instruction).as_bytes());
            self.execute_instruction(delay_instruction, timers);
            self.delay_slot = 0;
        }
    }

    pub fn execute_instruction(&mut self, instruction: u32, timers: &mut TimerState) {

        if self.pc % 4 != 0 || self.delay_slot % 4 != 0 {
            panic!("Address is not aligned!");
        }

        match instruction.opcode() {
            0x0 => {
                //SPECIAL INSTRUCTIONS
                match instruction.funct() {
                    0x0 => {
                        //SLL
                        if instruction == 0 {
                            //Actually a NOP
                            return;
                        }
                        self.write_reg(
                            instruction.rd(),
                            self.read_reg(instruction.rt()) << instruction.shamt(),
                        );
                    }

                    0x2 => {
                        //SRL
                        self.write_reg(
                            instruction.rd(),
                            self.read_reg(instruction.rt()) >> instruction.shamt(),
                        );
                    }

                    0x3 => {
                        //SRA
                        self.write_reg(
                            instruction.rd(),
                            (self.read_reg(instruction.rt()) as i32 >> instruction.shamt()) as u32,
                        );
                    }

                    0x4 => {
                        //SLLV
                        self.write_reg(
                            instruction.rd(),
                            ((self.read_reg(instruction.rt()))
                                << (self.read_reg(instruction.rs()) & 0x1F)) as u32
                        );
                    }

                    0x6 => {
                        //SRLV
                        self.write_reg(
                            instruction.rd(),
                            ((self.read_reg(instruction.rt()))
                                >> (self.read_reg(instruction.rs()) & 0x1F)) as u32
                        );
                    }

                    0x7 => {
                        //SRAV
                        self.write_reg(
                            instruction.rd(),
                            ((self.read_reg(instruction.rt()) as i32)
                                >> (self.read_reg(instruction.rs()) & 0x1F)) as u32
                        );
                    }

                    0x8 => {
                        //JR
                        self.delay_slot = self.pc;
                        self.pc = self.read_reg(instruction.rs());
                    }

                    0x9 => {
                        //JALR
                        self.delay_slot = self.pc;
                        self.pc = self.read_reg(instruction.rs());
                        self.write_reg(instruction.rd(), self.delay_slot + 4);
                    }

                    0xC => {
                        //SYSCALL
                        self.fire_exception(Exception::Sys);
                    }

                    0x10 => {
                        //MFHI
                        self.write_reg(instruction.rd(), self.hi);
                    }

                    0x11 => {
                        //MTHI
                        self.hi = self.read_reg(instruction.rs());
                    }

                    0x12 => {
                        //MFLO
                        self.write_reg(instruction.rd(), self.lo);
                    }

                    0x13 => {
                        //MTLO
                        self.lo = self.read_reg(instruction.rs());
                    }

                    0x1A => {
                        //DIV
                        let rs = self.read_reg(instruction.rs()) as i32;
                        let rt = self.read_reg(instruction.rt()) as i32;
                        self.lo = (rs / rt) as u32;
                        self.hi = (rs % rt) as u32;
                    }

                    0x1B => {
                        //DIVU
                        let rs = self.read_reg(instruction.rs());
                        let rt = self.read_reg(instruction.rt());
                        self.lo = rs / rt;
                        self.hi = rs % rt;
                    }

                    0x20 => {
                        //ADD
                        self.write_reg(
                            instruction.rd(),
                            match (self.read_reg(instruction.rs()) as i32)
                                .checked_add(self.read_reg(instruction.rt()) as i32)
                            {
                                Some(val) => val as u32,
                                None => panic!("ADD overflowed"),
                            },
                        )
                    }

                    0x2B => {
                        //SLTU
                        self.write_reg(
                            instruction.rd(),
                            (self.read_reg(instruction.rs()) < self.read_reg(instruction.rt()))
                                as u32,
                        );
                    }

                    0x23 => {
                        //SUBU
                        self.write_reg(
                            instruction.rd(),
                            (self.read_reg(instruction.rs()))
                                .wrapping_sub(self.read_reg(instruction.rt())),
                        );
                    }

                    0x24 => {
                        //AND
                        //println!("{} ({:#X}) & {} ({:#X}) = {} ({:#X})", instruction.rs(), self.read_reg(instruction.rs()), instruction.rt(), self.read_reg(instruction.rt()), instruction.rd(), self.read_reg(instruction.rs()) & self.read_reg(instruction.rt()));
                        self.write_reg(
                            instruction.rd(),
                            self.read_reg(instruction.rs()) & self.read_reg(instruction.rt()),
                        );
                    }

                    0x25 => {
                        //OR
                        self.write_reg(
                            instruction.rd(),
                            self.read_reg(instruction.rs()) | self.read_reg(instruction.rt()),
                        );
                    }

                    0x26 => {
                        //XOR
                        self.write_reg(
                            instruction.rd(),
                            self.read_reg(instruction.rs()) ^ self.read_reg(instruction.rt()),
                        );
                    }

                    0x27 => {
                        //NOR
                        self.write_reg(
                            instruction.rd(),
                            !(self.read_reg(instruction.rt()) | self.read_reg(instruction.rs())),
                        );
                    }

                    0x21 => {
                        //ADDU
                        self.write_reg(
                            instruction.rd(),
                            (self.read_reg(instruction.rt()))
                                .wrapping_add(self.read_reg(instruction.rs())),
                        );
                    }

                    0x19 => {
                        //MULTU
                        let result = (self.read_reg(instruction.rs()) as u64) * (self.read_reg(instruction.rt()) as u64);
                        self.lo = (result & 0xFFFF_FFFF) as u32;
                        self.hi = ((result >> 32) & 0xFFFF_FFFF) as u32;
                    }

                    0x2A => {
                        //SLT
                        self.write_reg(
                            instruction.rd(),
                            ((self.read_reg(instruction.rs()) as i32)
                                < (self.read_reg(instruction.rt()) as i32))
                                as u32,
                        );
                    }

                    _ => panic!(
                        "Unknown SPECIAL instruction. FUNCT is {0} ({0:#08b}, {0:#X})",
                        instruction.funct()
                    ),
                }
            }

            0x1 => {
                //"PC-relative" test and branch instructions
                match instruction.rt() {
                    0x0 => {
                        //BLTZ
                        if (self.read_reg(instruction.rs()) as i32) < 0 {
                            self.delay_slot = self.pc;
                            self.pc = ((instruction.immediate_sign_extended() << 2)
                                .wrapping_add(self.delay_slot));
                        }
                    }
                    0x1 => {
                        //BGEZ
                        if self.read_reg(instruction.rs()) as i32 >= 0 {
                            self.delay_slot = self.pc;
                            self.pc = ((instruction.immediate_sign_extended() << 2)
                                .wrapping_add(self.delay_slot));
                        }
                    }
                    _ => panic!(
                        "Unknown test and branch instruction {} ({0:#X})",
                        instruction.rt()
                    ),
                }
            }

            0x2 => {
                //J
                self.delay_slot = self.pc;
                self.pc = ((instruction.address() << 2)  | (self.delay_slot & 0xF0000000));
            }

            0x3 => {
                //JAL
                self.delay_slot = self.pc;
                self.write_reg(31, self.pc + 4);
                self.pc = ((instruction.address() << 2) | (self.delay_slot & 0xF0000000));
            }

            0x4 => {
                //BEQ
                if self.read_reg(instruction.rs()) == self.read_reg(instruction.rt()) {
                    self.delay_slot = self.pc;
                    self.pc = ((instruction.immediate_sign_extended() << 2)
                        .wrapping_add(self.delay_slot));
                }
            }

            0x5 => {
                //BNE
                if self.read_reg(instruction.rs()) != self.read_reg(instruction.rt()) {
                    self.delay_slot = self.pc;
                    self.pc = ((instruction.immediate_sign_extended() << 2)
                        .wrapping_add(self.delay_slot));
                }
            }

            0x6 => {
                //BLEZ
                if (self.read_reg(instruction.rs()) as i32) <= 0 {
                    self.delay_slot = self.pc;
                    self.pc = ((instruction.immediate_sign_extended() << 2)
                        .wrapping_add(self.delay_slot));
                }
            }

            0x7 => {
                //BGTZ
                if (self.read_reg(instruction.rs()) as i32) > 0 {
                    self.delay_slot = self.pc;
                    self.pc = ((instruction.immediate_sign_extended() << 2)
                        .wrapping_add(self.delay_slot));
                }
            }

            0x8 => {
                //ADDI
                self.write_reg(
                    instruction.rt(),
                    match (self.read_reg(instruction.rs()) as i32)
                        .checked_add(instruction.immediate_sign_extended() as i32)
                    {
                        Some(val) => val as u32,
                        None => panic!("ADDI overflowed"),
                    },
                );
            }

            0x9 => {
                //ADDIU
                //println!("Value {:#X}", instruction.immediate_sign_extended());
                self.write_reg(
                    instruction.rt(),
                    (self.read_reg(instruction.rs()))
                        .wrapping_add(instruction.immediate_sign_extended()),
                );
            }

            0xA => {
                //SLTI
                self.write_reg(
                    instruction.rt(),
                    (((self.read_reg(instruction.rs())) as i32)
                        < (instruction.immediate() as i32))
                        as u32,
                );
            }

            0xB => {
                //SLTIU
                self.write_reg(
                    instruction.rt(),
                    (self.read_reg(instruction.rs()) < instruction.immediate_sign_extended())
                        as u32,
                );
            }

            0xC => {
                //ANDI
                self.write_reg(
                    instruction.rt(),
                    (instruction & 0xFFFF) & self.read_reg(instruction.rs()),
                );
            }

            0xD => {
                //ORI
                self.write_reg(
                    instruction.rt(),
                    self.read_reg(instruction.rs()) | instruction.immediate().zero_extended(),
                );
            }
            0xF => {
                //LUI
                self.write_reg(instruction.rt(), (instruction.immediate() as u32) << 16);
            }

            0x10 => {
                match instruction.rs() {
                    0x4 => {
                        //MTC0
                        self.cop0
                            .write_reg(instruction.rd(), self.read_reg(instruction.rt()));
                    }
                    0x0 => {
                        //MFC0
                        //println!("Reading COP0 reg {}. Val {:#X}", instruction.rd(), self.cop0.read_reg(instruction.rd()));
                        self.write_reg(instruction.rt(), self.cop0.read_reg(instruction.rd()));
                    }

                    0x10 => {
                        //RFE
                        let status = self.cop0.read_reg(12);
                        self.cop0.write_reg(12, (status & 0xfffffff0) | ((status & 0x3c) >> 2));
                        self.pc = self.cop0.read_reg(14);
                    }
                    _ => (),
                }
            }

            0x20 => {
                //LB
                let addr = (instruction.immediate_sign_extended())
                    .wrapping_add(self.read_reg(instruction.rs()));
                let val = self.main_bus.read_byte(addr).sign_extended();
                self.write_reg(instruction.rt(), val);
            }

            0x21 => {
                //LH
                let addr = (instruction.immediate_sign_extended())
                    .wrapping_add(self.read_reg(instruction.rs()));
                let val = self.read_bus_half_word(addr, timers).sign_extended();
                self.write_reg(instruction.rt(), val);
            }

            0x23 => {
                //LW
                let addr = (instruction.immediate_sign_extended())
                    .wrapping_add(self.read_reg(instruction.rs()));
                let val = self.read_bus_word(addr, timers);
                //println!("LW read {:#X}. Storing in {}", val, instruction.rt());
                self.write_reg(instruction.rt(), val);
                // self.load_delay = Some(LoadDelay {
                //     register: instruction.rt(),
                //     value: val,
                // });
            }

            0x24 => {
                //LBU
                let addr = (instruction.immediate_sign_extended())
                    .wrapping_add(self.read_reg(instruction.rs()));
                let val = self.main_bus.read_byte(addr).zero_extended();
                self.write_reg(instruction.rt(), val);
            }

            0x25 => {
                //LHU
                let addr = (instruction.immediate_sign_extended())
                    .wrapping_add(self.read_reg(instruction.rs()));
                let val = self.read_bus_half_word(addr, timers).zero_extended();
                self.write_reg(instruction.rt(), val);
            }

            0x28 => {
                //SB
                let addr = instruction
                    .immediate()
                    .sign_extended()
                    .wrapping_add(self.read_reg(instruction.rs()));
                let val = (self.read_reg(instruction.rt()) & 0xFF) as u8;
                self.write_bus_byte(addr, val);
            }

            0x29 => {
                //SH
                let addr = instruction
                    .immediate()
                    .sign_extended()
                    .wrapping_add(self.read_reg(instruction.rs()));
                let val = (self.read_reg(instruction.rt()) & 0xFFFF) as u16;
                self.write_bus_half_word(addr, val, timers);
            }

            0x22 => {
                //LWL
                let addr = instruction
                    .immediate()
                    .sign_extended()
                    .wrapping_add(self.read_reg(instruction.rs()));

                let word = self.read_bus_word(addr & !3, timers);
                let reg_val = self.read_reg(instruction.rt());
                self.write_reg(instruction.rt(), match addr & 3 {
                    0 => (reg_val & 0x00ffffff) | (word << 24),
                    1 => (reg_val & 0x0000ffff) | (word << 16),
                    2 => (reg_val & 0x000000ff) | (word << 8),
                    3 => (reg_val & 0x00000000) | (word << 0),
                    _ => unreachable!(),
                });
                
            }

            0x26 => {
                //LWR
                let addr = instruction
                    .immediate()
                    .sign_extended()
                    .wrapping_add(self.read_reg(instruction.rs()));

                let word = self.read_bus_word(addr & !3, timers);
                let reg_val = self.read_reg(instruction.rt());
                self.write_reg(instruction.rt(), match addr & 3 {
                    3 => (reg_val & 0xffffff00) | (word >> 24),
                    2 => (reg_val & 0xffff0000) | (word >> 16),
                    1 => (reg_val & 0xff000000) | (word >> 8),
                    0 => (reg_val & 0x00000000) | (word >> 0),
                    _ => unreachable!(),
                });
            }

            0x2A => {
                //SWL
                let addr = instruction
                    .immediate()
                    .sign_extended()
                    .wrapping_add(self.read_reg(instruction.rs()));
                let word = self.read_bus_word(addr & !3, timers);
                let reg_val = self.read_reg(instruction.rt());
                self.write_bus_word(addr & !3, match addr & 3 {
                    0 => (word & 0xffffff00) | (reg_val >> 24),
                    1 => (word & 0xffff0000) | (reg_val >> 16),
                    2 => (word & 0xff000000) | (reg_val >> 8),
                    3 => (word & 0x00000000) | (reg_val >> 0),
                    _ => unreachable!(),
                }, timers);
            }

            0x2E => {
                //SWR
                let addr = instruction
                    .immediate()
                    .sign_extended()
                    .wrapping_add(self.read_reg(instruction.rs()));
                let word = self.read_bus_word(addr & !3, timers);
                let reg_val = self.read_reg(instruction.rt());
                self.write_bus_word(addr & !3, match addr & 3 {
                    0 => (word & 0x00000000) | (reg_val << 0),
                    1 => (word & 0x000000ff) | (reg_val << 8),
                    2 => (word & 0x0000ffff) | (reg_val << 16),
                    3 => (word & 0x00ffffff) | (reg_val << 24),
                    _ => unreachable!(),
                }, timers);
            }

            0x2B => {
                //SW
                //println!("R{} value {:#X}", instruction.rs(), self.read_reg(instruction.rs()));
                let addr = self
                    .read_reg(instruction.rs())
                    .wrapping_add(instruction.immediate_sign_extended());
                self.write_bus_word(addr, self.read_reg(instruction.rt()), timers);
            }
            _ => panic!(
                "Unknown opcode {0} ({0:#08b}, {0:#X})",
                instruction.opcode()
            ),
        };
    }

    pub fn fire_exception(&mut self, exception: Exception) {
        if self.delay_slot != 0 {
            panic!("Branch delay exception rollback is not implemented!");
        }
        self.cop0.set_cause_execode(exception);
        self.cop0.write_reg(14, self.pc);
        let old_status = self.cop0.read_reg(12);
        self.cop0.write_reg(12, (old_status & !0x3F) | (((old_status & 0x3f) << 2) & 0x3f));
        self.pc = if self.cop0.read_reg(12).get_bit(23) {
            0xBFC0_0180
        } else {
            0x8000_0080
        };

        self.cop0.write_reg(12, self.cop0.read_reg(12) << 4)
    }

    pub fn fire_external_interrupt(&mut self, source: InterruptSource) {
        let mask_bit = source as usize;
        //println!("mask_bit num = {}", mask_bit);

        self.i_status.set_bit(mask_bit, true);

        if self.i_mask.get_bit(mask_bit) {
            self.fire_exception(Exception::Int);
        }
    }

    fn read_bus_word(&mut self, addr: u32, timers: &mut TimerState) -> u32 {
        match addr {
            0x1F801070 => {
                //println!("Reading ISTATUS");
                self.i_status
            },
            0x1F801074 => self.i_mask,
            0x1F801100..=0x1F801128 => timers.read_word(addr),
            _ => self.main_bus.read_word(addr),
        }
    }

    fn write_bus_word(&mut self, addr: u32, val: u32, timers: &mut TimerState) {

        if self.cop0.cache_isolated() {
            //Cache is isolated, so don't write
            return;
        }

        match addr {
            0x1F801070 => {
                //println!("Writing I_STAT. val {:#X} pc {:#X} oldpc {:#X}", val, self.pc, self.old_pc);
                self.i_status &= val;
            },
            0x1F801074 => {
                //println!("Writing I_MASK val {:#X}", val);
                self.i_mask = val;
            },
            0x1F801100..=0x1F801128 => timers.write_word(addr, val),
            _ => self.main_bus.write_word(addr, val),
        };
    }

    fn read_bus_half_word(&mut self, addr: u32, timers: &mut TimerState) -> u16 {
        match addr {
            0x1F801070 => {
                self.i_status as u16
            },
            0x1F801074 => self.i_mask as u16,
            0x1F801100..=0x1F801128 => timers.read_half_word(addr),
            _ => self.main_bus.read_half_word(addr),
        }
    }

    fn write_bus_half_word(&mut self, addr: u32, val: u16, timers: &mut TimerState) {
        if self.cop0.cache_isolated() {
            //Cache is isolated, so don't write
            return;
        }
        match addr {
            0x1F801070 => {
                //println!("wrote ISTATUS {:#X}", val);
                self.i_status &= (val as u32);
            },
            0x1F801074 => self.i_mask = {
                //println!("Wrote IMASK {:#X}", val);
                val as u32
            },
            0x1F801100..=0x1F801128 => timers.write_half_word(addr, val),
            _ => self.main_bus.write_half_word(addr, val),
        };

    }

    fn write_bus_byte(&mut self, addr: u32, val: u8) {
        if self.cop0.cache_isolated() {
            //Cache is isolated, so don't write
            return;
        }
        self.main_bus.write_byte(addr, val);
    }

    /// Returns the value stored within the given register. Will panic if register_number > 31
    fn read_reg(&self, register_number: u8) -> u32 {
        self.gen_registers[register_number as usize]
    }

    /// Sets register to given value. Prevents setting R0, which should always be zero. Will panic if register_number > 31
    fn write_reg(&mut self, register_number: u8, value: u32) {
        match register_number {
            0 => (), //Prevent writing to the zero register
            _ => self.gen_registers[register_number as usize] = value,
        }
    }
}
