//! The Fujitsu MB8884 8-bit microcomputer, at its pins.
//!
//! Every item below names the claim or the open item it implements.
//!
//! # Examples
//!
//! ```
//! use mb8884::{Inputs, Mb8884, XTAL1_CYCLES_PER_MACHINE_CYCLE};
//!
//! // MOV A,#$5A : OUTL P1,A : JMP $003
//! let rom = [0x23, 0x5a, 0x39, 0x04, 0x03];
//! let mut mcu = Mb8884::new();
//! let mut pins = Inputs::idle();
//! let mut address = 0;
//! for _ in 0..8 * XTAL1_CYCLES_PER_MACHINE_CYCLE {
//!     let out = mcu.tick(pins);
//!     if out.ale.is_high() {
//!         address = out.d.unwrap_or(0);
//!     } else if out.psen_n.is_low() {
//!         pins.d = rom.get(usize::from(address)).copied().unwrap_or(0);
//!     }
//! }
//! assert_eq!(mcu.outputs().p1, 0x5a);
//! ```
//!
//! # The shape
//!
//! One [`Mb8884::tick`] is one period of the clock at `XTAL1`. Fifteen make
//! one machine cycle ([`XTAL1_CYCLES_PER_MACHINE_CYCLE`], `MB8884-002`), and an instruction
//! is one or two cycles (`MB8884-003`). Each cycle runs the same way at the
//! pins (`MB8884-OPEN-05`: this placement is the crate's own choice):
//!
//! - clocks 0–2: `ALE` High, the address's low byte on `DB`;
//! - clocks 3–11: the cycle's strobe Low — `/PSEN` for a fetch, `/RD` or
//!   `/WR` for a data or bus cycle, `PROG` for the expander — and `DB`
//!   sampled at the end of clock 11 by a read;
//! - clocks 12–14: every strobe High; at the end of clock 14 the timer
//!   counts, `T1` is sampled for the counter, and an instruction whose last
//!   cycle this was takes effect.
//!
//! During a fetch `P2`'s low four lines carry the address's bits 11–8;
//! otherwise they carry the port latch. The program is the host's: every
//! fetch is external (`MB8884-001`), so the host answers
//! `/PSEN` on `DB` from its own image.
//!
//! A pin's level is a [`Level`]. `RESET`, `INT`, `PSEN`, `RD` and `WR`, which
//! the sheet draws with a bar, and the strobe `PROG`, active Low, are named
//! `_n` here. The part is held in reset while
//! `RESET` is Low (`MB8884-005`).
//!
//! An opcode the MCS-48 map leaves blank runs as a one-cycle no-operation
//! (`MB8884-020`): the part does not stop.
//!
//! **What the crate chooses**, each an open item: `STRT T`, `STRT CNT`,
//! `STOP TCNT` and `MOV T,A` take one cycle each (`MB8884-OPEN-03`); `RESET`
//! keeps RAM, `A` and `T`, which start at 0 when the part is created
//! (`MB8884-OPEN-06`); an overflow records a timer request only while the
//! timer interrupt is enabled (`MB8884-OPEN-09`), and `DIS TCNTI` clears a
//! waiting one (`MB8884-OPEN-10`); `INT` is sampled at each instruction
//! boundary, taken before a timer request, and its call spends two cycles
//! (`MB8884-OPEN-11`); `ANL BUS,#` and `ORL BUS,#` change the bus latch and
//! strobe nothing (`MB8884-OPEN-12`); `T1` is sampled at the end of each
//! cycle (`MB8884-OPEN-13`); and `MOVD A,Pp` sets `P2`'s low four latch bits
//! to 1 (`MB8884-OPEN-14`).
//!
//! # What is not here
//!
//! `EA` Low and any internal program memory (`MB8884-OPEN-01`); `SS`,
//! single-step (`MB8884-OPEN-02`); the clock on `T0` after `ENT0 CLK`
//! (`MB8884-OPEN-04`). The open items are the full account.

#![no_std]

mod exec;

use exec::cycles;
pub use signal::Level;

/// Periods of the clock at `XTAL1` in one machine cycle (`MB8884-002`).
pub const XTAL1_CYCLES_PER_MACHINE_CYCLE: usize = 15;

/// Bytes of RAM inside the part (`MB8884-004`).
pub const RAM_BYTES: usize = 64;

