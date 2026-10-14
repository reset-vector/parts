//! The Intel 8080A, emulated at its pins.
//!
//! The crate depends only on `signal`, allocates nothing, does no I/O and is
//! `no_std`: an [`I8080`] is a plain value that advances **one state** at a
//! time — `I8080-010`'s unit, not a machine cycle and not an instruction — and
//! shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! A board of 64 KiB of memory running `MVI A,42h` then `HLT`, answering each
//! read on the tick after the part asks for it:
//!
//! ```
//! use i8080::{Address, Ctrl, I8080, Pins, status};
//!
//! let mut mem = [0u8; 0x1_0000];
//! mem[..3].copy_from_slice(&[0x3E, 0x42, 0x76]);
//! let mut cpu = I8080::new();
//! // The first state of the first machine cycle is a fetch, and the fetch is
//! // where the status word says so.
//! let mut out = cpu.tick(Pins::idle());
//! assert!(out.ctrl.contains(Ctrl::SYNC), "I8080-015");
//! assert_eq!(out.d, Some(status::FETCH), "I8080-014, I8080-013");
//! while !cpu.is_halted() {
//!     let mut pins = Pins::idle();
//!     if let (true, Some(Address::Stable(a))) = (cpu.is_reading(), out.address) {
//!         pins = pins.with_d(mem[usize::from(a)]);
//!     }
//!     out = cpu.tick(pins);
//! }
//! assert_eq!(cpu.registers().a, 0x42);
//! ```
//!
//! # The part, not the machine
//!
//! The two clock phases, the status latch and the bus control they turn into
//! are all on the other side of the pins (`I8080-004`, `I8080-011`). This crate
//! has no 8224 in it and no 8228: it puts the status word on
//! [`Pins::d`] during `T1` and leaves latching it to the caller, exactly as
//! the part does.
//!
//! # What this crate chooses
//!
//! Five behaviours here are the crate's choice, each under an open item, and
//! each is marked where it is written: what the twelve unlisted opcodes do
//! (`I8080-OPEN-01`), what the program counter does during the operand bytes
//! of an instruction jammed by an interrupt (`I8080-OPEN-02`), what an
//! instruction whose entry names no flags leaves alone (`I8080-OPEN-03`), that
//! an acknowledged hold floats the address and data buses and nothing else
//! (`I8080-OPEN-05`), and that [`I8080::new`] starts every register at zero
//! (`I8080-OPEN-06`).

#![no_std]

#[cfg(test)]
extern crate std;

mod alu;
mod decode;
mod exec;
mod pins;
mod seq;

pub use alu::{FLAG_AC, FLAG_CY, FLAG_MASK, FLAG_ONE, FLAG_P, FLAG_S, FLAG_Z};
pub use decode::{Alu8, Cond, Decoded, Inherent, Kind, LdaxPair, Mov, Pair, Reg, Restart, decode};
pub use pins::{Address, Ctrl, Pins, status};
/// A pin's level, as [`Run::HoldPending`] holds `HLDA`'s.
pub use signal::Level;

use seq::{At, Cycle, CycleKind};

/// The addresses the part starts from.
pub mod vector {
    /// Where execution begins after a reset (`I8080-026`).
    pub const RESET: u16 = 0x0000;
}

/// The programmer-visible registers, as [`I8080::at_boundary`] takes them.
///
/// `Default` is [`I8080::new`]'s registers: every one zero but the constant
/// bits of [`Registers::flags`].
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`I8080::registers`] and sets the fields it wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// Accumulator.
    pub a: u8,
    /// The flag register as `PUSH PSW` writes it, three constant bits included
    /// (`I8080-047`).
    pub flags: u8,
    /// Register `B`, the high half of the pair `BC`.
    pub b: u8,
    /// Register `C`.
    pub c: u8,
    /// Register `D`, the high half of the pair `DE`.
    pub d: u8,
    /// Register `E`.
    pub e: u8,
    /// Register `H`, the high half of the pair `HL`.
    pub h: u8,
    /// Register `L`.
    pub l: u8,
    /// Stack pointer (`I8080-041`).
    pub sp: u16,
    /// Program counter (`I8080-040`).
    pub pc: u16,
    /// The interrupt enable flip-flop, which `EI` and `DI` set and clear
    /// (`I8080-030`).
    pub inte: bool,
    /// The instruction just ended is an `EI`, so the interrupt system takes
    /// no request at this boundary, only after the next instruction
    /// (`I8080-072`).
    pub ei_deferred: bool,
    /// The accumulator and flags the arithmetic, logical or rotate
    /// instruction just ended produced, which reach [`Registers::a`] and
    /// [`Registers::flags`] only in `T2` of the next machine cycle
    /// (`I8080-074`), so a reset before then drops them. `None` when no
    /// result waits.
    pub alu_deferred: Option<AluResult>,
    /// The target of a taken `JMP`, `CALL`, `RET` or `RST`, held in `W,Z`:
    /// the next fetch addresses it in place of [`Registers::pc`], which still
    /// points past the transfer's last byte (`I8080-025`, `I8080-077`). `None`
    /// when the next fetch addresses `pc`.
    pub transfer_deferred: Option<u16>,
    /// Whether the part runs on, enters a hold, or stands halted at this
    /// boundary.
    pub run: Run,
}

