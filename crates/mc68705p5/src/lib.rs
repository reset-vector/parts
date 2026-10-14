//! The Motorola MC68705P5 8-bit EPROM microcomputer, at its pins.
//!
//! Every item below names the claim it implements. The program is the host's:
//! the part is this core with its EPROM filled, and the crate carries no
//! program and no bootstrap ROM. The host supplies all 2 048 addresses as one
//! image ([`IMAGE_BYTES`]), and the part reads its EPROM, its mask option
//! register and its bootstrap ROM out of the positions the memory map gives
//! them (`MC68705P5-004`).
//!
//! # Examples
//!
//! ```
//! use mc68705p5::{IMAGE_BYTES, Inputs, Mc68705p5, vector};
//!
//! let mut image = [0u8; IMAGE_BYTES];
//! // LDA #$FF : STA $04 : LDA #$55 : STA $00 : BRA *, at $080.
//! let program = [0xa6, 0xff, 0xb7, 0x04, 0xa6, 0x55, 0xb7, 0x00, 0x20, 0xfe];
//! image[0x080..0x080 + program.len()].copy_from_slice(&program);
//! image[usize::from(vector::RESET)..][..2].copy_from_slice(&[0x00, 0x80]);
//! let mut mcu = Mc68705p5::with_image(&image)?;
//! for _ in 0..40 {
//!     mcu.step(Inputs::idle());
//! }
//! assert_eq!(mcu.outputs().pa, 0x55, "MC68705P5-016");
//! # Ok::<(), mc68705p5::ImageError>(())
//! ```
//!
//! # The shape
//!
//! One [`Mc68705p5::step`] is one instruction cycle, four periods of the
//! clock at `EXTAL` (`MC68705P5-003`), which [`EXTAL_CYCLES_PER_MACHINE_CYCLE`] states for a
//! host counting a faster clock. Each step samples the pins once, as
//! [`Inputs`], runs the timer for one cycle, and advances the processor by
//! one cycle. An instruction takes the cycles Table 7 gives it and **its
//! whole effect lands on its first cycle**, reads and writes alike, which is
//! this crate's choice under `MC68705P5-OPEN-02` and what `MC68705P5-041`
//! requires of the timer; what the ports hold afterwards is
//! [`Mc68705p5::outputs`].
//!
//! Every pin level is a [`Level`]. `INT` and `RESET` are the pins the sheet draws
//! with a bar, named `_n` here: `INT`'s event is a High-to-Low edge between two samples
//! (`MC68705P5-031`) and the part is held in reset while `RESET` is Low
//! (`MC68705P5-029`).
//!
//! # What this crate chooses
//!
//! Each open item names a choice of this crate's, marked where it is written.
//! The ones a host meets first are `MC68705P5-OPEN-01` — an opcode the map
//! leaves blank stops the part until reset, [`Mc68705p5::is_jammed`] — and
//! `MC68705P5-OPEN-02`, above. The rest:
//!
//! - The prescaler counts down from `$7F`, and `TDR` decrements on a borrow
//!   of the selected bits (`MC68705P5-OPEN-03`).
//! - One step passes between `RESET` rising and the first fetch: the step
//!   that sees `RESET` High loads the vector (`MC68705P5-OPEN-04`).
//! - `A`, `X`, `CC` less `I`, the RAM and the port latches start at zero
//!   (`MC68705P5-OPEN-05`).
//! - Extended and 16-bit indexed addresses drop the bits above `$7FF`
//!   (`MC68705P5-OPEN-06`).
//! - An `INT` edge seen between two steps is latched at the second and taken
//!   at the next instruction boundary with `I` clear (`MC68705P5-OPEN-07`).
//!
//! **Bootstrap mode is not modelled** (`MC68705P5-036`): the crate has no
//! `TIMER` level above `VCC`, and a reset always takes the vector at `$7FE`.

#![no_std]

