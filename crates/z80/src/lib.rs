//! The Zilog Z80, emulated at its pins.
//!
//! The crate depends only on `signal`, allocates nothing, does no I/O and is
//! `no_std`: a `Z80` is a plain value that advances one T-state at a time and
//! shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! A board of 64 KiB of memory running `LD A,42h` then `HALT`, answering each
//! read on the tick after the part asks for it:
//!
//! ```
//! use z80::{Address, Ctrl, Pins, Z80};
//!
//! # fn main() -> Result<(), z80::StateError> {
//! let mut mem = [0u8; 0x1_0000];
//! mem[..3].copy_from_slice(&[0x3E, 0x42, 0x76]);
//! let mut cpu = Z80::new();
//! let mut pins = Pins::idle();
//! while !cpu.is_halted() {
//!     let out = cpu.tick(pins);
//!     pins = Pins::idle();
//!     if let (true, Some(Address::Stable(a))) = (
//!         cpu.is_reading() && out.ctrl.contains(Ctrl::MREQ),
//!         out.address,
//!     ) {
//!         pins = pins.with_d(mem[usize::from(a)]);
//!     }
//! }
//! assert_eq!(cpu.registers().a, 0x42);
//!
//! // A state image carries the part whole, mid-cycle included.
//! let mut copy = Z80::new();
//! copy.restore_state(&cpu.save_state())?;
//! assert_eq!(copy, cpu);
//! # Ok(())
//! # }
//! ```
//!
//! # What is here
//!
//! Everything is built: the pin boundary, the machine cycles, the
//! register file, reset, refresh and the bus request; decode with the loads,
//! exchanges, stack and control flow; the ALU and flag rules; the `CB`, `DD`,
//! `ED` and `FD` prefixes with the bit, index and block instructions; and
//! interrupts and `HALT`.
//!
//! The one thing deliberately not built is the general form of interrupt mode
//! 0: only the single-byte `RST` the manual costs at thirteen T-states is
//! executed (`Z80-073`).
//!
//! **What the crate chooses**, each an open item: one tick is one whole
//! T-state (`Z80-OPEN-02`); `RFSH` stays driven through a bus acknowledge
//! (`Z80-OPEN-01`); the refresh address carries bit 7 of `R` on `A7`
//! (`Z80-OPEN-04`) and `R` as it stood before the fetch increments it
//! (`Z80-OPEN-05`); interrupt mode 2 keeps the vector's low bit
//! (`Z80-OPEN-08`); the interrupt acknowledge's sixth and seventh
//! T-states fall as `Z80-075` breaks them down (`Z80-OPEN-13`); and an `NMI`
//! edge is not latched while `RESET` is held, and a reset drops one already
//! latched (`Z80-OPEN-15`).

#![no_std]

#[cfg(test)]
extern crate std;

mod alu;
mod exec;
mod pins;
mod regs;
mod seq;
mod state;

pub use pins::{Address, Ctrl, Pins, TRISTATE_CONTROLS};
pub use regs::{
    FLAG_C, FLAG_H, FLAG_N, FLAG_PV, FLAG_S, FLAG_XF, FLAG_YF, FLAG_Z, InterruptMode, Pending,
    Registers,
};
pub use signal::Level;
pub use state::{STATE_LEN, StateError};

use exec::{Index, Stage};

/// The addresses the part starts from.
pub mod vector {
    /// Where execution begins after a reset (`Z80-012`).
    pub const RESET: u16 = 0x0000;
}
use seq::{BusOp, Seq};

/// How long `RESET` must be held for a reset to complete (`Z80-010`).
const RESET_CLOCKS: u8 = 3;

/// What the part is doing besides executing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum State {
    /// Nothing is in flight: the next tick begins an opcode fetch, built from
    /// the registers as they stand then rather than as they stood earlier.
    Starting,
    /// The cycle in the sequencer belongs to an instruction.
    Running,
    /// The buses are released to another device, which is holding `BUSREQ`. The
    /// cycle carried here is the one that was about to begin (`Z80-008`).
    BusReleased(BusOp),
}

/// The part.
///
/// Advance it with [`Z80::tick`], one T-state per call. Cloneable, so a test can
/// fork one and run both halves.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Z80 {
    /// The register file, read through [`Z80::registers`].
    regs: Registers,
    seq: Seq,
    state: State,
    /// Consecutive clock cycles with `RESET` held, against `Z80-010`'s three:
    /// a shorter pulse leaves [`State`] and the cycle in flight as they were.
    reset_held: u8,
    /// Set only on the copy [`Z80::settle`] and
    /// [`Z80::is_at_instruction_boundary`] work on: the instruction's end stops
    /// at the boundary, before an interrupt is accepted.
    stop_at_boundary: bool,
    /// Where the part stands in the instruction it is executing.
    stage: Stage,
    /// Which index register a `DD` or `FD` prefix has put in `HL`'s place, for
    /// the one instruction that prefix reaches.
    index: Index,
    /// `IX+d` or `IY+d`, once the displacement byte has been read.
    indexed: u16,
    /// Whether the repeating block instruction in flight has another iteration
    /// to run (`Z80-054`).
    again: bool,
    /// `INT` as it stood at the last sample point (`Z80-070`).
    int_line: bool,
    /// The first operand byte an instruction has gathered — a displacement, an
    /// address's low half, or the low half of a word coming off the stack.
    lo: u8,
    /// The second operand byte, where an instruction takes two.
    hi: u8,
    /// [`Registers::q`] as it stood before the current opcode fetch cleared it,
    /// which is the value `SCF` and `CCF` read (`Z80-093`).
    q_previous: u8,
}