/// What the part does from a boundary ([`Registers::run`]): the three exclude
/// one another, since a halt answers `HOLD` itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Run {
    /// The next tick begins the fetch.
    #[default]
    Running,
    /// `HOLD` was sampled in the last machine cycle of the instruction just
    /// ended (`I8080-033`), so the part comes to rest in the hold state at its
    /// end (`I8080-035`): the next tick begins the fetch, samples `INT` for it
    /// as at the end of any instruction, and holds it over until the hold
    /// ends.
    HoldPending {
        /// `HLDA`'s level, High where it has risen for that hold already: it rises in `T3` of a cycle
        /// that reads and a state later in any other, a write or `DAD`'s
        /// internal cycles (`I8080-034`, `I8080-016`), so an instruction whose
        /// last cycle does not read ends with it still Low.
        hlda: Level,
    },
    /// The part is stopped by `HLT` (`I8080-036`) and waits for `INT`, a hold
    /// or a reset (`I8080-037`).
    Halted,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            flags: alu::FLAGS_CLEAR,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            sp: 0,
            pc: vector::RESET,
            inte: false,
            ei_deferred: false,
            alu_deferred: None,
            transfer_deferred: None,
            run: Run::Running,
        }
    }
}

/// An ALU result on its way to the accumulator and flags
/// ([`Registers::alu_deferred`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AluResult {
    /// The accumulator.
    pub a: u8,
    /// The flag register, as [`Registers::flags`].
    pub flags: u8,
}

/// What the part is doing at the largest scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Mode {
    /// Working through machine cycles.
    Running,
    /// Stopped by `HLT` (`I8080-036`).
    Halted,
    /// The buses belong to somebody else (`I8080-033`).
    Hold {
        /// Whether the part was halted when the request arrived, because
        /// `I8080-037` says it goes back there.
        from_halt: bool,
    },
}

/// The part.
///
/// Advance it with [`I8080::tick`], one state per call. Cloneable, so a test
/// can fork one and run both halves.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct I8080 {
    /// Accumulator.
    a: u8,
    /// The flag register as `PUSH PSW` writes it, three constant bits included
    /// (`I8080-047`).
    flags: u8,
    /// Register `B`, the high half of the pair `BC`.
    b: u8,
    /// Register `C`.
    c: u8,
    /// Register `D`, the high half of the pair `DE`.
    d: u8,
    /// Register `E`.
    e: u8,
    /// Register `H`, the high half of the pair `HL`.
    h: u8,
    /// Register `L`.
    l: u8,
    /// Stack pointer (`I8080-041`).
    sp: u16,
    /// Program counter (`I8080-040`).
    pc: u16,
    /// The high half of the temporary pair no program can address
    /// (`I8080-039`).
    w: u8,
    /// The low half of the same pair.
    z: u8,
    /// The interrupt enable flip-flop, which `INTE` shows (`I8080-030`).
    inte: bool,

    /// The machine cycle in progress, or `None` while the part is halted or
    /// holding.
    cycle: Option<Cycle>,
    /// The machine cycle a hold interrupted, kept so that leaving the hold
    /// resumes rather than recomputes (`I8080-033`).
    pending: Option<Cycle>,
    mode: Mode,

    /// The opcode of the instruction being executed, and its reading.
    opcode: u8,
    dec: Decoded,
    /// How many machine cycles of this instruction have finished.
    mstep: u8,
    /// The byte the last input cycle brought in.
    latch: u8,
    /// `TMP`, which `I8080-042` puts in the ALU.
    tmp: u8,
    /// Whether the current conditional instruction's condition held.
    taken: bool,
    /// Whether the next fetch takes its address from `W,Z` (`I8080-025`).
    transfer: bool,
    /// The accumulator and flags an arithmetic, logical or rotate instruction
    /// produced, waiting for `T2` of the next machine cycle (`I8080-074`).
    late: Option<(u8, u8)>,

    /// The internal interrupt latch of `I8080-029`.
    int_latch: bool,
    /// Whether the interrupt now being taken is getting the part out of a halt,
    /// which decides between the ninth and tenth status words (`I8080-038`).
    from_halt: bool,
    /// `EI` enables the system only after the next instruction (`I8080-072`).
    /// Set by `EI`, and cleared, with no request taken, by the end of the
    /// `EI` itself.
    ei_defer: bool,
    /// The control lines the last state drove.
    driven: Ctrl,

    /// Consecutive states with `RESET` High, against `I8080-027`'s three.
    reset_count: u8,
    /// A hold has been asked for and not yet acknowledged.
    hold_req: bool,
    /// `HLDA` is High.
    hlda: bool,
    /// The buses are floating, which lags `HLDA` by one state (`I8080-034`).
    floated: bool,
    /// The last state left the address and data lines high-impedance.
    released: bool,
}

