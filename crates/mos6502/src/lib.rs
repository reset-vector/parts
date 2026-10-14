//! The MOS Technology MCS6502, emulated at its pins.
//!
//! The crate depends only on `signal`, allocates nothing, does no I/O and is
//! `no_std`: a [`Mos6502`] is a plain value that advances one clock cycle at a
//! time and shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! ```
//! use mos6502::{Mos6502, Pins};
//!
//! let mut cpu = Mos6502::default();
//! let mut pins = Pins::idle();
//! // The part comes out of reset by reading its vector at $FFFC and $FFFD.
//! let mut seen = 0;
//! for _ in 0..8 {
//!     pins = cpu.tick(pins);
//!     if pins.address == 0xfffc || pins.address == 0xfffd {
//!         seen += 1;
//!     }
//!     pins = pins.with_d(0x00);
//! }
//! assert_eq!(seen, 2, "MOS6502-013: the reset vector is fetched from $FFFC");
//! ```
//!
//! # The part, not the machine
//!
//! No banking, no decode, no ROM map, and **no clock rate**: what a machine
//! runs the part at is the machine's. Page zero is ordinary memory here and
//! nothing in this crate answers for an address — which is the whole of the
//! difference between this part and the 6510, whose package puts
//! two registers at `$0000` and `$0001`.
//!
//! # What this crate chooses
//!
//! Eight behaviours here are the crate's choice, each under an open item, and
//! each is marked where it is written:
//!
//! - **Which opcodes stop the part, and what the buses do while it is
//!   stopped** — `MOS6502-OPEN-01` and `MOS6502-OPEN-09`.
//! - **The byte `ANE` and `LXA` mix into the accumulator** —
//!   `MOS6502-OPEN-02`. See [`DEFAULT_MAGIC_CONSTANT`].
//! - **Whether `SYNC` rises on the discarded fetch that begins a hardware
//!   interrupt sequence** — `MOS6502-OPEN-10`. This crate leaves it Low.
//! - **What the registers and the stack pointer hold after a reset** —
//!   `MOS6502-OPEN-05`. The manufacturer says outright that they are unknown
//!   (`MOS6502-014`), so a crate that has to start somewhere starts at zero.
//! - **Which vector `BRK` takes when `IRQ` or `NMI` arrives while it runs** —
//!   `MOS6502-OPEN-06`. This crate lets `BRK` finish through `$FFFE`.
//! - **Where a hardware interrupt's second cycle reads** — `MOS6502-OPEN-12`.
//!   This crate reads at `PC`, where `BRK` reads at `PC + 1`.
//! - **How the 105 opcodes outside the table decode** — `MOS6502-OPEN-04`.
//!   This crate writes each one out rather than deriving it from bit fields.
//! - **Which `(Indirect),Y` reads cost the page-crossing cycle** —
//!   `MOS6502-OPEN-08`. This crate costs it on all seven.

#![no_std]

#[cfg(test)]
extern crate std;

mod alu;
mod decode;
mod exec;
mod pins;

pub use alu::{FLAG_B, FLAG_C, FLAG_D, FLAG_I, FLAG_N, FLAG_U, FLAG_V, FLAG_Z};
pub use decode::{Alu8, Cond, Decoded, Inherent, Kind, Reg, Stacked, Unary, decode};
pub use pins::{Ctrl, Pins};
pub use signal::Level;

/// The addresses the hardware sequences take their program counter from
/// (`MOS6502-013`, `MOS6502-015`).
pub mod vector {
    /// `NMI`.
    pub const NMI: u16 = 0xfffa;
    /// `RES`.
    pub const RESET: u16 = 0xfffc;
    /// `IRQ`, and `BRK`.
    pub const IRQ: u16 = 0xfffe;
}

/// The value `ANE` and `LXA` mix into the accumulator.
///
/// On a part the byte is chip- and temperature-dependent. `$EE` is this
/// crate's default under `MOS6502-OPEN-02`, for one reason only — a function
/// that returns a byte has to return one. A part takes another in
/// [`Mos6502::new`].
pub const DEFAULT_MAGIC_CONSTANT: u8 = 0xee;