mod cycles;
mod exec;
mod timer;

use cycles::cycles;
pub use timer::{mor, tcr};

/// Periods of the `EXTAL` clock in one instruction cycle (`MC68705P5-003`).
pub const EXTAL_CYCLES_PER_MACHINE_CYCLE: usize = 4;

/// The address space, and the length of the image the host supplies
/// (`MC68705P5-004`).
pub const IMAGE_BYTES: usize = 2048;

/// The bytes of RAM, `$010`–`$07F` (`MC68705P5-004`).
pub const RAM_BYTES: usize = 112;

/// The bits of the program control register, `PCR` at `$00B`
/// (`MC68705P5-035`).
pub mod pcr {
    /// Bit 0, `PLE`: set by reset.
    pub const PLE: u8 = 0x01;
    /// Bit 1, `PGE`: set by reset.
    pub const PGE: u8 = 0x02;
    /// Bit 2, `VPON`: read-only, 1 without programming voltage.
    pub const VPON: u8 = 0x04;
}

/// The addresses of the registers whose bits have a `pub mod` here
/// (`MC68705P5-004`).
pub mod reg {
    /// Where the RAM begins; it runs to `$07F` (`MC68705P5-004`).
    pub const RAM_START: u16 = 0x010;
    /// `TDR`, the timer's counter (`MC68705P5-022`).
    pub const TDR: u16 = 0x008;
    /// `TCR`, the timer control register, the [`tcr`](crate::tcr) bits.
    pub const TCR: u16 = 0x009;
    /// `PCR`, the program control register, the [`pcr`](crate::pcr) bits.
    pub const PCR: u16 = 0x00B;
    /// `MOR`, the mask option register, the [`mor`](crate::mor) bits
    /// (`MC68705P5-034`).
    pub const MOR: u16 = 0x784;
}

/// The vectors of `MC68705P5-005`, high byte first at the address named.
pub mod vector {
    /// The timer interrupt.
    pub const TIMER: u16 = 0x7F8;
    /// `INT`.
    pub const INT: u16 = 0x7FA;
    /// `SWI`.
    pub const SWI: u16 = 0x7FC;
    /// `RESET`.
    pub const RESET: u16 = 0x7FE;
}

/// `CC`'s half carry (`MC68705P5-007`).
pub const FLAG_H: u8 = 0x10;
/// `CC`'s interrupt mask (`MC68705P5-007`).
pub const FLAG_I: u8 = 0x08;
/// `CC`'s negative flag (`MC68705P5-007`).
pub const FLAG_N: u8 = 0x04;
/// `CC`'s zero flag (`MC68705P5-007`).
pub const FLAG_Z: u8 = 0x02;
/// `CC`'s carry/borrow (`MC68705P5-007`).
pub const FLAG_C: u8 = 0x01;

/// An image the part cannot hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum ImageError {
    /// The image is `len` bytes long, not `expected` ([`IMAGE_BYTES`]).
    Len {
        /// The length the image had.
        len: usize,
        /// The length it must have.
        expected: usize,
    },
}

impl core::fmt::Display for ImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Len { len, expected } => {
                write!(f, "an image of {len} bytes, not {expected}")
            }
        }
    }
}

impl core::error::Error for ImageError {}

/// A pin's level, as [`Inputs`] and [`Registers`] hold it.
pub use signal::Level;