impl Default for I8080 {
    fn default() -> Self {
        Self::new()
    }
}

impl I8080 {
    /// A part with every register zero and the interrupt system disabled.
    ///
    /// That is the reset state, the fetch at [`vector::RESET`] next with
    /// `INTE` and `HLDA` Low (`I8080-026`, `I8080-028`), which a board reaches
    /// by holding `RESET` High ([`Ctrl::RESET`]) for three states
    /// (`I8080-027`).
    ///
    /// **The part itself has no such state.** `I8080-028` says a reset leaves
    /// the registers, the stack pointer and the flags alone, and p. 2-13 says
    /// their power-up contents "cannot be specified" — `I8080-OPEN-06`. Zero is
    /// this crate's choice because a constructor has to return something.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            a: 0,
            flags: alu::FLAGS_CLEAR,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            sp: 0,
            pc: vector::RESET,
            w: 0,
            z: 0,
            inte: false,
            cycle: None,
            pending: None,
            mode: Mode::Running,
            opcode: 0,
            dec: decode::runs(0),
            mstep: 0,
            latch: 0,
            tmp: 0,
            taken: false,
            transfer: false,
            late: None,
            int_latch: false,
            from_halt: false,
            ei_defer: false,
            driven: Ctrl::NONE,
            reset_count: 0,
            hold_req: false,
            hlda: false,
            floated: false,
            released: false,
        }
    }

    /// A part standing at an instruction boundary with these registers, as
    /// [`I8080::settle`] leaves one, with no reset pending: the next tick is
    /// the last state of the instruction before, and begins the fetch at
    /// [`Registers::transfer_deferred`], or at [`Registers::pc`] where that is
    /// `None`. On that tick the part samples `INT` as at the end of any
    /// instruction (`I8080-029`), so with [`Registers::inte`] set and
    /// [`Registers::ei_deferred`] clear a request then is taken. With
    /// [`Run::HoldPending`] that tick enters the hold state and the fetch
    /// waits for its end. With [`Run::Halted`] the part stands in the halt
    /// state instead, and the next tick is a halt state, which answers `HOLD`
    /// itself. The three constant bits of
    /// [`Registers::flags`] take their fixed values (`I8080-047`), as do those
    /// of [`Registers::alu_deferred`], which reaches the accumulator and flags
    /// in `T2` of that fetch, and `W,Z` holds the transfer's target, or zero.
    ///
    /// Nothing else is carried, and nothing else stands at a boundary: the
    /// part's place in a machine cycle and a byte read are settled first, and [`I8080::is_at_instruction_boundary`] is `false`
    /// while a reset or a hold is in progress. Until its first tick,
    /// [`I8080::is_reading`], [`I8080::is_writing`] and [`I8080::is_released`] read
    /// `false`, whatever the original's last state drove.
    #[must_use]
    pub fn at_boundary(regs: Registers) -> Self {
        let [w, z] = regs.transfer_deferred.unwrap_or(0).to_be_bytes();
        Self {
            a: regs.a,
            flags: alu::fix_constant_bits(regs.flags),
            b: regs.b,
            c: regs.c,
            d: regs.d,
            e: regs.e,
            h: regs.h,
            l: regs.l,
            sp: regs.sp,
            pc: regs.pc,
            w,
            z,
            inte: regs.inte,
            ei_defer: regs.ei_deferred,
            late: regs
                .alu_deferred
                .map(|r| (r.a, alu::fix_constant_bits(r.flags))),
            transfer: regs.transfer_deferred.is_some(),
            mode: if regs.run == Run::Halted {
                Mode::Halted
            } else {
                Mode::Running
            },
            hold_req: matches!(regs.run, Run::HoldPending { .. }),
            hlda: regs.run == Run::HoldPending { hlda: Level::High },
            ..Self::new()
        }
    }

    /// Advance one state and show the pins for it.
    ///
    /// The control inputs and [`Pins::d`] presented here belong to the state
    /// that is now ending: the part samples `READY` and `HOLD` during `T2` and
    /// `TW` (`I8080-023`, `I8080-033`) and takes its data during `T3`
    /// (`I8080-018`), and a caller that answers a `DBIN` by setting `d` and
    /// calling again is doing exactly that.
    #[must_use]
    pub fn tick(&mut self, pins: Pins) -> Pins {
        let out = self.state(pins);
        self.driven = out.ctrl;
        out
    }

    /// Take the byte of the read that ends the instruction now ending, and
    /// finish that instruction without starting the next, so that
    /// [`I8080::registers`] reads the boundary: the last read's byte is in
    /// place, and an arithmetic result, which waits for the next `T2`
    /// (`I8080-074`), is in [`Registers::alu_deferred`]. Returns whether the
    /// part now stands at a boundary that [`I8080::at_boundary`] rebuilds
    /// from [`I8080::registers`]: `false`, changing nothing, unless
    /// [`I8080::is_at_instruction_boundary`], and where `pins` carry
    /// [`Ctrl::RESET`], since a `RESET` shorter than three states does not
    /// take (`I8080-027`) and the part then finishes the instruction with the
    /// byte presented as `RESET` falls, which no rebuild can know. A part is
    /// rebuilt at a later boundary instead.
    ///
    /// `pins` is what the caller would present on the next tick, and the
    /// caller still presents it then: that tick samples `INT` and begins the
    /// fetch. Idempotent; ticking on afterwards gives the pins an unsettled
    /// part gives.
    #[must_use = "a part rebuilt where `settle` returned `false` is rebuilt off a boundary"]
    pub fn settle(&mut self, pins: Pins) -> bool {
        if !self.is_at_instruction_boundary() || pins.ctrl.contains(Ctrl::RESET) {
            return false;
        }
        let Some(cycle) = self.cycle else {
            return true;
        };
        if cycle.at == At::T3 && cycle.kind.is_input() {
            self.latch = pins.d.unwrap_or(0);
        }
        self.apply_cycle();
        self.mstep += 1;
        self.cycle = None;
        true
    }

    /// One state, as [`I8080::tick`] shows it.
    fn state(&mut self, pins: Pins) -> Pins {
        let ctrl = pins.ctrl;
        self.released = false;

        if ctrl.contains(Ctrl::RESET) {
            self.reset_count = self.reset_count.saturating_add(1);
            if self.reset_count >= 3 {
                self.reset();
            }
            return Pins {
                address: Some(Address::Undetermined),
                d: None,
                ctrl: Ctrl::NONE,
            };
        }
        self.reset_count = 0;

        match self.mode {
            Mode::Hold { from_halt } => return self.tick_hold(ctrl, from_halt),
            Mode::Halted => return self.tick_halted(ctrl),
            Mode::Running => {}
        }

        self.floated = self.hlda;

        match self.cycle {
            Some(cycle) => self.advance(cycle, ctrl, pins.d.unwrap_or(0)),
            None => {
                self.end_of_instruction(ctrl);
                let next = self.begin_instruction();
                self.go_on(next);
            }
        }
        self.released = self.floated;
        self.emit()
    }

    /// `I8080-027`'s three clock periods have passed with `RESET` High.
    fn reset(&mut self) {
        self.pc = vector::RESET;
        self.inte = false;
        self.hlda = false;
        self.floated = false;
        self.hold_req = false;
        self.int_latch = false;
        self.ei_defer = false;
        self.transfer = false;
        self.late = None;
        self.mode = Mode::Running;
        self.cycle = None;
        self.pending = None;
        self.mstep = 0;
    }

    fn tick_hold(&mut self, ctrl: Ctrl, from_halt: bool) -> Pins {
        if ctrl.contains(Ctrl::HOLD) {
            self.floated = true;
            self.released = true;
            return Pins {
                address: None,
                d: None,
                ctrl: self.outputs().with(Ctrl::HLDA),
            };
        }
        self.hlda = false;
        self.floated = false;
        if from_halt {
            self.mode = Mode::Halted;
            return Pins {
                address: Some(Address::Stable(self.pc)),
                d: None,
                ctrl: self.outputs(),
            };
        }
        self.mode = Mode::Running;
        self.cycle = self.pending.take();
        self.emit()
    }

    fn tick_halted(&mut self, ctrl: Ctrl) -> Pins {
        if ctrl.contains(Ctrl::HOLD) {
            self.hlda = true;
            self.mode = Mode::Hold { from_halt: true };
            return Pins {
                address: Some(Address::Stable(self.pc)),
                d: None,
                ctrl: self.outputs(),
            };
        }
        if ctrl.contains(Ctrl::INT) && self.inte {
            self.int_latch = true;
            self.from_halt = true;
            self.mode = Mode::Running;
            self.floated = false;
            self.cycle = Some(self.begin_instruction());
            return self.emit();
        }
        Pins {
            address: Some(Address::Stable(self.pc)),
            d: None,
            ctrl: self.outputs(),
        }
    }

    /// Move to the next state of `cycle`, or finish it.
    fn advance(&mut self, cycle: Cycle, ctrl: Ctrl, data: u8) {
        let mut cycle = cycle;

        if cycle.at == At::T3 && cycle.kind.is_input() {
            self.latch = data;
            if self.mstep == 0 {
                self.opcode = data;
                self.dec = decode::runs(data);
                cycle.states = exec::m1_states(self.dec.kind);
            }
        }
        if matches!(cycle.at, At::T2 | At::Tw)
            && !ctrl.contains(Ctrl::READY_LOW)
            && ctrl.contains(Ctrl::HOLD)
        {
            self.hold_req = true;
        }

        match cycle.next(!ctrl.contains(Ctrl::READY_LOW)) {
            Some(at) => {
                cycle.at = at;
                if self.hold_req && !self.hlda {
                    let inbound = cycle.kind.is_input();
                    if (inbound && at == At::T3) || (!inbound && matches!(at, At::T4 | At::T5)) {
                        self.hlda = true;
                    }
                }
                if at == At::T2
                    && let Some((a, flags)) = self.late.take()
                {
                    self.a = a;
                    self.flags = flags;
                }
                self.cycle = Some(cycle);
            }
            None => {
                self.cycle = Some(cycle);
                self.finish_cycle(ctrl);
            }
        }
    }

    fn finish_cycle(&mut self, ctrl: Ctrl) {
        self.apply_cycle();
        self.mstep += 1;

        if self.mode == Mode::Halted {
            self.hold_req = false;
            self.cycle = None;
            return;
        }

        let next = match self.next_cycle() {
            Some(cycle) => cycle,
            None => {
                self.end_of_instruction(ctrl);
                self.begin_instruction()
            }
        };
        self.go_on(next);
    }

    /// Start `next`, or, with a hold asked for, come to rest at the end of
    /// the machine cycle (`I8080-035`), keeping the cycle not yet started.
    fn go_on(&mut self, next: Cycle) {
        if self.hold_req {
            self.hold_req = false;
            self.hlda = true;
            self.pending = Some(next);
            self.cycle = None;
            self.mode = Mode::Hold { from_halt: false };
        } else {
            self.cycle = Some(next);
        }
    }

    /// `I8080-029`: the latch is set during the last state of an instruction
    /// cycle.
    fn end_of_instruction(&mut self, ctrl: Ctrl) {
        if self.ei_defer {
            self.ei_defer = false;
        } else if self.inte && ctrl.contains(Ctrl::INT) {
            self.int_latch = true;
            self.from_halt = false;
        }
    }

    /// Start a machine cycle `M1`, which `I8080-014` says every instruction
    /// cycle opens with.
    fn begin_instruction(&mut self) -> Cycle {
        self.mstep = 0;
        self.taken = false;
        let kind = if self.int_latch {
            self.int_latch = false;
            self.inte = false;
            if self.from_halt {
                self.from_halt = false;
                CycleKind::IntAckHalt
            } else {
                CycleKind::IntAck
            }
        } else {
            CycleKind::Fetch
        };

        let addr = if self.transfer {
            self.transfer = false;
            u16::from_be_bytes([self.w, self.z])
        } else {
            self.pc
        };
        self.pc = if matches!(kind, CycleKind::IntAck | CycleKind::IntAckHalt) {
            addr
        } else {
            addr.wrapping_add(1)
        };
        Cycle::new(kind, addr, 0, 4)
    }

    /// The pins for the state the part now stands in.
    fn emit(&self) -> Pins {
        let Some(cycle) = self.cycle else {
            if self.floated {
                return Pins {
                    address: None,
                    d: None,
                    ctrl: self.outputs(),
                };
            }
            return Pins {
                address: Some(Address::Stable(
                    self.pending.map_or(self.pc, |cycle| cycle.addr),
                )),
                d: None,
                ctrl: self.outputs(),
            };
        };

        let mut ctrl = self.outputs();
        let mut data = None;

        match cycle.at {
            At::T1 => {
                if let Some(word) = cycle.kind.status() {
                    ctrl |= Ctrl::SYNC;
                    data = Some(word);
                }
            }
            At::T2 | At::Tw | At::T3 => {
                if cycle.kind.is_input() {
                    ctrl |= Ctrl::DBIN;
                } else if cycle.kind.is_output() {
                    data = Some(cycle.out);
                    if cycle.at != At::T2 {
                        ctrl |= Ctrl::WR;
                    }
                }
                if cycle.at == At::Tw {
                    ctrl |= Ctrl::WAIT;
                }
            }
            At::T4 | At::T5 => {
                if cycle.kind.is_output() {
                    data = Some(cycle.out);
                }
            }
        }

        if self.floated {
            return Pins {
                address: None,
                d: None,
                ctrl: ctrl.without(Ctrl::WR).without(Ctrl::DBIN),
            };
        }
        Pins {
            address: Some(Address::Stable(cycle.addr)),
            d: data,
            ctrl,
        }
    }

    /// The two outputs that do not belong to a state: `INTE` and `HLDA`.
    fn outputs(&self) -> Ctrl {
        let mut ctrl = Ctrl::NONE;
        if self.inte {
            ctrl |= Ctrl::INTE;
        }
        if self.hlda {
            ctrl |= Ctrl::HLDA;
        }
        ctrl
    }

    pub(crate) fn reg(&self, r: Reg) -> u8 {
        match r {
            Reg::B => self.b,
            Reg::C => self.c,
            Reg::D => self.d,
            Reg::E => self.e,
            Reg::H => self.h,
            Reg::L => self.l,
            Reg::M => self.tmp,
            Reg::A => self.a,
        }
    }

    pub(crate) fn set_reg(&mut self, r: Reg, value: u8) {
        match r {
            Reg::B => self.b = value,
            Reg::C => self.c = value,
            Reg::D => self.d = value,
            Reg::E => self.e = value,
            Reg::H => self.h = value,
            Reg::L => self.l = value,
            Reg::M => self.tmp = value,
            Reg::A => self.a = value,
        }
    }

    pub(crate) fn pair(&self, p: Pair) -> u16 {
        match p {
            Pair::Bc => u16::from_be_bytes([self.b, self.c]),
            Pair::De => u16::from_be_bytes([self.d, self.e]),
            Pair::Hl => u16::from_be_bytes([self.h, self.l]),
            Pair::SpPsw => self.sp,
        }
    }

    pub(crate) fn set_pair(&mut self, p: Pair, value: u16) {
        let [high, low] = value.to_be_bytes();
        match p {
            Pair::Bc => {
                self.b = high;
                self.c = low;
            }
            Pair::De => {
                self.d = high;
                self.e = low;
            }
            Pair::Hl => {
                self.h = high;
                self.l = low;
            }
            Pair::SpPsw => self.sp = value,
        }
    }

    /// `H,L` as an address, which is what every `Reg::M` operand means.
    pub(crate) fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }

    /// `W,Z`, the pair `I8080-025` says drives the bus after a transfer.
    pub(crate) fn wz(&self) -> u16 {
        u16::from_be_bytes([self.w, self.z])
    }

    pub(crate) fn set_wz(&mut self, value: u16) {
        [self.w, self.z] = value.to_be_bytes();
    }

    /// One of the eight conditions of `I8080-064`.
    pub(crate) fn condition(&self, cond: Cond) -> bool {
        let f = self.flags;
        match cond {
            Cond::Nz => f & FLAG_Z == 0,
            Cond::Z => f & FLAG_Z != 0,
            Cond::Nc => f & FLAG_CY == 0,
            Cond::C => f & FLAG_CY != 0,
            Cond::Po => f & FLAG_P == 0,
            Cond::Pe => f & FLAG_P != 0,
            Cond::P => f & FLAG_S == 0,
            Cond::M => f & FLAG_S != 0,
        }
    }

    /// The programmer-visible registers as they stand.
    ///
    /// An instruction's last machine cycle, and the byte it reads, reach the
    /// registers only with the tick that ends it, and an arithmetic, logical
    /// or rotate result reaches `A` and the flags only in `T2` of the next
    /// machine cycle (`I8080-074`); until then they read as the instruction
    /// found them, and the result is [`Registers::alu_deferred`]. At a
    /// boundary, [`I8080::settle`] takes the read and finishes the cycle, and
    /// `I8080::at_boundary(cpu.registers())` then runs on as this part does.
    #[must_use]
    pub fn registers(&self) -> Registers {
        Registers {
            a: self.a,
            flags: self.flags,
            b: self.b,
            c: self.c,
            d: self.d,
            e: self.e,
            h: self.h,
            l: self.l,
            sp: self.sp,
            pc: self.pc,
            inte: self.inte,
            ei_deferred: self.ei_defer,
            alu_deferred: self.late.map(|(a, flags)| AluResult { a, flags }),
            transfer_deferred: self.transfer.then(|| self.wz()),
            run: if self.mode == Mode::Halted {
                Run::Halted
            } else if self.hold_req {
                Run::HoldPending {
                    hlda: Level::from_bool(self.hlda),
                }
            } else {
                Run::Running
            },
        }
    }

    /// Whether the next tick begins the fetch of a new instruction: the one
    /// before it is in its last state, with no reset pending. A hold asked for
    /// in that instruction's last machine cycle is pending there, and the
    /// fetch begins as the hold is entered ([`Run::HoldPending`]).
    /// Also `true` in each state of a halt (`I8080-036`), where
    /// [`Run::Halted`] carries it.
    ///
    /// A read that ends that last state still takes its byte with the next
    /// tick, and an arithmetic result is written in `T2` of the fetch after
    /// it (`I8080-074`). `false` while holding.
    #[must_use]
    pub fn is_at_instruction_boundary(&self) -> bool {
        if self.reset_count != 0 {
            return false;
        }
        match self.mode {
            Mode::Running => {}
            Mode::Halted => return true,
            Mode::Hold { .. } => return false,
        }
        let Some(cycle) = self.cycle else {
            return true;
        };
        if cycle.next(true).is_some() {
            return false;
        }
        let mut ahead = self.clone();
        ahead.apply_cycle();
        ahead.mstep += 1;
        ahead.mode == Mode::Running && ahead.next_cycle().is_none()
    }

    /// Whether the part is stopped by a `HLT` (`I8080-036`).
    ///
    /// Only where [`I8080::is_at_instruction_boundary`] holds: false while
    /// `RESET` is held, and during a hold taken from a halt; the part is
    /// halted again when the hold ends (`I8080-037`).
    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.mode == Mode::Halted && self.is_at_instruction_boundary()
    }

    /// Whether the part has given the buses up: the address and data lines
    /// are in their high-impedance state (`I8080-006`), which follows `HLDA`
    /// by a state (`I8080-034`). [`Pins::address`] and [`Pins::d`] then
    /// read `None`.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.released
    }

    /// `RESET` was High at the last tick (`I8080-026`), whether or not it
    /// has been held the three states a reset takes (`I8080-027`). False on
    /// a part [`I8080::new`] or [`I8080::at_boundary`] builds.
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.reset_count != 0
    }

    /// Whether the present state reads an address the part asked for:
    /// `DBIN` is High (`I8080-018`) in a fetch, memory, stack or input cycle,
    /// and the board places the byte at [`Pins::address`] into [`Pins::d`]
    /// before the next tick.
    ///
    /// `false` through an interrupt machine cycle, whose `DBIN` takes the byte
    /// the board jams rather than one at the address, the unincremented
    /// program counter (`I8080-031`): that is
    /// [`I8080::is_acknowledging_interrupt`].
    #[must_use]
    pub fn is_reading(&self) -> bool {
        self.driven.contains(Ctrl::DBIN) && !self.in_interrupt_cycle()
    }

    /// Whether the present state is the data window of an interrupt machine
    /// cycle, `DBIN` High with `INTA` in the cycle's status word: the board
    /// places the instruction it jams into [`Pins::d`] before the next tick
    /// (`I8080-031`), usually an `RST` (`I8080-032`).
    #[must_use]
    pub fn is_acknowledging_interrupt(&self) -> bool {
        self.driven.contains(Ctrl::DBIN) && self.in_interrupt_cycle()
    }

    /// Whether the machine cycle the last state belongs to is an interrupt
    /// acknowledgement.
    fn in_interrupt_cycle(&self) -> bool {
        self.cycle
            .is_some_and(|cycle| matches!(cycle.kind, CycleKind::IntAck | CycleKind::IntAckHalt))
    }

    /// Whether the present state writes: `WR` is Low (`I8080-019`).
    #[must_use]
    pub fn is_writing(&self) -> bool {
        self.driven.contains(Ctrl::WR)
    }
}