/// The programmer-visible registers, as [`Mos6502::at_boundary`] takes them,
/// with the two edge detectors and the `NMI` latch.
///
/// [`Default`] is every register zero but bit 5 of `p`, which is set as the
/// part holds it, no `NMI` latched, and `NMI` and `S.O.` last seen High.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`Mos6502::registers`] and sets the fields it wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// Accumulator.
    pub a: u8,
    /// Index register X.
    pub x: u8,
    /// Index register Y.
    pub y: u8,
    /// Stack pointer, the low half of an address in page one (`MOS6502-026`).
    pub s: u8,
    /// Status register, packed as the `FLAG_` constants lay it out. The part
    /// holds bit 5 set and bit 4 clear ([`FLAG_U`], [`FLAG_B`]), and
    /// [`Mos6502::at_boundary`] makes them so whatever `p` says.
    pub p: u8,
    /// Program counter.
    pub pc: u16,
    /// An `NMI` edge has been latched and its sequence has not yet begun
    /// (`MOS6502-016`).
    pub nmi_pending: bool,
    /// The level `NMI` stood at on the last tick, against which the next edge
    /// is found.
    pub nmi_n: Level,
    /// The level `S.O.` stood at on the last tick, against which the next edge
    /// is found (`MOS6502-006`).
    pub so_n: Level,
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
            nmi_pending: false,
            nmi_n: Level::High,
            so_n: Level::High,
        }
    }
}

/// Which of the three hardware sequences is running.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Interrupt {
    /// `RES`, vectoring through `$FFFC` and writing nothing (`MOS6502-013`).
    Reset,
    /// `NMI`, vectoring through `$FFFA` (`MOS6502-015`).
    Nmi,
    /// `IRQ`, vectoring through `$FFFE` (`MOS6502-015`).
    Irq,
}

/// What is left of an instruction once its last bus cycle has been asked for.
///
/// The part is pipelined: the byte a cycle reads is not in hand until the next
/// cycle begins, so the tail of every instruction that ends on a read runs
/// during the first cycle of the one after it (`MOS6502-041`).
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
/// Advance it with [`Mos6502::tick`], one clock cycle per call. Cloneable, so a
/// test can fork one and run both halves.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Mos6502 {
    /// Accumulator.
    a: u8,
    /// Index register X.
    x: u8,
    /// Index register Y.
    y: u8,
    /// Stack pointer, the low half of an address in page one (`MOS6502-026`).
    s: u8,
    /// Status register. Bit 4 is not a register bit at all, and bit 5 is the
    /// manufacturer's unused bit (`MOS6502-028`, `MOS6502-029`).
    p: u8,
    /// Program counter.
    pc: u16,

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

    /// The bus operation just asked for is an op-code fetch, so `SYNC` is High
    /// (`MOS6502-003`).
    fetching: bool,
    /// `RES` has been seen Low and its sequence has not run yet.
    res_pending: bool,
    /// `RES` is Low right now, so the sequencer does not advance
    /// (`MOS6502-013`).
    res_held: bool,
    /// `NMI` was High and is now Low: the edge is latched (`MOS6502-016`).
    nmi_pending: bool,
    /// The level `NMI` was presented at last tick, for the edge above.
    nmi_last: bool,
    /// The level `S.O.` was presented at last tick, for its edge
    /// (`MOS6502-006`).
    so_last: bool,
    /// `IRQ` is Low right now.
    irq_line: bool,
    /// An opcode outside the manufacturer's table has stopped the part
    /// (`MOS6502-OPEN-01`).
    stopped: bool,

    /// The bus operation the last tick asked for, re-emitted while `RDY` holds
    /// the part on a read.
    outstanding: Bus,
    /// Nothing has been asked for yet: the first tick has not run.
    started: bool,
    /// See [`DEFAULT_MAGIC_CONSTANT`].
    magic: u8,
}

impl Default for Mos6502 {
    /// [`Mos6502::new`] with [`DEFAULT_MAGIC_CONSTANT`].
    fn default() -> Self {
        Self::new(DEFAULT_MAGIC_CONSTANT)
    }
}