/// The programmer-visible registers, as [`Mc68705p5::registers`] reads them
/// and [`Mc68705p5::at_boundary`] takes them: the processor's, every
/// register at `$000`–`$00B` a program reads or writes, the RAM, which holds
/// the stack, and the `INT` and `TIMER` edge detectors.
///
/// `Default` is every field 0, `INT` High and nothing latched, which is not
/// the state a reset leaves: it has the timer counting with its interrupt
/// unmasked.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`Mc68705p5::registers`] and sets the fields it wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// `A`.
    pub a: u8,
    /// `X`.
    pub x: u8,
    /// `SP`'s low byte. The part keeps it in `$60`–`$7F`: bits 7–5 read
    /// `011` whatever is set.
    pub sp: u8,
    /// `PC`, eleven bits; bits 15–11 are ignored.
    pub pc: u16,
    /// `CC` in bits 4–0, the `FLAG_*` constants ([`FLAG_C`]); bits 7–5 are
    /// ignored.
    pub cc: u8,
    /// Port A's data latch and data direction register (`MC68705P5-016`).
    pub port_a: Port,
    /// Port B's.
    pub port_b: Port,
    /// Port C's, bits 3–0; bits 7–4 are ignored.
    pub port_c: Port,
    /// `TDR`, the timer's counter (`MC68705P5-022`).
    pub tdr: u8,
    /// The bits of `TCR` the part stores: `TIR`, `TIM`, and with `MOR`'s
    /// `TOPT` at 0 also `TIN`, `TIE` and `PS2`–`PS0`. `PSC` is never stored,
    /// and with `TOPT` at 1 only `TIR` and `TIM` are (`MC68705P5-025`).
    pub tcr: u8,
    /// The timer's prescaler, seven bits; bit 7 is ignored. No instruction
    /// reads it, but a write of `PSC` sets it (`MC68705P5-023`), and it
    /// decides when `TDR` next counts.
    pub prescaler: u8,
    /// `PCR`'s `PGE` and `PLE`, bits 1–0, the [`pcr`] bits; bits 7–2 are
    /// ignored (`MC68705P5-035`).
    pub pcr: u8,
    /// The RAM, `$010` first (`MC68705P5-004`). The stack is its top 32
    /// bytes (`MC68705P5-006`).
    pub ram: [u8; RAM_BYTES],
    /// An `INT` edge is latched and not yet served (`MC68705P5-031`).
    pub int_pending: bool,
    /// `INT`'s level at the last step, for its falling edge
    /// (`MC68705P5-031`).
    pub int_n: Level,
    /// `TIMER`'s level at the last step, for its rising edge
    /// (`MC68705P5-024`).
    pub timer_level: Level,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            x: 0,
            sp: 0,
            pc: 0,
            cc: 0,
            port_a: Port::default(),
            port_b: Port::default(),
            port_c: Port::default(),
            tdr: 0,
            tcr: 0,
            prescaler: 0,
            pcr: 0,
            ram: [0; RAM_BYTES],
            int_pending: false,
            int_n: Level::High,
            timer_level: Level::Low,
        }
    }
}

/// The pins the part samples, once a step.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `PA7`–`PA0` as the board holds them. A bit whose data direction is
    /// output is not read (`MC68705P5-017`); port A's pull-ups are the
    /// host's to resolve (`MC68705P5-021`).
    pub pa: u8,
    /// `PB7`–`PB0`.
    pub pb: u8,
    /// `PC3`–`PC0` in bits 3–0; bits 7–4 are ignored.
    pub pc: u8,
    /// `TIMER` (`MC68705P5-024`).
    pub timer: Level,
    /// `INT`, High when idle.
    pub int_n: Level,
    /// `RESET`, High to run.
    pub reset_n: Level,
}

impl Inputs {
    /// Every port line High, `TIMER` Low, `INT` and `RESET` High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            pa: 0xFF,
            pb: 0xFF,
            pc: 0x0F,
            timer: Level::Low,
            int_n: Level::High,
            reset_n: Level::High,
        }
    }
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

/// One port's output side (`MC68705P5-016`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Port {
    /// The latched output data.
    pub data: u8,
    /// The data direction register: a bit set where the line is an output.
    pub direction: u8,
}

impl Port {
    /// The line levels with the part's outputs over the board's `pins`.
    const fn level(self, pins: u8) -> u8 {
        (self.data & self.direction) | (pins & !self.direction)
    }
}