impl Z80 {
    /// A part at its power-on state (`Z80-025`), about to fetch from
    /// [`vector::RESET`].
    ///
    /// `PC`, `I`, `R`, both flip-flops and the mode stand as a reset leaves
    /// them (`Z80-012`), which a board reaches by holding `RESET`
    /// ([`Ctrl::RESET`]) for three clocks (`Z80-010`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            regs: Registers::power_on(),
            seq: Seq::new(BusOp::Internal(1)),
            state: State::Starting,
            reset_held: 0,
            stop_at_boundary: false,
            stage: Stage::Opcode,
            index: Index::None,
            indexed: 0,
            again: false,
            int_line: false,
            lo: 0,
            hi: 0,
            q_previous: 0,
        }
    }

    /// A part standing at an instruction boundary with these registers, with no
    /// reset pending: the next tick begins the opcode fetch at
    /// [`Registers::pc`], or, with [`Pending::Busreq`], releases
    /// the buses with that fetch deferred. [`Registers::halted`] and
    /// [`Registers::nmi_n`] carry a halt and `NMI`'s level across; with
    /// [`Registers::nmi_pending`] set the next tick accepts the latched `NMI`
    /// edge in place of the fetch, and otherwise, with
    /// [`Pending::Int`], `INT`.
    ///
    /// Nothing else is carried, and nothing else stands at a boundary: the
    /// machine cycle in flight is settled first, and a prefix is inside an
    /// instruction; a repeating block instruction stands at one between its
    /// iterations (`Z80-054`). Until its first tick, [`Z80::is_reading`], [`Z80::is_writing`] and
    /// [`Z80::is_acknowledging_interrupt`] read `false`, whatever the original's
    /// last T-state drove.
    #[must_use]
    pub fn at_boundary(regs: Registers) -> Self {
        Self {
            regs,
            ..Self::new()
        }
    }

    /// The register file as it stands, which is the whole of the introspection
    /// this crate offers. There is no assembler, disassembler or debugger
    /// surface.
    ///
    /// An instruction's last machine cycle reaches the registers only with the
    /// tick after it: at [`Z80::is_at_instruction_boundary`] they still stand
    /// before that cycle's work (the opcode not yet consumed, a read's byte not
    /// yet loaded). [`Z80::settle`] finishes it, after which they read the
    /// boundary and `Z80::at_boundary(cpu.registers())` runs on as the part
    /// does.
    #[must_use]
    pub const fn registers(&self) -> Registers {
        self.regs
    }

    /// Whether the instruction before the next tick has run its last machine
    /// cycle, and the next tick begins the opcode fetch of a new one or
    /// accepts `NMI` or `INT` in its place.
    ///
    /// A read that ends that last cycle still takes its byte with the next
    /// tick, so a register it loads stands in [`Z80::registers`] only after it,
    /// and an `NMI` edge on the next tick's pins is latched only then;
    /// [`Z80::settle`] takes both. `false` while `RESET` is held and while the
    /// buses are released.
    #[must_use]
    pub fn is_at_instruction_boundary(&self) -> bool {
        if self.reset_held != 0 {
            return false;
        }
        match self.state {
            State::Starting => true,
            State::BusReleased(_) => false,
            State::Running => {
                if !self.seq.on_last_t() {
                    return false;
                }
                let mut ahead = self.clone();
                ahead.stop_at_boundary = true;
                let byte = ahead.seq.sampled();
                let next = ahead.next_cycle(byte);
                ahead.ends_instruction(next)
            }
        }
    }

    /// Whether `next`, decided as a machine cycle ended, is the opcode fetch
    /// of a new instruction.
    fn ends_instruction(&self, next: BusOp) -> bool {
        matches!(next, BusOp::Fetch { .. })
            && self.stage == Stage::Opcode
            && self.index == Index::None
    }

    /// Finishes the instruction whose last machine cycle has just been driven,
    /// without starting the next, so [`Z80::registers`] reads the boundary.
    ///
    /// `inp` is what the next [`Z80::tick`] will be given: its `d` is the
    /// last read's byte, and an `NMI` edge on it is latched as it would be
    /// there, into [`Registers::nmi_pending`], so the next tick accepts it in
    /// place of the fetch; an `INT` the instruction's end accepts is
    /// [`Pending::Int`]. The part then runs on as
    /// `Z80::at_boundary(self.registers())` does, and ticking on with the same
    /// pins runs as if `settle` had not been called.
    ///
    /// Returns whether the part now stands at such a boundary: `false`,
    /// changing nothing, where [`Z80::is_at_instruction_boundary`] is `false`;
    /// on a part that has not ticked since it was built or reset when `inp`
    /// carries an `NMI` edge, which that first tick takes only at the next
    /// boundary; and where `inp` carries [`Ctrl::RESET`], since a pulse shorter than
    /// three clocks does not take (`Z80-010`) and the part then finishes the
    /// instruction with the byte presented as `RESET` is released, which no
    /// rebuild can know. A part is rebuilt at a later boundary instead.
    /// Idempotent.
    #[must_use = "a part rebuilt where `settle` returned `false` is rebuilt off a boundary"]
    pub fn settle(&mut self, inp: Pins) -> bool {
        if !self.is_at_instruction_boundary() || inp.ctrl.contains(Ctrl::RESET) {
            return false;
        }
        if self.state == State::Starting {
            return !(inp.ctrl.contains(Ctrl::NMI) && self.regs.nmi_n.is_high());
        }
        let mut ahead = self.clone();
        ahead.catch_nmi(inp);
        ahead.stop_at_boundary = true;
        let Some(byte) = ahead.seq.end_t(inp) else {
            return false;
        };
        let next = ahead.next_cycle(byte);
        if !ahead.ends_instruction(next) {
            return false;
        }
        ahead.stop_at_boundary = false;
        ahead.state = State::Starting;
        *self = ahead;
        true
    }

    /// Latches an `NMI` edge on `inp`, which `Z80-066` makes edge-triggered,
    /// unless `RESET` is held with it.
    const fn catch_nmi(&mut self, inp: Pins) {
        let nmi = inp.ctrl.contains(Ctrl::NMI);
        let reset = inp.ctrl.contains(Ctrl::RESET);
        self.regs.nmi_pending |= nmi && self.regs.nmi_n.is_high() && !reset;
        self.regs.nmi_n = Level::from_bool(!nmi);
    }

    /// The part has executed a `HALT` and is re-fetching it until an interrupt
    /// or a reset (`Z80-079`, `Z80-080`); only where
    /// [`Z80::is_at_instruction_boundary`] holds, so not inside a re-fetch, under
    /// `RESET` or while the buses are released.
    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.regs.halted && self.is_at_instruction_boundary()
    }

    /// The part has given the bus up to a device holding `BUSREQ`: the
    /// T-state [`Z80::tick`] last drove asserts [`Ctrl::BUSACK`], and the
    /// address, data and three-state control lines float (`Z80-008`). False
    /// before the first tick and while `RESET` is held.
    #[must_use]
    pub const fn is_released(&self) -> bool {
        matches!(self.state, State::BusReleased(_)) && self.reset_held == 0
    }

    /// `RESET` was held at the last tick, the buses floating and no control
    /// output asserted (`Z80-011`), whether or not it has been held long
    /// enough to complete (`Z80-010`). False on a part [`Z80::new`] or
    /// [`Z80::at_boundary`] builds, which stands as a reset leaves it.
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.reset_held != 0
    }

    /// Advances one T-state and returns the pins the part drives during it.
    ///
    /// `inp` is what the outside world presents: the five input signals, and
    /// `d` when the pins returned by the previous call were reading. The
    /// caller resolves memory and I/O by asking [`Z80::is_reading`] and
    /// [`Z80::is_writing`] and reading the pins' `MREQ` and `IORQ`; there is no callback, because a bus resolved through one
    /// is a machine leaking into the chip.
    #[must_use]
    pub fn tick(&mut self, inp: Pins) -> Pins {
        let carried = self.regs.nmi_pending;
        self.catch_nmi(inp);
        let reset = inp.ctrl.contains(Ctrl::RESET);

        if reset {
            self.reset_held = self.reset_held.saturating_add(1);
            if self.reset_held == RESET_CLOCKS {
                let mut regs = self.regs;
                regs.reset();
                regs.nmi_pending = false;
                regs.pending = None;
                regs.halted = false;
                *self = Self {
                    reset_held: RESET_CLOCKS,
                    ..Self::at_boundary(regs)
                };
            }
            return Pins::idle();
        }
        self.reset_held = 0;

        if self.state == State::Starting {
            self.state = State::Running;
            let fetch = self.opcode_fetch();
            let op = if carried || self.regs.pending == Some(Pending::Int) {
                self.after_boundary(fetch)
            } else {
                fetch
            };
            if self.regs.pending == Some(Pending::Busreq) {
                self.regs.pending = None;
                self.state = State::BusReleased(op);
                return self.released_pins();
            }
            self.seq.start(op);
            return self.shown();
        }

        if let State::BusReleased(deferred) = self.state {
            if inp.ctrl.contains(Ctrl::BUSREQ) {
                return self.released_pins();
            }
            self.state = State::Running;
            self.seq.start(deferred);
        } else if let Some(next) = self.seq.end_t(inp).map(|byte| self.next_cycle(byte)) {
            if self.regs.pending == Some(Pending::Busreq) {
                self.regs.pending = None;
                self.state = State::BusReleased(next);
                return self.released_pins();
            }
            self.seq.start(next);
        }
        if self.seq.on_last_t() {
            self.regs.pending = inp.ctrl.contains(Ctrl::BUSREQ).then_some(Pending::Busreq);
            self.int_line = inp.ctrl.contains(Ctrl::INT);
        }
        self.shown()
    }

    /// The pins the cycle in progress drives now.
    fn shown(&self) -> Pins {
        self.with_halt(self.seq.drive())
    }

    /// The pins while the buses are released: `BUSACK`, and `HALT`, which is
    /// not a three-state output ([`TRISTATE_CONTROLS`]), as a halt drives it.
    fn released_pins(&self) -> Pins {
        self.with_halt(Pins::idle().with(Ctrl::BUSACK))
    }

    /// `pins`, with `HALT` asserted while the part is halted.
    fn with_halt(&self, pins: Pins) -> Pins {
        if self.regs.halted {
            pins.with(Ctrl::HALT)
        } else {
            pins
        }
    }

    /// Whether the T-state [`Z80::tick`] last drove is the first of a machine
    /// cycle.
    ///
    /// Not a pin, but the edge a pin would show: on the part `MREQ` rises
    /// between two back-to-back cycles and falls again half a clock into T1
    /// (`Z80-013`), a transition inside a T-state that the pins here cannot
    /// carry (`Z80-OPEN-02`). A bus adapter that asks its memory once per
    /// cycle reads it to tell two reads of one address apart.
    #[must_use]
    pub const fn is_opening_machine_cycle(&self) -> bool {
        self.seq.opening()
    }

    /// The control outputs the T-state [`Z80::tick`] last drove asserts,
    /// `HALT` aside: none while the buses are released or `RESET` is held.
    fn asserted(&self) -> Ctrl {
        match self.state {
            State::Running if self.reset_held == 0 => self.seq.drive().ctrl,
            _ => Ctrl::NONE,
        }
    }

    /// The T-state [`Z80::tick`] last drove reads, `RD` with `MREQ` or `IORQ`:
    /// the board places the byte at [`Pins::address`] into [`Pins::d`] before
    /// the next tick. The pins' [`Ctrl::MREQ`] or [`Ctrl::IORQ`] says whether
    /// memory or an I/O port is read.
    ///
    /// An interrupt acknowledge asserts `IORQ` but never `RD` (`Z80-071`), so
    /// it does not answer here.
    #[must_use]
    pub fn is_reading(&self) -> bool {
        Self::accesses(self.asserted(), Ctrl::RD)
    }

    /// The T-state [`Z80::tick`] last drove writes [`Pins::d`] to the
    /// location on [`Pins::address`], `WR` with `MREQ` or `IORQ`; the pins'
    /// [`Ctrl::MREQ`] or [`Ctrl::IORQ`] says whether memory or an I/O port.
    #[must_use]
    pub fn is_writing(&self) -> bool {
        Self::accesses(self.asserted(), Ctrl::WR)
    }

    /// Whether `ctrl` asserts the strobe `rw` with a memory or I/O request.
    fn accesses(ctrl: Ctrl, rw: Ctrl) -> bool {
        ctrl.contains(Ctrl::MREQ | rw) || ctrl.contains(Ctrl::IORQ | rw)
    }

    /// The T-state [`Z80::tick`] last drove acknowledges an interrupt, `M1`
    /// with `IORQ`, which no ordinary cycle asserts together (`Z80-071`): the
    /// board places the vector byte, or the instruction in mode 0, into
    /// [`Pins::d`] before the next tick. In mode 0 only a single-byte `RST`
    /// is executed; for any other byte the part stacks `PC` and carries on
    /// from it (`Z80-073`).
    #[must_use]
    pub fn is_acknowledging_interrupt(&self) -> bool {
        self.asserted().contains(Ctrl::M1 | Ctrl::IORQ)
    }

    /// The opcode fetch that starts the next instruction.
    const fn opcode_fetch(&self) -> BusOp {
        BusOp::Fetch {
            pc: self.regs.pc,
            refresh: self.regs.refresh_address(),
        }
    }
}

