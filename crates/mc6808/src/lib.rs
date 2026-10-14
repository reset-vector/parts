//! The Motorola MC6808, emulated at its pins.
//!
//! The crate depends only on `signal`, allocates nothing, does no I/O and is
//! `no_std`: an [`Mc6808`] is a plain value that advances one E cycle at a time
//! and shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! ```
//! use mc6808::{Ctrl, Mc6808, Pins};
//!
//! let mut cpu = Mc6808::new();
//! // Hold RESET for three cycles (`MC6808-030`), then let it go.
//! for _ in 0..3 {
//!     let _ = cpu.tick(Pins::idle().with(Ctrl::RESET));
//! }
//! let mut pins = Pins::idle();
//! let mut vector = 0;
//! for _ in 0..3 {
//!     pins = cpu.tick(pins);
//!     if pins.ctrl.contains(Ctrl::VMA) && pins.address >= 0xfffe {
//!         vector += 1;
//!     }
//!     pins = pins.with_d(0x00);
//! }
//! assert_eq!(vector, 2, "MC6808-030: the restart vector is read at $FFFE, $FFFF");
//! ```
//!
//! # The shape
//!
//! One [`Mc6808::tick`] is one E cycle, and every cycle is a bus cycle: the
//! caller presents the pins the outside world drives, with [`Pins::d`]
//! holding the byte the bus produced for the request the **previous** tick
//! returned, and receives the request for this cycle. A cycle with
//! [`Ctrl::VMA`] clear is not a memory reference (`MC6808-007`): its address
//! is the one Table 8 prints and nothing should answer it. The control inputs
//! presented with a tick are the levels during the cycle that tick completes.
//!
//! # The part, not the machine
//!
//! No decode, no memory: the MC6808 has no on-chip RAM (`MC6808-001`) and
//! every address is the board's. The crystal and its divide-by-four are the
//! board's to count (`MC6808-003`), and so is Memory Ready: a stretched `E`
//! is a longer cycle, and the caller simply calls [`Mc6808::tick`] once per
//! `E` however long it was (`MC6808-004`).
//!
//! # What this crate chooses
//!
//! Each open item names a choice of this crate's, marked where it is written.
//! The ones a consumer meets first are `MC6808-OPEN-01` (an undefined opcode
//! is a two-cycle no-operation), `MC6808-OPEN-02` (`SWI` sets `I`),
//! `MC6808-OPEN-04` (the hardware interrupt's twelve cycles), `MC6808-OPEN-06`
//! (reset's bus) and `MC6808-OPEN-07` (the registers start at zero, the CCR
//! at `$D0`). The rest:
//!
//! - `WAI`'s ninth cycle writes the CCR (`MC6808-OPEN-03`).
//! - While waiting, `WAI` shows `SP` with `VMA` Low and `R/W` High, ignores
//!   `HALT`, and leaves in four cycles: two `VMA`-Low at `SP`, then the
//!   vector's two bytes (`MC6808-OPEN-05`).
//! - `DAA` adds `$06` when `H` is set or the low digit is above nine, and
//!   `$60` when `C` is set, the high digit is above nine, or it is nine with
//!   a low digit above nine; `C` takes the `$60` and `V` is left
//!   (`MC6808-OPEN-08`).
//! - A branch's fourth cycle shows the target, taken or not
//!   (`MC6808-OPEN-09`).
//! - `CPX` takes `N` and `V` from the high bytes' own eight-bit difference
//!   (`MC6808-OPEN-10`).
//! - An interrupt is decided on the levels during an instruction's last
//!   cycle; an `NMI` edge is a High sample followed by a Low one, the line
//!   taken as High at power-up (`MC6808-OPEN-11`).
//! - Extended `JSR` forms its address from the low byte read in its third
//!   cycle, not its ninth (`MC6808-OPEN-12`).

#![no_std]

mod alu;
mod decode;

pub use alu::{C as FLAG_C, H as FLAG_H, I as FLAG_I, N as FLAG_N, V as FLAG_V, Z as FLAG_Z};
use decode::Addressing;
pub use decode::{
    Accumulator, Alu8, Alu16, Cond, Decoded, Inherent, Kind, Reg16, Target, Unary, decode,
};

use alu::{I, ONES};
pub use signal::Level;

/// The vectors of Table 1 (`MC6808-037`), high byte at the address named.
pub mod vector {
    /// Restart.
    pub const RESET: u16 = 0xfffe;
    /// Non-maskable interrupt.
    pub const NMI: u16 = 0xfffc;
    /// Software interrupt.
    pub const SWI: u16 = 0xfffa;
    /// Interrupt request.
    pub const IRQ: u16 = 0xfff8;
}

/// `EXTAL` clock periods per E cycle: the oscillator divides by four
/// (`MC6808-003`). The crate counts E; a host counting the crystal divides.
pub const EXTAL_CYCLES_PER_MACHINE_CYCLE: usize = 4;