/// The levels at the port lines.
///
/// A line whose direction bit is set carries its port's latch; one whose bit
/// is clear is an input the part does not drive, and carries the level last
/// presented in [`Inputs`]. [`Registers::port_a`] and its siblings say which
/// is which (`MC68705P5-016`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Outputs {
    /// `PA7`–`PA0`.
    pub pa: u8,
    /// `PB7`–`PB0`.
    pub pb: u8,
    /// `PC3`–`PC0` in bits 3–0; bits 7–4 read 0.
    pub pc: u8,
}

/// The three ports' latches and directions.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
struct PortFile {
    a: Port,
    b: Port,
    c: Port,
}

/// An MC68705P5.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Mc68705p5 {
    image: [u8; IMAGE_BYTES],
    ram: [u8; RAM_BYTES],
    a: u8,
    x: u8,
    sp: u8,
    pc: u16,
    cc: u8,
    ports: PortFile,
    timer: timer::Timer,
    /// `PGE` and `PLE`, the writable bits of the `PCR` (`MC68705P5-035`).
    pcr: u8,
    int_latch: bool,
    int_before: bool,
    in_reset: bool,
    /// Cycles left of the instruction or interrupt in progress.
    remaining: u8,
    jammed: bool,
    pins: Inputs,
}

impl Mc68705p5 {
    /// A part holding `image` at power-up, held in reset.
    fn new(image: [u8; IMAGE_BYTES]) -> Self {
        let mor = image[usize::from(reg::MOR)];
        let mut part = Self {
            image,
            ram: [0; RAM_BYTES],
            a: 0,
            x: 0,
            sp: 0x7F,
            pc: 0,
            cc: 0,
            ports: PortFile::default(),
            timer: timer::Timer::reset(mor),
            pcr: 0x03,
            int_latch: false,
            int_before: true,
            in_reset: true,
            remaining: 0,
            jammed: false,
            pins: Inputs::idle(),
        };
        part.reset();
        part
    }

    /// A part holding `image`, which must be [`IMAGE_BYTES`] long, at
    /// power-up and held in reset: the state `RESET` Low leaves
    /// (`MC68705P5-029`), with what reset does not set at zero
    /// (`MC68705P5-OPEN-05`). A board holds `RESET` Low to return it there;
    /// the first step that sees `RESET` High releases it.
    ///
    /// # Errors
    ///
    /// [`ImageError`] for any other length.
    ///
    /// # Examples
    ///
    /// ```
    /// use mc68705p5::{IMAGE_BYTES, Mc68705p5};
    ///
    /// let mcu = Mc68705p5::with_image(&[0; IMAGE_BYTES])?;
    /// assert!(mcu.is_in_reset());
    /// assert!(Mc68705p5::with_image(&[0; 1024]).is_err());
    /// # Ok::<(), mc68705p5::ImageError>(())
    /// ```
    pub fn with_image(image: &[u8]) -> Result<Self, ImageError> {
        let image: [u8; IMAGE_BYTES] = image.try_into().map_err(|_| ImageError::Len {
            len: image.len(),
            expected: IMAGE_BYTES,
        })?;
        Ok(Self::new(image))
    }

