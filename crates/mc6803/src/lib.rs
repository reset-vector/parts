//! The Motorola MC6803, emulated at its pins.
//!
//! The crate depends only on `signal`, allocates nothing, does no I/O and is
//! `no_std`: an [`Mc6803`] is a plain value that advances one E cycle at a time
//! and shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! ```
//! use mc6803::{Ctrl, Mc6803, Pins};
//!
//! let mut cpu = Mc6803::new();
//! // Mode 2 is latched off P22 P21 P20 = L H L on the rising edge of RESET
//! // (`MC6803-004`), as the idle pins hold them; hold RESET for three
//! // cycles, then let it go.
//! let held = Pins::idle().with(Ctrl::RESET);
//! for _ in 0..3 {
//!     let _ = cpu.tick(held);
//! }
//! let mut pins = Pins::idle();
//! let mut vector = 0;
//! for _ in 0..4 {
//!     pins = cpu.tick(pins);
//!     if pins.address == 0xfffe || pins.address == 0xffff {
//!         vector += 1;
//!     }
//!     pins = pins.with_d(0x00);
//! }
//! assert!(vector >= 2, "MC6803-010: the reset vector is fetched from $FFFE");
//! ```
//!
//! # The shape
//!
//! One [`Mc6803::tick`] is one E cycle (`MC6803-003`), and every cycle is a
//! bus cycle (`MC6803-007`): the caller presents the pins the outside world
//! drives, with [`Pins::d`] holding the byte the bus produced for the
//! request the **previous** tick returned if that was an external read, and
//! receives the request for this cycle. The register area and, in mode 2, the
//! internal RAM are served inside the package, which
//! [`Mc6803::is_internal_access`] reports (`MC6803-005`, `MC6803-008`). Ports 1 and 2 are levels in both
//! directions on the same word.
//!
//! # The part, not the machine
//!
//! No decode, no memory, no ROM: the `MC6803` has none (`MC6803-001`), and
//! what sits at every external address is the board's. The clock rate is the
//! board's too; nothing here knows what an E cycle is in seconds.
//!
//! # What this crate chooses
//!
//! Each open item names a choice of this crate's, marked where it is written.
//! The ones a consumer meets first are `MC6803-OPEN-01` (a mode other than 2
//! or 3 is treated as mode 2 with the latched bits read back as latched),
//! `MC6803-OPEN-02` (an undefined opcode is a two-cycle no-operation),
//! `MC6803-OPEN-04` (the registers and RAM start at zero, the CCR at `$D0`)
//! and `MC6803-OPEN-05` (one idle `$FFFE` cycle between the release of `RESET`
//! and the vector read). The
//! serial interface's bi-phase format is not implemented (`MC6803-OPEN-07`):
//! with `CC1:CC0` = `00` the lines carry NRZ. The rest:
//!
//! - `DAA` leaves `V` as it was and, outside its table's rows, adds by the
//!   nearest row's rule (`MC6803-OPEN-03`).
//! - The RMCR and the transmit data register read back the bits last
//!   written; the reserved addresses `$15`–`$1F` and the unused bits of `$14`
//!   read zero (`MC6803-OPEN-06`).
//! - The NRZ receiver samples a bit once, in the fifth of its eight
//!   sub-intervals (`MC6803-OPEN-08`).
//! - A second edge at `P20` before the first's capture transfer is ignored
//!   (`MC6803-OPEN-09`).
//! - Out of `WAI`, the service routine's opcode is fetched five cycles after
//!   the cycle in which an `NMI` edge is presented and six after an `IRQ1`
//!   level is (`MC6803-OPEN-10`).
//! - A port data register read returns the output register's level for an
//!   output bit (`MC6803-OPEN-11`).
//! - A maskable request presented in the cycle an interrupt sequence sets
//!   `I` is not latched (`MC6803-OPEN-12`).
//! - An `NMI` edge or `IRQ2` level presented in an instruction's last cycle
//!   is taken at the boundary after it, an `IRQ1` level one cycle earlier
//!   (`MC6803-OPEN-13`).

#![no_std]

mod alu;
mod decode;
mod exec;
mod pins;
mod sci;
mod timer;

pub use alu::{C as FLAG_C, H as FLAG_H, I as FLAG_I, N as FLAG_N, V as FLAG_V, Z as FLAG_Z};
pub use decode::{
    Accumulator, Alu8, Alu16, Cond, Decoded, Inherent, Kind, Reg16, Stacked, Target, Unary, decode,
};
pub use pins::{Ctrl, Pins};
pub use sci::{Receiver, Sci, Transmitter, rmcr, trcsr};
pub use signal::Level;
pub use timer::{COUNTER_PRESET, Timer, tcsr};