/// The control signals.
///
/// A **set flag means the condition it names holds**, and the empty set is
/// every pin at its idle level. Several pins are **active Low**: an
/// active-low pin's flag bears the pin's name and set means asserted (Low).
/// Each constant's doc says which pin level it is.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u8);

impl Ctrl {
    /// No condition present.
    pub const NONE: Self = Self(0);

    /// Out. `R/W` is Low: [`Pins::d`] holds the byte the part drives.
    pub const WRITE: Self = Self(1 << 0);
    /// Out. `VMA` is High: the address is a valid memory reference
    /// (`MC6808-007`).
    pub const VMA: Self = Self(1 << 1);
    /// Out. `BA` is High: the part is halted or waiting (`MC6808-036`).
    pub const BA: Self = Self(1 << 2);

    /// In. `RESET` is Low (`MC6808-030`).
    pub const RESET: Self = Self(1 << 4);
    /// In. `NMI` is Low. The part responds to the falling edge
    /// (`MC6808-032`).
    pub const NMI: Self = Self(1 << 5);
    /// In. `IRQ` is Low: a level (`MC6808-031`).
    pub const IRQ: Self = Self(1 << 6);
    /// In. `HALT` is Low (`MC6808-035`).
    pub const HALT: Self = Self(1 << 7);

    /// Every signal in `other` is present here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// This word with the signals in `other` also set.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// This word with the signals in `other` cleared.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl core::ops::BitOr for Ctrl {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.with(rhs)
    }
}

impl core::ops::BitOrAssign for Ctrl {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.with(rhs);
    }
}

/// Named, so a failing assertion prints signals rather than a hex word.
impl core::fmt::Debug for Ctrl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        const NAMES: [(Ctrl, &str); 7] = [
            (Ctrl::WRITE, "WRITE"),
            (Ctrl::VMA, "VMA"),
            (Ctrl::BA, "BA"),
            (Ctrl::RESET, "RESET"),
            (Ctrl::NMI, "NMI"),
            (Ctrl::IRQ, "IRQ"),
            (Ctrl::HALT, "HALT"),
        ];
        if self.0 == 0 {
            return f.write_str("read");
        }
        let mut first = true;
        for (bit, name) in NAMES {
            if self.contains(bit) {
                if !first {
                    f.write_str("|")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        Ok(())
    }
}

/// The pin state for one E cycle, in both directions.
///
/// `Copy`, with no lifetime and no callback: a bus resolved through a
/// callback is a machine leaking into the chip.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub struct Pins {
    /// `A0`–`A15`. Driven every cycle; never three-state (`MC6808-006`).
    pub address: u16,
    /// `D0`–`D7`: `None` while nothing drives them. The part returns `Some`
    /// only on a write. A caller answers a read with `Some(byte)` on the next
    /// tick, and a read that finds `None` takes 0.
    pub d: Option<u8>,
    /// The control signals.
    pub ctrl: Ctrl,
}

impl Pins {
    /// All lines quiet: both buses at zero, no input asserted.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            address: 0,
            d: None,
            ctrl: Ctrl::NONE,
        }
    }

    /// These pins with the signals in `c` also set.
    #[must_use]
    pub const fn with(self, c: Ctrl) -> Self {
        Self {
            ctrl: self.ctrl.with(c),
            ..self
        }
    }

    /// These pins with the signals in `c` cleared.
    #[must_use]
    pub const fn without(self, c: Ctrl) -> Self {
        Self {
            ctrl: self.ctrl.without(c),
            ..self
        }
    }

    /// These pins with `d` driven on the data bus.
    #[must_use]
    pub const fn with_d(self, d: u8) -> Self {
        Self { d: Some(d), ..self }
    }
}

/// What the sequencer is running.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Run {
    /// Nothing yet: the next tick is a boundary.
    Idle,
    /// An instruction; step 1 is its opcode fetch.
    Op,
    /// The two vector reads after `RESET` rises (`MC6808-OPEN-06`).
    Reset,
    /// The twelve-cycle hardware interrupt (`MC6808-OPEN-04`).
    Interrupt,
    /// One cycle of `WAI`'s wait state (`MC6808-OPEN-05`).
    Wait,
    /// Leaving the wait state: four cycles (`MC6808-028`).
    WaitExit,
    /// One cycle halted (`MC6808-035`).
    Halt,
}

/// One bus cycle the part requests.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
struct Cyc {
    address: u16,
    vma: bool,
    write: Option<u8>,
}

const fn rd(address: u16) -> Cyc {
    Cyc {
        address,
        vma: true,
        write: None,
    }
}

const fn dead(address: u16) -> Cyc {
    Cyc {
        address,
        vma: false,
        write: None,
    }
}

const fn wr(address: u16, v: u8) -> Cyc {
    Cyc {
        address,
        vma: true,
        write: Some(v),
    }
}