/// Where the part starts and where its interrupts call.
pub mod vector {
    /// Where the counter stands when `RESET` is released (`MB8884-005`).
    pub const RESET: u16 = 0x000;
    /// The external interrupt's (`MB8884-025`).
    pub const INT: u16 = 0x003;
    /// The timer interrupt's (`MB8884-024`).
    pub const TIMER: u16 = 0x007;
}

/// The program status word's carry (`MB8884-007`).
pub const FLAG_C: u8 = 0x80;
/// The program status word's auxiliary carry, out of bit 3 (`MB8884-007`).
pub const FLAG_AC: u8 = 0x40;
/// The program status word's user flag 0 (`MB8884-007`).
pub const FLAG_F0: u8 = 0x20;
/// The program status word's register bank select (`MB8884-007`).
pub const FLAG_BS: u8 = 0x10;
/// The program status word's bit 3, which always reads 1 (`MB8884-007`).
pub const FLAG_ONE: u8 = 0x08;
/// The program status word's stack pointer, three bits (`MB8884-007`).
pub const SP_MASK: u8 = 0x07;

/// The programmer-visible registers, for [`Mb8884::registers`] and
/// [`Mb8884::at_boundary`]: the processor's, the port and bus latches, the
/// timer's and the interrupts' state the instructions set, the RAM, which
/// holds `R0`–`R7` of both banks and the return stack (`MB8884-006`,
/// `MB8884-008`), and the level the counter last sampled at `T1`.
///
/// `Default` is what a released `RESET` leaves (`MB8884-005`), with `A`, `T`
/// and the RAM 0 and `T1` High.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`Mb8884::registers`] and sets the fields it wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// The 64 bytes of RAM (`MB8884-004`).
    pub ram: [u8; RAM_BYTES],
    /// The accumulator.
    pub a: u8,
    /// The program counter, twelve bits; bits 15–12 are ignored.
    pub pc: u16,
    /// The program status word as `MOV A,PSW` reads it: the `FLAG_*` bits
    /// and the stack pointer, [`SP_MASK`]. Bit 3 reads 1 and is ignored by
    /// [`Mb8884::at_boundary`] (`MB8884-007`).
    pub psw: u8,
    /// `F1`, the second user flag.
    pub f1: bool,
    /// `MBF`, the memory bank flip-flop.
    pub mbf: bool,
    /// The timer or counter's value `T`.
    pub t: u8,
    /// `P1`'s latch.
    pub p1: u8,
    /// `P2`'s latch.
    pub p2: u8,
    /// The bus latch `OUTL BUS`, `ANL BUS` and `ORL BUS` write.
    pub bus: u8,
    /// `EN I` has enabled the external interrupt and `DIS I` has not since
    /// disabled it.
    pub int_enabled: bool,
    /// `EN TCNTI` has enabled the timer interrupt and `DIS TCNTI` has not
    /// since disabled it.
    pub timer_int_enabled: bool,
    /// The timer flag `JTF` tests and clears, set when `T` overflows.
    pub timer_flag: bool,
    /// An overflow has requested the timer interrupt and the request has
    /// not been taken (`MB8884-024`).
    pub timer_pending: bool,
    /// An interrupt routine runs: no interrupt is taken until `RETR`
    /// (`MB8884-026`).
    pub in_service: bool,
    /// What `T` counts (`MB8884-021` to `MB8884-023`).
    pub timer_mode: TimerMode,
    /// The timer's divide-by-32 prescaler, 0–31; `STRT T` clears it. A
    /// value above 31 is taken as 0.
    pub prescaler: u8,
    /// `ENT0 CLK` has run (`MB8884-OPEN-04`).
    pub t0_clock: bool,
    /// `T1` as the end of the last machine cycle sampled it, against which
    /// the counter finds its next High-to-Low edge (`MB8884-023`).
    pub t1: Level,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            ram: [0; RAM_BYTES],
            a: 0,
            pc: vector::RESET,
            psw: FLAG_ONE,
            f1: false,
            mbf: false,
            t: 0,
            p1: 0xFF,
            p2: 0xFF,
            bus: 0xFF,
            int_enabled: false,
            timer_int_enabled: false,
            timer_flag: false,
            timer_pending: false,
            in_service: false,
            timer_mode: TimerMode::Stopped,
            prescaler: 0,
            t0_clock: false,
            t1: Level::High,
        }
    }
}