/// The vectors of `MC6803-031`, high byte first at the address named.
pub mod vector {
    /// `RESET`.
    pub const RESET: u16 = 0xfffe;
    /// `NMI`.
    pub const NMI: u16 = 0xfffc;
    /// `SWI`.
    pub const SWI: u16 = 0xfffa;
    /// `IRQ1`.
    pub const IRQ1: u16 = 0xfff8;
    /// Timer input capture.
    pub const ICF: u16 = 0xfff6;
    /// Timer output compare.
    pub const OCF: u16 = 0xfff4;
    /// Timer overflow.
    pub const TOF: u16 = 0xfff2;
    /// Serial interface.
    pub const SCI: u16 = 0xfff0;
}

/// The internal register area of `MC6803-041` and `MC6803-046`, by address.
pub mod reg {
    /// The first address of the internal RAM.
    pub const RAM_START: u16 = 0x80;
    /// Port 1 data direction register, write-only.
    pub const P1DDR: u16 = 0x00;
    /// Port 2 data direction register, write-only.
    pub const P2DDR: u16 = 0x01;
    /// Port 1 data register.
    pub const P1DATA: u16 = 0x02;
    /// Port 2 data register.
    pub const P2DATA: u16 = 0x03;
    /// Timer control and status register.
    pub const TCSR: u16 = 0x08;
    /// Counter, high byte.
    pub const COUNTER_UPPER: u16 = 0x09;
    /// Counter, low byte.
    pub const COUNTER_LOWER: u16 = 0x0a;
    /// Output compare register, high byte.
    pub const OCR_UPPER: u16 = 0x0b;
    /// Output compare register, low byte.
    pub const OCR_LOWER: u16 = 0x0c;
    /// Input capture register, high byte.
    pub const ICR_UPPER: u16 = 0x0d;
    /// Input capture register, low byte.
    pub const ICR_LOWER: u16 = 0x0e;
    /// Rate and mode control register, write-only.
    pub const RMCR: u16 = 0x10;
    /// Transmit/receive control and status register.
    pub const TRCSR: u16 = 0x11;
    /// Receive data register, read-only.
    pub const RDR: u16 = 0x12;
    /// Transmit data register, write-only.
    pub const TDR: u16 = 0x13;
    /// RAM control register.
    pub const RAMCR: u16 = 0x14;
}

/// The bits of the RAM control register (`MC6803-012`).
pub mod ramcr {
    /// Bit 6: the internal RAM answers to `$80`–`$FF`.
    pub const RAME: u8 = 1 << 6;
    /// Bit 7: standby power stayed up. Software's to set; nothing here can
    /// clear it.
    pub const STBY_PWR: u8 = 1 << 7;
}

/// `EXTAL` clock periods per E cycle: the clock generator divides by four
/// (`MC6803-003`). The crate counts E; a host counting the crystal divides.
pub const EXTAL_CYCLES_PER_MACHINE_CYCLE: usize = 4;

/// The bytes of internal RAM (`MC6803-005`).
pub const RAM_BYTES: usize = 128;

/// The bus operation one cycle performs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Bus {
    /// A read from this address.
    Read(u16),
    /// A read from this address whose byte the part discards: the cycles
    /// the sheet's tables leave without a use for the data, as an opcode read
    /// again where an interrupt opens or `SP` read while the vector is chosen.
    Dummy(u16),
    /// The idle cycle: a read of [`IDLE_ADDRESS`] no instruction asked for
    /// (`MC6803-007`).
    Idle,
    /// A write of this byte to this address.
    Write(u16, u8),
}

/// The address every idle bus cycle reads (`MC6803-007`).
pub const IDLE_ADDRESS: u16 = 0xffff;

/// What the sequencer is in the middle of.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Seq {
    /// `RESET` is Low: the part is held (`MC6803-009`).
    Held,
    /// `RESET` has gone High: the vector is being fetched (`MC6803-011`).
    Reset,
    /// An instruction, decoded at `op`.
    Instruction,
    /// The twelve-cycle hardware interrupt sequence (`MC6803-035`).
    Interrupt,
    /// The wait state after `WAI` (`MC6803-029`).
    Wait,
    /// Leaving the wait state: the vector is being fetched.
    WaitExit,
}