    /// A part holding `image`, which must be [`IMAGE_BYTES`] long, out of
    /// reset at an instruction boundary with these registers, RAM, latched
    /// `INT` and edge detectors: the next step begins an instruction at
    /// `regs.pc`, or an interrupt. `MOR` is the image's. Nothing else is
    /// carried: the pins last sampled are the next step's. A jammed part
    /// (`MC68705P5-OPEN-01`) stands at no boundary, so its registers build
    /// no part that runs on as it does. For a test bench; a board starts at
    /// [`Mc68705p5::with_image`]. The image is an argument because the part's memory
    /// map is built from it.
    ///
    /// # Errors
    ///
    /// [`ImageError`] for an image of any other length.
    ///
    /// # Examples
    ///
    /// ```
    /// use mc68705p5::{IMAGE_BYTES, Mc68705p5, Registers};
    ///
    /// let mut regs = Registers::default();
    /// regs.pc = 0x080;
    /// regs.a = 0x42;
    /// let mcu = Mc68705p5::at_boundary(&[0; IMAGE_BYTES], regs)?;
    /// assert_eq!(mcu.registers().a, 0x42);
    /// assert!(mcu.is_at_instruction_boundary());
    /// # Ok::<(), mc68705p5::ImageError>(())
    /// ```
    pub fn at_boundary(image: &[u8], regs: Registers) -> Result<Self, ImageError> {
        let mut part = Self::with_image(image)?;
        part.in_reset = false;
        part.a = regs.a;
        part.x = regs.x;
        part.sp = 0x60 | (regs.sp & 0x1F);
        part.pc = regs.pc & 0x7FF;
        part.cc = regs.cc & 0x1F;
        part.ports = PortFile {
            a: regs.port_a,
            b: regs.port_b,
            c: Port {
                data: regs.port_c.data & 0x0F,
                direction: regs.port_c.direction & 0x0F,
            },
        };
        part.timer.load(regs.tdr, regs.tcr, regs.prescaler);
        part.pcr = regs.pcr & 0x03;
        part.ram = regs.ram;
        part.int_latch = regs.int_pending;
        part.int_before = regs.int_n.is_high();
        part.timer.pin_before = regs.timer_level.is_high();
        Ok(part)
    }

    /// What `RESET` Low does, and keeps doing while it is held
    /// (`MC68705P5-029`, `MC68705P5-027`, `MC68705P5-035`).
    fn reset(&mut self) {
        self.cc |= FLAG_I;
        self.sp = 0x7F;
        self.ports.a.direction = 0;
        self.ports.b.direction = 0;
        self.ports.c.direction = 0;
        self.int_latch = false;
        self.timer = timer::Timer::reset(self.image[usize::from(reg::MOR)]);
        self.pcr = 0x03;
        self.in_reset = true;
        self.remaining = 0;
        self.jammed = false;
    }

    pub(crate) fn vector(&self, at: u16) -> u16 {
        let hi = u16::from(self.image[usize::from(at)] & 0x07);
        (hi << 8) | u16::from(self.image[usize::from(at + 1)])
    }