impl Default for Z80 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Ctrl, InterruptMode, Pending, Pins, Registers, Z80};
    use std::vec::Vec;

    /// Runs `ticks` T-states, presenting whatever `input` gives for each, and
    /// returns the pins the part drove.
    fn run(cpu: &mut Z80, ticks: usize, mut input: impl FnMut(usize) -> Pins) -> Vec<Pins> {
        (0..ticks).map(|t| cpu.tick(input(t))).collect()
    }

    /// The outside world holding every input line quiet and every read
    /// answering with `NOP`.
    fn quiet(_: usize) -> Pins {
        Pins::idle()
    }

    #[test]
    fn is_released_shows_busack_and_is_in_reset_follows_the_pin() {
        let mut cpu = Z80::new();
        assert!(!cpu.is_released() && !cpu.is_in_reset());
        let mut released = false;
        for _ in 0..8 {
            let out = cpu.tick(Pins::idle().with(Ctrl::BUSREQ));
            assert_eq!(cpu.is_released(), out.ctrl.contains(Ctrl::BUSACK));
            released |= cpu.is_released();
        }
        assert!(released, "BUSREQ never released the buses");
        let _ = cpu.tick(Pins::idle().with(Ctrl::BUSREQ | Ctrl::RESET));
        assert!(cpu.is_in_reset() && !cpu.is_released());
        for _ in 0..3 {
            let _ = cpu.tick(Pins::idle().with(Ctrl::RESET));
        }
        assert!(cpu.is_in_reset());
        let _ = cpu.tick(Pins::idle());
        assert!(!cpu.is_in_reset() && !cpu.is_released());
    }

    #[test]
    fn a_boundary_stands_before_each_instruction_and_not_inside_one() {
        let program = [0x00, 0x3E, 0x12, 0xDD, 0x21, 0x34, 0x12, 0xCB, 0x00, 0x76];
        let mut cpu = Z80::new();
        let mut inp = Pins::idle();
        let mut boundaries = Vec::new();
        for t in 0..36 {
            if cpu.is_at_instruction_boundary() {
                boundaries.push(t);
            }
            let out = cpu.tick(inp);
            inp = Pins::idle();
            if cpu.is_reading() && out.ctrl.contains(Ctrl::MREQ) {
                inp = inp.with_d(
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
        assert_eq!(boundaries, [0, 4, 11, 25, 33]);
        assert_eq!(cpu.registers().ix, 0x1234);
    }

    #[test]
    fn the_first_cycle_is_an_opcode_fetch_at_the_program_counter() {
        let mut cpu = Z80::new();
        let driven = run(&mut cpu, 4, quiet);
        assert!(driven[0].ctrl.contains(Ctrl::M1));
        assert_eq!(driven[0].address, Some(crate::Address::Stable(0x0000)));
        assert!(cpu.is_reading() == driven[3].ctrl.contains(Ctrl::MREQ | Ctrl::RD));
        assert!(driven[0].ctrl.contains(Ctrl::MREQ | Ctrl::RD));
    }

    #[test]
    fn one_fetch_takes_four_t_states_and_moves_pc_by_one() {
        let mut cpu = Z80::new();
        run(&mut cpu, 4, quiet);
        assert_eq!(
            cpu.regs.pc, 0x0000,
            "PC still points at the byte being fetched"
        );
        let driven = run(&mut cpu, 1, quiet);
        assert_eq!(cpu.regs.pc, 0x0001);
        assert!(
            driven[0].ctrl.contains(Ctrl::M1),
            "and the next fetch starts at once"
        );
        assert_eq!(driven[0].address, Some(crate::Address::Stable(0x0001)));
    }

    #[test]
    fn every_fetch_increments_r_once() {
        let mut cpu = Z80::new();
        cpu.regs.r = 0x7E;
        run(&mut cpu, 4 * 3 + 1, quiet);
        assert_eq!(cpu.regs.r, 0x01);
    }

    #[test]
    fn the_refresh_address_carries_i_and_the_pre_increment_r() {
        let mut cpu = Z80::new();
        cpu.regs.i = 0x4B;
        cpu.regs.r = 0x07;
        let driven = run(&mut cpu, 8, quiet);
        assert_eq!(driven[2].address, Some(crate::Address::Stable(0x4B07)));
        assert_eq!(driven[3].address, Some(crate::Address::Stable(0x4B07)));
        assert!(driven[2].ctrl.contains(Ctrl::RFSH));
        assert_eq!(driven[6].address, Some(crate::Address::Stable(0x4B08)));
    }

    #[test]
    fn a_reset_held_three_clocks_takes() {
        let mut cpu = Z80::new();
        cpu.regs.pc = 0x1234;
        cpu.regs.i = 0x55;
        cpu.regs.r = 0x66;
        cpu.regs.iff1 = true;
        cpu.regs.iff2 = true;
        cpu.regs.im = InterruptMode::Im2;
        cpu.regs.set_bc(0xBEEF);

        run(&mut cpu, 3, |_| Pins::idle().with(Ctrl::RESET));
        assert_eq!(cpu.regs.pc, 0);
        assert_eq!(cpu.regs.i, 0);
        assert_eq!(cpu.regs.r, 0);
        assert!(!cpu.regs.iff1);
        assert!(!cpu.regs.iff2);
        assert_eq!(cpu.regs.im, InterruptMode::Im0);
        assert_eq!(cpu.regs.bc(), 0xBEEF, "and nothing Z80-012 does not name");
    }

    #[test]
    fn a_reset_held_two_clocks_does_not() {
        let mut cpu = Z80::new();
        cpu.regs.pc = 0x1234;
        run(&mut cpu, 2, |_| Pins::idle().with(Ctrl::RESET));
        assert_eq!(cpu.regs.pc, 0x1234);
    }

    #[test]
    fn an_nmi_edge_under_reset_is_not_taken_on_release() {
        let mut cpu = Z80::new();
        run(&mut cpu, 1, |_| Pins::idle().with(Ctrl::NMI));
        run(&mut cpu, 1, quiet);
        assert!(
            cpu.regs.nmi_pending,
            "an edge before the reset is latched as ever"
        );
        run(&mut cpu, 6, |t| {
            let pins = Pins::idle().with(Ctrl::RESET);
            if t % 2 == 0 {
                pins.with(Ctrl::NMI)
            } else {
                pins
            }
        });
        assert!(!cpu.regs.nmi_pending);
        let driven = run(&mut cpu, 8, quiet);
        assert_eq!(driven[0].address, Some(crate::Address::Stable(0x0000)));
        assert!(driven[4].ctrl.contains(Ctrl::M1));
        assert_eq!(
            driven[4].address,
            Some(crate::Address::Stable(0x0001)),
            "the next opcode, no `NMI` taken"
        );
    }

    #[test]
    fn a_short_reset_keeps_a_latched_nmi_edge() {
        let mut cpu = Z80::new();
        run(&mut cpu, 1, |_| Pins::idle().with(Ctrl::NMI));
        run(&mut cpu, 2, |_| Pins::idle().with(Ctrl::RESET));
        assert!(cpu.regs.nmi_pending);
    }

    #[test]
    fn a_short_reset_keeps_power_up_and_a_granted_bus() {
        let mut cpu = Z80::new();
        run(&mut cpu, 1, |_| Pins::idle().with(Ctrl::RESET));
        let driven = run(&mut cpu, 1, quiet);
        assert!(driven[0].ctrl.contains(Ctrl::M1));
        assert_eq!(driven[0].address, Some(crate::Address::Stable(0x0000)));

        let mut cpu = Z80::new();
        let busreq = || Pins::idle().with(Ctrl::BUSREQ);
        run(&mut cpu, 12, |_| busreq());
        assert!(
            run(&mut cpu, 1, |_| busreq())[0]
                .ctrl
                .contains(Ctrl::BUSACK)
        );
        run(&mut cpu, 1, |_| busreq().with(Ctrl::RESET));
        assert!(
            run(&mut cpu, 1, |_| busreq())[0]
                .ctrl
                .contains(Ctrl::BUSACK)
        );
    }

    #[test]
    fn halt_stays_driven_while_the_buses_are_released() {
        let mut cpu = Z80::at_boundary(Registers {
            halted: true,
            ..Registers::default()
        });
        run(&mut cpu, 8, quiet);
        let driven = run(&mut cpu, 12, |_| Pins::idle().with(Ctrl::BUSREQ));
        let released = driven.last().expect("twelve ticks");
        assert!(released.ctrl.contains(Ctrl::BUSACK | Ctrl::HALT));
        assert!(!cpu.is_halted());
    }

    #[test]
    fn a_boundary_stands_before_int_is_accepted() {
        let mut cpu = Z80::at_boundary(Registers {
            iff1: true,
            iff2: true,
            im: InterruptMode::Im1,
            ..Registers::default()
        });
        let int = |_| Pins::idle().with(Ctrl::INT).with_d(0x00);
        let mut accepting = None;
        for t in 0..64 {
            let next = int(t);
            let mut settled = cpu.clone();
            if settled.settle(next) && settled.registers().pending == Some(Pending::Int) {
                accepting = Some(settled);
                break;
            }
            let _ = cpu.tick(next);
        }
        let mut settled = accepting.expect("a boundary before INT is accepted");
        let mut rebuilt = Z80::at_boundary(settled.registers());
        for t in 0..16 {
            assert_eq!(settled.tick(int(t)), rebuilt.tick(int(t)), "tick {t}");
        }
        assert!(!rebuilt.registers().iff1, "Z80-072");
    }

    #[test]
    fn the_pins_are_quiet_while_reset_is_held() {
        let mut cpu = Z80::new();
        let driven = run(&mut cpu, 5, |_| Pins::idle().with(Ctrl::RESET));
        for (t, pins) in driven.iter().enumerate() {
            assert_eq!(*pins, Pins::idle(), "clock {t}");
        }
    }

    #[test]
    fn the_first_fetch_after_a_reset_is_at_zero() {
        let mut cpu = Z80::new();
        cpu.regs.pc = 0x1234;
        cpu.regs.i = 0x77;
        run(&mut cpu, 3, |_| Pins::idle().with(Ctrl::RESET));
        let driven = run(&mut cpu, 4, quiet);
        assert!(driven[0].ctrl.contains(Ctrl::M1));
        assert_eq!(driven[0].address, Some(crate::Address::Stable(0x0000)));
        assert_eq!(
            driven[2].address,
            Some(crate::Address::Stable(0x0000)),
            "and `I` was cleared with it"
        );
    }

    #[test]
    fn a_reset_abandons_the_instruction_in_flight() {
        let program = [0x3A, 0x10, 0x40, 0x3C, 0x32, 0x10, 0x40, 0x18, 0xFE];
        for start in 100..112 {
            let mut ram = std::vec![0u8; 0x1_0000];
            ram[..program.len()].copy_from_slice(&program);
            let mut cpu = Z80::new();
            let mut driven = Pins::idle();
            for t in 0..400 {
                let mut inp = Pins::idle();
                if cpu.is_reading() && driven.ctrl.contains(Ctrl::MREQ) {
                    inp = inp.with_d(
                        ram[usize::from(
                            driven
                                .address
                                .and_then(crate::Address::stable)
                                .expect("the cycle drives an address"),
                        )],
                    );
                }
                if cpu.is_writing() && driven.ctrl.contains(Ctrl::MREQ) {
                    ram[usize::from(
                        driven
                            .address
                            .and_then(crate::Address::stable)
                            .expect("the cycle drives an address"),
                    )] = driven.d.expect("a write drives d");
                }
                if (start..start + 10).contains(&t) {
                    inp = inp.with(Ctrl::RESET);
                }
                driven = cpu.tick(inp);
            }
            assert_eq!(ram[0x4010], 2, "reset from T-state {start}");
        }
    }

    #[test]
    fn a_bus_request_is_answered_at_the_end_of_the_machine_cycle() {
        let mut cpu = Z80::new();
        let driven = run(&mut cpu, 6, |tick| {
            let pins = Pins::idle();
            if tick >= 3 {
                pins.with(Ctrl::BUSREQ)
            } else {
                pins
            }
        });
        for (t, pins) in driven.iter().take(4).enumerate() {
            assert!(
                !pins.ctrl.contains(Ctrl::BUSACK),
                "the fetch finishes first, T{}",
                t + 1
            );
        }
        for pins in &driven[4..] {
            assert!(pins.ctrl.contains(Ctrl::BUSACK));
            assert!(
                !pins.ctrl.intersects(super::TRISTATE_CONTROLS),
                "and the bus is released"
            );
            assert_eq!(pins.address, None);
        }
    }

    #[test]
    fn a_request_raised_too_late_waits_for_the_next_cycle() {
        let mut cpu = Z80::new();
        let driven = run(&mut cpu, 9, |tick| {
            let pins = Pins::idle();
            if tick == 4 {
                pins.with(Ctrl::BUSREQ)
            } else {
                pins
            }
        });
        assert!(
            !driven[4].ctrl.contains(Ctrl::BUSACK),
            "missed this cycle's edge"
        );
        assert!(
            !driven[8].ctrl.contains(Ctrl::BUSACK),
            "and was gone by the next"
        );
    }

    #[test]
    fn the_bus_comes_back_when_the_request_drops() {
        let mut cpu = Z80::new();
        run(&mut cpu, 5, |tick| {
            let pins = Pins::idle();
            if tick >= 3 {
                pins.with(Ctrl::BUSREQ)
            } else {
                pins
            }
        });
        let driven = run(&mut cpu, 1, quiet);
        assert!(!driven[0].ctrl.contains(Ctrl::BUSACK));
        assert!(
            driven[0].ctrl.contains(Ctrl::M1),
            "the deferred fetch begins"
        );
        assert_eq!(driven[0].address, Some(crate::Address::Stable(0x0001)));
    }

    #[test]
    fn a_fork_runs_independently() {
        let mut cpu = Z80::new();
        run(&mut cpu, 4, quiet);
        let mut fork = cpu.clone();
        run(&mut cpu, 4, quiet);
        assert_ne!(cpu, fork);
        run(&mut fork, 4, quiet);
        assert_eq!(cpu, fork);
    }

    /// `LD A,42h; LD SP,8000h; EI; NOP; CALL 0010h; ...`, with `INT` held from
    /// the start in mode 1, so the interrupt lands one instruction after `EI`
    /// (`Z80-076`). Each tick: the pins driven, and the part after it.
    const EI_PROGRAM: [u8; 9] = [0x3E, 0x42, 0x31, 0x00, 0x80, 0xFB, 0x00, 0xCD, 0x10];

    /// Runs `ticks` T-states against [`EI_PROGRAM`] with `INT` held, calling
    /// `at_boundary` before each tick that begins an instruction with the pins
    /// that tick will be given, and returns the pins driven.
    fn run_ei_program(
        ticks: usize,
        mut at_boundary: impl FnMut(&mut Z80, usize, Pins),
    ) -> Vec<Pins> {
        let mut regs = super::Registers::power_on();
        regs.im = InterruptMode::Im1;
        let mut cpu = Z80::at_boundary(regs);
        let mut ram = std::vec![0u8; 0x1_0000];
        ram[..EI_PROGRAM.len()].copy_from_slice(&EI_PROGRAM);
        let mut driven = Pins::idle();
        let mut boundary = 0;
        let mut out = Vec::new();
        for _ in 0..ticks {
            let mut inp = Pins::idle().with(Ctrl::INT);
            if cpu.is_reading() && driven.ctrl.contains(Ctrl::MREQ) {
                inp = inp.with_d(
                    ram[usize::from(
                        driven
                            .address
                            .and_then(crate::Address::stable)
                            .expect("the cycle drives an address"),
                    )],
                );
            }
            if cpu.is_writing() && driven.ctrl.contains(Ctrl::MREQ) {
                ram[usize::from(
                    driven
                        .address
                        .and_then(crate::Address::stable)
                        .expect("the cycle drives an address"),
                )] = driven.d.expect("a write drives d");
            }
            if cpu.is_at_instruction_boundary() {
                at_boundary(&mut cpu, boundary, inp);
                boundary += 1;
            }
            driven = cpu.tick(inp);
            out.push(driven);
        }
        out
    }

    #[test]
    fn settling_changes_nothing_that_follows() {
        let plain = run_ei_program(120, |_, _, _| {});
        let settled = run_ei_program(120, |cpu, _, inp| {
            let _ = cpu.settle(inp);
            let once = cpu.clone();
            let _ = cpu.settle(inp);
            assert_eq!(*cpu, once, "settle is idempotent");
        });
        assert_eq!(plain, settled);
    }

    #[test]
    fn a_settled_round_trip_runs_on_as_the_part_does() {
        let plain = run_ei_program(120, |_, _, _| {});
        let acks = plain
            .iter()
            .filter(|p| p.ctrl.contains(Ctrl::M1 | Ctrl::IORQ))
            .count();
        assert!(acks > 0, "the program is interrupted");
        for at in 1..6 {
            let restored = run_ei_program(120, |cpu, boundary, inp| {
                if boundary == at {
                    assert!(cpu.settle(inp), "boundary {at} settles");
                    *cpu = Z80::at_boundary(cpu.registers());
                }
            });
            assert_eq!(plain, restored, "round trip at boundary {at}");
        }
    }

    /// `LD A,42h` from power-up, with `NMI` asserted from the tick after its
    /// boundary, `PUSH AF` at `0066h`; `rebuild` settles and rebuilds the
    /// part at that boundary.
    fn run_nmi_after_boundary(rebuild: bool) -> Vec<Pins> {
        let mut ram = std::vec![0u8; 0x1_0000];
        ram[..2].copy_from_slice(&[0x3E, 0x42]);
        ram[0x66] = 0xF5;
        let mut cpu = Z80::new();
        let mut driven = Pins::idle();
        let mut out = Vec::new();
        for t in 0..40 {
            let mut inp = Pins::idle();
            if (7..10).contains(&t) {
                inp = inp.with(Ctrl::NMI);
            }
            if cpu.is_reading() && driven.ctrl.contains(Ctrl::MREQ) {
                inp = inp.with_d(
                    ram[usize::from(
                        driven
                            .address
                            .and_then(crate::Address::stable)
                            .expect("the cycle drives an address"),
                    )],
                );
            }
            if cpu.is_writing() && driven.ctrl.contains(Ctrl::MREQ) {
                ram[usize::from(
                    driven
                        .address
                        .and_then(crate::Address::stable)
                        .expect("the cycle drives an address"),
                )] = driven.d.expect("a write drives d");
            }
            if t == 7 && rebuild {
                assert!(cpu.is_at_instruction_boundary());
                assert!(cpu.settle(inp));
                let regs = cpu.registers();
                assert_eq!((regs.a, regs.nmi_pending), (0x42, true));
                cpu = Z80::at_boundary(regs);
            }
            driven = cpu.tick(inp);
            out.push(driven);
        }
        out
    }

    #[test]
    fn an_nmi_edge_after_a_boundary_is_settled_into_the_registers() {
        let plain = run_nmi_after_boundary(false);
        assert!(
            plain
                .iter()
                .any(|p| p.ctrl.contains(Ctrl::MREQ | Ctrl::WR) && p.d == Some(0x42)),
            "PUSH AF in the handler writes A"
        );
        assert_eq!(plain, run_nmi_after_boundary(true));
    }
}
