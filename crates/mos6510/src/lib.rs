//! The MOS 6510, emulated at its pins.
//!
//! The crate depends only on `signal`, allocates nothing, does no I/O and is
//! `no_std`: a [`Mos6510`] is a plain value that advances one clock cycle at a
//! time and shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! ```
//! use mos6510::{Mos6510, Pins};
//!
//! let mut cpu = Mos6510::default();
//! let mut pins = Pins::idle();
//! // The part comes out of reset by reading its vector at $FFFC and $FFFD.
//! let mut seen = 0;
//! for _ in 0..8 {
//!     pins = cpu.tick(pins);
//!     if matches!(pins.address, Some(0xfffc | 0xfffd)) {
//!         seen += 1;
//!     }
//!     pins = pins.with_d(0x00);
//! }
//! assert_eq!(seen, 2, "MOS6510-010: the reset vector is fetched from $FFFC");
//! ```
//!
//! # The part, not the machine
//!
//! The two registers of the on-chip port live here because they are inside the
//! package (`MOS6510-018`). **What the port lines select in a Commodore 64 does
//! not** — no banking, no PLA, no ROM, no address decode. Those are the board's,
//! and this crate cannot see them.
//!
//! # What this crate chooses
//!
//! Twelve behaviours here are the crate's choice, each under an open item, and
//! each is marked where it is written:
//!
//! - **Whether `AEC` Low floats `R/W` and the data bus with the address bus**
//!   — `MOS6510-OPEN-01`. This crate floats all three.
//! - **Whether an access to `$0000` or `$0001` appears on the bus** —
//!   `MOS6510-OPEN-02`. This crate makes the cycle and takes the register's
//!   byte in place of the bus's.
//! - **What `RDY` Low does** — `MOS6510-OPEN-03`.
//! - **What the two port registers hold after reset** — `MOS6510-OPEN-04`.
//!   This crate starts them at zero.
//! - **How the decode produces the 105 opcodes outside the table** —
//!   `MOS6510-OPEN-06`. This crate writes each one out rather than deriving
//!   it from bit fields.
//! - **What bits 6 and 7 of the two port registers do on the six-line
//!   package** — `MOS6510-OPEN-09`. This crate keeps both bits of both
//!   registers live.
//! - **What the data bus carries while the part is jammed** —
//!   `MOS6510-OPEN-05`. This crate reads `$FFFF` and drives nothing.
//! - **The value of the `magic constant` that `ANE` and `LXA` mix into the
//!   accumulator** — `MOS6510-OPEN-07`. The part has none (`MOS6510-072`), so
//!   a crate that must produce a byte is choosing one.
//! - **Where `NMI` vectors, and whether it is an edge** — `MOS6510-OPEN-08`.
//!   This crate latches a falling edge and vectors through `$FFFA`.
//! - **What the registers and the stack pointer hold after a reset** —
//!   `MOS6510-OPEN-10`. This crate starts `A`, `X`, `Y` and the stack pointer
//!   at zero.
//! - **Which vector `BRK` takes when `NMI` arrives while it runs** —
//!   `MOS6510-OPEN-11`. This crate lets `BRK` finish through `$FFFE`.
//! - **Where a hardware interrupt's second cycle reads** — `MOS6510-OPEN-12`.
//!   This crate reads at `PC + 1`, as `BRK` does, and the counter does not
//!   move.

#![no_std]

#[cfg(test)]
extern crate std;

mod alu;
mod decode;
mod exec;
mod pins;
mod port;

pub use alu::{FLAG_B, FLAG_C, FLAG_D, FLAG_I, FLAG_N, FLAG_U, FLAG_V, FLAG_Z};
pub use decode::{Alu8, Cond, Decoded, Inherent, Kind, Reg, Stacked, Unary, decode};
pub use pins::{Ctrl, Pins};
pub use port::Port;
pub use signal::Level;

/// The addresses the hardware sequences take their program counter from.
pub mod vector {
    /// `NMI`. `$FFFA` is this crate's choice under `MOS6510-OPEN-08`.
    pub const NMI: u16 = 0xfffa;
    /// `RES` (`MOS6510-010`).
    pub const RESET: u16 = 0xfffc;
    /// `IRQ` (`MOS6510-012`), and `BRK`.
    pub const IRQ: u16 = 0xfffe;
}