#[cfg(test)]
mod tests {
    use super::{Ctrl, I8080, Level, Pins, Registers, Run};

    /// Every `Run` a boundary can stand in round-trips, so no value of
    /// `Registers` is one `at_boundary` silently normalises.
    #[test]
    fn every_run_round_trips() {
        for run in [
            Run::Running,
            Run::HoldPending { hlda: Level::Low },
            Run::HoldPending { hlda: Level::High },
            Run::Halted,
        ] {
            let regs = Registers {
                run,
                ..Registers::default()
            };
            assert_eq!(I8080::at_boundary(regs).registers(), regs, "{run:?}");
        }
    }

    #[test]
    fn is_in_reset_follows_the_reset_pin_from_its_first_state() {
        let mut cpu = I8080::new();
        assert!(!cpu.is_in_reset());
        for _ in 0..4 {
            let _ = cpu.tick(Pins::idle().with(Ctrl::RESET));
            assert!(cpu.is_in_reset());
        }
        let _ = cpu.tick(Pins::idle());
        assert!(!cpu.is_in_reset());
    }
    use std::vec::Vec;

    #[test]
    fn a_boundary_stands_before_each_instruction_and_not_inside_one() {
        let program = [0x00, 0x3e, 0x5a, 0x76];
        let mut cpu = I8080::new();
        let mut inp = Pins::idle();
        let mut boundaries = Vec::new();
        for t in 0..20 {
            if cpu.is_at_instruction_boundary() {
                boundaries.push(t);
            }
            let out = cpu.tick(inp);
            inp = Pins::idle();
            if out.ctrl.contains(Ctrl::DBIN) {
                inp.d = Some(
                    program
                        .get(usize::from(
                            out.address
                                .and_then(crate::Address::stable)
                                .expect("the cycle drives an address"),
                        ))
                        .copied()
                        .unwrap_or(0),
                );
            }
        }
        assert_eq!(boundaries, [0, 4, 11, 18, 19]);
        assert!(cpu.is_halted());
        assert_eq!(cpu.registers().a, 0x5a);
    }

