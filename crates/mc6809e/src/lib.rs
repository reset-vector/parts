//! The Motorola MC6809E, emulated at its pins.
//!
//! The crate depends only on `signal`, allocates nothing, does no I/O and is
//! `no_std`: an [`Mc6809e`] is a plain value that advances one `E` cycle at a
//! time and shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! ```
//! use mc6809e::{Ctrl, Mc6809e, Pins};
//!
//! let mut cpu = Mc6809e::new();
//! // Hold RESET for two cycles (`MC6809E-033`), then let it go.
//! for _ in 0..2 {
//!     cpu.tick(Pins::idle().with(Ctrl::RESET));
//! }
//! let mut pins = Pins::idle();
//! let mut acknowledged = 0;
//! for _ in 0..6 {
//!     pins = cpu.tick(pins);
//!     if pins.ctrl.contains(Ctrl::BS) && pins.address.is_some_and(|a| a >= 0xfffe) {
//!         acknowledged += 1;
//!     }
//!     pins = pins.with_d(0x00);
//! }
//! assert_eq!(acknowledged, 2, "MC6809E-005: BS marks both vector reads");
//! ```
//!
//! # The shape
//!
//! One [`Mc6809e::tick`] is one `E` cycle. The caller presents the pins the
//! outside world drives, with [`Pins::d`] holding the byte the bus
//! produced for the request the **previous** tick returned, and receives the
//! request for this cycle. The control inputs presented with a tick are the
//! levels during the cycle that tick completes; `TSC` is the exception and
//! applies to the cycle the tick returns (`MC6809E-OPEN-14`).
//!
//! A cycle the part does not need the bus for is a **dummy access**:
//! `$FFFF`, `R/W` High, `BS` Low (`MC6809E-004`). The part has no `VMA`
//! pin; [`Mc6809e::is_accessing`] says what the pins cannot, that the cycle is a
//! real access, and it is the previous cycle's `AVMA` (`MC6809E-006`).
//!
//! # The part, not the machine
//!
//! No decode, no memory, no clock generator: `E` and `Q` are the board's,
//! and the caller simply calls [`Mc6809e::tick`] once per `E`
//! (`MC6809E-002`).
//!
//! # What this crate chooses
//!
//! Each open item names a choice of this crate's, marked where it is written.
//! The ones a consumer meets first are `MC6809E-OPEN-01` (an undefined
//! opcode), `MC6809E-OPEN-07` (the registers at power-up), `MC6809E-OPEN-12`
//! (which cycle's level decides an interrupt) and `MC6809E-OPEN-13` (what the
//! address pins show while the bus floats). The rest:
//!
//! - An indexed postbyte with low nibble `7`, `A` or `E`, or `F` without the
//!   indirect bit, runs as `,R`; `,R+` and `,-R` with the indirect bit run as
//!   their direct forms followed by the indirection (`MC6809E-OPEN-02`).
//! - `TFR` and `EXG` read an eight-bit register as sixteen bits with `$FF`
//!   above it, write an eight-bit one from the low byte, read an undefined
//!   code as `$FFFF` and write it nowhere (`MC6809E-OPEN-03`).
//! - The fifth cycle of an extended-indirect instruction is a dummy access
//!   with `AVMA` Low (`MC6809E-OPEN-04`).
//! - `H` is left as it was by the subtract-like rows, `NEG` and `ASL`
//!   (`MC6809E-OPEN-05`).
//! - A chain of prefixes takes the first prefix's page; every further prefix
//!   is fetched (`MC6809E-OPEN-06`).
//! - While `RESET` is Low the bus reads `$FFFE` with `R/W` High on every
//!   cycle (`MC6809E-OPEN-08`).
//! - `NMI` is armed by `LDS`, `LEAS`, `TFR` or `EXG` into `S` and `PULU` of
//!   `S`, not by the pointer's own pushes, pulls or index steps; an edge
//!   while disarmed is discarded (`MC6809E-OPEN-09`).
//! - `DAA` adds `$06` when `H` is set or the low digit is above nine, and
//!   `$60` when `C` is set, the high digit is above nine, or it is above
//!   eight with a low digit above nine; `C` takes the `$60`
//!   (`MC6809E-OPEN-10`).
//! - `BUSY` rises on the first byte of a sixteen-bit immediate operand as on
//!   a memory one, and not on `JSR`'s, `RTS`'s or `RTI`'s stack accesses or a
//!   hardware interrupt's pushes (`MC6809E-OPEN-11`).

#![no_std]

mod alu;
mod decode;

pub use alu::{
    C as FLAG_C, E as FLAG_E, F as FLAG_F, H as FLAG_H, I as FLAG_I, N as FLAG_N, V as FLAG_V,
    Z as FLAG_Z,
};
use decode::Addressing;
pub use decode::{
    Accumulator, Alu8, Alu16, Cond, Decoded, Inherent, Kind, LeaRegister, Page, Reg16, Stack, Swi,
    Target, Unary, decode,
};

use alu::{E, F, I, Z};
pub use signal::Level;

/// The vectors of Table 1 (`MC6809E-034`), high byte at the address named.
pub mod vector {
    /// Restart.
    pub const RESET: u16 = 0xfffe;
    /// Non-maskable interrupt.
    pub const NMI: u16 = 0xfffc;
    /// Software interrupt.
    pub const SWI: u16 = 0xfffa;
    /// Interrupt request.
    pub const IRQ: u16 = 0xfff8;
    /// Fast interrupt request.
    pub const FIRQ: u16 = 0xfff6;
    /// Software interrupt 2.
    pub const SWI2: u16 = 0xfff4;
    /// Software interrupt 3.
    pub const SWI3: u16 = 0xfff2;
}

/// The address of a dummy access (`MC6809E-004`).
pub const IDLE_ADDRESS: u16 = 0xffff;

/// The control signals.
///
/// A **set flag means the condition it names holds**, and the empty set is
/// every pin at its idle level. Several pins are **active Low**: an
/// active-low pin's flag bears the pin's name and set means asserted (Low).
/// Each constant's doc says which pin level it is.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u16);

impl Ctrl {
    /// No condition present.
    pub const NONE: Self = Self(0);

    /// Out. `R/W` is Low: [`Pins::d`] holds the byte the part drives.
    pub const WRITE: Self = Self(1 << 0);
    /// Out. `BA` is High: the buses float (`MC6809E-005`).
    pub const BA: Self = Self(1 << 1);
    /// Out. `BS` is High (`MC6809E-005`).
    pub const BS: Self = Self(1 << 2);
    /// Out. `AVMA` is High: the part uses the bus next cycle
    /// (`MC6809E-006`).
    pub const AVMA: Self = Self(1 << 3);
    /// Out. `LIC` is High (`MC6809E-007`).
    pub const LIC: Self = Self(1 << 4);
    /// Out. `BUSY` is High (`MC6809E-008`).
    pub const BUSY: Self = Self(1 << 5);

    /// In. `RESET` is Low (`MC6809E-033`).
    pub const RESET: Self = Self(1 << 6);
    /// In. `NMI` is Low; the part responds to the falling edge
    /// (`MC6809E-035`).
    pub const NMI: Self = Self(1 << 7);
    /// In. `FIRQ` is Low: a level (`MC6809E-036`).
    pub const FIRQ: Self = Self(1 << 8);
    /// In. `IRQ` is Low: a level (`MC6809E-037`).
    pub const IRQ: Self = Self(1 << 9);
    /// In. `HALT` is Low (`MC6809E-043`).
    pub const HALT: Self = Self(1 << 10);
    /// In. `TSC` is High (`MC6809E-009`).
    pub const TSC: Self = Self(1 << 11);

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
        const NAMES: [(Ctrl, &str); 12] = [
            (Ctrl::WRITE, "WRITE"),
            (Ctrl::BA, "BA"),
            (Ctrl::BS, "BS"),
            (Ctrl::AVMA, "AVMA"),
            (Ctrl::LIC, "LIC"),
            (Ctrl::BUSY, "BUSY"),
            (Ctrl::RESET, "RESET"),
            (Ctrl::NMI, "NMI"),
            (Ctrl::FIRQ, "FIRQ"),
            (Ctrl::IRQ, "IRQ"),
            (Ctrl::HALT, "HALT"),
            (Ctrl::TSC, "TSC"),
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

/// The pin state for one `E` cycle, in both directions.
///
/// `Copy`, with no lifetime and no callback: a bus resolved through a
/// callback is a machine leaking into the chip.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub struct Pins {
    /// `A0`–`A15`: `None` while they float (`MC6809E-OPEN-13`).
    pub address: Option<u16>,
    /// `D0`–`D7`: `None` while nothing drives them. The part returns `Some`
    /// only on a write. A caller answers a read with `Some(byte)` on the next
    /// tick, and a read that finds `None` takes 0.
    pub d: Option<u8>,
    /// The control signals.
    pub ctrl: Ctrl,
}

impl Pins {
    /// All lines quiet: both buses undriven, no input asserted.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            address: None,
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

/// A register a push, a pull or a transfer names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Reg {
    A,
    B,
    Dp,
    Cc,
    X,
    Y,
    U,
    S,
    Pc,
}

/// An index register an indexed postbyte names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Pointer {
    X,
    Y,
    U,
    S,
}