/// The value `ANE` and `LXA` mix into the accumulator, where the part has none.
///
/// `MOS6510-072` says the constant is not a property of the part at all: it is
/// chip- and temperature-dependent and may change between two executions of the
/// same instruction. `MOS6510-OPEN-07` is the open item. `$EE` is the value
/// *NMOS 6510 Unintended Opcodes — no more secrets* names first among
/// those "often reported", and it is this crate's default for one reason only —
/// a function that returns a byte has to return one. A part takes another in
/// [`Mos6510::new`].
pub const DEFAULT_MAGIC_CONSTANT: u8 = 0xee;

/// The programmer-visible registers, as [`Mos6510::at_boundary`] takes them.
///
/// The port's two registers are here as well, though a program reaches them
/// as memory (`MOS6510-018`), and so are the `NMI` latch and its edge
/// detector.
///
/// [`Default`] is every register zero but bit 5 of `p`, which is set as the
/// part holds it, no `NMI` latched, and `NMI` last seen High.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`Mos6510::registers`] and sets the fields it wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// Accumulator.
    pub a: u8,
    /// Index register X.
    pub x: u8,
    /// Index register Y.
    pub y: u8,
    /// Stack pointer, the low half of an address in page one (`MOS6510-022`).
    pub s: u8,
    /// Status register, packed as the `FLAG_` constants lay it out. The part
    /// holds bit 5 set and bit 4 clear ([`FLAG_U`], [`FLAG_B`]), and
    /// [`Mos6510::at_boundary`] makes them so whatever `p` says.
    pub p: u8,
    /// Program counter.
    pub pc: u16,
    /// The port's two registers (`MOS6510-018`, `MOS6510-019`).
    pub port: Port,
    /// An `NMI` edge has been latched and its sequence has not yet begun. See
    /// [`Ctrl::NMI`].
    pub nmi_pending: bool,
    /// The level `NMI` stood at on the last tick, against which the next edge
    /// is found.
    pub nmi_n: Level,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            s: 0,
            p: FLAG_U,
            pc: 0,
            port: Port {
                data: 0,
                direction: 0,
            },
            nmi_pending: false,
            nmi_n: Level::High,
        }
    }
}

/// Which of the three hardware sequences is running.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Interrupt {
    /// `RES`, vectoring through `$FFFC` and writing nothing (`MOS6510-010`).
    Reset,
    /// `NMI`. The pin is `MOS6510-014`; the vector is `MOS6510-OPEN-08` and
    /// `$FFFA` is this crate's choice.
    Nmi,
    /// `IRQ`, vectoring through `$FFFE` (`MOS6510-012`).
    Irq,
}

/// What is left of an instruction once its last bus cycle has been asked for.
///
/// The part is pipelined: the byte a cycle reads is not in hand until the next
/// cycle begins, so the tail of every instruction that ends on a read runs
/// during the first cycle of the one after it (`MOS6510-041`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Finish {
    /// Nothing: the instruction ended on a write, or needed no byte.
    None,
    /// The byte just read is the operand.
    Operand,
    /// The byte just read came off the stack, for `PLA` or `PLP`.
    Pull,
    /// The byte just read is the high half of a new program counter.
    PcHigh,
}

/// The bus operation one cycle performs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Bus {
    /// A read from this address.
    Read(u16),
    /// A write of this byte to this address.
    Write(u16, u8),
}