/// What the tail of an instruction still has to do with the byte its last
/// cycle read.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Finish {
    None,
    /// An eight-bit operation on the byte, into an accumulator.
    Alu8(Alu8, Accumulator),
    /// A pull into an accumulator.
    Pull(Accumulator),
    /// The low half of a sixteen-bit load; `tmp` holds the high half.
    Ld16(Reg16),
    /// The low half of a new program counter; `tmp` holds the high half.
    Pc,
    /// The low half of `X`; `tmp` holds the high half.
    X,
}

/// One port's output side: the registers a program writes to drive its
/// lines (`MC6803-038`, `MC6803-039`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Port {
    /// The latched output data.
    pub data: u8,
    /// The data direction register, 1 for output.
    pub direction: u8,
}

/// The programmer-visible registers, as [`Mc6803::registers`] reads them and
/// [`Mc6803::at_boundary`] takes them: the processor's, every on-chip
/// register a program reads or writes, the internal RAM, and the latches
/// that carry an interrupt or a wait across an instruction boundary. `D` is
/// `A:B`.
///
/// `Default` is the processor's registers and the RAM at zero, every on-chip
/// register as reset leaves it in mode 2 (`MC6803-009`, `MC6803-012`),
/// nothing pending, not waiting, and `NMI` and `IRQ1` last seen High.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`Mc6803::registers`] and sets the fields it wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// Accumulator A.
    pub a: u8,
    /// Accumulator B.
    pub b: u8,
    /// Index register.
    pub x: u16,
    /// Stack pointer.
    pub sp: u16,
    /// Program counter.
    pub pc: u16,
    /// Condition code register; bits 6 and 7 always read as ones
    /// (`MC6803-024`). Its flags are the `FLAG_` constants.
    pub ccr: u8,
    /// An instruction has cleared `I` and the clear has not yet landed:
    /// `ccr` still shows `I` set, and it clears in the next cycle, after
    /// that cycle has been decided with `I` set (`MC6803-030`).
    pub cli_deferred: bool,
    /// The operating mode, `PC2 PC1 PC0` as latched at the last rising edge
    /// of `RESET` (`MC6803-004`), which a read of port 2's data register
    /// returns in bits 7-5 (`MC6803-039`). Not an addressing mode.
    pub operating_mode: u8,
    /// The RAM control register, `$14`; its bits are [`ramcr`]
    /// (`MC6803-012`).
    pub ramcr: u8,
    /// Port 1: its output data register, `$02`, and its data direction
    /// register, `$00`, a set bit a line the part drives (`MC6803-038`).
    pub port_1: Port,
    /// Port 2: its output data register, `$03`, and its data direction
    /// register, `$01`, five bits each, with the lines the serial interface
    /// has taken over folded into the direction (`MC6803-039`, `MC6803-047`,
    /// `MC6803-048`).
    pub port_2: Port,
    /// The programmable timer (`MC6803-041` to `MC6803-045`).
    pub timer: Timer,
    /// The serial communications interface (`MC6803-046` to `MC6803-050`).
    pub sci: Sci,
    /// The internal RAM, `$80` first (`MC6803-005`).
    pub ram: [u8; RAM_BYTES],
    /// `WAI` has stacked the state and the part is in its wait state
    /// (`MC6803-029`).
    pub halted: bool,
    /// The `NMI` flip-flop: an edge seen and not yet served, if any
    /// (`MC6803-034`).
    pub nmi: Option<Nmi>,
    /// The `IRQ1` flip-flop (`MC6803-034`).
    pub irq1_pending: bool,
    /// The `IRQ2` flip-flop, set by an enabled timer or serial flag
    /// (`MC6803-034`, `MC6803-037`).
    pub irq2_pending: bool,
    /// `NMI`'s level at the last tick, for its edge (`MC6803-032`).
    pub nmi_n: Level,
    /// `IRQ1`'s level at the last tick, which reaches its flip-flop a cycle
    /// later (`MC6803-035`).
    pub irq1_n: Level,
}

/// An `NMI` edge latched in the flip-flop, held in [`Registers::nmi`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Nmi {
    /// Not yet served: the next boundary takes it.
    Pending,
    /// Came on the pins of the tick after this boundary, which
    /// [`Mc6803::settle`] latched as that tick does, at its end: that tick
    /// decides without it, and the boundary after takes it. Set by
    /// [`Mc6803::settle`] alone.
    Unseen,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            b: 0,
            x: 0,
            sp: 0,
            pc: 0,
            ccr: alu::UPPER,
            cli_deferred: false,
            operating_mode: 0b010,
            ramcr: ramcr::RAME,
            port_1: Port {
                data: 0,
                direction: 0,
            },
            port_2: Port {
                data: 0,
                direction: 0,
            },
            timer: Timer::default(),
            sci: Sci::default(),
            ram: [0; RAM_BYTES],
            halted: false,
            nmi: None,
            irq1_pending: false,
            irq2_pending: false,
            nmi_n: Level::High,
            irq1_n: Level::High,
        }
    }
}