impl Pointer {
    const fn reg(self) -> Reg {
        match self {
            Self::X => Reg::X,
            Self::Y => Reg::Y,
            Self::U => Reg::U,
            Self::S => Reg::S,
        }
    }
}

/// Where a cycle's address comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Ad {
    /// `PC` plus a constant, `PC` left alone: Figure 17's "Don't Care
    /// NNNN+k".
    Pc(u8),
    Ea,
    Ea1,
    Abs(u16),
    /// A stack pointer as it stands.
    Top(Stack),
}

/// What a write drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Val {
    Hi(Reg),
    Lo(Reg),
    Byte(Reg),
    /// A read-modify-write's result.
    Tmp,
    Store8(Accumulator),
    Store16Hi(Reg16),
    Store16Lo(Reg16),
}

/// What a read's byte does, applied when the byte arrives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Act {
    None,
    Opcode,
    Page,
    Post,
    DirLo,
    EaHi,
    EaLo,
    Off8,
    OffHi,
    OffLo,
    IndHi,
    IndLo,
    TmpHi,
    Alu8,
    Alu16,
    Ld16,
    Rmw,
    Tst,
    Andcc,
    Orcc,
    Tfr,
    Exg,
    PushPost,
    PullPost,
    PullByte(Reg),
    PullHi(Reg),
    PullLo(Reg),
    RtiCc,
    Br8,
    LbrLo,
    VecHi,
    VecLo,
}

/// A zero-cycle action between bus cycles.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Ctl {
    Exec,
    Jump,
    Lea,
    Inherent,
    /// `EA` = the postbyte's register plus the offset just read.
    EaOff(u8),
    /// `EA` = `PC` plus the offset just read.
    EaPcOff,
    SetE(bool),
    Mask(u8),
    Sync,
    Wait,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Uop {
    Fetch,
    Prog(Act),
    Rd(Ad, Act),
    Dummy,
    Wr(Ad, Val),
    Push(Stack, Val),
    Pull(Stack, Act),
    Ctl(Ctl),
}

const BUSY: u8 = 1;
const ACK: u8 = 2;
const LIC: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
struct Step {
    u: Uop,
    f: u8,
}

const fn s(u: Uop) -> Step {
    Step { u, f: 0 }
}

const CAP: usize = 40;

/// The levels of `ctrl` a later cycle decides on: `FIRQ`, `IRQ` and `HALT`
/// (`MC6809E-038`). `NMI` is an edge, latched as it comes.
const fn levels(ctrl: Ctrl) -> Ctrl {
    let mut lv = Ctrl::NONE;
    let mut i = 0;
    let decided = [Ctrl::FIRQ, Ctrl::IRQ, Ctrl::HALT];
    while i < decided.len() {
        if ctrl.contains(decided[i]) {
            lv = lv.with(decided[i]);
        }
        i += 1;
    }
    lv
}

/// The steps still to run, oldest first. Fixed capacity, no allocation.
/// Two queues are equal, and print, by the steps still to run.
#[derive(Clone, Copy)]
struct Queue {
    buf: [Step; CAP],
    head: usize,
    len: usize,
}

impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        self.live() == other.live()
    }
}

impl Eq for Queue {}

impl core::hash::Hash for Queue {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.live().hash(state);
    }
}

impl core::fmt::Debug for Queue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.live()).finish()
    }
}

impl Queue {
    /// The steps still to run.
    fn live(&self) -> &[Step] {
        &self.buf[self.head..self.len]
    }

    const EMPTY: Self = Self {
        buf: [s(Uop::Dummy); CAP],
        head: 0,
        len: 0,
    };

    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    fn pop(&mut self) -> Option<Step> {
        if self.head == self.len {
            self.clear();
            return None;
        }
        let x = self.buf[self.head];
        self.head += 1;
        Some(x)
    }

    fn push(&mut self, x: Step) {
        if self.head > 0 && self.len == CAP {
            self.buf.copy_within(self.head..self.len, 0);
            self.len -= self.head;
            self.head = 0;
        }
        self.buf[self.len] = x;
        self.len += 1;
    }

    /// Put `xs` before everything still queued.
    fn push_front(&mut self, xs: &[Step]) {
        let rest = self.len - self.head;
        self.buf.copy_within(self.head..self.len, xs.len());
        self.buf[..xs.len()].copy_from_slice(xs);
        self.head = 0;
        self.len = xs.len() + rest;
    }
}

/// What kind of cycle a request is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Use {
    /// A memory access.
    Bus,
    /// `$FFFF`, `R/W` High, `BS` Low (`MC6809E-004`).
    Dummy,
    /// `BA` High: the buses float.
    Float,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
struct Cyc {
    address: u16,
    write: Option<u8>,
    kind: Use,
    f: u8,
    fetch: bool,
}

impl Cyc {
    const fn dummy(f: u8) -> Self {
        Self {
            address: IDLE_ADDRESS,
            write: None,
            kind: Use::Dummy,
            f,
            fetch: false,
        }
    }

    /// `BA` High; `BS` too when `halt`.
    const fn float(halt: bool) -> Self {
        Self {
            address: IDLE_ADDRESS,
            write: None,
            kind: Use::Float,
            f: if halt { ACK } else { 0 },
            fetch: false,
        }
    }
}

/// What the part does next.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Next {
    Cyc(Cyc),
    /// An instruction has ended: the next cycle is a fetch, an interrupt's
    /// first cycle, or a halt.
    Boundary,
}

/// What the sequencer is doing outside an instruction.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Run {
    Normal,
    /// `HALT` acknowledged (`MC6809E-043`).
    Halted,
    /// `SYNC`'s acknowledge state (`MC6809E-042`).
    Sync,
    /// `CWAI`'s wait (`MC6809E-041`).
    Wait,
}

/// A hardware interrupt, highest priority first (`MC6809E-035`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Interrupt {
    Nmi,
    Firq,
    Irq,
}

/// The programmer-visible registers, as [`Mc6809e::registers`] reads them
/// and [`Mc6809e::at_boundary`] takes them, with the interrupt latches and
/// the input levels the part decides its next cycle on.
///
/// `Default` is every register 0, nothing pending, every input High and the
/// part running.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`Mc6809e::registers`] and sets the fields it wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// Accumulator A, the high byte of `D` (`MC6809E-010`).
    pub a: u8,
    /// Accumulator B, the low byte of `D`.
    pub b: u8,
    /// The direct page register (`MC6809E-012`).
    pub dp: u8,
    /// The condition code register, `E F H I N Z V C` from bit 7; the
    /// `FLAG_*` constants name its bits.
    pub cc: u8,
    /// Index register X.
    pub x: u16,
    /// Index register Y.
    pub y: u16,
    /// The user stack pointer.
    pub u: u16,
    /// The hardware stack pointer, pointing at the last byte pushed
    /// (`MC6809E-011`).
    pub s: u16,
    /// The program counter.
    pub pc: u16,
    /// Whether `NMI` is armed: an `NMI` edge is taken only once `S` has been
    /// loaded since the last reset (`MC6809E-035`).
    pub nmi_arming: Arming,
    /// An `NMI` edge latched and not yet taken, if any (`MC6809E-035`,
    /// `MC6809E-038`).
    pub nmi: Option<Nmi>,
    /// `NMI`'s level during the cycle the last tick completed, for its edge.
    pub nmi_n: Level,
    /// `FIRQ`'s level during the cycle the last tick completed, which the
    /// next boundary decides on (`MC6809E-038`).
    pub firq_n: Level,
    /// `IRQ`'s level during the cycle the last tick completed.
    pub irq_n: Level,
    /// `HALT`'s level during the cycle the last tick completed.
    pub halt_n: Level,
    /// The wait an instruction has stopped the part in, if any.
    pub wait: Option<Wait>,
}