/// The part.
///
/// Advance it with [`Mos6510::tick`], one clock cycle per call. Cloneable, so a
/// test can fork one and run both halves.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Mos6510 {
    /// Accumulator.
    a: u8,
    /// Index register X.
    x: u8,
    /// Index register Y.
    y: u8,
    /// Stack pointer, the low half of an address in page one (`MOS6510-022`).
    s: u8,
    /// Status register. Bit 5 is always set and bit 4 is not a register bit at
    /// all (`MOS6510-023`, `MOS6510-078`).
    p: u8,
    /// Program counter.
    pc: u16,
    /// The on-chip peripheral port (`MOS6510-018`).
    port: Port,
    /// The levels the outside world holds the port lines at, as last presented
    /// on [`Pins::p`]. Only the lines the direction register makes inputs
    /// are read from here.
    port_pins: u8,

    /// The opcode being executed.
    op: u8,
    /// Which cycle of it comes next; 1 means the next cycle fetches an opcode.
    cycle: u8,
    /// The byte the previous cycle read.
    dl: u8,
    /// Low half of the effective address.
    adl: u8,
    /// High half of the effective address.
    adh: u8,
    /// The zero page pointer or un-indexed offset an addressing mode is built
    /// from.
    base: u8,
    /// The byte a read-modify-write instruction read.
    tmp: u8,
    /// The low half of a program counter being assembled.
    pcl: u8,
    /// Indexing crossed a page boundary.
    cross: bool,
    /// What the tail of this instruction still has to do.
    finish: Finish,
    /// The hardware sequence in flight, if the current one is not an opcode.
    interrupt: Option<Interrupt>,

    /// `RES` has been seen Low and its sequence has not run yet.
    res_pending: bool,
    /// `RES` is Low right now, so the sequencer does not advance
    /// (`MOS6510-013`).
    res_held: bool,
    /// `NMI` was High and is now Low: the edge is latched (`MOS6510-014`).
    nmi_pending: bool,
    /// The level `NMI` was presented at last tick, for the edge above.
    nmi_last: bool,
    /// `IRQ` is Low right now.
    irq_line: bool,
    /// A `JAM` opcode has stopped the part (`MOS6510-058`).
    jammed: bool,

    /// The bus operation the last tick asked for, re-emitted while `RDY` holds
    /// the part on a read.
    outstanding: Bus,
    /// Nothing has been asked for yet: the first tick has not run.
    started: bool,
    /// The last tick presented `AEC` Low, so the part drove no cycle.
    aec_low: bool,
    /// See [`DEFAULT_MAGIC_CONSTANT`].
    magic: u8,
}

impl Default for Mos6510 {
    /// [`Mos6510::new`] with [`DEFAULT_MAGIC_CONSTANT`].
    fn default() -> Self {
        Self::new(DEFAULT_MAGIC_CONSTANT)
    }
}

