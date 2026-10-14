//! The Zilog Z80 CTC counter/timer circuit, emulated at its pins.
//!
//! This crate's dependencies are `units`, for the sheet's times, and `signal`,
//! for a pin's [`Level`]; it allocates
//! nothing, does no I/O and is `no_std`: a [`Z80Ctc`] is a plain value that
//! advances one period of its `CLK` input at a time, sees its input pins as
//! [`Inputs`] and drives [`Outputs`].
//!
//! # Examples
//!
//! A timer interrupting every 32 clocks:
//!
//! ```
//! use z80ctc::control::{CONTROL, INTERRUPT, TIME_CONSTANT};
//! use z80ctc::{Inputs, Select, Z80Ctc};
//!
//! let mut ctc = Z80Ctc::new();
//! // An I/O write to channel 0 is IORQ and CE Low with RD High, for the
//! // clocks the CPU holds them; the part takes the byte as the cycle ends.
//! let write = |ctc: &mut Z80Ctc, d: u8| {
//!     for _ in 0..3 {
//!         let _ = ctc.tick(Inputs::write_access(Select::new(0), d));
//!     }
//!     let _ = ctc.tick(Inputs::default());
//! };
//! write(&mut ctc, 0x10); // the vector: 10h
//! write(&mut ctc, INTERRUPT | TIME_CONSTANT | CONTROL); // timer, prescaler 16
//! write(&mut ctc, 2); // two prescaler periods: 32 clocks
//! let mut clocks = 0;
//! while ctc.tick(Inputs::default()).int_n.is_high() {
//!     clocks += 1;
//! }
//! // Z80CTC-020, Z80CTC-019, Z80CTC-023: the prescaler counts from the
//! // clock after the write was taken, the 32nd clock is the zero count, and
//! // INT falls with the clock after it.
//! assert_eq!(clocks, 32);
//! ```
//!
//! # One tick is one `CLK` period
//!
//! [`Z80Ctc::tick`] takes the levels on the input pins during one period of
//! the system clock, as they stand at the rising edge that begins it, and
//! returns what the part drives during that period. An input that changed
//! before that edge is taken to have met its setup time to it; one that
//! misses it is presented with the next tick, which is the one-clock delay
//! the part shows for a late edge (`Z80CTC-015`, `Z80CTC-021`). So a Z80
//! stepped one T-state at a time and this part ticked once per T-state, on
//! the pins the CPU drove in it, share one clock. Parts in one daisy chain
//! are ticked in chain order, each given the `IEO` the part above it
//! returned for the same tick as its `IEI`.
//!
//! The part sees the CPU's bus cycles by their control lines and nothing
//! else: an I/O write is taken when the run of clocks that showed it ends,
//! an I/O read is answered from the clock that begins it, an interrupt
//! acknowledge is answered while `M1` and `IORQ` are both Low, and an opcode
//! fetch (`M1` and `RD` Low) is decoded for `RETI` from the byte on the bus
//! at its last clock.
//!
//! # Power-up
//!
//! The part's state at power-on is undefined (`Z80CTC-034`). [`Z80Ctc::new`]
//! starts it in its reset state instead, as a Low on `RESET` leaves it
//! (`Z80CTC-032`): every channel stopped with its interrupt disabled, nothing
//! requested; and with every register `00h`, the vector, each control word,
//! time constant and down-counter. A board reaches the first half by holding
//! `RESET` Low for three clocks (`Z80CTC-033`); `RESET` keeps the registers,
//! so the real part's are what power-on left until the program writes them.
//!
//! # What this crate chooses
//!
//! Each an open item: the down-counter moves with the prescaler's last count
//! (`Z80CTC-OPEN-01`); a software reset leaves an interrupt requested or under
//! service alone (`Z80CTC-OPEN-02`); a vector word to channel 1, 2 or 3 is
//! ignored (`Z80CTC-OPEN-03`); clearing bit 7 keeps a pending request
//! (`Z80CTC-OPEN-04`); power-up is the reset state (`Z80CTC-OPEN-05`); a
//! counter counts edges from the clock after its time constant is taken
//! (`Z80CTC-OPEN-06`); the under-service latch sets at the acknowledge's first
//! clock (`Z80CTC-OPEN-07`); a read returns the down-counter after a decrement
//! at T2's edge (`Z80CTC-OPEN-09`); a control word with bit 3 set and no time
//! constant leaves a stopped channel stopped (`Z80CTC-OPEN-10`); and the served
//! channel is released as the `4Dh` fetch ends, `IEO` rising with the clock
//! after (`Z80CTC-OPEN-11`).

