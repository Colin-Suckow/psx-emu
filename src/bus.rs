use crate::bios::Bios;
use crate::gpu::Gpu;
use crate::memory::Memory;

pub struct MainBus {
    pub bios: Bios,
    memory: Memory,
    pub gpu: Gpu,
}

impl MainBus {
    pub fn new(bios: Bios, memory: Memory, gpu: Gpu) -> MainBus {
        MainBus { bios, memory, gpu }
    }

    pub fn read_word(&mut self, addr: u32) -> u32 {
        match addr {
            0x0..=0x001f_ffff => self.memory.read_word(addr), //KUSEG
            //0x8001_0000..=0x8001_f000 => self.bios.read_word(addr - 0x8001_0000), for test roms
            0x8000_0000..=0x801f_ffff => self.memory.read_word(addr - 0x8000_0000), //KSEG0
            0x1f801810 => self.gpu.read_word_gp0(),
            0x1f801814 => self.gpu.read_status_register(),
            0x1f80_1000..=0x1f80_2fff => {
                println!("Something tried to read the hardware control registers. These are not currently emulated, so a 0 is being returned. The address was {:#X}", addr);
                0
            }
            0xA000_0000..=0xA01f_ffff => self.memory.read_word(addr - 0xA000_0000), //KSEG1
            0xbfc0_0000..=0xbfc7_ffff => self.bios.read_word(addr - 0xbfc0_0000),
            _ => panic!(
                "Invalid word read at address {:#X}! This address is not mapped to any device.",
                addr
            ),
        }
    }

    pub fn write_word(&mut self, addr: u32, word: u32) {
        match addr {
            0x0..=0x001f_ffff => self.memory.write_word(addr, word), //KUSEG
            0x1F801074 => println!("IRQ mask write {:#b}", word),
            0x8000_0000..=0x801f_ffff => self.memory.write_word(addr - 0x8000_0000, word), //KSEG0
            0xA000_0000..=0xA01f_ffff => self.memory.write_word(addr - 0xA000_0000, word), //KSEG1
            0x1F801810 => self.gpu.send_gp0_command(word),
            0x1F801814 => self.gpu.send_gp1_command(word),
            0x1f80_1000..=0x1f80_2fff => println!("Something tried to write to the hardware control registers. These are not currently emulated. The address was {:#X}. Value {:#X}", addr, word),
            0xbfc0_0000..=0xbfc7_ffff => {
                panic!("Something tried to write to the bios rom. This is not a valid action")
            }
            0xFFFE0000..=0xFFFE0200 => (), //println!("Something tried to write to the cache control registers. These are not currently emulated. The address was {:#X}", addr),
            _ => panic!(
                "Invalid word write at address {:#X}! This address is not mapped to any device.",
                addr
            ),
        }
    }

    pub fn read_half_word(&self, addr: u32) -> u16 {
        match addr {
            0x8000_0000..=0x801f_ffff => self.memory.read_half_word(addr - 0x8000_0000), //KSEG0
            0x1f80_1000..=0x1f80_2fff => {
                //println!("Something tried to read the hardware control registers. These are not currently emulated, so a 0 is being returned. The address was {:#X}", addr);
                0
            },
            _ => panic!("Invalid half word read at address {:#X}! This address is not mapped to any device.", addr)
        }
    }

    pub fn write_half_word(&mut self, addr: u32, value: u16) {
        match addr {
            0x0..=0x001f_ffff => self.memory.write_half_word(addr, value), //KUSEG
            0x8000_0000..=0x801f_ffff => self.memory.write_half_word(addr - 0x8000_0000, value), //KSEG0
            0x1F80_1000..=0x1F80_2000 => (), //println!("Something tried to write to the I/O ports. This is not currently emulated. The address was {:#X}", addr),
            _ => panic!("Invalid half word write at address {:#X}! This address is not mapped to any device.", addr)
        }
    }

    pub fn read_byte(&self, addr: u32) -> u8 {
        match addr {
            0x0..=0x001f_ffff => self.memory.read_byte(addr), //KUSEG
            0x1F00_0000..=0x1f00_FFFF => {
                //println!("Something tried to read the parallel port. This is not currently emulated, so a 0 was returned. The address was {:#X}", addr);
                0
            }
            0x8000_0000..=0x801f_ffff => self.memory.read_byte(addr - 0x8000_0000), //KSEG0
            0xbfc0_0000..=0xbfc7_ffff => self.bios.read_byte(addr - 0xbfc0_0000),
            _ => panic!(
                "Invalid byte read at address {:#X}! This address is not mapped to any device.",
                addr
            ),
        }
    }

    pub fn write_byte(&mut self, addr: u32, value: u8) {
        match addr {
            0x1F80_2000..=0x1F80_3000 => (), //println!("Something tried to write to the second expansion port. This is not currently emulated. The address was {:#X}", addr),
            0x0..=0x001f_ffff => self.memory.write_byte(addr, value), //KUSEG
            0x8000_0000..=0x801f_ffff => self.memory.write_byte(addr - 0x8000_0000, value), //KSEG0
            0xA000_0000..=0xA01f_ffff => self.memory.write_byte(addr - 0xA000_0000, value), //KSEG1
            _ => panic!(
                "Invalid byte write at address {:#X}! This address is not mapped to any device.",
                addr
            ),
        }
    }
}