/// The programmer-visible registers, as [`Mc6808::registers`] reads them and
/// [`Mc6808::at_boundary`] takes them, with the latches that carry an
/// interrupt or a wait across an instruction boundary.
///
/// `Default` is every register 0 but `ccr`, which holds its two unused bits
/// at 1 as the part reads them, with `NMI` last seen High, nothing pending and
/// not waiting.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`Mc6808::registers`] and sets the fields it wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// Accumulator A.
    pub a: u8,
    /// Accumulator B.
    pub b: u8,
    /// The index register.
    pub x: u16,
    /// The stack pointer: the next free byte (`MC6808-026`).
    pub sp: u16,
    /// The program counter.
    pub pc: u16,
    /// The condition code register; bits 6 and 7 always read as ones. Its
    /// flags are the `FLAG_` constants.
    pub ccr: u8,
    /// `WAI` has stacked the state and the part is in its wait state
    /// (`MC6808-028`).
    pub halted: bool,
    /// An `NMI` edge has been seen and not yet served (`MC6808-032`).
    pub nmi_pending: bool,
    /// `NMI`'s level at the last tick, for its edge.
    pub nmi_n: Level,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            b: 0,
            x: 0,
            sp: 0,
            pc: 0,
            ccr: ONES,
            halted: false,
            nmi_pending: false,
            nmi_n: Level::High,
        }
    }
}

/// The part.
///
/// Advance it with [`Mc6808::tick`], one E cycle per call; read its
/// registers with [`Mc6808::registers`].
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Mc6808 {
    a: u8,
    b: u8,
    x: u16,
    sp: u16,
    pc: u16,
    ccr: u8,

    held: bool,
    waiting: bool,
    run: Run,
    step: u8,
    total: u8,
    /// The address of the opcode, for an instruction; the return address,
    /// for a hardware interrupt.
    ip: u16,
    instr: Decoded,
    /// An address formed from the instruction's bytes, or a vector.
    ea: u16,
    /// A byte or a pair read as operand, or a memory unary's new value.
    w: u16,
    /// A memory unary's operand as read.
    operand: u8,
    sp0: u16,
    x0: u16,
    wrote: bool,
    /// `VMA` during the cycle the last tick returned.
    vma: bool,
    /// `BA` during the cycle the last tick returned.
    ba: bool,
    nmi_low: bool,
    nmi_pending: bool,
}

impl Default for Mc6808 {
    fn default() -> Self {
        Self::new()
    }
}

impl Mc6808 {
    /// The part at power-up, held as if `RESET` were Low: the first tick
    /// without [`Ctrl::RESET`] begins the restart (`MC6808-OPEN-06`,
    /// `MC6808-OPEN-07`).
    #[must_use]
    pub const fn new() -> Self {
        let mut cpu = Self::at_boundary(Registers {
            a: 0,
            b: 0,
            x: 0,
            sp: 0,
            pc: 0,
            ccr: I,
            halted: false,
            nmi_pending: false,
            nmi_n: Level::High,
        });
        cpu.held = true;
        cpu
    }

    /// The part at an instruction boundary with these registers, out of
    /// reset: the next tick fetches, takes an interrupt or a halt, or, with
    /// [`Registers::halted`], waits on. A pending `NMI`, `NMI`'s last level
    /// and the wait state are carried by `regs`; nothing else is in flight.
    /// Bits 6 and 7 of the CCR are set whatever `regs.ccr` says. Until its
    /// first tick, [`Mc6808::is_reading`], [`Mc6808::is_writing`] and
    /// [`Mc6808::is_bus_available`] read `false`, whatever the original's last
    /// cycle did. For a test bench; a
    /// board starts at [`Mc6808::new`].
    #[must_use]
    pub const fn at_boundary(regs: Registers) -> Self {
        Self {
            a: regs.a,
            b: regs.b,
            x: regs.x,
            sp: regs.sp,
            pc: regs.pc,
            ccr: regs.ccr | ONES,
            held: false,
            waiting: regs.halted,
            run: Run::Idle,
            step: 0,
            total: 0,
            ip: 0,
            instr: decode(0x01),
            ea: 0,
            w: 0,
            operand: 0,
            sp0: 0,
            x0: 0,
            wrote: false,
            vma: false,
            ba: false,
            nmi_low: regs.nmi_n.is_low(),
            nmi_pending: regs.nmi_pending,
        }
    }

    /// The next tick begins an instruction, a hardware interrupt or a halted
    /// cycle (`MC6808-034`), or is one more cycle of `WAI`'s wait state:
    /// nothing is in flight. The byte of the last read, and what it does to
    /// the registers, still comes with that tick. False while `RESET` holds
    /// the part and through a halted cycle, since `HALT` is a pin and not a
    /// register.
    #[must_use]
    pub const fn is_at_instruction_boundary(&self) -> bool {
        let fetching = matches!(self.run, Run::Op) && self.step == 1;
        let halt = matches!(self.run, Run::Halt);
        !self.held && !halt && !fetching && self.step >= self.total
    }