/// The levels at the input pins for one period of the clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `RESET` (drawn with a bar): Low holds the part in reset, and every
    /// [`Mb8884::tick`] under it resets it again (`MB8884-005`).
    pub reset_n: Level,
    /// `INT` (drawn with a bar).
    pub int_n: Level,
    /// `T0`, read by `JT0` and `JNT0`.
    pub t0: Level,
    /// `T1`, read by `JT1` and `JNT1` and counted in counter mode.
    pub t1: Level,
    /// What the host drives on `DB` while a strobe reads it.
    pub d: u8,
    /// The levels the world holds `P1` at: a line reads Low if either the
    /// world or the latch pulls it Low (`MB8884-027`).
    pub p1: u8,
    /// The same for `P2`.
    pub p2: u8,
}

impl Inputs {
    /// Out of reset, nothing asserted, the ports and the bus pulled High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            reset_n: Level::High,
            int_n: Level::High,
            t0: Level::High,
            t1: Level::High,
            d: 0xFF,
            p1: 0xFF,
            p2: 0xFF,
        }
    }
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

/// The levels at the output pins at the end of one period of the clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `ALE`: High while the address is on `DB`.
    pub ale: Level,
    /// `PSEN` (drawn with a bar): Low while a program byte is read.
    pub psen_n: Level,
    /// `RD` (drawn with a bar).
    pub rd_n: Level,
    /// `WR` (drawn with a bar).
    pub wr_n: Level,
    /// `PROG`, a strobe active Low: Low while the expander takes a nibble.
    pub prog_n: Level,
    /// What the part drives on `DB`, or `None` where it floats.
    pub d: Option<u8>,
    /// `P1`'s latch.
    pub p1: u8,
    /// `P2`'s latch, its low four lines carrying the address during a
    /// fetch.
    pub p2: u8,
}

impl Outputs {
    const RESET: Self = Self {
        ale: Level::Low,
        psen_n: Level::High,
        rd_n: Level::High,
        wr_n: Level::High,
        prog_n: Level::High,
        d: None,
        p1: 0xFF,
        p2: 0xFF,
    };
}

/// What `T` counts (`MB8884-021` to `MB8884-023`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum TimerMode {
    /// Nothing: `STOP TCNT`, or a reset.
    #[default]
    Stopped,
    /// Machine cycles, one count every 32: `STRT T`.
    Timer,
    /// High-to-Low edges at `T1`: `STRT CNT`.
    Counter,
}

/// What the current machine cycle is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Cycle {
    /// The first cycle of an instruction: its opcode.
    Opcode,
    /// The second cycle of `op`.
    Second(u8, exec::Second),
    /// One of the two cycles an interrupt's call takes; `true` on the
    /// second, and the vector.
    Interrupt(bool, u16),
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mb8884 {
    ram: [u8; RAM_BYTES],
    a: u8,
    pc: u16,
    psw: u8,
    f1: bool,
    mbf: bool,
    in_irq: bool,
    xie: bool,
    tie: bool,
    tirq: bool,
    tf: bool,
    t: u8,
    prescaler: u8,
    mode: TimerMode,
    t1_last: bool,
    t0_clock: bool,
    p1: u8,
    p2: u8,
    bus: u8,
    operand_at: u16,
    held: bool,
    phase: u8,
    cycle: Cycle,
    sampled: u8,
    out: Outputs,
}

impl Default for Mb8884 {
    fn default() -> Self {
        Self::new()
    }
}

impl Mb8884 {
    /// A part that has just left reset. RAM and `T` hold 0 here; the part
    /// leaves both as they were (`MB8884-OPEN-06`).
    #[must_use]
    pub fn new() -> Self {
        let mut part = Self {
            ram: [0; RAM_BYTES],
            a: 0,
            pc: 0,
            psw: 0,
            f1: false,
            mbf: false,
            in_irq: false,
            xie: false,
            tie: false,
            tirq: false,
            tf: false,
            t: 0,
            prescaler: 0,
            mode: TimerMode::Stopped,
            t1_last: true,
            t0_clock: false,
            p1: 0xFF,
            p2: 0xFF,
            bus: 0xFF,
            operand_at: 0,
            held: false,
            phase: 0,
            cycle: Cycle::Opcode,
            sampled: 0,
            out: Outputs::RESET,
        };
        part.reset();
        part
    }