/// The part.
///
/// Advance it with [`Mc6803::tick`], one E cycle per call. Cloneable, so a
/// test can fork one and run both halves.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Mc6803 {
    a: u8,
    b: u8,
    x: u16,
    sp: u16,
    pc: u16,
    /// Bits 6 and 7 are always 1 (`MC6803-024`).
    ccr: u8,

    /// The 128 bytes at `$80`–`$FF` in mode 2 (`MC6803-005`).
    ram: [u8; RAM_BYTES],
    /// `PC2 PC1 PC0`, latched off `P22 P21 P20` on the rising edge of
    /// `RESET` (`MC6803-004`).
    mode: u8,
    /// The RAM control register (`MC6803-012`).
    ramcr: u8,

    /// Port 1 data direction (`MC6803-038`): 1 is an output.
    p1_ddr: u8,
    /// Port 1 output data register.
    p1_out: u8,
    /// Port 2 data direction, five bits (`MC6803-039`).
    p2_ddr: u8,
    /// Port 2 output data register, five bits.
    p2_out: u8,
    /// The levels last presented on port 1.
    p1_in: u8,
    /// The levels last presented on port 2.
    p2_in: u8,
    /// The output register and direction as the pins show them this cycle:
    /// a write lands at the end of its cycle (`MC6803-040`), so the pins
    /// follow the registers one tick behind.
    p1_shown: (u8, u8),
    p2_shown: (u8, u8),

    /// The programmable timer (`MC6803-041` to `MC6803-045`).
    timer: timer::Unit,
    /// The serial communications interface (`MC6803-046` to `MC6803-050`).
    sci: sci::Unit,

    /// The sequencer.
    seq: Seq,
    /// The instruction in flight.
    instr: Decoded,
    /// Which cycle of it comes next; 1 means the next cycle fetches an opcode.
    cycle: u8,
    /// The byte the previous cycle read.
    dl: u8,
    /// The effective address being assembled.
    ea: u16,
    /// A byte held between cycles.
    tmp: u8,
    /// The vector chosen for the interrupt sequence in flight.
    vector: u16,
    /// What the tail of the instruction still has to do.
    finish: Finish,

    /// The `NMI` interrupt flip-flop (`MC6803-034`), and whether `settle`
    /// set it from the next tick's pins, to be held back from that tick's
    /// decision.
    nmi: Option<Nmi>,
    /// The `IRQ1` flip-flop.
    irq1_ff: bool,
    /// The `IRQ2` flip-flop.
    irq2_ff: bool,
    /// The level `NMI` was presented at last tick, for its edge.
    nmi_last: bool,
    /// `IRQ1` as sampled last tick: the extra synchronisation cycle of
    /// `MC6803-035`.
    irq1_sync: bool,
    /// An instruction cleared `I` and the clear has not yet landed
    /// (`MC6803-030`).
    i_clear_pending: bool,

    /// The bus operation the last tick asked for.
    outstanding: Bus,
    /// The byte an internal read produced, taken instead of the pins' data.
    internal: Option<u8>,
    /// Nothing has been asked for yet: the first tick has not run.
    started: bool,
}

impl Default for Mc6803 {
    fn default() -> Self {
        Self::new()
    }
}