/// An `NMI` edge latched and not yet taken, held in [`Registers::nmi`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Nmi {
    /// The next boundary decides on it.
    Pending,
    /// Came on the pins of the tick after this boundary, which
    /// [`Mc6809e::settle`] latched as that tick latches it: the part decides
    /// on it a cycle later, so that tick's decision does not see it yet. Set
    /// by [`Mc6809e::settle`] alone.
    Unseen,
}

/// Whether `NMI` is armed, held in [`Registers::nmi_arming`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Arming {
    /// `S` has not been loaded since the last reset: an `NMI` edge is not
    /// taken.
    Disarmed,
    /// The instruction just ended has loaded `S`, and `NMI` is armed only
    /// once the next tick has sampled it, as the part arms it in that cycle:
    /// an edge in it is not taken. Set by [`Mc6809e::settle`] alone.
    Deferred,
    /// An `NMI` edge is taken.
    Armed,
}

/// A wait an instruction stops the part in, held in [`Registers::wait`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Wait {
    /// `CWAI`'s wait, the entire state stacked, until an interrupt is taken
    /// (`MC6809E-041`).
    Cwai,
    /// `SYNC`'s acknowledge state, until an interrupt line is asserted
    /// (`MC6809E-042`).
    Sync,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            b: 0,
            dp: 0,
            cc: 0,
            x: 0,
            y: 0,
            u: 0,
            s: 0,
            pc: 0,
            nmi_arming: Arming::Disarmed,
            nmi: None,
            nmi_n: Level::High,
            firq_n: Level::High,
            irq_n: Level::High,
            halt_n: Level::High,
            wait: None,
        }
    }
}

/// What the cycle the last tick returned was, beyond its pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
struct Last {
    fetch: bool,
    float: bool,
    access: bool,
    write: bool,
}

/// The part.
///
/// Advance it with [`Mc6809e::tick`], one `E` cycle per call; read the
/// registers with [`Mc6809e::registers`] and start the part on chosen ones
/// with [`Mc6809e::at_boundary`].
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Mc6809e {
    a: u8,
    b: u8,
    dp: u8,
    cc: u8,
    x: u16,
    y: u16,
    u: u16,
    s: u16,
    pc: u16,

    last: Last,
    held: bool,
    run: Run,
    q: Queue,
    /// What the outstanding read's byte does.
    act: Act,
    page: u8,
    op: Decoded,
    ea: u16,
    tmp: u16,
    off: u16,
    /// The levels during the cycle the last tick completed, and the one
    /// before (`MC6809E-038`), of the inputs [`levels`] keeps.
    lv: Ctrl,
    lv_prev: Ctrl,
    nmi_low: bool,
    /// An `NMI` edge seen up to the cycle the last tick completed, and up to
    /// the one before.
    nmi: Option<Nmi>,
    nmi_seen: bool,
    nmi_arming: Arming,
    sync_exit: bool,
    wait_first: bool,
}

impl Default for Mc6809e {
    fn default() -> Self {
        Self::new()
    }
}

const NOP: Decoded = Decoded {
    kind: Kind::Inherent(Inherent::Nop),
    addressing: Addressing::Inherent,
};

impl Mc6809e {
    /// The part at power-up, held as if `RESET` were Low: the first tick
    /// without [`Ctrl::RESET`] begins the restart (`MC6809E-033`,
    /// `MC6809E-OPEN-07`).
    #[must_use]
    pub const fn new() -> Self {
        let mut cpu = Self::at_boundary(Registers {
            a: 0,
            b: 0,
            dp: 0,
            cc: F | I,
            x: 0,
            y: 0,
            u: 0,
            s: 0,
            pc: 0,
            nmi_arming: Arming::Disarmed,
            nmi: None,
            nmi_n: Level::High,
            firq_n: Level::High,
            irq_n: Level::High,
            halt_n: Level::High,
            wait: None,
        });
        cpu.held = true;
        cpu
    }

    /// The part at an instruction boundary with these registers, out of
    /// reset: `NMI` armed, arming and an edge pending as `regs` says, the levels of
    /// `NMI`, `FIRQ`, `IRQ` and `HALT` during the last cycle as `regs` gives
    /// them, and in the wait [`Registers::wait`] names. The next tick
    /// fetches, takes an interrupt or a halt, or goes on waiting. Nothing
    /// else is carried: no instruction is in flight. Until its first tick,
    /// the predicates of the last cycle ([`Mc6809e::is_reading`] and its
    /// siblings) read `false`, whatever the original's last cycle did. For a
    /// test bench; a board starts at [`Mc6809e::new`].
    #[must_use]
    pub const fn at_boundary(regs: Registers) -> Self {
        let mut lv = Ctrl::NONE;
        if regs.firq_n.is_low() {
            lv = lv.with(Ctrl::FIRQ);
        }
        if regs.irq_n.is_low() {
            lv = lv.with(Ctrl::IRQ);
        }
        if regs.halt_n.is_low() {
            lv = lv.with(Ctrl::HALT);
        }
        Self {
            a: regs.a,
            b: regs.b,
            dp: regs.dp,
            cc: regs.cc,
            x: regs.x,
            y: regs.y,
            u: regs.u,
            s: regs.s,
            pc: regs.pc,
            last: Last {
                fetch: false,
                float: false,
                access: false,
                write: false,
            },
            held: false,
            run: match regs.wait {
                None => Run::Normal,
                Some(Wait::Cwai) => Run::Wait,
                Some(Wait::Sync) => Run::Sync,
            },
            q: Queue::EMPTY,
            act: Act::None,
            page: 0,
            op: NOP,
            ea: 0,
            tmp: 0,
            off: 0,
            lv,
            lv_prev: lv,
            nmi_low: regs.nmi_n.is_low(),
            nmi: regs.nmi,
            nmi_seen: false,
            nmi_arming: regs.nmi_arming,
            sync_exit: false,
            wait_first: false,
        }
    }

    /// The programmer-visible registers as they stand. A read's byte reaches
    /// them with the next tick, so at an instruction boundary
    /// ([`Mc6809e::is_at_instruction_boundary`]) they lack the byte of an
    /// instruction whose last cycle is a read until that tick or
    /// [`Mc6809e::settle`]; after it they are the architectural state, and
    /// `Mc6809e::at_boundary(cpu.registers())` round-trips and the rebuilt
    /// part runs on as this one does. They carry the `NMI` latch, the input
    /// levels the next boundary decides on and the wait an instruction has
    /// stopped the part in; a halt by the `HALT` pin is not carried, and no
    /// instruction boundary stands while it holds the part between
    /// instructions. In `CWAI`'s wait and `SYNC`'s acknowledge state `HALT`
    /// does not end the boundary, and a part rebuilt there runs on alike.
    #[must_use]
    pub const fn registers(&self) -> Registers {
        Registers {
            a: self.a,
            b: self.b,
            dp: self.dp,
            cc: self.cc,
            x: self.x,
            y: self.y,
            u: self.u,
            s: self.s,
            pc: self.pc,
            nmi_arming: self.nmi_arming,
            nmi: self.nmi,
            nmi_n: Level::from_bool(!self.nmi_low),
            firq_n: Level::from_bool(!self.lv.contains(Ctrl::FIRQ)),
            irq_n: Level::from_bool(!self.lv.contains(Ctrl::IRQ)),
            halt_n: Level::from_bool(!self.lv.contains(Ctrl::HALT)),
            wait: match self.run {
                Run::Wait => Some(Wait::Cwai),
                Run::Sync => Some(Wait::Sync),
                Run::Normal | Run::Halted => None,
            },
        }
    }

    /// `D`, `A` high (`MC6809E-010`).
    const fn d(&self) -> u16 {
        (self.a as u16) << 8 | self.b as u16
    }

    const fn set_d(&mut self, d: u16) {
        self.a = (d >> 8) as u8;
        self.b = d as u8;
    }

    /// The part has given the bus up for the cycle the last tick returned:
    /// address, data and `R/W` are high-impedance, because `BA` is High (a
    /// halt by `HALT`, `SYNC`'s acknowledge) or `TSC` is (`MC6809E-003`,
    /// `MC6809E-009`, `MC6809E-043`).
    #[must_use]
    pub const fn is_released(&self) -> bool {
        self.last.float
    }