    /// The next state's inputs: `INT` held, and a read answered from memory.
    fn answer(memory: &[u8], out: Pins) -> Pins {
        let mut inp = Pins {
            ctrl: Ctrl::INT,
            ..Pins::idle()
        };
        if out.ctrl.contains(Ctrl::DBIN) {
            inp.d = Some(
                memory
                    .get(usize::from(
                        out.address
                            .and_then(crate::Address::stable)
                            .expect("the cycle drives an address"),
                    ))
                    .copied()
                    .unwrap_or(0),
            );
        }
        inp
    }

    #[test]
    fn the_buses_stay_floating_from_the_state_after_hlda_rises() {
        for start in 0..8 {
            let mut cpu = I8080::new();
            let mut hlda_before = false;
            let mut floated = 0;
            for t in 0..40 {
                let mut inp = Pins::idle();
                if (start..start + 6).contains(&t) {
                    inp.ctrl |= Ctrl::HOLD;
                }
                let out = cpu.tick(inp);
                let hlda = out.ctrl.contains(Ctrl::HLDA);
                if hlda && hlda_before {
                    assert!(cpu.is_released(), "start {start}, state {t}");
                    assert_eq!(
                        (out.address, out.d),
                        (None, None),
                        "start {start}, state {t}"
                    );
                    floated += 1;
                }
                hlda_before = hlda;
            }
            assert!(floated > 0, "start {start}: never floated");
        }
    }