#![no_std]

#[cfg(test)]
extern crate std;

mod channel;

use channel::{Chan, Run};

/// How many pins the package has (`Z80CTC-001`).
pub const PIN_COUNT: usize = 28;

/// The 28-pin package's pin names, by pin (`Z80CTC-001`). An active-Low pin
/// is written with a trailing `_N`.
pub const PIN_NAMES: [&str; PIN_COUNT] = [
    "D4", "D5", "D6", "D7", "GND", "RD_N", "ZC/TO0", "ZC/TO1", "ZC/TO2", "IORQ_N", "IEO", "INT_N",
    "IEI", "M1_N", "CLK", "CE_N", "RESET_N", "CS0", "CS1", "CLK/TRG3", "CLK/TRG2", "CLK/TRG1",
    "CLK/TRG0", "+5V", "D0", "D1", "D2", "D3",
];

/// How many counter/timer channels the part has (`Z80CTC-026`).
pub const CHANNEL_COUNT: usize = 4;

/// A channel control word's bits (`Z80CTC-007`).
pub mod control {
    /// Control word bit 7: a zero count requests an interrupt (`Z80CTC-007`,
    /// `Z80CTC-023`).
    pub const INTERRUPT: u8 = 0x80;
    /// Control word bit 6: counter mode, set; timer mode, clear (`Z80CTC-007`).
    pub const COUNTER: u8 = 0x40;
    /// Control word bit 5: the timer's prescaler divides by 256, set, or 16,
    /// clear (`Z80CTC-019`).
    pub const PRESCALE_256: u8 = 0x20;
    /// Control word bit 4: the active edge of `CLK/TRG` is rising, set, or
    /// falling, clear (`Z80CTC-015`).
    pub const RISING_EDGE: u8 = 0x10;
    /// Control word bit 3: a timer waits for an edge on `CLK/TRG`, set, or starts
    /// when its time constant is loaded, clear (`Z80CTC-020`, `Z80CTC-021`).
    pub const TRIGGER: u8 = 0x08;
    /// Control word bit 2: the next word written to the channel is its time
    /// constant (`Z80CTC-008`).
    pub const TIME_CONSTANT: u8 = 0x04;
    /// Control word bit 1: the channel stops (`Z80CTC-011`).
    pub const SOFTWARE_RESET: u8 = 0x02;
    /// Bit 0: the word is a control word, set, or the interrupt vector, clear
    /// (`Z80CTC-007`, `Z80CTC-010`).
    pub const CONTROL: u8 = 0x01;
}

/// The interrupt vector word's fields (`Z80CTC-010`).
pub mod interrupt_vector {
    /// `V7`–`V3`, the bits the CPU writes; the part supplies the rest
    /// (`Z80CTC-010`).
    pub const V_MASK: u8 = 0xf8;
}

use control::INTERRUPT;
use units::Picoseconds;

/// A pin's level, as [`Inputs`] and [`Outputs`] hold it.
pub use signal::Level;

/// The shortest `RESET` Low that the part is specified to take, in clocks
/// (`Z80CTC-033`).
pub const RESET_N_MIN_CYCLES: usize = 3;
/// `TcCTR`, the shortest period of a `CLK/TRG` input in counter mode, in
/// clocks (`Z80CTC-037`).
pub const TCCTR_MIN_CYCLES: usize = 2;

/// The two NMOS speed grades (`Z80CTC-035`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Grade {
    /// 4 MHz.
    Z0843004,
    /// 6 MHz, the grade a 6.144 MHz CPU clock takes.
    Z0843006,
}