    /// One instruction cycle with the pins at `inputs`.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        self.pins = inputs;
        if inputs.reset_n.is_low() {
            self.reset();
            self.int_before = inputs.int_n.is_high();
            return self.lines();
        }
        if self.in_reset {
            self.reset();
            self.in_reset = false;
            self.pc = self.vector(vector::RESET);
            self.int_before = inputs.int_n.is_high();
            return self.lines();
        }
        if self.int_before && inputs.int_n.is_low() {
            self.int_latch = true;
        }
        self.int_before = inputs.int_n.is_high();
        self.timer.cycle(inputs.timer.is_high());
        if self.jammed {
            return self.lines();
        }
        if self.remaining == 0 {
            self.begin();
            if self.jammed {
                return self.lines();
            }
        }
        self.remaining -= 1;
        self.lines()
    }

    /// Begin a hardware interrupt, `INT` first, or the next instruction
    /// (`MC68705P5-030`); its whole effect lands now, on its first cycle
    /// (`MC68705P5-OPEN-02`).
    fn begin(&mut self) {
        if self.cc & FLAG_I == 0 {
            if self.int_latch {
                self.int_latch = false;
                self.interrupt(vector::INT);
                self.remaining = 11;
                return;
            }
            if self.timer.requesting() {
                self.interrupt(vector::TIMER);
                self.remaining = 11;
                return;
            }
        }
        let at = self.pc;
        let opcode = self.read(at);
        match cycles(opcode) {
            Some(n) => {
                self.pc = (at + 1) & 0x7FF;
                self.execute(opcode);
                self.remaining = n;
            }
            None => self.jammed = true,
        }
    }

    /// A read of `at` as the processor sees it, pins as last sampled
    /// (`MC68705P5-004`, `MC68705P5-017`, `MC68705P5-019`, `MC68705P5-020`).
    pub(crate) fn read(&self, at: u16) -> u8 {
        let at = at & 0x7FF;
        match at {
            0x000 => self.ports.a.level(self.pins.pa),
            0x001 => self.ports.b.level(self.pins.pb),
            0x002 => 0xF0 | self.ports.c.level(self.pins.pc & 0x0F),
            0x004..=0x006 => 0xFF,
            0x008 => self.timer.data,
            0x009 => self.timer.read_control(),
            0x00B => 0xFC | self.pcr,
            0x003 | 0x007 | 0x00A | 0x00C..=0x00F => 0xFF,
            0x010..=0x07F => self.ram[usize::from(at - reg::RAM_START)],
            _ => self.image[usize::from(at)],
        }
    }

    pub(crate) fn write(&mut self, at: u16, v: u8) {
        let at = at & 0x7FF;
        match at {
            0x000 => self.ports.a.data = v,
            0x001 => self.ports.b.data = v,
            0x002 => self.ports.c.data = v & 0x0F,
            0x004 => self.ports.a.direction = v,
            0x005 => self.ports.b.direction = v,
            0x006 => self.ports.c.direction = v & 0x0F,
            0x008 => self.timer.data = v,
            0x009 => self.timer.write_control(v),
            0x00B => self.pcr = v & 0x03,
            0x010..=0x07F => self.ram[usize::from(at - reg::RAM_START)] = v,
            _ => {}
        }
    }

    /// The port lines as they stand, over the levels last presented.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        self.lines()
    }

    const fn lines(&self) -> Outputs {
        Outputs {
            pa: self.ports.a.level(self.pins.pa),
            pb: self.ports.b.level(self.pins.pb),
            pc: self.ports.c.level(self.pins.pc & 0x0F),
        }
    }

    /// The part fetched an opcode Table 7 leaves blank and has stopped until
    /// reset (`MC68705P5-OPEN-01`). [`Registers::pc`] is the opcode's address.
    #[must_use]
    pub const fn is_jammed(&self) -> bool {
        self.jammed
    }

    /// Whether `RESET` holds the part, or has not yet been released since
    /// power-up.
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.in_reset
    }

    /// Whether the next step begins an instruction or an interrupt: false
    /// from power-up and while `RESET` holds the part, until the step that
    /// releases it has loaded the vector at `$7FE` (`MC68705P5-029`,
    /// `MC68705P5-OPEN-04`), and false while [`Mc68705p5::is_jammed`], since the
    /// next step begins nothing.
    #[must_use]
    pub const fn is_at_instruction_boundary(&self) -> bool {
        self.remaining == 0 && !self.in_reset && !self.jammed
    }

    /// The registers as they stand: `PC` eleven bits, `SP`'s low byte (the
    /// high three bits of the eleven are 0), `CC` in bits 4–0, the RAM and the
    /// `INT` and `TIMER` latches. At an instruction boundary,
    /// `at_boundary(image, part.registers())` builds a part that runs on as
    /// this one does.
    #[must_use]
    pub const fn registers(&self) -> Registers {
        Registers {
            a: self.a,
            x: self.x,
            sp: self.sp,
            pc: self.pc,
            cc: self.cc,
            port_a: self.ports.a,
            port_b: self.ports.b,
            port_c: self.ports.c,
            tdr: self.timer.data,
            tcr: self.timer.control,
            prescaler: self.timer.prescaler,
            pcr: self.pcr,
            ram: self.ram,
            int_pending: self.int_latch,
            int_n: Level::from_bool(self.int_before),
            timer_level: Level::from_bool(self.timer.pin_before),
        }
    }
}