    /// An instruction has stopped the part until an interrupt or a reset:
    /// `CWAI`'s wait (`MC6809E-041`) or `SYNC`'s acknowledge
    /// (`MC6809E-042`). `BA` and `BS` tell the two apart: `SYNC` drives `BA`
    /// High and `BS` Low, `CWAI` neither. Only where
    /// [`Mc6809e::is_at_instruction_boundary`] holds: not in `CWAI`'s first cycle
    /// of the wait, nor in the cycle `SYNC` leaves its acknowledge in once an
    /// interrupt line is seen, nor while `RESET` holds the part.
    #[must_use]
    pub const fn is_halted(&self) -> bool {
        !self.held
            && match self.run {
                Run::Sync => !self.sync_exit,
                Run::Wait => !self.wait_first,
                _ => false,
            }
    }

    /// The cycle the last tick returned reads an instruction's first byte;
    /// false under reset, as [`Mc6809e::is_accessing`] is.
    #[must_use]
    pub const fn is_fetching(&self) -> bool {
        self.last.fetch && !self.held
    }

    /// The cycle the last tick returned is a memory access by the part:
    /// neither a dummy access (`MC6809E-004`) nor one with the bus released,
    /// and not under reset, whatever `R/W` shows. Otherwise it is the previous
    /// cycle's `AVMA` (`MC6809E-006`).
    #[must_use]
    pub const fn is_accessing(&self) -> bool {
        self.last.access && !self.held
    }

    /// The cycle the last tick returned is a memory read by the part:
    /// [`Mc6809e::is_accessing`] with `R/W` High.
    #[must_use]
    pub const fn is_reading(&self) -> bool {
        self.is_accessing() && !self.last.write
    }

    /// The cycle the last tick returned is a write: `R/W` Low, and not under
    /// reset.
    #[must_use]
    pub const fn is_writing(&self) -> bool {
        self.last.write && !self.held
    }