impl Mos6502 {
    /// A part that has just been powered up, with `RES` pending.
    ///
    /// The first ticks run the reset sequence of `MOS6502-013`: seven cycles,
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
            fetching: false,
            res_pending: true,
            res_held: false,
            nmi_pending: false,
            nmi_last: false,
            so_last: false,
            irq_line: false,
            stopped: false,
            outstanding: Bus::Read(0),
            started: false,
            magic: magic_constant,
        }
    }

    /// The value `ANE` and `LXA` are mixing in.
    #[must_use]
    pub const fn magic_constant(&self) -> u8 {
        self.magic
    }

    /// The part has executed an opcode this crate jams on ([`Kind::Undefined`]), and
    /// only `RES` will restart it (`MOS6502-OPEN-01`).
    #[must_use]
    pub const fn is_jammed(&self) -> bool {
        self.stopped
    }

    /// The present cycle, the one the last [`Mos6502::tick`] asked for, reads:
    /// the caller places the byte at [`Pins::address`] into [`Pins::d`]
    /// before the next tick.
    ///
    /// Every cycle is a bus cycle (`MOS6502-011`), so once the part has ticked
    /// this is the complement of [`Mos6502::is_writing`] but under reset and
    /// while [`Mos6502::is_jammed`], when execution has ceased and no byte is
    /// taken, from the cycle that stops the part (`MOS6502-OPEN-01`). Both are
    /// false before the first tick, when no cycle has been asked for, and after
    /// a tick that presented [`Ctrl::RES`], whatever `R/W` shows.
    #[must_use]
    pub const fn is_reading(&self) -> bool {
        self.started && !self.res_held && !self.stopped && matches!(self.outstanding, Bus::Read(_))
    }

    /// The present cycle writes [`Pins::d`] to [`Pins::address`]; the pin
    /// word shows it as [`Ctrl::WRITE`].
    #[must_use]
    pub const fn is_writing(&self) -> bool {
        self.started && !self.res_held && matches!(self.outstanding, Bus::Write(..))
    }

    /// The programmer-visible registers as they stand, with the `NMI` latch and
    /// the `NMI` and `S.O.` edge detectors.
    ///
    /// Between the last cycle of an instruction and the first of the next, a
    /// byte that last cycle read has not yet reached them; [`Mos6502::settle`]
    /// puts it there, with an `NMI` edge the next tick's pins carry. Not
    /// carried: the value of [`Mos6502::magic_constant`], which the caller sets,
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
            nmi_pending: self.nmi_pending,
            nmi_n: Level::from_bool(!self.nmi_last),
            so_n: Level::from_bool(!self.so_last),
        }
    }

    /// The next cycle begins an instruction or a hardware interrupt sequence,
    /// and nothing is in flight.
    ///
    /// False while `RES` holds the part and through the reset sequence it
    /// begins on release, power-up's included (`MOS6502-013`), and once it has
    /// [`Mos6502::is_jammed`]. A read `RDY` holds has not completed: a caller
    /// presenting [`Ctrl::RDY_LOW`] to the next tick is not yet at the
    /// boundary this reports.
    #[must_use]
    pub const fn is_at_instruction_boundary(&self) -> bool {
        self.cycle == 1 && !self.resetting()
    }

    /// `RES` holds the part. False through the reset sequence it begins on
    /// release (`MOS6502-013`), and through the one [`Mos6502::new`] begins.
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
    /// Every cycle is a bus cycle (`MOS6502-011`).
    #[must_use]
    pub fn tick(&mut self, inp: Pins) -> Pins {
        self.sample(inp.ctrl);

        if !self.stalls(inp.ctrl) {
            self.latch(inp);
            self.outstanding = self.step();
            self.started = true;
        }

        self.show()
    }

    /// `MOS6502-005`: `RDY` Low delays a read cycle — the same request goes
    /// out again and nothing is latched — and never delays a write, which runs
    /// to completion so that the part stops in the read cycle after it.
    const fn stalls(&self, ctrl: Ctrl) -> bool {
        self.started && ctrl.contains(Ctrl::RDY_LOW) && matches!(self.outstanding, Bus::Read(_))
    }

    /// Take the byte the outstanding read asked for.
    fn latch(&mut self, inp: Pins) {
        if let Bus::Read(_) = self.outstanding
            && self.started
        {
            self.dl = inp.d.unwrap_or(0);
        }
    }

    /// Take the outstanding read's byte and finish the instruction that has
    /// just ended, without starting the next, so [`Mos6502::registers`] reads
    /// the boundary.
    ///
    /// The part is pipelined (`MOS6502-041`): between the last cycle of an
    /// instruction and the first of the next, the register file has not yet
    /// taken the byte that last cycle read. A caller that wants to read the
    /// registers at an instruction boundary calls this first, presenting the
    /// pins it would have presented to the next [`Mos6502::tick`]. Ticking on
    /// does the same thing and also begins the next instruction, so this is
    /// idempotent and ticking after it is correct. An `NMI` or `S.O.` edge on
    /// these pins is taken first, as that tick takes it: the `NMI` latch and the
    /// edge detectors in [`Registers`] hold it, and `V` is set before the
    /// instruction's tail writes it (`MOS6502-016`, `MOS6502-006`).
    ///
    /// Returns whether the part now stands at a boundary that
    /// [`Mos6502::at_boundary`] rebuilds from [`Mos6502::registers`]:
    /// `false`, changing nothing, unless
    /// [`Mos6502::is_at_instruction_boundary`], where pins present
    /// [`Ctrl::RDY_LOW`] while a read is outstanding, which hold that read as
    /// [`Mos6502::tick`] does, and where they present [`Ctrl::RES`], under
    /// which that tick drops the byte and the tail (`MOS6502-013`). A part is
    /// rebuilt at a later boundary instead.
    #[must_use]
    pub fn settle(&mut self, inp: Pins) -> bool {
        if !self.is_at_instruction_boundary()
            || inp.ctrl.contains(Ctrl::RES)
            || self.stalls(inp.ctrl)
        {
            return false;
        }
        self.edges(inp.ctrl);
        self.latch(inp);
        self.complete();
        true
    }

    /// A part standing at an instruction boundary with these registers, past
    /// reset, with the `NMI` latch and the edge detectors as `regs` holds them.
    ///
    /// `regs.p` is taken as the part holds it: bit 5 set (`MOS6502-056`) and
    /// bit 4 clear, since it is not a register bit (`MOS6502-028`).
    ///
    /// `magic_constant` is as [`Mos6502::new`] takes it. Until its
    /// first tick, [`Mos6502::is_reading`] and [`Mos6502::is_writing`] read `false`,
    /// whatever the original's last cycle did.
    #[must_use]
    pub const fn at_boundary(magic_constant: u8, regs: Registers) -> Self {
        let mut cpu = Self::new(magic_constant);
        cpu.a = regs.a;
        cpu.x = regs.x;
        cpu.y = regs.y;
        cpu.s = regs.s;
        cpu.p = (regs.p | FLAG_U) & !FLAG_B;
        cpu.pc = regs.pc;
        cpu.nmi_pending = regs.nmi_pending;
        cpu.nmi_last = regs.nmi_n.is_low();
        cpu.so_last = regs.so_n.is_low();
        cpu.res_pending = false;
        cpu
    }

    /// Latch the input lines this tick presents.
    fn sample(&mut self, ctrl: Ctrl) {
        self.res_held = ctrl.contains(Ctrl::RES);
        if self.res_held {
            self.res_pending = true;
            self.stopped = false;
            self.cycle = 1;
            self.finish = Finish::None;
            self.interrupt = None;
        }
        self.edges(ctrl);
        self.irq_line = ctrl.contains(Ctrl::IRQ);
    }

    /// Take the `NMI` and `S.O.` edges this tick presents.
    fn edges(&mut self, ctrl: Ctrl) {
        let nmi = ctrl.contains(Ctrl::NMI);
        if nmi && !self.nmi_last {
            self.nmi_pending = true;
        }
        self.nmi_last = nmi;
        let so = ctrl.contains(Ctrl::SO);
        if so && !self.so_last {
            self.p |= FLAG_V;
        }
        self.so_last = so;
    }

    /// Turn the outstanding bus operation into pins.
    fn show(&self) -> Pins {
        let mut out = Pins {
            address: 0,
            d: None,
            ctrl: Ctrl::NONE,
        };
        if self.fetching {
            out.ctrl |= Ctrl::SYNC;
        }
        match self.outstanding {
            Bus::Read(address) => out.address = address,
            Bus::Write(address, d) => {
                out.address = address;
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
            Mos6502::at_boundary(
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
        assert_eq!(rebuilt(0x00), FLAG_U, "MOS6502-056");
        assert_eq!(rebuilt(0xff), !FLAG_B, "MOS6502-028");
        let regs = Registers::default();
        assert_eq!(
            Mos6502::at_boundary(DEFAULT_MAGIC_CONSTANT, regs).registers(),
            regs
        );
    }

    #[test]
    fn a_new_part_reaches_its_reset_vector_in_seven_cycles_without_writing() {
        let mut cpu = Mos6502::default();
        let mut pins = Pins::idle();
        let mut cycles = std::vec::Vec::new();
        for _ in 0..7 {
            pins = cpu.tick(pins);
            cycles.push((pins.address, cpu.is_writing()));
            pins = pins.with_d(match pins.address {
                0xfffc => 0x34,
                0xfffd => 0x12,
                _ => 0xaa,
            });
        }
        assert!(
            cycles.iter().all(|&(_, write)| !write),
            "MOS6502-013: while RES is Low writing is inhibited: {cycles:?}"
        );
        assert_eq!(cycles[5].0, 0xfffc);
        assert_eq!(cycles[6].0, 0xfffd);
        pins = cpu.tick(pins);
        assert_eq!(pins.address, 0x1234);
        assert!(pins.ctrl.contains(Ctrl::SYNC), "MOS6502-003");
    }

    /// One bus cycle on a flat 64 KiB memory, `IRQ` held as given.
    fn cycle(cpu: &mut Mos6502, pins: Pins, mem: &mut [u8], irq: bool) -> (Pins, (u16, u8, bool)) {
        let ctrl = if irq { Ctrl::IRQ } else { Ctrl::NONE };
        let mut out = cpu.tick(Pins { ctrl, ..pins });
        if cpu.is_writing() {
            let d = out.d.expect("a write drives d");
            mem[usize::from(out.address)] = d;
            (out, (out.address, d, true))
        } else {
            let d = mem[usize::from(out.address)];
            out.d = Some(d);
            (out, (out.address, d, false))
        }
    }

    #[test]
    fn settle_under_rdy_low_takes_nothing_and_rdy_high_takes_the_byte() {
        let mut mem = std::vec![0xeau8; 0x10000];
        mem[0x0200..0x0202].copy_from_slice(&[0xa9, 0x42]);
        let mut cpu = Mos6502::at_boundary(
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
        assert_eq!(held, cpu, "MOS6502-005: the read is held, nothing taken");
        assert!(cpu.settle(pins));
        assert_eq!(cpu.registers().a, 0x42);
    }

    #[test]
    fn a_jammed_part_reads_nothing() {
        let mut mem = std::vec![0xeau8; 0x10000];
        mem[0x0200] = 0x02;
        let mut cpu = Mos6502::at_boundary(
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
            assert_eq!(pins.address, 0xffff, "MOS6502-OPEN-09");
            assert!(!cpu.is_reading() && !cpu.is_writing());
        }
    }

    #[test]
    fn a_new_part_is_at_no_boundary_until_its_vector_is_read() {
        let mut cpu = Mos6502::default();
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
        let program = [0xa2, 0x05, 0x58, 0xca, 0x48, 0x9d, 0x00, 0x03, 0xea, 0xea];
        let mut mem = std::vec![0xeau8; 0x10000];
        mem[0x0200..0x0200 + program.len()].copy_from_slice(&program);
        mem[0xfffe] = 0x00;
        mem[0xffff] = 0x04;
        let mut cpu = Mos6502::at_boundary(
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
        while instructions < 2 {
            (pins, _) = cycle(&mut cpu, pins, &mut mem, false);
            if cpu.is_at_instruction_boundary() {
                instructions += 1;
            }
        }
        assert!(cpu.settle(pins));
        let regs = cpu.registers();
        assert_eq!(regs.p & FLAG_I, 0, "CLI has run");
        let mut copy = Mos6502::at_boundary(DEFAULT_MAGIC_CONSTANT, regs);
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