    /// The registers as they stand: what the instructions have written so
    /// far, the pending `NMI`, `NMI`'s last level and the wait state. The
    /// instruction that has just ended reaches them, with the last read's
    /// byte, only with the next tick, so at an instruction boundary
    /// ([`Mc6808::is_at_instruction_boundary`]) they read the boundary, and
    /// `Mc6808::at_boundary(cpu.registers())` runs on as this part does,
    /// only after [`Mc6808::settle`]. The levels of `IRQ` and `HALT` are
    /// the board's and are not carried.
    #[must_use]
    pub const fn registers(&self) -> Registers {
        Registers {
            a: self.a,
            b: self.b,
            x: self.x,
            sp: self.sp,
            pc: self.pc,
            ccr: self.ccr,
            halted: self.waiting,
            nmi_pending: self.nmi_pending,
            nmi_n: Level::from_bool(!self.nmi_low),
        }
    }

    /// `WAI` has stopped the part until an interrupt or a reset: it is in the
    /// wait state (`MC6808-028`).
    #[must_use]
    pub const fn is_halted(&self) -> bool {
        self.waiting
    }

    /// The bus is available to the board: the cycle the last tick returned
    /// drives `BA` High, as a halted cycle (`HALT` Low at the boundary,
    /// `MC6808-035`) or a cycle of `WAI`'s wait state (`MC6808-036`) does.
    /// [`Mc6808::is_halted`] tells the two apart. The part does not release the
    /// bus: its address lines stay driven (`MC6808-006`) and `VMA` is Low.
    /// [`Mc6808::settle`] leaves it as the last tick drove it.
    #[must_use]
    pub const fn is_bus_available(&self) -> bool {
        self.ba
    }

    /// The cycle the last tick returned is a valid memory read: `VMA` High
    /// and `R/W` High.
    #[must_use]
    pub const fn is_reading(&self) -> bool {
        self.vma && !self.wrote
    }

    /// The cycle the last tick returned is a valid memory write: `VMA` High
    /// and `R/W` Low. `TST`'s write cycle drives `R/W` Low with `VMA` Low and
    /// writes nothing.
    #[must_use]
    pub const fn is_writing(&self) -> bool {
        self.vma && self.wrote
    }

    /// The last tick's cycle reads an opcode.
    #[must_use]
    pub const fn is_fetching(&self) -> bool {
        matches!(self.run, Run::Op) && self.step == 1
    }