impl Grade {
    /// `TcC`, the shortest clock period (`Z80CTC-035`).
    #[must_use]
    pub const fn tcc_min_ps(self) -> Picoseconds {
        match self {
            Self::Z0843004 => Picoseconds(250_000),
            Self::Z0843006 => Picoseconds(162_000),
        }
    }

    /// `TwCh` and `TwCl`, one figure: the longest the clock may stay High,
    /// and Low (`Z80CTC-035`).
    #[must_use]
    pub const fn twc_max_ps(self) -> Picoseconds {
        Picoseconds(2_000_000)
    }

    /// `TsCTR(Cs)` and `TsCTR(Ct)`, one figure: how long before a rising
    /// clock edge a `CLK/TRG` edge must arrive for that edge to take it: a
    /// count in counter mode, a start in timer mode (`Z80CTC-022`).
    #[must_use]
    pub const fn tsctr_min_ps(self) -> Picoseconds {
        match self {
            Self::Z0843004 => Picoseconds(210_000),
            Self::Z0843006 => Picoseconds(150_000),
        }
    }

    /// `TwCTRh` and `TwCTRl`, one figure: the shortest `CLK/TRG` High, and
    /// Low (`Z80CTC-037`).
    #[must_use]
    pub const fn twctr_min_ps(self) -> Picoseconds {
        match self {
            Self::Z0843004 => Picoseconds(200_000),
            Self::Z0843006 => Picoseconds(120_000),
        }
    }

    /// `TdC(ZC/TOr)` and `TdC(ZC/TOf)`, one figure: the longest delay from a
    /// clock edge to the `ZC/TO` edge it causes, rising after a rising clock
    /// edge and falling after a falling one (`Z80CTC-036`).
    #[must_use]
    pub const fn tdc_zc_to_max_ps(self) -> Picoseconds {
        match self {
            Self::Z0843004 => Picoseconds(190_000),
            Self::Z0843006 => Picoseconds(140_000),
        }
    }
}