    /// A part standing at an instruction boundary with `regs` loaded, RAM
    /// included, and the rest as [`Self::new`] leaves it. Nothing else is
    /// held between instructions, so
    /// `at_boundary(part.registers())` at any boundary builds a part that
    /// runs on as `part` does. Until its first tick, [`Self::outputs`] reads
    /// `p1` and `p2` as `regs` holds them and the rest as [`Self::new`]
    /// leaves it, not as `part`'s last tick drove it.
    #[must_use]
    pub fn at_boundary(regs: Registers) -> Self {
        let mut part = Self::new();
        part.ram = regs.ram;
        part.t1_last = regs.t1.is_high();
        part.a = regs.a;
        part.pc = regs.pc & 0xFFF;
        part.psw = regs.psw & !FLAG_ONE;
        part.f1 = regs.f1;
        part.mbf = regs.mbf;
        part.t = regs.t;
        part.p1 = regs.p1;
        part.p2 = regs.p2;
        part.bus = regs.bus;
        part.xie = regs.int_enabled;
        part.tie = regs.timer_int_enabled;
        part.tf = regs.timer_flag;
        part.tirq = regs.timer_pending;
        part.in_irq = regs.in_service;
        part.mode = regs.timer_mode;
        part.prescaler = if regs.prescaler < 32 {
            regs.prescaler
        } else {
            0
        };
        part.t0_clock = regs.t0_clock;
        part.out.p1 = regs.p1;
        part.out.p2 = regs.p2;
        part
    }

    /// The registers as they stand, RAM included. Mid-instruction, the cycle
    /// under way is not among them; at an instruction boundary they are the
    /// whole of the part's state ([`Self::at_boundary`]).
    #[must_use]
    pub const fn registers(&self) -> Registers {
        Registers {
            ram: self.ram,
            t1: Level::from_bool(self.t1_last),
            a: self.a,
            pc: self.pc,
            psw: self.psw | FLAG_ONE,
            f1: self.f1,
            mbf: self.mbf,
            t: self.t,
            p1: self.p1,
            p2: self.p2,
            bus: self.bus,
            int_enabled: self.xie,
            timer_int_enabled: self.tie,
            timer_flag: self.tf,
            timer_pending: self.tirq,
            in_service: self.in_irq,
            timer_mode: self.mode,
            prescaler: self.prescaler,
            t0_clock: self.t0_clock,
        }
    }

    /// What `RESET` Low does (`MB8884-005`): the counter to 0, the PSW to 0
    /// (bank 0, stack pointer 0, the flags clear), F1 and `MBF` clear, both
    /// interrupts disabled and none in progress, the timer stopped with its
    /// flag clear, `P1` and `P2` High, `DB` floating. `A`, the registers,
    /// RAM and `T` are left as they were.
    fn reset(&mut self) {
        self.pc = vector::RESET;
        self.psw = 0;
        self.f1 = false;
        self.mbf = false;
        self.in_irq = false;
        self.xie = false;
        self.tie = false;
        self.tirq = false;
        self.tf = false;
        self.prescaler = 0;
        self.mode = TimerMode::Stopped;
        self.t0_clock = false;
        self.p1 = 0xFF;
        self.p2 = 0xFF;
        self.bus = 0xFF;
        self.phase = 0;
        self.cycle = Cycle::Opcode;
        self.out = Outputs::RESET;
    }

    /// One period of the clock at `XTAL1`.
    pub fn tick(&mut self, pins: Inputs) -> Outputs {
        let pins = &pins;
        if pins.reset_n.is_low() {
            self.reset();
            self.held = true;
            return self.out;
        }
        self.held = false;
        if self.phase == 0 && self.cycle == Cycle::Opcode {
            self.cycle = self.boundary(pins);
        }
        let k = self.phase;
        self.drive(k);
        if k == 11 {
            self.sampled = pins.d;
        }
        if usize::from(k) == XTAL1_CYCLES_PER_MACHINE_CYCLE - 1 {
            self.end_of_cycle(pins);
            self.phase = 0;
        } else {
            self.phase += 1;
        }
        self.out
    }