impl Mos6510 {
    /// A part that has just been powered up, with `RES` pending.
    ///
    /// The first ticks run the reset sequence of `MOS6510-010`: seven cycles,
    /// none of them a write, ending with the vector at `$FFFC` and `$FFFD` in
    /// the program counter. `magic_constant` is the byte `ANE` and `LXA`
    /// mix in, which an open item leaves to the caller (see
    /// [`DEFAULT_MAGIC_CONSTANT`]).
    #[must_use]
    pub const fn new(magic_constant: u8) -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            s: 0,
            p: FLAG_U | FLAG_I,
            pc: 0,
            port: Port::new(),
            port_pins: 0,
            op: 0,
            cycle: 1,
            dl: 0,
            adl: 0,
            adh: 0,
            base: 0,
            tmp: 0,
            pcl: 0,
            cross: false,
            finish: Finish::None,
            interrupt: None,
            res_pending: true,
            res_held: false,
            nmi_pending: false,
            nmi_last: false,
            irq_line: false,
            jammed: false,
            outstanding: Bus::Read(0),
            started: false,
            aec_low: false,
            magic: magic_constant,
        }
    }

    /// The value `ANE` and `LXA` are mixing in.
    #[must_use]
    pub const fn magic_constant(&self) -> u8 {
        self.magic
    }

    /// The part has executed a `JAM` opcode and only `RES` will restart it
    /// (`MOS6510-058`).
    #[must_use]
    pub const fn is_jammed(&self) -> bool {
        self.jammed
    }

    /// The present cycle, the one the last [`Mos6510::tick`] asked for, reads:
    /// the caller places the byte at [`Pins::address`] into [`Pins::d`]
    /// before the next tick.
    ///
    /// False before the first tick, when no cycle has been asked for, after a
    /// tick that presented [`Ctrl::RES`], whatever `R/W` shows, and while that
    /// tick presented [`Ctrl::AEC_LOW`], when the part drives no cycle
    /// (`MOS6510-004`), and while [`Mos6510::is_jammed`], when execution has
    /// ceased and no byte is taken, from the cycle that stops the part
    /// (`MOS6510-058`). Otherwise every cycle is a bus cycle
    /// (`MOS6510-007`) and this is the complement of [`Mos6510::is_writing`]. A
    /// read of `$0000` or `$0001` is answered by the port itself
    /// (`MOS6510-021`).
    #[must_use]
    pub const fn is_reading(&self) -> bool {
        self.started
            && !self.res_held
            && !self.aec_low
            && !self.jammed
            && matches!(self.outstanding, Bus::Read(_))
    }

    /// The present cycle writes [`Pins::d`] to [`Pins::address`]; the pin
    /// word shows it as [`Ctrl::WRITE`]. False under [`Ctrl::RES`] and
    /// [`Ctrl::AEC_LOW`], as [`Mos6510::is_reading`] is.
    #[must_use]
    pub const fn is_writing(&self) -> bool {
        self.started
            && !self.res_held
            && !self.aec_low
            && matches!(self.outstanding, Bus::Write(..))
    }

    /// The part has given the bus up: the last tick presented
    /// [`Ctrl::AEC_LOW`], so the address lines, the data lines and `R/W` are
    /// high-impedance (`MOS6510-004`), and [`Pins::address`] and [`Pins::d`]
    /// read `None`. False before the first tick, and on a part
    /// [`Mos6510::at_boundary`] builds until its first tick.
    #[must_use]
    pub const fn is_released(&self) -> bool {
        self.aec_low
    }

    /// The programmer-visible registers as they stand, with the port's two,
    /// the `NMI` latch and its edge detector.
    ///
    /// Between the last cycle of an instruction and the first of the next, a
    /// byte that last cycle read has not yet reached them; [`Mos6510::settle`]
    /// puts it there, with an `NMI` edge the next tick's pins carry. Not
    /// carried: the value of [`Mos6510::magic_constant`], which the caller sets,
    /// and a read that `RDY` is holding, while which no boundary stands.
    #[must_use]
    pub const fn registers(&self) -> Registers {
        Registers {
            a: self.a,
            x: self.x,
            y: self.y,
            s: self.s,
            p: self.p,
            pc: self.pc,
            port: self.port,
            nmi_pending: self.nmi_pending,
            nmi_n: Level::from_bool(!self.nmi_last),
        }
    }

    /// The next cycle begins an instruction or a hardware interrupt sequence,
    /// and nothing is in flight.
    ///
    /// False while `RES` holds the part and through the reset sequence it
    /// begins on release, power-up's included (`MOS6510-010`), and once it has
    /// [`Mos6510::is_jammed`]. A read `RDY` holds has not completed: a caller
    /// presenting [`Ctrl::RDY_LOW`] to the next tick is not yet at the
    /// boundary this reports.
    #[must_use]
    pub const fn is_at_instruction_boundary(&self) -> bool {
        self.cycle == 1 && !self.resetting()
    }

    /// `RES` holds the part. False through the reset sequence it begins on
    /// release (`MOS6510-010`), and through the one [`Mos6510::new`] begins.
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.res_held
    }

    /// `RES` holds the part, or the reset sequence it begins on release,
    /// power-up included, has not yet finished.
    const fn resetting(&self) -> bool {
        self.res_held
            || self.res_pending
            || (matches!(self.interrupt, Some(Interrupt::Reset)) && self.cycle != 1)
    }

    /// Advance one clock cycle.
    ///
    /// The caller presents the input pins, and [`Pins::d`] holding the byte
    /// the bus produced for the request the **previous** tick returned, if that
    /// was a read. What comes back is the request for this cycle.
    ///
    /// Every cycle is a bus cycle (`MOS6510-007`), except while `AEC` is Low,
    /// where the part drives nothing (`MOS6510-004`): `R/W` and the data bus
    /// float with the address bus, this crate's choice under
    /// `MOS6510-OPEN-01`.
    #[must_use]
    pub fn tick(&mut self, inp: Pins) -> Pins {
        self.port_pins = inp.p;
        self.sample(inp.ctrl);

        if !self.stalls(inp.ctrl) {
            self.latch(inp);
            self.outstanding = self.step();
            self.started = true;
        }

        self.aec_low = inp.ctrl.contains(Ctrl::AEC_LOW);
        self.show(inp.ctrl)
    }

    /// `RDY` Low holds the part on a read cycle: the same request goes out
    /// again and nothing is latched, this crate's choice under
    /// `MOS6510-OPEN-03`.
    const fn stalls(&self, ctrl: Ctrl) -> bool {
        self.started && ctrl.contains(Ctrl::RDY_LOW) && matches!(self.outstanding, Bus::Read(_))
    }

    /// Take the byte the outstanding read asked for.
    ///
    /// The two port registers answer for themselves, wherever the outside world
    /// would have answered (`MOS6510-021`). The cycle still goes out on the
    /// bus, this crate's choice under `MOS6510-OPEN-02`.
    fn latch(&mut self, inp: Pins) {
        if let Bus::Read(address) = self.outstanding
            && self.started
        {
            self.dl = self
                .port
                .read(address, self.port_pins)
                .unwrap_or(inp.d.unwrap_or(0));
        }
    }

    /// Take the outstanding read's byte and finish the instruction that has
    /// just ended, without starting the next, so [`Mos6510::registers`] reads
    /// the boundary.
    ///
    /// The part is pipelined (`MOS6510-041`): between the last cycle of an
    /// instruction and the first of the next, the register file has not yet
    /// taken the byte that last cycle read. A caller that wants to read the
    /// registers at an instruction boundary calls this first, presenting the
    /// pins it would have presented to the next [`Mos6510::tick`]. Ticking on
    /// does the same thing and also begins the next instruction, so this is
    /// idempotent and ticking after it is correct. An `NMI` edge on these pins
    /// is latched first, as that tick latches it, into the `NMI` latch and edge
    /// detector [`Registers`] holds.
    ///
    /// Returns whether the part now stands at a boundary that
    /// [`Mos6510::at_boundary`] rebuilds from [`Mos6510::registers`]:
    /// `false`, changing nothing, unless
    /// [`Mos6510::is_at_instruction_boundary`], where pins present
    /// [`Ctrl::RDY_LOW`] while a read is outstanding, which hold that read as
    /// [`Mos6510::tick`] does, and where they present [`Ctrl::RES`], under
    /// which that tick drops the byte and the tail (`MOS6510-010`). A part is
    /// rebuilt at a later boundary instead.
    #[must_use]
    pub fn settle(&mut self, inp: Pins) -> bool {
        if !self.is_at_instruction_boundary()
            || inp.ctrl.contains(Ctrl::RES)
            || self.stalls(inp.ctrl)
        {
            return false;
        }
        self.nmi_edge(inp.ctrl);
        self.port_pins = inp.p;
        self.latch(inp);
        self.complete();
        true
    }

    /// A part standing at an instruction boundary with these registers, past
    /// reset, with the `NMI` latch and its edge detector as `regs` holds them.
    ///
    /// `regs.p` is taken as the part holds it: bit 5 set, since no instruction
    /// clears it (`MOS6510-023`), and bit 4 clear, since it is not a register
    /// bit (`MOS6510-026`).
    ///
    /// `magic_constant` is as [`Mos6510::new`] takes it. Until its first tick,
    /// [`Mos6510::is_reading`], [`Mos6510::is_writing`] and
    /// [`Mos6510::is_released`] read `false`, whatever the original's last
    /// cycle did.
    #[must_use]
    pub const fn at_boundary(magic_constant: u8, regs: Registers) -> Self {
        let mut cpu = Self::new(magic_constant);
        cpu.a = regs.a;
        cpu.x = regs.x;
        cpu.y = regs.y;
        cpu.s = regs.s;
        cpu.p = (regs.p | FLAG_U) & !FLAG_B;
        cpu.pc = regs.pc;
        cpu.port = regs.port;
        cpu.nmi_pending = regs.nmi_pending;
        cpu.nmi_last = regs.nmi_n.is_low();
        cpu.res_pending = false;
        cpu
    }

    /// Latch the input lines this tick presents.
    fn sample(&mut self, ctrl: Ctrl) {
        self.res_held = ctrl.contains(Ctrl::RES);
        if self.res_held {
            self.res_pending = true;
            self.jammed = false;
            self.cycle = 1;
            self.finish = Finish::None;
            self.interrupt = None;
            self.port = Port::new();
        }
        self.nmi_edge(ctrl);
        self.irq_line = ctrl.contains(Ctrl::IRQ);
    }

    /// Latch the `NMI` edge this tick presents (`MOS6510-OPEN-08`: an edge is
    /// this crate's choice).
    fn nmi_edge(&mut self, ctrl: Ctrl) {
        let nmi = ctrl.contains(Ctrl::NMI);
        if nmi && !self.nmi_last {
            self.nmi_pending = true;
        }
        self.nmi_last = nmi;
    }

    /// Turn the outstanding bus operation into pins.
    fn show(&self, ctrl: Ctrl) -> Pins {
        let mut out = Pins {
            address: None,
            d: None,
            ctrl: Ctrl::NONE,
            p: self.port.lines(self.port_pins),
        };
        if ctrl.contains(Ctrl::AEC_LOW) {
            return out;
        }
        match self.outstanding {
            Bus::Read(address) => out.address = Some(address),
            Bus::Write(address, d) => {
                out.address = Some(address);
                out.d = Some(d);
                out.ctrl |= Ctrl::WRITE;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_boundary_holds_bit_5_set_and_bit_4_clear() {
        let rebuilt = |p| {
            Mos6510::at_boundary(
                DEFAULT_MAGIC_CONSTANT,
                Registers {
                    p,
                    ..Registers::default()
                },
            )
            .registers()
            .p
        };
        assert_eq!(Registers::default().p, FLAG_U);
        assert_eq!(rebuilt(Registers::default().p), FLAG_U);
        assert_eq!(rebuilt(0x00), FLAG_U, "MOS6510-023");
        assert_eq!(rebuilt(0xff), !FLAG_B, "MOS6510-026");
        let regs = Registers::default();
        assert_eq!(
            Mos6510::at_boundary(DEFAULT_MAGIC_CONSTANT, regs).registers(),
            regs
        );
    }

    #[test]
    fn a_new_part_reaches_its_reset_vector_in_seven_cycles_without_writing() {
        let mut cpu = Mos6510::default();
        let mut pins = Pins::idle();
        let mut cycles = std::vec::Vec::new();
        for _ in 0..7 {
            pins = cpu.tick(pins);
            cycles.push((pins.address, cpu.is_writing()));
            pins = pins.with_d(match pins.address {
                Some(0xfffc) => 0x34,
                Some(0xfffd) => 0x12,
                _ => 0xaa,
            });
        }
        assert!(
            cycles.iter().all(|&(_, write)| !write),
            "MOS6510-011: reset writes nothing, so the three stack cycles are reads: {cycles:?}"
        );
        assert_eq!(cycles[5].0, Some(0xfffc));
        assert_eq!(cycles[6].0, Some(0xfffd));
        pins = cpu.tick(pins);
        assert_eq!(pins.address, Some(0x1234));
    }

    #[test]
    fn res_returns_the_port_registers_to_zero() {
        let mut cpu = Mos6510::default();
        cpu.port.write(Port::DIRECTION_ADDRESS, 0xff);
        cpu.port.write(Port::DATA_ADDRESS, 0x37);
        let _ = cpu.tick(Pins::idle().with(Ctrl::RES));
        assert_eq!(cpu.port, Port::default());
        assert_eq!(
            cpu.port.lines(cpu.port_pins),
            cpu.port_pins,
            "every line an input"
        );
    }

    /// One bus cycle on a flat 64 KiB memory, `IRQ` held as given.
    fn cycle(cpu: &mut Mos6510, pins: Pins, mem: &mut [u8], irq: bool) -> (Pins, (u16, u8, bool)) {
        let ctrl = if irq { Ctrl::IRQ } else { Ctrl::NONE };
        let mut out = cpu.tick(Pins { ctrl, ..pins });
        if cpu.is_writing() {
            let d = out.d.expect("a write drives d");
            mem[usize::from(out.address.expect("the cycle drives an address"))] = d;
            (
                out,
                (out.address.expect("the cycle drives an address"), d, true),
            )
        } else {
            let d = mem[usize::from(out.address.expect("the cycle drives an address"))];
            out.d = Some(d);
            (
                out,
                (out.address.expect("the cycle drives an address"), d, false),
            )
        }
    }

    #[test]
    fn settle_under_rdy_low_takes_nothing_and_rdy_high_takes_the_byte() {
        let mut mem = std::vec![0xeau8; 0x10000];
        mem[0x0200..0x0202].copy_from_slice(&[0xa9, 0x42]);
        let mut cpu = Mos6510::at_boundary(
            DEFAULT_MAGIC_CONSTANT,
            Registers {
                pc: 0x0200,
                ..Registers::default()
            },
        );
        let mut pins = Pins::idle();
        for _ in 0..2 {
            (pins, _) = cycle(&mut cpu, pins, &mut mem, false);
        }
        assert!(cpu.is_at_instruction_boundary());
        let mut held = cpu.clone();
        assert!(!held.settle(pins.with_d(0x99).with(Ctrl::RDY_LOW)));
        assert_eq!(held, cpu, "a read RDY holds is not taken");
        assert!(cpu.settle(pins));
        assert_eq!(cpu.registers().a, 0x42);
    }

    #[test]
    fn a_jammed_part_reads_nothing() {
        let mut mem = std::vec![0xeau8; 0x10000];
        mem[0x0200] = 0x02;
        let mut cpu = Mos6510::at_boundary(
            DEFAULT_MAGIC_CONSTANT,
            Registers {
                pc: 0x0200,
                ..Registers::default()
            },
        );
        let mut pins = Pins::idle();
        for _ in 0..3 {
            (pins, _) = cycle(&mut cpu, pins, &mut mem, false);
        }
        assert!(cpu.is_jammed());
        for _ in 0..4 {
            (pins, _) = cycle(&mut cpu, pins, &mut mem, false);
            assert_eq!(pins.address, Some(0xffff), "MOS6510-058");
            assert!(!cpu.is_reading() && !cpu.is_writing());
        }
    }

    #[test]
    fn aec_low_releases_the_bus_for_the_tick_that_presents_it() {
        let mut cpu = Mos6510::at_boundary(DEFAULT_MAGIC_CONSTANT, Registers::default());
        assert!(!cpu.is_released());
        let out = cpu.tick(Pins::idle().with(Ctrl::AEC_LOW));
        assert!(cpu.is_released(), "MOS6510-004");
        assert_eq!((out.address, out.d), (None, None));
        assert_eq!(out.ctrl, Ctrl::NONE, "an input is not echoed");
        assert!(!cpu.is_reading() && !cpu.is_writing());
        let _ = cpu.tick(Pins::idle());
        assert!(!cpu.is_released());
    }

    #[test]
    fn a_new_part_is_at_no_boundary_until_its_vector_is_read() {
        let mut cpu = Mos6510::default();
        let mut pins = Pins::idle();
        let mut boundaries = 0;
        for _ in 0..7 {
            assert!(!cpu.is_in_reset());
            boundaries += usize::from(cpu.is_at_instruction_boundary());
            pins = cpu.tick(pins).with_d(0x00);
        }
        assert_eq!(boundaries, 0);
        assert!(cpu.is_at_instruction_boundary());
    }

    #[test]
    fn a_round_trip_mid_program_runs_on_as_the_part_would() {
        let program = [
            0xa9, 0x0f, 0x85, 0x00, 0xa9, 0x05, 0x85, 0x01, 0xa2, 0x05, 0x58, 0xca, 0x48, 0x9d,
            0x00, 0x03, 0xea, 0xea,
        ];
        let mut mem = std::vec![0xeau8; 0x10000];
        mem[0x0200..0x0200 + program.len()].copy_from_slice(&program);
        mem[0xfffe] = 0x00;
        mem[0xffff] = 0x04;
        let mut cpu = Mos6510::at_boundary(
            DEFAULT_MAGIC_CONSTANT,
            Registers {
                s: 0xfd,
                p: FLAG_U | FLAG_I,
                pc: 0x0200,
                ..Registers::default()
            },
        );
        let mut pins = Pins::idle();
        let mut instructions = 0;
        while instructions < 6 {
            (pins, _) = cycle(&mut cpu, pins, &mut mem, false);
            if cpu.is_at_instruction_boundary() {
                instructions += 1;
            }
        }
        assert!(cpu.settle(pins));
        let regs = cpu.registers();
        assert_eq!(regs.p & FLAG_I, 0, "CLI has run");
        assert_eq!(
            regs.port,
            Port {
                data: 0x05,
                direction: 0x0f
            }
        );
        let mut copy = Mos6510::at_boundary(DEFAULT_MAGIC_CONSTANT, regs);
        let mut copy_mem = mem.clone();
        let mut copy_pins = pins;
        let mut vectored = false;
        for _ in 0..40 {
            let (next, seen) = cycle(&mut cpu, pins, &mut mem, true);
            let (copy_next, copy_seen) = cycle(&mut copy, copy_pins, &mut copy_mem, true);
            assert_eq!(seen, copy_seen);
            vectored |= seen.0 == vector::IRQ;
            (pins, copy_pins) = (next, copy_next);
        }
        assert!(vectored, "the IRQ the CLI let in was taken");
        assert_eq!(cpu.registers(), copy.registers());
        assert_eq!(mem, copy_mem);
    }
}