/// The levels on the part's input pins during one clock period.
///
/// `Default` is the part idle and enabled: not selected, no bus cycle, `IEI`
/// High so the part may interrupt, `RESET` High, the data bus and every
/// `CLK/TRG` Low.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `D7`–`D0` as the rest of the bus drives them. Read during an I/O write
    /// to the part and during an opcode fetch (`Z80CTC-003`, `Z80CTC-030`).
    pub d: u8,
    /// `CE`: Low selects the part for an I/O read or write (`Z80CTC-003`).
    pub ce_n: Level,
    /// `CS1`·`CS0` in bits 1–0, the channel number (`Z80CTC-002`); the other
    /// bits are ignored.
    pub cs: u8,
    /// `M1` from the CPU (`Z80CTC-003`, `Z80CTC-029`, `Z80CTC-030`).
    pub m1_n: Level,
    /// `IORQ` from the CPU (`Z80CTC-003`, `Z80CTC-029`).
    pub iorq_n: Level,
    /// `RD` from the CPU (`Z80CTC-003`).
    pub rd_n: Level,
    /// `IEI`: High while no device above the part in the daisy chain is
    /// being serviced or asking (`Z80CTC-027`).
    pub iei: Level,
    /// `RESET` (`Z80CTC-032`).
    pub reset_n: Level,
    /// `CLK/TRG0`–`CLK/TRG3`, by channel.
    pub clk_trg: [Level; CHANNEL_COUNT],
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The part idle and enabled; see [`Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            d: 0,
            ce_n: Level::High,
            cs: 0,
            m1_n: Level::High,
            iorq_n: Level::High,
            rd_n: Level::High,
            iei: Level::High,
            reset_n: Level::High,
            clk_trg: [Level::Low; CHANNEL_COUNT],
        }
    }
}

impl Inputs {
    /// [`Inputs::default`] with an I/O read of the channel `cs` selects in
    /// this clock: `IORQ`, `CE` and `RD` Low.
    #[must_use]
    pub const fn read_access(cs: Select) -> Self {
        let mut inputs = Self::idle();
        inputs.cs = cs.levels();
        inputs.ce_n = Level::Low;
        inputs.iorq_n = Level::Low;
        inputs.rd_n = Level::Low;
        inputs
    }

    /// [`Inputs::default`] with an I/O write of `d` to the channel `cs`
    /// selects in this clock: `IORQ` and `CE` Low, `RD` High.
    #[must_use]
    pub const fn write_access(cs: Select, d: u8) -> Self {
        let mut inputs = Self::idle();
        inputs.cs = cs.levels();
        inputs.ce_n = Level::Low;
        inputs.iorq_n = Level::Low;
        inputs.d = d;
        inputs
    }

    /// The channel `CS1` and `CS0` select (`Z80CTC-002`).
    const fn select(self) -> Select {
        Select::new(self.cs)
    }
}

/// The levels of the select pins `CS1`·`CS0`, `CS1` in bit 1 and `CS0` in
/// bit 0, the channel number, as [`Z80Ctc::read`], [`Z80Ctc::write`] and the
/// [`Inputs`] presets take them (`Z80CTC-002`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The select at `levels`, masked to `CS1`·`CS0` (bits 1–0); the other
    /// bits are ignored.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0b11)
    }

    /// The levels of `CS1`·`CS0`, in bits 1–0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }

    /// The channel these levels select.
    const fn channel(self) -> usize {
        self.0 as usize
    }
}

/// What the part drives during one clock period.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `D7`–`D0`: `Some` during an I/O read of the part and while it answers
    /// an interrupt acknowledge, `None` (high impedance) otherwise
    /// (`Z80CTC-004`).
    pub d: Option<u8>,
    /// `INT`, open drain: Low while the part requests an interrupt, High
    /// meaning released, the level the pull-up leaves (`Z80CTC-023`).
    pub int_n: Level,
    /// `IEO` (`Z80CTC-027`).
    pub ieo: Level,
    /// `ZC/TO0`–`ZC/TO2`; channel 3 has no pin (`Z80CTC-017`).
    pub zc_to: [Level; 3],
}

/// A channel as it stands, for a debugger: none of it is a pin.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Channel {
    /// The channel control register (`Z80CTC-007`).
    pub control: u8,
    /// The time constant register, `0` for 256 (`Z80CTC-009`).
    pub time_constant: u8,
    /// The down-counter, as a read would return it (`Z80CTC-006`).
    pub down_counter: u8,
    /// Counting, or waiting to start or for its trigger: given a time
    /// constant and not stopped since.
    pub operating: bool,
    /// An interrupt requested and not yet acknowledged.
    pub interrupt_pending: bool,
    /// An interrupt acknowledged and not yet released by `RETI`.
    pub in_service: bool,
}

/// Which channel answers the interrupt acknowledge in progress.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Ack {
    /// No acknowledge in progress.
    Idle,
    /// This channel is answering it.
    Answering(usize),
    /// An acknowledge this part does not answer.
    Silent,
}

/// The whole part.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Z80Ctc {
    channels: [Chan; CHANNEL_COUNT],
    /// The interrupt vector's top five bits (`Z80CTC-010`).
    vector: u8,
    /// An I/O write to the part in progress: the channel's select and the
    /// byte as it last stood.
    write: Option<(Select, u8)>,
    /// The down-counter value an I/O read in progress returns.
    read: Option<u8>,
    ack: Ack,
    /// An opcode fetch in progress, and the byte on the bus at its last clock.
    fetch: Option<u8>,
    /// The last opcode fetched was `EDh` (`Z80CTC-030`).
    ed_decoded: bool,
    /// What the last clock period drove.
    outputs: Outputs,
}

impl Default for Z80Ctc {
    fn default() -> Self {
        Self::new()
    }
}

impl Z80Ctc {
    /// The part in its reset state with every register `00h`; `RESET` Low
    /// reaches the reset state and keeps the registers. See the crate
    /// documentation's "Power-up".
    #[must_use]
    pub const fn new() -> Self {
        Self {
            outputs: Outputs {
                d: None,
                int_n: Level::High,
                ieo: Inputs::idle().iei,
                zc_to: [Level::Low; 3],
            },
            channels: [Chan::reset(0, 0, 0, false); CHANNEL_COUNT],
            vector: 0,
            write: None,
            read: None,
            ack: Ack::Idle,
            fetch: None,
            ed_decoded: false,
        }
    }

    /// Channel `i` as it stands.
    ///
    /// # Panics
    ///
    /// If `i` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn channel(&self, i: usize) -> Channel {
        assert!(i < CHANNEL_COUNT, "the part has four channels");
        let c = &self.channels[i];
        Channel {
            control: c.control,
            time_constant: c.tc,
            down_counter: c.counter,
            operating: !matches!(c.run, Run::Stopped),
            interrupt_pending: c.pending,
            in_service: c.in_service,
        }
    }

    /// The interrupt vector's top five bits as written, the rest clear
    /// (`Z80CTC-010`).
    #[must_use]
    pub const fn interrupt_vector(&self) -> u8 {
        self.vector
    }

    /// The byte an I/O read of the channel `cs` selects returns: its
    /// down-counter as it stands (`Z80CTC-006`). A read on the bus returns
    /// it as it stood at the read's first clock; this is that clock's.
    #[must_use]
    pub const fn read(&self, cs: Select) -> u8 {
        self.channels[cs.channel()].counter
    }

    /// The write [`Z80Ctc::tick`] takes when an I/O write of `d` to the
    /// channel `cs` selects ends (`Z80CTC-005`): a control word, a time
    /// constant, or, to channel 0, the interrupt vector (`Z80CTC-010`). A
    /// vector word to another channel is ignored (`Z80CTC-OPEN-03`).
    pub fn write(&mut self, cs: Select, d: u8) {
        let ch = cs.channel();
        if let Some(v) = self.channels[ch].write(d)
            && ch == 0
        {
            self.vector = v & interrupt_vector::V_MASK;
        }
    }

    /// What the part drives as the last clock period left it, `IEO` from the
    /// `IEI` it was shown then; before the first, what it drives at reset
    /// with every input idle.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        self.outputs
    }

    /// One period of the system clock. See the crate documentation for what
    /// `inp` is and what comes back, which [`Z80Ctc::outputs`] reads again.
    pub fn tick(&mut self, inp: Inputs) -> Outputs {
        self.outputs = self.clock(inp);
        self.outputs
    }

    fn clock(&mut self, inp: Inputs) -> Outputs {
        if inp.reset_n.is_low() {
            for (c, &clk) in self.channels.iter_mut().zip(inp.clk_trg.iter()) {
                *c = Chan::reset(c.control, c.tc, c.counter, clk.is_high());
            }
            self.write = None;
            self.read = None;
            self.ack = Ack::Idle;
            self.fetch = None;
            self.ed_decoded = false;
            return Outputs {
                d: None,
                int_n: Level::High,
                ieo: inp.iei,
                zc_to: [Level::Low; 3],
            };
        }

        let (ce, m1, iorq, rd) = (
            inp.ce_n.is_low(),
            inp.m1_n.is_low(),
            inp.iorq_n.is_low(),
            inp.rd_n.is_low(),
        );
        let iei = inp.iei.is_high();

        if !m1 {
            for c in &mut self.channels {
                if core::mem::take(&mut c.raise) {
                    c.pending = true;
                }
            }
        }

        let writing = iorq && ce && !rd && !m1;
        if writing {
            self.write = Some((inp.select(), inp.d));
        } else if let Some((cs, byte)) = self.write.take() {
            self.write(cs, byte);
        }

        let mut released = false;
        let fetching = m1 && rd && !iorq;
        if fetching {
            self.fetch = Some(inp.d);
        } else if let Some(op) = self.fetch.take() {
            if self.ed_decoded
                && op == 0x4d
                && iei
                && let Some(c) = self.channels.iter_mut().find(|c| c.in_service)
            {
                c.in_service = false;
                released = true;
            }
            self.ed_decoded = op == 0xed;
        }

        let mut zc_to = [Level::Low; 3];
        for (i, (c, &clk)) in self.channels.iter_mut().zip(inp.clk_trg.iter()).enumerate() {
            if c.clock(clk.is_high()) {
                if let Some(pin) = zc_to.get_mut(i) {
                    *pin = Level::High;
                }
                if c.control & INTERRUPT != 0 {
                    c.raise = true;
                }
            }
        }

        let mut d = None;

        if iorq && ce && rd && !m1 {
            let value = *self.read.get_or_insert(self.read(inp.select()));
            d = Some(value);
        } else {
            self.read = None;
        }

        if m1 && iorq {
            if self.ack == Ack::Idle {
                self.ack = match self.requester(iei) {
                    Some(i) => {
                        let c = &mut self.channels[i];
                        c.pending = false;
                        c.in_service = true;
                        Ack::Answering(i)
                    }
                    None => Ack::Silent,
                };
            }
            if let Ack::Answering(i) = self.ack {
                d = Some(self.vector | (i as u8) << 1);
            }
        } else {
            self.ack = Ack::Idle;
        }

        Outputs {
            d,
            int_n: Level::from_bool(self.requester(iei).is_none()),
            ieo: Level::from_bool(!released && self.ieo(iei)),
            zc_to,
        }
    }

    /// The channel that would answer an acknowledge now: the highest with a
    /// request whose place in the chain is enabled (`Z80CTC-026`,
    /// `Z80CTC-028`).
    fn requester(&self, iei: bool) -> Option<usize> {
        let mut enabled = iei;
        for (i, c) in self.channels.iter().enumerate() {
            if c.pending && enabled {
                return Some(i);
            }
            enabled &= !c.pending && !c.in_service;
        }
        None
    }

    /// `IEO`: `IEI`, unless a channel is under service or has a request
    /// pending; after `EDh` a request not yet acknowledged no longer counts
    /// (`Z80CTC-027`, `Z80CTC-030`).
    fn ieo(&self, iei: bool) -> bool {
        iei && self
            .channels
            .iter()
            .all(|c| !c.in_service && (self.ed_decoded || !c.pending))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `write` and `read` called without the bus are the access `tick` makes
    /// from the presets.
    #[test]
    fn bus_calls_match_the_pins() {
        let mut by_pins = Z80Ctc::new();
        let mut by_calls = Z80Ctc::new();
        for (cs, d) in [
            (0, 0x18),
            (2, control::TIME_CONSTANT | control::CONTROL),
            (2, 7),
        ] {
            for _ in 0..3 {
                let _ = by_pins.tick(Inputs::write_access(Select::new(cs), d));
            }
            let _ = by_pins.tick(Inputs::idle());
            by_calls.write(Select::new(cs), d);
        }
        assert_eq!(by_pins.interrupt_vector(), 0x18);
        assert_eq!(by_calls.interrupt_vector(), 0x18);
        assert_eq!(by_calls.channel(2), by_pins.channel(2));
        let read = by_pins.read(Select::new(2));
        assert_eq!(
            by_pins.tick(Inputs::read_access(Select::new(2))).d,
            Some(read)
        );
        assert_eq!(Select::new(0xff).levels(), 0b11);
    }

    /// `outputs()` reads back what each clock period drove, `ZC/TO` and `INT`
    /// among them.
    #[test]
    fn outputs_reads_back_the_last_period() {
        let mut ctc = Z80Ctc::new();
        assert!(ctc.outputs().int_n.is_high() && ctc.outputs().ieo.is_high());
        for d in [
            control::INTERRUPT | control::COUNTER | control::TIME_CONSTANT | control::CONTROL,
            1,
        ] {
            for _ in 0..3 {
                let out = ctc.tick(Inputs::write_access(Select::new(0), d));
                assert_eq!(ctc.outputs(), out);
            }
            let _ = ctc.tick(Inputs::idle());
        }
        let mut seen = (false, false);
        for i in 0..8 {
            let mut inp = Inputs::idle();
            inp.clk_trg[0] = Level::from_bool(i % 2 == 1);
            let out = ctc.tick(inp);
            assert_eq!(ctc.outputs(), out);
            seen.0 |= out.zc_to[0].is_high();
            seen.1 |= out.int_n.is_low();
        }
        assert_eq!(seen, (true, true), "the run moved ZC/TO0 and INT");
    }
}