    /// An instruction boundary: an interrupt call, or an opcode fetch.
    /// `INT` Low is taken before a timer request (`MB8884-OPEN-11`), and
    /// neither while a routine runs (`MB8884-024` to `MB8884-026`).
    fn boundary(&mut self, pins: &Inputs) -> Cycle {
        if !self.in_irq {
            if self.xie && pins.int_n.is_low() {
                return Cycle::Interrupt(false, vector::INT);
            }
            if self.tirq {
                self.tirq = false;
                return Cycle::Interrupt(false, vector::TIMER);
            }
        }
        Cycle::Opcode
    }

    /// The pins for clock k of the current cycle.
    fn drive(&mut self, k: u8) {
        use exec::Second;
        let ale = k < 3;
        let strobe_n = Level::from_bool(!(3..12).contains(&k));
        let mut o = Outputs {
            ale: Level::from_bool(ale),
            psen_n: Level::High,
            rd_n: Level::High,
            wr_n: Level::High,
            prog_n: Level::High,
            d: None,
            p1: self.p1,
            p2: self.p2,
        };
        let fetch_at = match self.cycle {
            Cycle::Opcode => Some(self.pc),
            Cycle::Second(_, Second::Operand) => Some(self.pc),
            Cycle::Second(_, Second::FetchAt(a)) => Some(a),
            _ => None,
        };
        if let Some(a) = fetch_at {
            o.p2 = (self.p2 & 0xF0) | ((a >> 8) as u8 & 0x0F);
            if ale {
                o.d = Some(a as u8);
            }
            o.psen_n = strobe_n;
        }
        match self.cycle {
            Cycle::Second(_, Second::Read(addr)) => {
                if ale {
                    o.d = addr;
                }
                o.rd_n = strobe_n;
            }
            Cycle::Second(_, Second::Write(addr, data)) => {
                o.d = if ale {
                    addr
                } else if k < 12 {
                    Some(data)
                } else {
                    None
                };
                o.wr_n = strobe_n;
            }
            Cycle::Second(_, Second::Prog(n)) => {
                o.p2 = (self.p2 & 0xF0) | n;
                o.prog_n = strobe_n;
            }
            _ => {}
        }
        self.out = o;
    }

    fn end_of_cycle(&mut self, pins: &Inputs) {
        self.count(pins);
        match self.cycle {
            Cycle::Opcode => {
                let op = self.sampled;
                self.pc = next(self.pc);
                if cycles(op) == 1 {
                    self.execute(op, 0, pins);
                    self.cycle = Cycle::Opcode;
                } else {
                    self.cycle = Cycle::Second(op, self.second(op));
                }
            }
            Cycle::Second(op, second) => {
                if second == exec::Second::Operand {
                    self.operand_at = self.pc;
                    self.pc = next(self.pc);
                }
                let b = self.sampled;
                self.execute(op, b, pins);
                self.cycle = Cycle::Opcode;
            }
            Cycle::Interrupt(false, v) => self.cycle = Cycle::Interrupt(true, v),
            Cycle::Interrupt(true, v) => {
                self.push();
                self.in_irq = true;
                self.pc = v;
                self.cycle = Cycle::Opcode;
            }
        }
    }

    /// One cycle of the timer or the counter (`MB8884-021` to `MB8884-023`).
    fn count(&mut self, pins: &Inputs) {
        let edge = self.t1_last && pins.t1.is_low();
        self.t1_last = pins.t1.is_high();

        let tick = match self.mode {
            TimerMode::Stopped => false,
            TimerMode::Timer => {
                self.prescaler += 1;
                if self.prescaler == 32 {
                    self.prescaler = 0;
                    true
                } else {
                    false
                }
            }
            TimerMode::Counter => edge,
        };
        if tick {
            self.t = self.t.wrapping_add(1);
            if self.t == 0 {
                self.tf = true;
                if self.tie {
                    self.tirq = true;
                }
            }
        }
    }

    /// The output pins as the last [`Self::tick`] left them.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        self.out
    }

    /// The last [`Self::tick`] saw `RESET` Low, so the part is held in reset
    /// (`MB8884-005`).
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.held
    }

    /// Whether the next clock begins a machine cycle at an instruction
    /// boundary.
    #[must_use]
    pub fn is_at_instruction_boundary(&self) -> bool {
        self.phase == 0 && self.cycle == Cycle::Opcode && !self.held
    }
}

/// The counter's increment: eleven bits wrap, bit 11 stays (`MB8884-018`).
const fn next(pc: u16) -> u16 {
    (pc & 0x800) | ((pc + 1) & 0x7FF)
}