impl Mc6803 {
    /// A part that has just been powered up, with `RESET` not yet seen.
    ///
    /// Everything the manual lists as cleared or preset by reset is so here,
    /// and everything it does not list — the accumulators, `X`, `SP`, `PC`,
    /// the RAM — starts at zero, which is `MC6803-OPEN-04`. The part sits in
    /// the held state, and the first tick without [`Ctrl::RESET`] is the
    /// rising edge: it latches the mode from port 2 (`MC6803-004`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            a: 0,
            b: 0,
            x: 0,
            sp: 0,
            pc: 0,
            ccr: alu::UPPER | alu::I,
            ram: [0; RAM_BYTES],
            mode: 0b010,
            ramcr: ramcr::RAME,
            p1_ddr: 0,
            p1_out: 0,
            p2_ddr: 0,
            p2_out: 0,
            p1_in: 0,
            p2_in: 0,
            p1_shown: (0, 0),
            p2_shown: (0, 0),
            timer: timer::Unit::new(),
            sci: sci::Unit::new(),
            seq: Seq::Held,
            instr: decode(0x01),
            cycle: 1,
            dl: 0,
            ea: 0,
            tmp: 0,
            vector: vector::RESET,
            finish: Finish::None,
            nmi: None,
            irq1_ff: false,
            irq2_ff: false,
            nmi_last: false,
            irq1_sync: false,
            i_clear_pending: false,
            outstanding: Bus::Read(vector::RESET),
            internal: None,
            started: false,
        }
    }

    /// The registers as they stand. Between instructions they are the
    /// boundary's, once [`Mc6803::settle`] has taken the last read's byte,
    /// and `Mc6803::at_boundary(cpu.registers())` runs on as this part does;
    /// mid-instruction, what the instruction has written so far.
    #[must_use]
    pub const fn registers(&self) -> Registers {
        Registers {
            a: self.a,
            b: self.b,
            x: self.x,
            sp: self.sp,
            pc: self.pc,
            ccr: self.ccr,
            cli_deferred: self.i_clear_pending,
            operating_mode: self.mode,
            ramcr: self.ramcr,
            port_1: Port {
                data: self.p1_out,
                direction: self.p1_ddr,
            },
            port_2: Port {
                data: self.p2_out,
                direction: self.p2_ddr,
            },
            timer: self.timer.registers(),
            sci: self.sci.registers(),
            ram: self.ram,
            halted: matches!(self.seq, Seq::Wait),
            nmi: self.nmi,
            irq1_pending: self.irq1_ff,
            irq2_pending: self.irq2_ff,
            nmi_n: Level::from_bool(!self.nmi_last),
            irq1_n: Level::from_bool(!self.irq1_sync),
        }
    }

    /// The double accumulator, `A:B`.
    pub(crate) const fn d(&self) -> u16 {
        ((self.a as u16) << 8) | self.b as u16
    }

    /// Set the double accumulator.
    pub(crate) const fn set_d(&mut self, d: u16) {
        self.a = (d >> 8) as u8;
        self.b = d as u8;
    }

    /// The internal RAM answers at `$80`–`$FF`: mode 2 with `RAME` set
    /// (`MC6803-005`, `MC6803-012`). `MC6803-OPEN-01`: any latched value but
    /// 3 is taken as mode 2.
    #[must_use]
    pub const fn is_ram_enabled(&self) -> bool {
        self.mode != 0b011 && self.ramcr & ramcr::RAME != 0
    }

    /// The next cycle begins an instruction, or is one more cycle of
    /// `WAI`'s wait state: nothing is in flight.
    #[must_use]
    pub const fn is_at_instruction_boundary(&self) -> bool {
        matches!(self.seq, Seq::Instruction) && self.cycle == 1 || matches!(self.seq, Seq::Wait)
    }

    /// `WAI` has stopped the part until an interrupt or a reset: it is in
    /// the wait state (`MC6803-029`).
    #[must_use]
    pub const fn is_halted(&self) -> bool {
        matches!(self.seq, Seq::Wait)
    }

    /// The last tick's cycle reads an address an instruction or a sequence
    /// asked for: place the byte at its address into [`Pins::d`] before the
    /// next tick, unless [`Mc6803::is_internal_access`]. False, though `R/W`
    /// is High and the address driven, on an idle cycle, whose read of
    /// [`IDLE_ADDRESS`] takes nothing (`MC6803-007`), and on a dummy cycle,
    /// whose byte the part discards: the read of `$FFFE` after `RESET`
    /// rises, the two opcode reads and the `SP` read that open an interrupt,
    /// each cycle of the wait state and the `SP` read that leaves it, the
    /// `SP` read of `INS`, `DES`, `TSX`, the pulls, `RTS`, `RTI` and `SWI`;
    /// also while `RESET` holds the part, and before the first tick.
    #[must_use]
    pub const fn is_reading(&self) -> bool {
        self.started && !matches!(self.seq, Seq::Held) && matches!(self.outstanding, Bus::Read(_))
    }

    /// The last tick's cycle writes its data to its address: `R/W` is Low.
    #[must_use]
    pub const fn is_writing(&self) -> bool {
        matches!(self.outstanding, Bus::Write(..))
    }

    /// The last tick's cycle reads an opcode: the first cycle of an
    /// instruction.
    #[must_use]
    pub const fn is_fetching(&self) -> bool {
        matches!(self.seq, Seq::Instruction) && self.cycle == 2
    }

    /// The last tick's address is served inside the package — the register
    /// area or, in mode 2, the internal RAM (`MC6803-005`). The address and
    /// `R/W` still go out on ports 3 and 4; what does not is the data of a
    /// read, which never reaches the external bus (`MC6803-008`), so a caller
    /// leaves [`Pins::d`] alone. A write here does drive the data out, as
    /// the part does. False while `RESET` holds the part.
    #[must_use]
    pub const fn is_internal_access(&self) -> bool {
        let address = match self.outstanding {
            Bus::Read(a) | Bus::Dummy(a) | Bus::Write(a, _) => a,
            Bus::Idle => IDLE_ADDRESS,
        };
        !matches!(self.seq, Seq::Held) && self.internal(address)
    }

    /// `RESET` holds the part, or has not yet been released since power-up
    /// (`MC6803-009`).
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        matches!(self.seq, Seq::Held)
    }

    /// A part standing at an instruction boundary with these registers, out
    /// of reset, waiting on if [`Registers::halted`] says so. The internal
    /// RAM, the interrupt flip-flops, the last levels of `NMI` and `IRQ1`,
    /// and the timer's and the serial interface's latches, shifters and bit
    /// clock are all carried by `regs`. Bits 6 and 7 of the CCR are set
    /// whatever `regs.ccr` says (`MC6803-024`), and the port 2 direction
    /// takes the serial interface's forcing (`MC6803-047`, `MC6803-048`), so
    /// `at_boundary(cpu.registers())` gives the registers back. What no part
    /// can hold is masked to what it can: the operating mode to three bits,
    /// RAMCR to `RAME` and `STBY PWR`, port 2 to its five lines, a serial
    /// shifter to its frame (a count above nine is nine), so any `regs` builds
    /// a part that ticks without overflow. Not carried:
    /// the levels last presented on the ports, which the next tick presents
    /// afresh before anything reads them. Until its first tick,
    /// [`Mc6803::is_reading`] and [`Mc6803::is_writing`] read `false`, whatever
    /// the original's last cycle did.
    #[must_use]
    pub const fn at_boundary(regs: Registers) -> Self {
        let mut cpu = Self::new();
        cpu.a = regs.a;
        cpu.b = regs.b;
        cpu.x = regs.x;
        cpu.sp = regs.sp;
        cpu.pc = regs.pc;
        cpu.ccr = regs.ccr | alu::UPPER;
        cpu.i_clear_pending = regs.cli_deferred;
        cpu.mode = regs.operating_mode & 0b111;
        cpu.ramcr = regs.ramcr & (ramcr::RAME | ramcr::STBY_PWR);
        cpu.p1_ddr = regs.port_1.direction;
        cpu.p1_out = regs.port_1.data;
        cpu.p2_out = regs.port_2.data & 0x1f;
        cpu.timer = timer::Unit::from_registers(regs.timer);
        cpu.sci = sci::Unit::from_registers(regs.sci);
        cpu.p2_ddr = cpu.sci.direction(regs.port_2.direction & 0x1f);
        cpu.ram = regs.ram;
        cpu.nmi = regs.nmi;
        cpu.irq1_ff = regs.irq1_pending;
        cpu.irq2_ff = regs.irq2_pending;
        cpu.nmi_last = regs.nmi_n.is_low();
        cpu.irq1_sync = regs.irq1_n.is_low();
        cpu.seq = if regs.halted {
            Seq::Wait
        } else {
            Seq::Instruction
        };
        cpu.cycle = 1;
        cpu
    }

    /// Advance one E cycle.
    ///
    /// The caller presents the input pins, and [`Pins::d`] holding the
    /// byte the bus produced for the request the **previous** tick returned,
    /// if that was an external read. What comes back is the request for this
    /// cycle, with the levels of both ports.
    #[must_use]
    pub fn tick(&mut self, inp: Pins) -> Pins {
        self.sample(inp);

        if inp.ctrl.contains(Ctrl::RESET) {
            self.hold_reset();
        } else if matches!(self.seq, Seq::Held) {
            self.mode = inp.p2 & 0b111;
            self.seq = Seq::Reset;
            self.cycle = 1;
            self.finish = Finish::None;
        }

        self.p1_shown = (self.p1_out, self.p1_ddr);
        self.p2_shown = (self.p2_out, self.p2_ddr);

        let held = matches!(self.seq, Seq::Held);
        let irq2_level = if held {
            false
        } else {
            let from_timer = self.timer.step(self.p20());
            let from_sci = self.sci.step(&self.timer, self.p2_in, self.p2_ddr);
            from_timer || from_sci
        };

        let clear_i = self.i_clear_pending;

        let late = self.nmi == Some(Nmi::Unseen);
        if late {
            self.nmi = None;
        }

        if !held {
            self.latch(inp);
            self.complete();
            self.outstanding = self.step();
            self.started = true;
        }

        if clear_i && self.i_clear_pending {
            self.i_clear_pending = false;
            self.ccr &= !alu::I;
        }

        let nmi = inp.ctrl.contains(Ctrl::NMI);
        if (late || (nmi && !self.nmi_last)) && !held {
            self.nmi = Some(Nmi::Pending);
        }
        self.nmi_last = nmi;
        let i_set = self.ccr & alu::I != 0;
        if i_set || held {
            self.irq1_ff = false;
            self.irq2_ff = false;
        } else {
            if self.irq1_sync {
                self.irq1_ff = true;
            }
            if irq2_level {
                self.irq2_ff = true;
            }
        }
        self.irq1_sync = inp.ctrl.contains(Ctrl::IRQ1) && !held;

        self.show()
    }

    /// Take the levels off the port pins.
    fn sample(&mut self, inp: Pins) {
        self.p1_in = inp.p1;
        self.p2_in = inp.p2 & 0x1f;
    }

    /// The level at `P20`, the timer's input capture line: the pin if the
    /// bit is an input, the output register if it is an output
    /// (`MC6803-044`).
    const fn p20(&self) -> bool {
        if self.p2_ddr & 1 != 0 {
            self.p2_out & 1 != 0
        } else {
            self.p2_in & 1 != 0
        }
    }

    /// Everything `RESET` Low does (`MC6803-009`).
    fn hold_reset(&mut self) {
        self.seq = Seq::Held;
        self.cycle = 1;
        self.finish = Finish::None;
        self.ccr |= alu::I;
        self.i_clear_pending = false;
        self.nmi = None;
        self.irq1_ff = false;
        self.irq2_ff = false;
        self.p1_ddr = 0;
        self.p2_ddr = 0;
        self.timer.reset();
        self.sci.reset();
        self.ramcr |= ramcr::RAME;
        self.outstanding = Bus::Read(vector::RESET);
        self.internal = None;
    }

    /// Take the byte the outstanding read asked for.
    fn latch(&mut self, inp: Pins) {
        if let Bus::Read(_) | Bus::Dummy(_) | Bus::Idle = self.outstanding
            && self.started
        {
            self.dl = self.internal.take().unwrap_or(inp.d.unwrap_or(0));
        }
    }

    /// The pins as the part drives them for the outstanding request.
    fn show(&self) -> Pins {
        let mut ctrl = Ctrl::NONE;
        let (address, d) = match self.outstanding {
            Bus::Read(a) | Bus::Dummy(a) => (a, None),
            Bus::Idle => (IDLE_ADDRESS, None),
            Bus::Write(a, d) => {
                ctrl |= Ctrl::WRITE;
                (a, Some(d))
            }
        };
        Pins {
            address,
            d,
            ctrl,
            p1: self.port1_levels(),
            p2: self.port2_levels(),
        }
    }

    /// The levels on port 1: driven bits from the output register, the rest
    /// as presented (`MC6803-038`).
    const fn port1_levels(&self) -> u8 {
        let (out, ddr) = self.p1_shown;
        (out & ddr) | (self.p1_in & !ddr)
    }

    /// The levels on port 2, with `P21` taken over by the timer output level
    /// when its direction is output (`MC6803-039`, `MC6803-043`) and the
    /// serial lines by the interface (`MC6803-047`, `MC6803-048`).
    const fn port2_levels(&self) -> u8 {
        let (mut out, ddr) = self.p2_shown;
        if ddr & 0b10 != 0 {
            out = (out & !0b10) | ((self.timer.output_level() as u8) << 1);
        }
        out = self.sci.drive(out);
        ((out & ddr) | (self.p2_in & !ddr)) & 0x1f
    }

    /// Whether an address is served inside the package (`MC6803-005`):
    /// the register area less the five port 3 and port 4 addresses, and the
    /// RAM when it is enabled.
    const fn internal(&self, address: u16) -> bool {
        match address {
            0x04..=0x07 | 0x0f => false,
            0x00..=0x1f => true,
            0x80..=0xff => self.is_ram_enabled(),
            _ => false,
        }
    }

    /// An internal read, with its side effects (`MC6803-042`, `MC6803-045`,
    /// `MC6803-046`).
    fn read_internal(&mut self, address: u16) -> u8 {
        match address {
            reg::P1DDR | reg::P2DDR => 0xff,
            reg::P1DATA => self.port1_levels(),
            reg::P2DATA => (self.mode << 5) | self.port2_levels(),
            reg::TCSR
            | reg::COUNTER_UPPER
            | reg::COUNTER_LOWER
            | reg::OCR_UPPER
            | reg::OCR_LOWER
            | reg::ICR_UPPER
            | reg::ICR_LOWER => self.timer.read(address),
            reg::RMCR | reg::TRCSR | reg::RDR | reg::TDR => self.sci.read(address),
            reg::RAMCR => self.ramcr,
            0x15..=0x1f => 0,
            _ => self.ram[(address - reg::RAM_START) as usize],
        }
    }

    /// An internal write.
    fn write_internal(&mut self, address: u16, data: u8) {
        match address {
            reg::P1DDR => self.p1_ddr = data,
            reg::P2DDR => self.p2_ddr = self.sci.direction(data & 0x1f),
            reg::P1DATA => self.p1_out = data,
            reg::P2DATA => self.p2_out = data & 0x1f,
            reg::TCSR
            | reg::COUNTER_UPPER
            | reg::COUNTER_LOWER
            | reg::OCR_UPPER
            | reg::OCR_LOWER
            | reg::ICR_UPPER
            | reg::ICR_LOWER => self.timer.write(address, data),
            reg::RMCR | reg::TRCSR | reg::RDR | reg::TDR => {
                self.sci.write(address, data);
                self.p2_ddr = self.sci.direction(self.p2_ddr);
            }
            reg::RAMCR => self.ramcr = data & (ramcr::RAME | ramcr::STBY_PWR),
            0x15..=0x1f => {}
            _ => self.ram[(address - reg::RAM_START) as usize] = data,
        }
    }

    /// Ask for a read, serving it here if the address is internal.
    pub(crate) fn read(&mut self, address: u16) -> Bus {
        if self.internal(address) {
            let v = self.read_internal(address);
            self.internal = Some(v);
        }
        Bus::Read(address)
    }

    /// Ask for a read whose byte is discarded, serving it here as
    /// [`Mc6803::read`] does if the address is internal.
    pub(crate) fn dummy(&mut self, address: u16) -> Bus {
        match self.read(address) {
            Bus::Read(a) => Bus::Dummy(a),
            bus => bus,
        }
    }

    /// Ask for a write, performing it here if the address is internal.
    pub(crate) fn write(&mut self, address: u16, data: u8) -> Bus {
        if self.internal(address) {
            self.write_internal(address, data);
        }
        Bus::Write(address, data)
    }

    /// The vector the priority encoder selects in the tenth cycle of an
    /// interrupt sequence (`MC6803-036`, `MC6803-037`).
    pub(crate) fn select_vector(&self) -> u16 {
        if self.nmi.is_some() {
            vector::NMI
        } else if self.irq1_ff {
            vector::IRQ1
        } else {
            self.timer.vector().unwrap_or(vector::SCI)
        }
    }

    /// Any interrupt flip-flop is set.
    pub(crate) const fn interrupt_pending(&self) -> bool {
        self.nmi.is_some() || self.irq1_ff || self.irq2_ff
    }

    /// Set `I`, which also resets the two maskable flip-flops
    /// (`MC6803-034`).
    pub(crate) const fn set_i(&mut self) {
        self.ccr |= alu::I;
        self.i_clear_pending = false;
        self.irq1_ff = false;
        self.irq2_ff = false;
    }

    /// Write the CCR from software: bits 0–5 taken, `I` clearing delayed
    /// one cycle (`MC6803-024`, `MC6803-030`).
    pub(crate) const fn set_ccr(&mut self, value: u8) {
        let want_i = value & alu::I != 0;
        let was_i = self.ccr & alu::I != 0;
        self.ccr = (value & 0x3f) | alu::UPPER | alu::I;
        if want_i {
            self.i_clear_pending = false;
            if !was_i {
                self.irq1_ff = false;
                self.irq2_ff = false;
            }
        } else {
            self.i_clear_pending = true;
        }
    }

    /// Clear the NMI flip-flop, as cycle 10 of a hardware interrupt sequence
    /// does (`MC6803-034`).
    pub(crate) const fn clear_nmi(&mut self) {
        self.nmi = None;
    }
}