    /// `RESET` holds the part, or has not yet been released since power-up
    /// (`MC6809E-033`).
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.held
    }

    /// The next tick begins an instruction, an interrupt's first cycle or a
    /// halt: nothing is in flight. True too in `CWAI`'s wait after its first
    /// cycle and in `SYNC`'s acknowledge state until an interrupt line is
    /// seen, where [`Registers::wait`] carries it, whatever `HALT`
    /// shows there. False while `RESET` holds the part and while `HALT`
    /// holds it between instructions.
    #[must_use]
    pub fn is_at_instruction_boundary(&self) -> bool {
        match self.run {
            _ if self.held => return false,
            Run::Halted => return false,
            Run::Sync => return !self.sync_exit,
            Run::Wait => return !self.wait_first,
            Run::Normal => {}
        }
        let mut ahead = self.clone();
        ahead.lv_prev = ahead.lv;
        ahead.nmi_seen = ahead.nmi.is_some();
        let a = core::mem::replace(&mut ahead.act, Act::None);
        ahead.latch(a, 0);
        matches!(ahead.produce(), Next::Boundary)
    }

    /// At an instruction boundary, take the outstanding read's byte from
    /// `pins.d` and finish the instruction that has just ended without
    /// starting the next, so [`Mc6809e::registers`] reads the boundary.
    /// Idempotent; ticking on afterwards is correct, and the next tick
    /// ignores `d`. An `NMI` edge on `pins`, while armed, is latched into
    /// [`Registers::nmi`] as the next tick latches it, and
    /// [`Nmi::Unseen`] keeps that tick from deciding on it
    /// (`MC6809E-038`). A load of `S` that arms `NMI` arms it only after the
    /// next tick samples `NMI`, as the part's own cycle does
    /// ([`Arming::Deferred`]).
    ///
    /// Returns whether the part now stands at a boundary that
    /// [`Mc6809e::at_boundary`] rebuilds from [`Mc6809e::registers`]:
    /// `false`, changing nothing, elsewhere, and with [`Ctrl::RESET`] on
    /// `pins`, under which the next tick drops the byte and the instruction's
    /// end (`MC6809E-033`). A part is rebuilt at a later boundary instead.
    #[must_use]
    pub fn settle(&mut self, pins: Pins) -> bool {
        if pins.ctrl.contains(Ctrl::RESET) || !self.is_at_instruction_boundary() {
            return false;
        }
        let nmi = pins.ctrl.contains(Ctrl::NMI);
        if nmi && !self.nmi_low && self.nmi_arming == Arming::Armed && self.nmi.is_none() {
            self.nmi = Some(Nmi::Unseen);
        }
        self.nmi_low = nmi;
        let arming = self.nmi_arming;
        let a = core::mem::replace(&mut self.act, Act::None);
        self.latch(a, pins.d.unwrap_or(0));
        while let Some(Step { u: Uop::Ctl(c), .. }) = self.q.pop() {
            self.ctl(c);
        }
        if self.nmi_arming == Arming::Armed && arming != Arming::Armed {
            self.nmi_arming = Arming::Deferred;
        }
        true
    }

    /// One `E` cycle.
    #[must_use]
    pub fn tick(&mut self, pins: Pins) -> Pins {
        let ctrl = pins.ctrl;
        let tsc = ctrl.contains(Ctrl::TSC);
        if ctrl.contains(Ctrl::RESET) {
            self.hold(ctrl);
            let c = Cyc {
                address: vector::RESET,
                write: None,
                kind: Use::Bus,
                f: 0,
                fetch: false,
            };
            return self.out(c, true, false, tsc);
        }
        self.sample(ctrl);
        if self.nmi_arming == Arming::Deferred {
            self.nmi_arming = Arming::Armed;
        }
        if self.held {
            self.held = false;
            self.restart();
        } else {
            let a = core::mem::replace(&mut self.act, Act::None);
            self.latch(a, pins.d.unwrap_or(0));
        }
        let c = self.next_cycle();

        let mut ahead = self.clone();
        ahead.lv_prev = ahead.lv;
        ahead.nmi_seen = ahead.nmi.is_some();
        let a = core::mem::replace(&mut ahead.act, Act::None);
        ahead.latch(a, 0);
        let next = ahead.produce();
        let avma = c.kind != Use::Float
            && match next {
                Next::Boundary => true,
                Next::Cyc(n) => n.kind != Use::Dummy,
            };
        let lic = c.f & LIC != 0
            || c.kind == Use::Float
            || match next {
                Next::Boundary => true,
                Next::Cyc(n) => n.kind == Use::Float,
            };
        self.out(c, avma, lic, tsc)
    }

    fn out(&mut self, c: Cyc, avma: bool, lic: bool, tsc: bool) -> Pins {
        let mut ctrl = Ctrl::NONE;
        let float = c.kind == Use::Float || tsc;
        self.last = Last {
            fetch: c.fetch,
            float,
            access: !float && c.kind == Use::Bus,
            write: !float && c.write.is_some(),
        };
        if c.kind == Use::Float {
            ctrl |= Ctrl::BA;
        }
        if c.f & ACK != 0 {
            ctrl |= Ctrl::BS;
        }
        if c.f & BUSY != 0 {
            ctrl |= Ctrl::BUSY;
        }
        if avma {
            ctrl |= Ctrl::AVMA;
        }
        if lic {
            ctrl |= Ctrl::LIC;
        }
        if float {
            return Pins {
                address: None,
                d: None,
                ctrl,
            };
        }
        if c.write.is_some() {
            ctrl |= Ctrl::WRITE;
        }
        Pins {
            address: Some(c.address),
            d: c.write,
            ctrl,
        }
    }

    /// `MC6809E-033`: `RESET` Low. `DP` cleared, `F` and `I` set, `NMI`
    /// disarmed and forgotten; the part is inactive until it rises.
    fn hold(&mut self, ctrl: Ctrl) {
        self.held = true;
        self.run = Run::Normal;
        self.q.clear();
        self.act = Act::None;
        self.dp = 0;
        self.cc |= F | I;
        self.nmi_arming = Arming::Disarmed;
        self.nmi = None;
        self.nmi_seen = false;
        self.nmi_low = ctrl.contains(Ctrl::NMI);
        self.sync_exit = false;
        self.wait_first = false;
        self.lv = levels(ctrl);
        self.lv_prev = self.lv;
    }

    /// Figure 6: two more idle reads of `$FFFE`, the vector with `BS` High,
    /// a dummy access, then the fetch.
    fn restart(&mut self) {
        self.page = 0;
        for _ in 0..2 {
            self.q.push(s(Uop::Rd(Ad::Abs(vector::RESET), Act::None)));
        }
        self.read_vector(vector::RESET, 0);
    }

    /// `MC6809E-038`: the levels of each cycle, one cycle late when
    /// decided on; an `NMI` edge latched while armed (`MC6809E-OPEN-09`).
    fn sample(&mut self, ctrl: Ctrl) {
        self.lv_prev = self.lv;
        self.lv = levels(ctrl);
        self.nmi_seen = self.nmi == Some(Nmi::Pending);
        if self.nmi.is_some() {
            self.nmi = Some(Nmi::Pending);
        }
        let nmi = ctrl.contains(Ctrl::NMI);
        if nmi && !self.nmi_low && self.nmi_arming == Arming::Armed && self.nmi.is_none() {
            self.nmi = Some(Nmi::Pending);
        }
        self.nmi_low = nmi;
    }

    fn next_cycle(&mut self) -> Cyc {
        match self.produce() {
            Next::Cyc(c) => c,
            Next::Boundary => self.boundary(),
        }
    }

    /// The next cycle of whatever is running, or [`Next::Boundary`].
    fn produce(&mut self) -> Next {
        loop {
            match self.run {
                Run::Halted => {
                    if self.lv_prev.contains(Ctrl::HALT) {
                        return Next::Cyc(Cyc::float(true));
                    }
                    self.run = Run::Normal;
                    return Next::Cyc(Cyc::dummy(0));
                }
                Run::Sync => {
                    if self.sync_exit {
                        self.sync_exit = false;
                        self.run = Run::Normal;
                        return Next::Cyc(Cyc::dummy(0));
                    }
                    if self.nmi_seen
                        || self.lv_prev.contains(Ctrl::FIRQ)
                        || self.lv_prev.contains(Ctrl::IRQ)
                    {
                        self.sync_exit = true;
                    }
                    return Next::Cyc(Cyc::float(self.lv_prev.contains(Ctrl::HALT)));
                }
                Run::Wait => {
                    if self.lv_prev.contains(Ctrl::HALT) {
                        return Next::Cyc(Cyc::float(true));
                    }
                    if !self.wait_first
                        && let Some(k) = self.pending()
                    {
                        self.run = Run::Normal;
                        self.vector_for(k, 0);
                        continue;
                    }
                    self.wait_first = false;
                    return Next::Cyc(Cyc::dummy(0));
                }
                Run::Normal => match self.q.pop() {
                    None => return Next::Boundary,
                    Some(Step { u: Uop::Ctl(c), .. }) => self.ctl(c),
                    Some(st) => return Next::Cyc(self.emit(st)),
                },
            }
        }
    }

    /// Figure 14 at the top of the loop: `HALT`, then `NMI`, `FIRQ`, `IRQ`,
    /// then the next opcode.
    fn boundary(&mut self) -> Cyc {
        self.page = 0;
        if self.lv_prev.contains(Ctrl::HALT) {
            self.run = Run::Halted;
            return Cyc::float(true);
        }
        match self.pending() {
            Some(k) => self.interrupt(k),
            None => self.q.push(s(Uop::Fetch)),
        }
        match self.produce() {
            Next::Cyc(c) => c,
            Next::Boundary => Cyc::dummy(0),
        }
    }

    /// The interrupt Figure 14 would take now, if any (`MC6809E-035` to
    /// `MC6809E-037`).
    fn pending(&self) -> Option<Interrupt> {
        if self.nmi_arming == Arming::Armed && self.nmi_seen {
            Some(Interrupt::Nmi)
        } else if self.lv_prev.contains(Ctrl::FIRQ) && self.cc & F == 0 {
            Some(Interrupt::Firq)
        } else if self.lv_prev.contains(Ctrl::IRQ) && self.cc & I == 0 {
            Some(Interrupt::Irq)
        } else {
            None
        }
    }

    /// Figures 8 and 9: two reads at `PC`, a dummy access, the stacking, a
    /// dummy access, the vector with `BS` High and a dummy access, `LIC`
    /// High throughout.
    fn interrupt(&mut self, k: Interrupt) {
        let full = k != Interrupt::Firq;
        self.ctl(Ctl::SetE(full));
        let pc = self.pc;
        self.q.push(Step {
            u: Uop::Rd(Ad::Abs(pc), Act::None),
            f: LIC,
        });
        self.q.push(Step {
            u: Uop::Rd(Ad::Abs(pc), Act::None),
            f: LIC,
        });
        self.q.push(Step {
            u: Uop::Dummy,
            f: LIC,
        });
        self.push_regs(Stack::S, if full { 0xff } else { 0x81 }, LIC);
        self.q.push(Step {
            u: Uop::Dummy,
            f: LIC,
        });
        self.vector_for(k, LIC);
    }

    /// Set the masks an interrupt sets and queue its vector
    /// (`MC6809E-035` to `MC6809E-037`).
    fn vector_for(&mut self, k: Interrupt, f: u8) {
        let (v, mask) = match k {
            Interrupt::Nmi => {
                self.nmi = None;
                self.nmi_seen = false;
                (vector::NMI, F | I)
            }
            Interrupt::Firq => (vector::FIRQ, F | I),
            Interrupt::Irq => (vector::IRQ, I),
        };
        self.q.push(s(Uop::Ctl(Ctl::Mask(mask))));
        self.read_vector(v, f);
    }

    /// The vector's two bytes with `BS` High, `BUSY` on the first
    /// (`MC6809E-005`, `MC6809E-008`), then a dummy access.
    fn read_vector(&mut self, v: u16, f: u8) {
        self.q.push(Step {
            u: Uop::Rd(Ad::Abs(v), Act::VecHi),
            f: f | ACK | BUSY,
        });
        self.q.push(Step {
            u: Uop::Rd(Ad::Abs(v.wrapping_add(1)), Act::VecLo),
            f: f | ACK,
        });
        self.q.push(Step { u: Uop::Dummy, f });
    }

    /// Queue the pushes `post` names in Figure 17's order, `PC` first and
    /// `CC` last, each sixteen-bit register low byte first (`MC6809E-022`).
    fn push_regs(&mut self, st: Stack, post: u8, f: u8) {
        let other = match st {
            Stack::S => Reg::U,
            Stack::U => Reg::S,
        };
        for (bit, r) in [
            (0x80, Reg::Pc),
            (0x40, other),
            (0x20, Reg::Y),
            (0x10, Reg::X),
        ] {
            if post & bit != 0 {
                self.q.push(Step {
                    u: Uop::Push(st, Val::Lo(r)),
                    f,
                });
                self.q.push(Step {
                    u: Uop::Push(st, Val::Hi(r)),
                    f,
                });
            }
        }
        for (bit, r) in [
            (0x08, Reg::Dp),
            (0x04, Reg::B),
            (0x02, Reg::A),
            (0x01, Reg::Cc),
        ] {
            if post & bit != 0 {
                self.q.push(Step {
                    u: Uop::Push(st, Val::Byte(r)),
                    f,
                });
            }
        }
    }

    /// Queue the pulls `post` names in Figure 17's order, `CC` first and
    /// `PC` last, each sixteen-bit register high byte first.
    fn pull_regs(&mut self, st: Stack, post: u8) {
        let other = match st {
            Stack::S => Reg::U,
            Stack::U => Reg::S,
        };
        for (bit, r) in [
            (0x01, Reg::Cc),
            (0x02, Reg::A),
            (0x04, Reg::B),
            (0x08, Reg::Dp),
        ] {
            if post & bit != 0 {
                self.q.push(s(Uop::Pull(st, Act::PullByte(r))));
            }
        }
        for (bit, r) in [
            (0x10, Reg::X),
            (0x20, Reg::Y),
            (0x40, other),
            (0x80, Reg::Pc),
        ] {
            if post & bit != 0 {
                self.q.push(s(Uop::Pull(st, Act::PullHi(r))));
                self.q.push(s(Uop::Pull(st, Act::PullLo(r))));
            }
        }
    }

    fn sp(&mut self, st: Stack) -> &mut u16 {
        match st {
            Stack::S => &mut self.s,
            Stack::U => &mut self.u,
        }
    }

    fn address(&self, ad: Ad) -> u16 {
        match ad {
            Ad::Pc(k) => self.pc.wrapping_add(u16::from(k)),
            Ad::Ea => self.ea,
            Ad::Ea1 => self.ea.wrapping_add(1),
            Ad::Abs(a) => a,
            Ad::Top(Stack::S) => self.s,
            Ad::Top(Stack::U) => self.u,
        }
    }

    fn read16(&self, r: Reg) -> u16 {
        match r {
            Reg::A | Reg::B | Reg::Dp | Reg::Cc => u16::from(self.read8(r)),
            Reg::X => self.x,
            Reg::Y => self.y,
            Reg::U => self.u,
            Reg::S => self.s,
            Reg::Pc => self.pc,
        }
    }

    fn read8(&self, r: Reg) -> u8 {
        match r {
            Reg::A => self.a,
            Reg::B => self.b,
            Reg::Dp => self.dp,
            Reg::Cc => self.cc,
            Reg::X | Reg::Y | Reg::U | Reg::S | Reg::Pc => self.read16(r) as u8,
        }
    }

    fn write8(&mut self, r: Reg, v: u8) {
        match r {
            Reg::A => self.a = v,
            Reg::B => self.b = v,
            Reg::Dp => self.dp = v,
            Reg::Cc => self.cc = v,
            Reg::X | Reg::Y | Reg::U | Reg::S | Reg::Pc => {}
        }
    }

    fn write16(&mut self, r: Reg, v: u16) {
        match r {
            Reg::X => self.x = v,
            Reg::Y => self.y = v,
            Reg::U => self.u = v,
            Reg::S => {
                self.s = v;
                self.nmi_arming = Arming::Armed;
            }
            Reg::Pc => self.pc = v,
            Reg::A | Reg::B | Reg::Dp | Reg::Cc => self.write8(r, v as u8),
        }
    }

    /// An index register's automatic increment or decrement: a write to
    /// `S` that is not a load, and does not arm `NMI` (`MC6809E-OPEN-09`).
    fn set_pointer(&mut self, r: Pointer, v: u16) {
        match r {
            Pointer::X => self.x = v,
            Pointer::Y => self.y = v,
            Pointer::U => self.u = v,
            Pointer::S => self.s = v,
        }
    }

    fn reg16(r: Reg16) -> Option<Reg> {
        match r {
            Reg16::D => None,
            Reg16::X => Some(Reg::X),
            Reg16::Y => Some(Reg::Y),
            Reg16::U => Some(Reg::U),
            Reg16::S => Some(Reg::S),
        }
    }

    fn get16(&self, r: Reg16) -> u16 {
        match Self::reg16(r) {
            None => self.d(),
            Some(x) => self.read16(x),
        }
    }

    fn set16(&mut self, r: Reg16, v: u16) {
        match Self::reg16(r) {
            None => self.set_d(v),
            Some(x) => self.write16(x, v),
        }
    }

    fn acc(&self, a: Accumulator) -> u8 {
        match a {
            Accumulator::A => self.a,
            Accumulator::B => self.b,
        }
    }

    fn val(&mut self, v: Val) -> u8 {
        match v {
            Val::Hi(r) => (self.read16(r) >> 8) as u8,
            Val::Lo(r) => self.read16(r) as u8,
            Val::Byte(r) => self.read8(r),
            Val::Tmp => self.tmp as u8,
            Val::Store8(a) => {
                let v = self.acc(a);
                self.cc = alu::logic(self.cc, v);
                v
            }
            Val::Store16Hi(r) => (self.get16(r) >> 8) as u8,
            Val::Store16Lo(r) => {
                let v = self.get16(r);
                self.cc = alu::logic16(self.cc, v);
                v as u8
            }
        }
    }

    /// The cycle a step asks for.
    fn emit(&mut self, st: Step) -> Cyc {
        let mut c = Cyc {
            address: 0,
            write: None,
            kind: Use::Bus,
            f: st.f,
            fetch: false,
        };
        match st.u {
            Uop::Fetch => {
                c.address = self.pc;
                c.fetch = true;
                self.pc = self.pc.wrapping_add(1);
                self.act = Act::Opcode;
            }
            Uop::Prog(a) => {
                c.address = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.act = a;
            }
            Uop::Rd(ad, a) => {
                c.address = self.address(ad);
                self.act = a;
            }
            Uop::Dummy => return Cyc::dummy(st.f),
            Uop::Wr(ad, v) => {
                c.address = self.address(ad);
                c.write = Some(self.val(v));
            }
            Uop::Push(stk, v) => {
                let p = self.sp(stk);
                *p = p.wrapping_sub(1);
                c.address = *p;
                c.write = Some(self.val(v));
            }
            Uop::Pull(stk, a) => {
                let p = self.sp(stk);
                c.address = *p;
                *p = p.wrapping_add(1);
                self.act = a;
            }
            Uop::Ctl(_) => return Cyc::dummy(0),
        }
        c
    }

    fn push(&mut self, u: Uop) {
        self.q.push(s(u));
    }

    fn busy(&mut self, u: Uop) {
        self.q.push(Step { u, f: BUSY });
    }

    /// A read's byte has arrived.
    #[allow(clippy::too_many_lines)]
    fn latch(&mut self, a: Act, d: u8) {
        match a {
            Act::None => {}
            Act::Opcode => {
                if d == 0x10 || d == 0x11 {
                    self.page = d;
                    self.push(Uop::Prog(Act::Page));
                } else {
                    self.setup(Page::One, d);
                }
            }
            Act::Page => {
                if d == 0x10 || d == 0x11 {
                    self.push(Uop::Prog(Act::Page));
                } else {
                    let page = if self.page == 0x10 {
                        Page::Two
                    } else {
                        Page::Three
                    };
                    self.setup(page, d);
                }
            }
            Act::Post => self.index(d),
            Act::DirLo => self.ea = u16::from(self.dp) << 8 | u16::from(d),
            Act::EaHi => self.ea = u16::from(d) << 8,
            Act::EaLo => self.ea |= u16::from(d),
            Act::Off8 => self.off = d as i8 as u16,
            Act::OffHi => self.off = u16::from(d) << 8,
            Act::OffLo => self.off |= u16::from(d),
            Act::IndHi | Act::TmpHi | Act::VecHi => self.tmp = u16::from(d) << 8,
            Act::IndLo => self.ea = self.tmp | u16::from(d),
            Act::VecLo => self.pc = self.tmp | u16::from(d),
            Act::Alu8 => {
                if let Kind::Alu8(op, acc) = self.op.kind {
                    let (r, cc) = alu::alu8(self.cc, op, self.acc(acc), d);
                    self.cc = cc;
                    if let Some(r) = r {
                        match acc {
                            Accumulator::A => self.a = r,
                            Accumulator::B => self.b = r,
                        }
                    }
                }
            }
            Act::Alu16 => {
                let m = self.tmp | u16::from(d);
                if let Kind::Alu16(op) = self.op.kind {
                    let r = op.register();
                    let v = self.get16(r);
                    let (res, cc) = match op {
                        Alu16::AddD => alu::add16(self.cc, v, m),
                        Alu16::SubD
                        | Alu16::CmpD
                        | Alu16::CmpX
                        | Alu16::CmpY
                        | Alu16::CmpU
                        | Alu16::CmpS => alu::sub16(self.cc, v, m),
                    };
                    self.cc = cc;
                    if matches!(op, Alu16::SubD | Alu16::AddD) {
                        self.set16(r, res);
                    }
                }
            }
            Act::Ld16 => {
                let m = self.tmp | u16::from(d);
                if let Kind::Ld16(r) = self.op.kind {
                    self.cc = alu::logic16(self.cc, m);
                    self.set16(r, m);
                }
            }
            Act::Rmw | Act::Tst => {
                if let Kind::Unary(u, _) = self.op.kind {
                    let (r, cc) = alu::unary(self.cc, u, d);
                    self.cc = cc;
                    self.tmp = u16::from(r.unwrap_or(d));
                }
            }
            Act::Andcc => self.cc &= d,
            Act::Orcc => self.cc |= d,
            Act::Tfr => {
                let v = self.xfer_read(d >> 4);
                self.xfer_write(d & 0xf, v);
            }
            Act::Exg => {
                let p = self.xfer_read(d >> 4);
                let q = self.xfer_read(d & 0xf);
                self.xfer_write(d >> 4, q);
                self.xfer_write(d & 0xf, p);
            }
            Act::PushPost => {
                if let Kind::Push(st) = self.op.kind {
                    self.push(Uop::Dummy);
                    self.push(Uop::Dummy);
                    self.push(Uop::Rd(Ad::Top(st), Act::None));
                    self.push_regs(st, d, 0);
                }
            }
            Act::PullPost => {
                if let Kind::Pull(st) = self.op.kind {
                    self.push(Uop::Dummy);
                    self.push(Uop::Dummy);
                    self.pull_regs(st, d);
                    self.push(Uop::Rd(Ad::Top(st), Act::None));
                }
            }
            Act::PullByte(r) => self.write8(r, d),
            Act::PullHi(r) => {
                let v = self.read16(r) & 0x00ff | u16::from(d) << 8;
                self.write16(r, v);
            }
            Act::PullLo(r) => {
                let v = self.read16(r) & 0xff00 | u16::from(d);
                self.write16(r, v);
            }
            Act::RtiCc => {
                self.cc = d;
                let post = if d & E != 0 { 0xfe } else { 0x80 };
                self.pull_regs(Stack::S, post);
                self.push(Uop::Rd(Ad::Top(Stack::S), Act::None));
            }
            Act::Br8 => {
                self.ea = self.pc.wrapping_add(d as i8 as u16);
                if let Kind::Branch(c) = self.op.kind
                    && alu::taken(self.cc, c)
                {
                    self.pc = self.ea;
                }
            }
            Act::LbrLo => {
                self.ea = self.pc.wrapping_add(self.tmp | u16::from(d));
                if let Kind::Branch(c) = self.op.kind
                    && alu::taken(self.cc, c)
                {
                    self.pc = self.ea;
                }
            }
        }
    }

    /// `MC6809E-023`: a transfer register code's value. An eight-bit
    /// register reads with `$FF` above it and an undefined code as `$FFFF`
    /// (`MC6809E-OPEN-03`).
    fn xfer_read(&self, code: u8) -> u16 {
        match code {
            0 => self.d(),
            1 => self.x,
            2 => self.y,
            3 => self.u,
            4 => self.s,
            5 => self.pc,
            8 => 0xff00 | u16::from(self.a),
            9 => 0xff00 | u16::from(self.b),
            0xa => 0xff00 | u16::from(self.cc),
            0xb => 0xff00 | u16::from(self.dp),
            _ => 0xffff,
        }
    }

    /// Write a transfer register code; an eight-bit register takes the low
    /// byte, and an undefined code takes nothing (`MC6809E-OPEN-03`).
    fn xfer_write(&mut self, code: u8, v: u16) {
        match code {
            0 => self.set_d(v),
            1 => self.x = v,
            2 => self.y = v,
            3 => self.u = v,
            4 => self.write16(Reg::S, v),
            5 => self.pc = v,
            8 => self.a = v as u8,
            9 => self.b = v as u8,
            0xa => self.cc = v as u8,
            0xb => self.dp = v as u8,
            _ => {}
        }
    }

    /// The opcode is in: queue the rest of the instruction (Figure 17).
    fn setup(&mut self, page: Page, code: u8) {
        let op = decode(page, code);
        if matches!(op.kind, Kind::Undefined) {
            self.op = NOP;
            self.push(Uop::Rd(Ad::Pc(0), Act::None));
            return;
        }
        self.op = op;
        match op.addressing {
            Addressing::Inherent => self.inherent(op.kind),
            Addressing::Immediate => self.immediate(op.kind),
            Addressing::Direct => {
                self.push(Uop::Prog(Act::DirLo));
                self.push(Uop::Dummy);
                self.push(Uop::Ctl(Ctl::Exec));
            }
            Addressing::Extended => {
                self.push(Uop::Prog(Act::EaHi));
                self.push(Uop::Prog(Act::EaLo));
                self.push(Uop::Dummy);
                self.push(Uop::Ctl(Ctl::Exec));
            }
            Addressing::Indexed => {
                self.push(Uop::Prog(Act::Post));
                self.push(Uop::Ctl(Ctl::Exec));
            }
            Addressing::Relative => {
                self.push(Uop::Prog(Act::Br8));
                self.push(Uop::Dummy);
                if op.kind == Kind::Bsr {
                    self.call();
                }
            }
            Addressing::LongRelative => {
                self.push(Uop::Prog(Act::TmpHi));
                self.push(Uop::Prog(Act::LbrLo));
                self.push(Uop::Dummy);
                match op.kind {
                    Kind::Bsr => {
                        self.push(Uop::Dummy);
                        self.call();
                    }
                    Kind::Branch(c) if alu::taken(self.cc, c) => self.push(Uop::Dummy),
                    _ => {}
                }
            }
        }
    }

    /// `BSR` and `LBSR` after the branch: the subroutine's address read and
    /// ignored, a dummy access, the return address pushed low byte first,
    /// then the jump.
    fn call(&mut self) {
        self.push(Uop::Rd(Ad::Ea, Act::None));
        self.push(Uop::Dummy);
        self.push(Uop::Push(Stack::S, Val::Lo(Reg::Pc)));
        self.push(Uop::Push(Stack::S, Val::Hi(Reg::Pc)));
        self.push(Uop::Ctl(Ctl::Jump));
    }

    /// Figure 17 sheet 2.
    fn inherent(&mut self, k: Kind) {
        match k {
            Kind::Inherent(Inherent::Rts) => {
                self.push(Uop::Rd(Ad::Pc(0), Act::None));
                self.push(Uop::Pull(Stack::S, Act::PullHi(Reg::Pc)));
                self.push(Uop::Pull(Stack::S, Act::PullLo(Reg::Pc)));
                self.push(Uop::Dummy);
            }
            Kind::Inherent(Inherent::Rti) => {
                self.push(Uop::Rd(Ad::Pc(0), Act::None));
                self.push(Uop::Pull(Stack::S, Act::RtiCc));
            }
            Kind::Inherent(Inherent::Sync) => {
                self.push(Uop::Rd(Ad::Pc(0), Act::None));
                self.push(Uop::Ctl(Ctl::Sync));
            }
            Kind::Inherent(Inherent::Swi(n)) => {
                self.push(Uop::Rd(Ad::Pc(0), Act::None));
                self.push(Uop::Dummy);
                self.push(Uop::Ctl(Ctl::SetE(true)));
                self.push_regs(Stack::S, 0xff, 0);
                self.push(Uop::Dummy);
                let (v, mask) = match n {
                    Swi::Swi => (vector::SWI, F | I),
                    Swi::Swi2 => (vector::SWI2, 0),
                    Swi::Swi3 => (vector::SWI3, 0),
                };
                self.push(Uop::Ctl(Ctl::Mask(mask)));
                self.read_vector(v, 0);
            }
            _ => {
                self.push(Uop::Ctl(Ctl::Inherent));
                self.push(Uop::Rd(Ad::Pc(0), Act::None));
                let dummies = match k {
                    Kind::Inherent(Inherent::Abx) => 1,
                    Kind::Inherent(Inherent::Mul) => 9,
                    _ => 0,
                };
                for _ in 0..dummies {
                    self.push(Uop::Dummy);
                }
            }
        }
    }

    /// Immediate forms: the operand bytes follow the opcode.
    fn immediate(&mut self, k: Kind) {
        match k {
            Kind::Alu8(..) => self.push(Uop::Prog(Act::Alu8)),
            Kind::Alu16(..) => {
                self.busy(Uop::Prog(Act::TmpHi));
                self.push(Uop::Prog(Act::Alu16));
                self.push(Uop::Dummy);
            }
            Kind::Ld16(_) => {
                self.busy(Uop::Prog(Act::TmpHi));
                self.push(Uop::Prog(Act::Ld16));
            }
            Kind::Orcc | Kind::Andcc => {
                let a = if k == Kind::Orcc {
                    Act::Orcc
                } else {
                    Act::Andcc
                };
                self.push(Uop::Prog(a));
                self.push(Uop::Rd(Ad::Pc(0), Act::None));
            }
            Kind::Cwai => {
                self.push(Uop::Prog(Act::Andcc));
                self.push(Uop::Rd(Ad::Pc(0), Act::None));
                self.push(Uop::Dummy);
                self.push(Uop::Ctl(Ctl::SetE(true)));
                self.push_regs(Stack::S, 0xff, 0);
                self.push(Uop::Ctl(Ctl::Wait));
            }
            Kind::Tfr | Kind::Exg => {
                let (a, n) = if k == Kind::Tfr {
                    (Act::Tfr, 4)
                } else {
                    (Act::Exg, 6)
                };
                self.push(Uop::Prog(a));
                for _ in 0..n {
                    self.push(Uop::Dummy);
                }
            }
            Kind::Push(_) => self.push(Uop::Prog(Act::PushPost)),
            Kind::Pull(_) => self.push(Uop::Prog(Act::PullPost)),
            _ => {}
        }
    }

    /// Figure 17 sheet 4: the cycles an indexed postbyte names, put ahead of
    /// the execution (`MC6809E-019`, `MC6809E-021`).
    fn index(&mut self, p: u8) {
        let rr = (p >> 5) & 3;
        let r = match rr {
            0 => Pointer::X,
            1 => Pointer::Y,
            2 => Pointer::U,
            _ => Pointer::S,
        };
        let base = self.read16(r.reg());
        let mut seq = [s(Uop::Dummy); 12];
        let mut n = 0;
        let mut add = |u: Uop, f: u8| {
            seq[n] = Step { u, f };
            n += 1;
        };
        let peek = |k: u8| Uop::Rd(Ad::Pc(k), Act::None);
        if p & 0x80 == 0 {
            let off = ((p & 0x1f) ^ 0x10).wrapping_sub(0x10) as i8;
            self.ea = base.wrapping_add(off as u16);
            add(peek(0), 0);
            add(Uop::Dummy, 0);
            self.q.push_front(&seq[..n]);
            return;
        }
        let indirect = p & 0x10 != 0;
        match p & 0x0f {
            0x0 | 0x1 => {
                self.ea = base;
                let by = u16::from(p & 1) + 1;
                self.set_pointer(r, base.wrapping_add(by));
                add(peek(0), 0);
                for _ in 0..=by {
                    add(Uop::Dummy, 0);
                }
            }
            0x2 | 0x3 => {
                let by = u16::from(p & 1) + 1;
                let v = base.wrapping_sub(by);
                self.set_pointer(r, v);
                self.ea = v;
                add(peek(0), 0);
                for _ in 0..=by {
                    add(Uop::Dummy, 0);
                }
            }
            0x5 | 0x6 => {
                let acc = if p & 0x0f == 0x5 { self.b } else { self.a };
                self.ea = base.wrapping_add(acc as i8 as u16);
                add(peek(0), 0);
                add(Uop::Dummy, 0);
            }
            0x8 => {
                add(Uop::Prog(Act::Off8), 0);
                add(Uop::Dummy, 0);
                add(Uop::Ctl(Ctl::EaOff(rr)), 0);
            }
            0x9 => {
                add(Uop::Prog(Act::OffHi), 0);
                add(Uop::Prog(Act::OffLo), 0);
                add(peek(0), 0);
                add(Uop::Dummy, 0);
                add(Uop::Dummy, 0);
                add(Uop::Ctl(Ctl::EaOff(rr)), 0);
            }
            0xb => {
                self.ea = base.wrapping_add(self.d());
                add(peek(0), 0);
                add(peek(1), 0);
                add(peek(2), 0);
                add(Uop::Dummy, 0);
                add(Uop::Dummy, 0);
            }
            0xc => {
                add(Uop::Prog(Act::Off8), 0);
                add(Uop::Dummy, 0);
                add(Uop::Ctl(Ctl::EaPcOff), 0);
            }
            0xd => {
                add(Uop::Prog(Act::OffHi), 0);
                add(Uop::Prog(Act::OffLo), 0);
                add(peek(0), 0);
                add(Uop::Dummy, 0);
                add(Uop::Dummy, 0);
                add(Uop::Dummy, 0);
                add(Uop::Ctl(Ctl::EaPcOff), 0);
            }
            0xf if indirect => {
                add(Uop::Prog(Act::EaHi), 0);
                add(Uop::Prog(Act::EaLo), 0);
                add(Uop::Dummy, 0);
            }
            _ => {
                self.ea = base;
                add(peek(0), 0);
            }
        }
        if indirect {
            add(Uop::Rd(Ad::Ea, Act::IndHi), BUSY);
            add(Uop::Rd(Ad::Ea1, Act::IndLo), 0);
            add(Uop::Dummy, 0);
        }
        self.q.push_front(&seq[..n]);
    }

    fn ctl(&mut self, c: Ctl) {
        match c {
            Ctl::Exec => self.exec(),
            Ctl::Jump => self.pc = self.ea,
            Ctl::Lea => {
                if let Kind::Lea(r) = self.op.kind {
                    if matches!(r, LeaRegister::X | LeaRegister::Y) {
                        self.cc = if self.ea == 0 {
                            self.cc | Z
                        } else {
                            self.cc & !Z
                        };
                    }
                    self.set16(r.into(), self.ea);
                }
            }
            Ctl::Inherent => self.inherent_op(),
            Ctl::EaOff(rr) => {
                let r = match rr {
                    0 => self.x,
                    1 => self.y,
                    2 => self.u,
                    _ => self.s,
                };
                self.ea = r.wrapping_add(self.off);
            }
            Ctl::EaPcOff => self.ea = self.pc.wrapping_add(self.off),
            Ctl::SetE(on) => {
                self.cc = if on { self.cc | E } else { self.cc & !E };
            }
            Ctl::Mask(m) => self.cc |= m,
            Ctl::Sync => {
                self.run = Run::Sync;
                self.sync_exit = false;
            }
            Ctl::Wait => {
                self.run = Run::Wait;
                self.wait_first = true;
            }
        }
    }

    /// Figure 17 sheet 5: the cycles after the effective address.
    fn exec(&mut self) {
        match self.op.kind {
            Kind::Unary(Unary::Tst, _) => {
                self.push(Uop::Rd(Ad::Ea, Act::Tst));
                self.push(Uop::Dummy);
                self.push(Uop::Dummy);
            }
            Kind::Unary(..) => {
                self.busy(Uop::Rd(Ad::Ea, Act::Rmw));
                self.busy(Uop::Dummy);
                self.push(Uop::Wr(Ad::Ea, Val::Tmp));
            }
            Kind::Alu8(..) => self.push(Uop::Rd(Ad::Ea, Act::Alu8)),
            Kind::St8(a) => self.push(Uop::Wr(Ad::Ea, Val::Store8(a))),
            Kind::Alu16(..) => {
                self.busy(Uop::Rd(Ad::Ea, Act::TmpHi));
                self.push(Uop::Rd(Ad::Ea1, Act::Alu16));
                self.push(Uop::Dummy);
            }
            Kind::Ld16(_) => {
                self.busy(Uop::Rd(Ad::Ea, Act::TmpHi));
                self.push(Uop::Rd(Ad::Ea1, Act::Ld16));
            }
            Kind::St16(r) => {
                self.busy(Uop::Wr(Ad::Ea, Val::Store16Hi(r)));
                self.push(Uop::Wr(Ad::Ea1, Val::Store16Lo(r)));
            }
            Kind::Jmp => self.push(Uop::Ctl(Ctl::Jump)),
            Kind::Jsr => self.call(),
            Kind::Lea(_) => {
                self.push(Uop::Dummy);
                self.push(Uop::Ctl(Ctl::Lea));
            }
            _ => {}
        }
    }

    /// The inherent operations that touch only registers.
    fn inherent_op(&mut self) {
        match self.op.kind {
            Kind::Unary(u, t) => {
                let m = if t == Target::A { self.a } else { self.b };
                let (r, cc) = alu::unary(self.cc, u, m);
                self.cc = cc;
                if let Some(r) = r {
                    if t == Target::A {
                        self.a = r;
                    } else {
                        self.b = r;
                    }
                }
            }
            Kind::Inherent(Inherent::Daa) => {
                let (r, cc) = alu::daa(self.cc, self.a);
                self.a = r;
                self.cc = cc;
            }
            Kind::Inherent(Inherent::Sex) => {
                self.a = if self.b & 0x80 != 0 { 0xff } else { 0 };
                self.cc = alu::logic16(self.cc, self.d());
            }
            Kind::Inherent(Inherent::Abx) => self.x = self.x.wrapping_add(u16::from(self.b)),
            Kind::Inherent(Inherent::Mul) => {
                let (d, cc) = alu::mul(self.cc, self.a, self.b);
                self.set_d(d);
                self.cc = cc;
            }
            _ => {}
        }
    }
}