    /// `RESET` holds the part, or has not yet been released since power-up
    /// (`MC6808-030`).
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.held
    }

    /// At an instruction boundary, take the outstanding read's byte from
    /// `pins.d` and finish the instruction that has just ended without
    /// starting the next, so [`Mc6808::registers`] reads the boundary. An
    /// `NMI` edge on `pins` is latched as the next tick latches it, into
    /// [`Registers::nmi_pending`] and [`Registers::nmi_n`] (`MC6808-032`).
    /// Idempotent; ticking on afterwards is correct, and the next tick
    /// ignores `d`.
    ///
    /// Returns whether the part now stands at a boundary that
    /// [`Mc6808::at_boundary`] rebuilds from [`Mc6808::registers`]: `false`,
    /// changing nothing, elsewhere, and with [`Ctrl::RESET`] on `pins`, under
    /// which that tick drops the byte and the instruction's end
    /// (`MC6808-030`). A part is rebuilt at a later boundary instead.
    #[must_use]
    pub fn settle(&mut self, pins: Pins) -> bool {
        if pins.ctrl.contains(Ctrl::RESET) || !self.is_at_instruction_boundary() {
            return false;
        }
        self.nmi_edge(pins.ctrl);
        if !self.wrote {
            self.latch(pins.d.unwrap_or(0));
        }
        self.finish();
        true
    }

    /// One E cycle.
    #[must_use]
    pub fn tick(&mut self, pins: Pins) -> Pins {
        let ctrl = pins.ctrl;
        let nmi = ctrl.contains(Ctrl::NMI);
        if ctrl.contains(Ctrl::RESET) {
            self.held = true;
            self.waiting = false;
            self.nmi_pending = false;
            self.nmi_low = nmi;
            self.ccr |= I;
            self.run = Run::Idle;
            self.step = 0;
            self.total = 0;
            return self.out(dead(vector::RESET), false);
        }
        self.nmi_edge(ctrl);
        if self.held {
            self.held = false;
            return self.start(Run::Reset, 2);
        }
        if !self.wrote {
            self.latch(pins.d.unwrap_or(0));
        }
        if self.step < self.total {
            self.step += 1;
            let c = self.emit();
            return self.out(c, false);
        }
        self.finish();
        self.boundary(ctrl)
    }

    /// `MC6808-032`: the falling edge, whatever `I` and `HALT` say.
    const fn nmi_edge(&mut self, ctrl: Ctrl) {
        let nmi = ctrl.contains(Ctrl::NMI);
        if nmi && !self.nmi_low {
            self.nmi_pending = true;
        }
        self.nmi_low = nmi;
    }

    fn out(&mut self, c: Cyc, ba: bool) -> Pins {
        let mut ctrl = Ctrl::NONE;
        if c.vma {
            ctrl |= Ctrl::VMA;
        }
        if c.write.is_some() {
            ctrl |= Ctrl::WRITE;
        }
        if ba {
            ctrl |= Ctrl::BA;
        }
        self.wrote = c.write.is_some();
        self.vma = c.vma;
        self.ba = ba;
        Pins {
            address: c.address,
            d: c.write,
            ctrl,
        }
    }

    fn start(&mut self, run: Run, total: u8) -> Pins {
        self.run = run;
        self.step = 1;
        self.total = total;
        let c = self.emit();
        let ba = matches!(run, Run::Wait | Run::Halt);
        self.out(c, ba)
    }

    /// `MC6808-034`: `HALT`, then `NMI`, then `IRQ` against the mask, then
    /// the next opcode. In the wait state only the two interrupts are
    /// looked at.
    fn boundary(&mut self, ctrl: Ctrl) -> Pins {
        let irq = ctrl.contains(Ctrl::IRQ) && self.ccr & I == 0;
        if self.waiting {
            if self.nmi_pending || irq {
                self.waiting = false;
                return self.start(Run::WaitExit, 4);
            }
            return self.start(Run::Wait, 1);
        }
        if ctrl.contains(Ctrl::HALT) {
            return self.start(Run::Halt, 1);
        }
        if self.nmi_pending || irq {
            self.ip = self.pc;
            self.sp0 = self.sp;
            return self.start(Run::Interrupt, 12);
        }
        self.ip = self.pc;
        self.start(Run::Op, 1)
    }

    /// The interrupt vector, chosen when the sequence reaches it: `NMI` if
    /// one is pending, else `IRQ` (`MC6808-OPEN-04`). `I` is set with it.
    fn choose_vector(&mut self) {
        self.ccr |= I;
        self.ea = if self.nmi_pending {
            self.nmi_pending = false;
            vector::NMI
        } else {
            vector::IRQ
        };
    }

    /// The `k`th byte of the machine state pushed by `SWI`, `WAI` and an
    /// interrupt, at `SP - k` (`MC6808-026`); `ret` is the `PC` stacked.
    fn push(&self, k: u8, ret: u16) -> Cyc {
        let v = match k {
            0 => ret as u8,
            1 => (ret >> 8) as u8,
            2 => self.x as u8,
            3 => (self.x >> 8) as u8,
            4 => self.a,
            5 => self.b,
            _ => self.ccr | ONES,
        };
        wr(self.sp0.wrapping_sub(u16::from(k)), v)
    }

    fn acc(&self, a: Accumulator) -> u8 {
        match a {
            Accumulator::A => self.a,
            Accumulator::B => self.b,
        }
    }

    fn set_acc(&mut self, a: Accumulator, v: u8) {
        match a {
            Accumulator::A => self.a = v,
            Accumulator::B => self.b = v,
        }
    }

    fn reg16(&self, r: Reg16) -> u16 {
        match r {
            Reg16::X => self.x,
            Reg16::S => self.sp,
        }
    }

    /// The step at which an instruction's operand is read, and at which an
    /// indexed or extended address is complete (Table 8).
    const fn operand_step(addressing: Addressing) -> u8 {
        match addressing {
            Addressing::Immediate => 2,
            Addressing::Direct => 3,
            Addressing::Extended => 4,
            _ => 5,
        }
    }

    /// The address `X` plus the offset shows before the carry has reached
    /// its high byte (`MC6808-009`).
    const fn without_carry(&self) -> u16 {
        (self.x0 & 0xff00) | (self.ea & 0x00ff)
    }

    /// The bus cycle for `self.step`.
    fn emit(&mut self) -> Cyc {
        let s = self.step;
        match self.run {
            Run::Idle => dead(self.pc),
            Run::Reset => rd(vector::RESET + u16::from(s - 1)),
            Run::Halt => dead(self.pc),
            Run::Wait => dead(self.sp),
            Run::WaitExit => match s {
                1 => {
                    self.choose_vector();
                    dead(self.sp)
                }
                2 => dead(self.sp),
                _ => rd(self.ea + u16::from(s - 3)),
            },
            Run::Interrupt => match s {
                1 | 2 => dead(self.ip),
                3..=9 => self.push(s - 3, self.ip),
                10 => {
                    self.choose_vector();
                    dead(self.sp0.wrapping_sub(7))
                }
                _ => rd(self.ea + u16::from(s - 11)),
            },
            Run::Op => {
                if s == 1 {
                    rd(self.ip)
                } else {
                    self.emit_op(s)
                }
            }
        }
    }

    /// A store row's cycle for step `s`, writing `bytes` high first; an
    /// 8-bit store passes its byte twice.
    fn store_cycle(&self, addressing: Addressing, s: u8, bytes: [u8; 2]) -> Cyc {
        let ea = self.ea;
        let ws = Self::operand_step(addressing) + 1;
        match (addressing, s) {
            (Addressing::Indexed, 2) | (Addressing::Extended, 2 | 3) | (Addressing::Direct, 2) => {
                rd(self.ip.wrapping_add(u16::from(s) - 1))
            }
            (Addressing::Indexed, 3) => dead(self.x0),
            (Addressing::Indexed, 4) => dead(self.without_carry()),
            _ if s < ws => dead(ea),
            _ => {
                let v = if s == ws { bytes[0] } else { bytes[1] };
                wr(ea.wrapping_add(u16::from(s - ws)), v)
            }
        }
    }

    /// Table 8, row by row (`MC6808-008` to `MC6808-012`).
    fn emit_op(&mut self, s: u8) -> Cyc {
        let ip = self.ip;
        let next = |k: u16| ip.wrapping_add(k);
        let Decoded {
            kind, addressing, ..
        } = self.instr;
        let ea = self.ea;
        match kind {
            Kind::Alu8(..) | Kind::Alu16(..) | Kind::Ld16(_) => {
                let os = Self::operand_step(addressing);
                match (addressing, s) {
                    (Addressing::Immediate, _) => rd(next(u16::from(s) - 1)),
                    (Addressing::Indexed, 2)
                    | (Addressing::Extended, 2 | 3)
                    | (Addressing::Direct, 2) => rd(next(u16::from(s) - 1)),
                    (Addressing::Indexed, 3) => dead(self.x0),
                    (Addressing::Indexed, 4) => dead(self.without_carry()),
                    _ => rd(ea.wrapping_add(u16::from(s - os))),
                }
            }
            Kind::St8(a) => {
                let v = self.acc(a);
                self.store_cycle(addressing, s, [v, v])
            }
            Kind::St16(r) => self.store_cycle(addressing, s, self.reg16(r).to_be_bytes()),
            Kind::Unary(u, Target::Memory) => {
                let os = Self::operand_step(addressing);
                match (addressing, s) {
                    (Addressing::Indexed, 2) | (Addressing::Extended, 2 | 3) => {
                        rd(next(u16::from(s) - 1))
                    }
                    (Addressing::Indexed, 3) => dead(self.x0),
                    (Addressing::Indexed, 4) => dead(self.without_carry()),
                    _ if s == os => rd(ea),
                    _ if s == os + 1 => dead(ea),
                    _ => Cyc {
                        address: ea,
                        vma: u != Unary::Tst,
                        write: Some(self.w as u8),
                    },
                }
            }
            Kind::Jmp => match (addressing, s) {
                (Addressing::Indexed, 3) => dead(self.x0),
                (Addressing::Indexed, 4) => dead(self.without_carry()),
                _ => rd(next(u16::from(s) - 1)),
            },
            Kind::Jsr => {
                let ret = next(self.instr.len.get() as u16);
                match (addressing, s) {
                    (Addressing::Indexed, 2) => rd(next(1)),
                    (Addressing::Indexed, 3 | 7) => dead(self.x0),
                    (Addressing::Indexed, 4) => self.push(0, ret),
                    (Addressing::Indexed, 5) => self.push(1, ret),
                    (Addressing::Indexed, 6) => dead(self.sp0.wrapping_sub(2)),
                    (Addressing::Indexed, _) => dead(self.without_carry()),
                    (_, 2 | 3) => rd(next(u16::from(s) - 1)),
                    (_, 4) => rd(ea),
                    (_, 5) => self.push(0, ret),
                    (_, 6) => self.push(1, ret),
                    (_, 7) => dead(self.sp0.wrapping_sub(2)),
                    (_, 8) => dead(next(2)),
                    _ => rd(next(2)),
                }
            }
            Kind::Bsr => {
                let ret = next(2);
                match s {
                    2 => rd(next(1)),
                    3 | 7 => dead(ret),
                    4 => self.push(0, ret),
                    5 => self.push(1, ret),
                    6 => dead(self.sp0.wrapping_sub(2)),
                    _ => dead((ip & 0xff00) | (ea & 0x00ff)),
                }
            }
            Kind::Branch(_) => match s {
                2 => rd(next(1)),
                3 => dead(next(2)),
                _ => dead(ea),
            },
            Kind::Inherent(h) => self.emit_inherent(h, s),
            Kind::Unary(_, Target::A | Target::B) | Kind::Undefined => rd(next(1)),
        }
    }

    fn emit_inherent(&mut self, h: Inherent, s: u8) -> Cyc {
        let ip = self.ip;
        let sp0 = self.sp0;
        let x0 = self.x0;
        if s == 2 {
            return rd(ip.wrapping_add(1));
        }
        match h {
            Inherent::Inx => dead(if s == 3 { x0 } else { x0.wrapping_add(1) }),
            Inherent::Dex => dead(if s == 3 { x0 } else { x0.wrapping_sub(1) }),
            Inherent::Ins => dead(if s == 3 { sp0 } else { sp0.wrapping_add(1) }),
            Inherent::Des => dead(if s == 3 { sp0 } else { sp0.wrapping_sub(1) }),
            Inherent::Tsx => dead(if s == 3 { sp0 } else { sp0.wrapping_add(1) }),
            Inherent::Txs => dead(if s == 3 { x0 } else { x0.wrapping_sub(1) }),
            Inherent::Push(a) => {
                if s == 3 {
                    wr(sp0, self.acc(a))
                } else {
                    dead(sp0.wrapping_sub(1))
                }
            }
            Inherent::Pull(_) | Inherent::Rts | Inherent::Rti => {
                if s == 3 {
                    dead(sp0)
                } else {
                    rd(sp0.wrapping_add(u16::from(s - 3)))
                }
            }
            Inherent::Swi => match s {
                3..=9 => self.push(s - 3, ip.wrapping_add(1)),
                10 => {
                    self.ccr |= I;
                    dead(sp0.wrapping_sub(7))
                }
                _ => rd(vector::SWI + u16::from(s - 11)),
            },
            Inherent::Wai => self.push(s - 3, ip.wrapping_add(1)),
            _ => rd(ip.wrapping_add(1)),
        }
    }

    /// Take the byte the bus produced for the cycle just completed.
    fn latch(&mut self, d: u8) {
        let s = self.step;
        let hi = u16::from(d) << 8;
        match self.run {
            Run::Reset | Run::WaitExit | Run::Interrupt => {
                let first = match self.run {
                    Run::Reset => 1,
                    Run::WaitExit => 3,
                    _ => 11,
                };
                if s == first {
                    self.w = hi;
                } else if s == first + 1 {
                    self.w |= u16::from(d);
                }
            }
            Run::Op => self.latch_op(s, d),
            _ => {}
        }
    }

    fn latch_op(&mut self, s: u8, d: u8) {
        let hi = u16::from(d) << 8;
        if s == 1 {
            self.instr = decode(d);
            self.total = self.instr.cycles.get() as u8;
            self.sp0 = self.sp;
            self.x0 = self.x;
            return;
        }
        let Decoded {
            kind, addressing, ..
        } = self.instr;
        match kind {
            Kind::Inherent(Inherent::Pull(_)) if s == 4 => self.w = u16::from(d),
            Kind::Inherent(Inherent::Rts) => match s {
                4 => self.w = hi,
                5 => self.w |= u16::from(d),
                _ => {}
            },
            Kind::Inherent(Inherent::Rti) => match s {
                4 => self.ccr = d | ONES,
                5 => self.b = d,
                6 => self.a = d,
                7 => self.x = hi | (self.x & 0x00ff),
                8 => self.x = (self.x & 0xff00) | u16::from(d),
                9 => self.w = hi,
                10 => self.w |= u16::from(d),
                _ => {}
            },
            Kind::Inherent(Inherent::Swi) => match s {
                11 => self.w = hi,
                12 => self.w |= u16::from(d),
                _ => {}
            },
            Kind::Branch(_) | Kind::Bsr if s == 2 => {
                self.ea = self.ip.wrapping_add(2).wrapping_add(d as i8 as u16);
            }
            Kind::Inherent(_) | Kind::Branch(_) | Kind::Bsr | Kind::Undefined => {}
            Kind::Unary(_, Target::A | Target::B) => {}
            _ => {
                match (addressing, s) {
                    (Addressing::Direct, 2) => self.ea = u16::from(d),
                    (Addressing::Indexed, 2) => self.ea = self.x0.wrapping_add(u16::from(d)),
                    (Addressing::Extended, 2) => self.ea = hi,
                    (Addressing::Extended, 3) => self.ea |= u16::from(d),
                    _ => {}
                }
                let os = Self::operand_step(addressing);
                let reads = matches!(kind, Kind::Alu8(..) | Kind::Alu16(..) | Kind::Ld16(_))
                    || matches!(kind, Kind::Unary(_, Target::Memory));
                if reads && s == os {
                    self.w = if matches!(kind, Kind::Alu16(..) | Kind::Ld16(_)) {
                        hi
                    } else {
                        u16::from(d)
                    };
                    if let Kind::Unary(u, Target::Memory) = kind {
                        self.w = u16::from(alu::unary(u, self.ccr, d).0);
                        self.operand = d;
                    }
                } else if reads && s == os + 1 && matches!(kind, Kind::Alu16(..) | Kind::Ld16(_)) {
                    self.w |= u16::from(d);
                }
            }
        }
    }

    /// The register effects of the run just completed.
    fn finish(&mut self) {
        match self.run {
            Run::Reset | Run::WaitExit | Run::Interrupt => {
                self.pc = self.w;
                if self.run == Run::Interrupt {
                    self.sp = self.sp0.wrapping_sub(7);
                }
            }
            Run::Op => self.finish_op(),
            Run::Idle | Run::Wait | Run::Halt => {}
        }
        self.run = Run::Idle;
        self.step = 0;
        self.total = 0;
    }

    fn finish_op(&mut self) {
        let Decoded { kind, .. } = self.instr;
        let fall = self.ip.wrapping_add(self.instr.len.get() as u16);
        self.pc = fall;
        let m = self.w as u8;
        match kind {
            Kind::Alu8(op, a) => {
                let (r, f) = alu::alu8(op, self.ccr, self.acc(a), m);
                if let Some(r) = r {
                    self.set_acc(a, r);
                }
                self.ccr = f;
            }
            Kind::St8(a) => self.ccr = alu::logic(self.ccr, self.acc(a)),
            Kind::Alu16(Alu16::CmpX) => self.ccr = alu::cpx(self.ccr, self.x, self.w),
            Kind::Ld16(r) => {
                match r {
                    Reg16::X => self.x = self.w,
                    Reg16::S => self.sp = self.w,
                }
                self.ccr = alu::nz16(self.ccr, self.w) & !alu::V;
            }
            Kind::St16(r) => self.ccr = alu::nz16(self.ccr, self.reg16(r)) & !alu::V,
            Kind::Unary(u, t @ (Target::A | Target::B)) => {
                let a = if matches!(t, Target::A) {
                    Accumulator::A
                } else {
                    Accumulator::B
                };
                let (r, f) = alu::unary(u, self.ccr, self.acc(a));
                if u != Unary::Tst {
                    self.set_acc(a, r);
                }
                self.ccr = f;
            }
            Kind::Unary(u, Target::Memory) => self.ccr = alu::unary(u, self.ccr, self.operand).1,
            Kind::Jmp => self.pc = self.ea,
            Kind::Jsr | Kind::Bsr => {
                self.pc = self.ea;
                self.sp = self.sp0.wrapping_sub(2);
            }
            Kind::Branch(c) => {
                if alu::taken(c, self.ccr) {
                    self.pc = self.ea;
                }
            }
            Kind::Inherent(h) => self.finish_inherent(h),
            Kind::Undefined => {}
        }
    }

    fn finish_inherent(&mut self, h: Inherent) {
        let c = self.ccr;
        match h {
            Inherent::Nop => {}
            Inherent::Tap => self.ccr = self.a | ONES,
            Inherent::Tpa => self.a = c | ONES,
            Inherent::Inx => {
                self.x = self.x.wrapping_add(1);
                self.ccr = set(c, alu::Z, self.x == 0);
            }
            Inherent::Dex => {
                self.x = self.x.wrapping_sub(1);
                self.ccr = set(c, alu::Z, self.x == 0);
            }
            Inherent::Clv => self.ccr = c & !alu::V,
            Inherent::Sev => self.ccr = c | alu::V,
            Inherent::Clc => self.ccr = c & !alu::C,
            Inherent::Sec => self.ccr = c | alu::C,
            Inherent::Cli => self.ccr = c & !I,
            Inherent::Sei => self.ccr = c | I,
            Inherent::Sba => {
                let (r, f) = alu::sub(c, self.a, self.b, false);
                self.a = r;
                self.ccr = f;
            }
            Inherent::Cba => self.ccr = alu::sub(c, self.a, self.b, false).1,
            Inherent::Tab => {
                self.b = self.a;
                self.ccr = alu::logic(c, self.a);
            }
            Inherent::Tba => {
                self.a = self.b;
                self.ccr = alu::logic(c, self.b);
            }
            Inherent::Daa => {
                let (r, f) = alu::daa(c, self.a);
                self.a = r;
                self.ccr = f;
            }
            Inherent::Aba => {
                let (r, f) = alu::add(c, self.a, self.b, false);
                self.a = r;
                self.ccr = f;
            }
            Inherent::Tsx => self.x = self.sp.wrapping_add(1),
            Inherent::Txs => self.sp = self.x.wrapping_sub(1),
            Inherent::Ins => self.sp = self.sp.wrapping_add(1),
            Inherent::Des => self.sp = self.sp.wrapping_sub(1),
            Inherent::Push(_) => self.sp = self.sp0.wrapping_sub(1),
            Inherent::Pull(a) => {
                self.set_acc(a, self.w as u8);
                self.sp = self.sp0.wrapping_add(1);
            }
            Inherent::Rts => {
                self.pc = self.w;
                self.sp = self.sp0.wrapping_add(2);
            }
            Inherent::Rti => {
                self.pc = self.w;
                self.sp = self.sp0.wrapping_add(7);
            }
            Inherent::Swi => {
                self.pc = self.w;
                self.sp = self.sp0.wrapping_sub(7);
            }
            Inherent::Wai => {
                self.sp = self.sp0.wrapping_sub(7);
                self.waiting = true;
            }
        }
    }
}

const fn set(ccr: u8, bit: u8, on: bool) -> u8 {
    if on { ccr | bit } else { ccr & !bit }
}
