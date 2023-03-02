
#[allow(non_snake_case)]
pub struct NES {
    pub cycles: u64,

    pub A: u8,
    pub X: u8,
    pub Y: u8,
    /// An 'empty' stack that grows downwards, in memory area 0x0100 - 0x01FF.
    /// SP points to the next free location.
    /// See https://www.nesdev.org/wiki/Stack
    pub SP: u8,
    pub SR: StatusRegister,

    pub PC: u16,

    pub ram: [u8; 2048],
}

/// https://www.nesdev.org/wiki/Status_flags
#[allow(non_snake_case)]
pub struct StatusRegister {
    /// Carry
    pub C: bool,
    /// Zero
    pub Z: bool,
    /// Interrupt disable
    pub I: bool,
    /// Decimal
    pub D: bool,
    /// Overflow
    pub V: bool,
    /// Negative
    pub N: bool,
}

impl StatusRegister {
    pub const FLAG_C: u8 = 0b00000001;
    pub const FLAG_Z: u8 = 0b00000010;
    pub const FLAG_I: u8 = 0b00000100;
    pub const FLAG_D: u8 = 0b00001000;
    pub const FLAG_B: u8 = 0b00010000;
    pub const FLAG_U: u8 = 0b00100000;
    pub const FLAG_V: u8 = 0b01000000;
    pub const FLAG_N: u8 = 0b10000000;
}

pub const NES_NMI_VECTOR: u16 = 0xFFFA;
pub const NES_RESET_VECTOR: u16 = 0xFFFC;
pub const NES_IRQ_VECTOR: u16 = 0xFFFE;

impl StatusRegister {
    pub fn to_byte(&self) -> u8 {
        return
            (self.C as u8     ) |
            ((self.Z as u8) << 1) |
            ((self.I as u8) << 2) |
            ((self.D as u8) << 3) |
            StatusRegister::FLAG_U |
            ((self.V as u8) << 6) |
            ((self.N as u8) << 7);
    }

    pub fn from_byte(value: u8) -> StatusRegister {
        return StatusRegister {
            C: value & StatusRegister::FLAG_C != 0,
            Z: value & StatusRegister::FLAG_Z != 0,
            I: value & StatusRegister::FLAG_I != 0,
            D: value & StatusRegister::FLAG_D != 0,
            V: value & StatusRegister::FLAG_V != 0,
            N: value & StatusRegister::FLAG_N != 0,
        };
    }
}

impl NES {
    pub fn read8(&mut self, addr: u16) -> u8 {
        self.cycles += 1;
        if addr < 0x2000 {
            return self.ram[addr as usize % 0x800];
        }
        println!("out of bounds read from {:04X}", addr);
        0
    }

    pub fn read_addr(&mut self, addr: u16) -> u16 {
        let low = self.read8(addr);
        let high = self.read8(addr.wrapping_add(1));
        (high as u16) << 8 | (low as u16)
    }

    pub fn read_code(&mut self) -> u8 {
        let val = self.read8(self.PC);
        self.PC = self.PC.wrapping_add(1);
        val
    }

    pub fn read_code_addr(&mut self) -> u16 {
        let low = self.read_code();
        let high = self.read_code();
        (high as u16) << 8 | (low as u16)
    }

    pub fn write8(&mut self, addr: u16, val: u8) {
        self.cycles += 1;
        if addr < 0x2000 {
            self.ram[addr as usize % 0x800] = val;
        } else {
            println!("out of bounds write to {:04X} = {:02X}", addr, val);
        }
    }

    pub fn reset_state(&mut self) {
        self.SP = 0xFD;
    }

    pub fn set_status_register(&mut self, value: u8) {
        self.SR = StatusRegister::from_byte(value);
    }

    pub fn get_status_register(&mut self) -> u8 {
        self.SR.to_byte()
    }

    pub fn push8(&mut self, value: u8) {
        self.write8(0x0100 + self.SP as u16, value);
        self.SP = self.SP.wrapping_sub(1);
    }

    pub fn pop8(&mut self) -> u8 {
        self.SP = self.SP.wrapping_add(1);
        self.read8(0x0100 + self.SP as u16)
    }

    pub fn push16(&mut self, value: u16) {
        self.push8((value >> 8) as u8);
        self.push8((value & 0xFF) as u8);
    }

    pub fn pop16(&mut self) -> u16 {
        let low = self.pop8();
        let high = self.pop8();
        (high as u16) << 8 | (low as u16)
    }
}