    #[test]
    fn a_settled_boundary_round_trips_mid_program() {
        let memory = [0x3e, 0x41, 0xfb, 0x80, 0x00, 0x00];
        let start = I8080::at_boundary(super::Registers {
            b: 0x01,
            sp: 0x100,
            ..super::Registers::default()
        });
        let mut checked = Vec::new();
        for boundary in 1..5 {
            let mut cpu = start.clone();
            let mut inp = answer(&memory, Pins::idle());
            let mut seen = 0;
            loop {
                if cpu.is_at_instruction_boundary() {
                    seen += 1;
                    if seen > boundary {
                        break;
                    }
                }
                inp = answer(&memory, cpu.tick(inp));
            }
            let mut settled = cpu.clone();
            assert!(settled.settle(inp), "boundary {boundary} settles");
            let mut twice = settled.clone();
            assert!(twice.settle(inp));
            assert_eq!(twice, settled, "settle is idempotent");
            let regs = settled.registers();
            let mut restored = I8080::at_boundary(regs);
            assert_eq!(restored.registers(), regs);
            checked.push((
                regs.a,
                regs.alu_deferred.map(|r| r.a),
                regs.inte,
                regs.ei_deferred,
            ));
            let (mut a, mut b, mut c) = (inp, inp, inp);
            let mut acknowledged = false;
            for _ in 0..40 {
                let out = cpu.tick(a);
                assert_eq!(settled.tick(b), out, "after boundary {boundary}");
                assert_eq!(restored.tick(c), out, "after boundary {boundary}");
                assert_eq!(settled.is_reading(), cpu.is_reading());
                assert_eq!(restored.is_writing(), cpu.is_writing());
                acknowledged |= out.ctrl.contains(Ctrl::SYNC)
                    && out.d.is_some_and(|d| d & super::status::INTA != 0);
                a = answer(&memory, out);
                (b, c) = (a, a);
            }
            assert_eq!(acknowledged, boundary < 4, "INTA after boundary {boundary}");
        }
        assert_eq!(
            checked,
            [
                (0x41, None, false, false),
                (0x41, None, true, true),
                (0x41, Some(0x42), true, false),
                (0x42, None, false, false),
            ]
        );
    }
}
