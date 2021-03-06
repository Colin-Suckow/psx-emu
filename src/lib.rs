use bios::Bios;
use bus::MainBus;
use controller::{ButtonState, controller_execute_cycle, ControllerType};
use cpu::R3000;
use gpu::Resolution;
use log::trace;
use std::panic;
use timer::TimerState;

use crate::cdrom::disc::Disc;
use crate::cpu::InterruptSource;
use crate::dma::execute_dma_cycle;
use crate::gpu::Gpu;
use crate::memory::Memory;

mod bios;
mod bus;
pub mod cdrom;
pub mod controller;
pub mod cpu;
mod dma;
pub mod gpu;
mod memory;
mod spu;
mod timer;

static mut LOGGING: bool = false;

pub struct PSXEmu {
    pub r3000: R3000,
    timers: TimerState,
    cycle_count: u32,
    halt_requested: bool,
    sw_breakpoints: Vec<u32>,
    watchpoints: Vec<u32>
}

impl PSXEmu {
    /// Creates a new instance of the emulator.
    pub fn new(bios: Vec<u8>) -> PSXEmu {
        let bios = Bios::new(bios);
        let memory = Memory::new();
        let gpu = Gpu::new();
        let bus = MainBus::new(bios, memory, gpu);
        let r3000 = R3000::new(bus);

        let mut emu = PSXEmu {
            r3000: r3000,
            timers: TimerState::new(),
            cycle_count: 0,
            halt_requested: false,
            sw_breakpoints: Vec::new(),
            watchpoints: Vec::new(),
        };
        emu.reset();
        emu
    }

    /// Resets system to startup condition
    pub fn reset(&mut self) {
        self.r3000.reset();
        self.r3000.main_bus.gpu.reset();
    }

    /// Runs a single time unit. Each unit has the correct-ish ratio of cpu:gpu cycles
    pub fn step_cycle(&mut self) {
        for _ in 0..2 {
            if self.halt_requested {return};
            self.run_cpu_cycle();
            self.run_gpu_cycle();
        }

        //Two extra gpu cycles gets close enough to correct timing
        self.run_gpu_cycle();
        self.run_gpu_cycle();
    }

    pub fn run_cpu_cycle(&mut self) {
        if self.sw_breakpoints.contains(&self.r3000.pc) {
            self.halt_requested = true;
            return;
        }

        if self.watchpoints.contains(&self.r3000.last_touched_addr) {
            self.halt_requested = true;
            return;
        }

        
 
        controller_execute_cycle(&mut self.r3000);
        cdrom::step_cycle(&mut self.r3000);
        self.r3000.step_instruction(&mut self.timers);
        execute_dma_cycle(&mut self.r3000);
        self.cycle_count += 1;
        self.timers.update_sys_clock(&mut self.r3000);
        if self.cycle_count % 8 == 0 {
            self.timers.update_sys_div_8(&mut self.r3000);
        }
    }

    fn run_gpu_cycle(&mut self) {
        self.r3000.main_bus.gpu.execute_cycle();
        self.timers.update_dot_clock(&mut self.r3000);
        if self.r3000.main_bus.gpu.consume_hblank() {
            self.timers.update_h_blank(&mut self.r3000);
        }
    }

    ///Runs the emulator till one frame has been generated
    pub fn run_frame(&mut self) {
        while !self.r3000.main_bus.gpu.take_frame_ready() {
            self.step_cycle();
        }
        //Step the gpu once more to get it off this frame
        self.r3000.main_bus.gpu.execute_cycle();
    }

    pub fn load_executable(&mut self, start_addr: u32, entrypoint: u32, _sp: u32, data: &Vec<u8>) {
        for (index, val) in data.iter().enumerate() {
            self.r3000
                .main_bus
                .write_byte((index + start_addr as usize) as u32, val.clone());
        }
        self.r3000.load_exe = true;
        //self.r3000.pc = entrypoint;
        // self.r3000.gen_registers[29] = sp;
        // self.r3000.gen_registers[30] = sp;
    }

    pub fn load_disc(&mut self, disc: Disc) {
        self.r3000.main_bus.cd_drive.load_disc(disc);
    }

    pub fn loaded_disc(&self) -> &Option<Disc> {
        self.r3000.main_bus.cd_drive.disc()
    }

    pub fn remove_disc(&mut self) {
        self.r3000.main_bus.cd_drive.remove_disc();
    }

    pub fn get_vram(&self) -> &Vec<u16> {
        self.r3000.main_bus.gpu.get_vram()
    }

    pub fn get_bios(&self) -> &Vec<u8> {
        self.r3000.main_bus.bios.get_data()
    }

    pub fn manually_fire_interrupt(&mut self, source: InterruptSource) {
        self.r3000.fire_external_interrupt(source);
    }

    pub fn read_gen_reg(&self, reg_num: usize) -> u32 {
        self.r3000.gen_registers[reg_num]
    }

    pub fn set_gen_reg(&mut self, reg_num: usize, value: u32) {
        self.r3000.gen_registers[reg_num] = value;
    }

    pub fn halt_requested(&self) -> bool {
        self.halt_requested
    }

    pub fn clear_halt(&mut self) {
        self.halt_requested = false;
    }

    pub fn add_sw_breakpoint(&mut self, addr: u32) {
        println!("Adding breakpoint");
        self.sw_breakpoints.push(addr);
    }

    pub fn remove_sw_breakpoint(&mut self, addr: u32) {
        self.sw_breakpoints.retain(|&x| x != addr);
    }

    pub fn display_resolution(&self) -> Resolution {
        self.r3000.main_bus.gpu.resolution()
    }

    pub fn update_controller_state(&mut self, state: ButtonState) {
        self.r3000.main_bus.controllers.update_button_state(state);
    }

    pub fn frame_ready(&mut self) -> bool {
        self.r3000.main_bus.gpu.take_frame_ready()
    }

    pub fn add_watchpoint(&mut self, addr: u32) {
        println!("Adding watchpoint for addr {:#X} ({:#X} masked)", addr, addr & 0x1fffffff);
        self.watchpoints.push(addr & 0x1FFFFFFF);
    }

    pub fn remove_watchpoint(&mut self, addr: u32) {
        self.watchpoints.retain(|&x| x != addr & 0x1FFFFFFF);
    }
}
