//! The Yamaha YM2149 software-controlled sound generator at its digital pins.
//!
//! Three verbs, as for the AY-3-8910 it is pin compatible with:
//! [`Ym2149::step`] is one settled bus transaction, [`Ym2149::tick`] one
//! rising edge of the `CLOCK` pin with the levels of `/RESET` and `/SEL` the
//! last [`Ym2149::step`] showed, and [`Ym2149::advance`] any number of them.
//! `/SEL`
//! Low halves `CLOCK` to make the master clock and High passes it
//! (`YM2149-003`).
//!
//! The output is each channel's five-bit converter code
//! ([`ChannelState::code`]); [`dac`] holds what the catalogue says a code
//! becomes, which stops at code 19, so codes 0 to 18 have no level here and
//! [`Ym2149::audio`] takes the caller's table of all thirty-two. Nothing here
//! is analogue.
//!
//! Yamaha's catalogue (LSI-2121492) leaves the period-zero behaviour, the noise
//! register, register read-back and the code a false mixer level gives open;
//! the behaviour here is `YM2149-011`, `YM2149-013`, `YM2149-015`,
//! `YM2149-016`, `YM2149-021` and `YM2149-028`. What the crate chooses, each
//! an open item: every generator's grain, direction and reset state
//! (`YM2149-OPEN-05`); a latch that does not match deselects the part
//! (`YM2149-OPEN-06`); a read of R14 or R15 returns the pins as the host
//! resolves them (`YM2149-OPEN-07`); and "mixed" is the AND of the tone and
//! noise waves (`YM2149-OPEN-08`).
//!
//! # Examples
//!
//! ```
//! use units::PerMille;
//! use ym2149::{Inputs, Ym2149};
//!
//! let mut psg = Ym2149::new();
//! // Tone and noise off on every channel, channel A at fixed volume 15.
//! for (register, value) in [(7, 0x3F), (8, 0x0F)] {
//!     let _ = psg.step(Inputs::latch_access(register));
//!     let _ = psg.step(Inputs::write_access(value));
//! }
//! assert_eq!(psg.step(Inputs::read_access()).d, Some(0x0F));
//! psg.advance(64);
//! assert_eq!(psg.state().channels[0].code, 31);
//! // A table of each code's level, here a straight line in per mille.
//! let table: [PerMille; 32] = core::array::from_fn(|code| PerMille(code as u32 * 1000 / 31));
//! assert_eq!(psg.audio(&table).0, 1000 - 32);
//! ```

#![no_std]

mod bus;
mod channel;
pub mod dac;
mod generators;

use bus::BusFunction;
pub use bus::{Inputs, Outputs};
pub use channel::{ChannelState, VOLUME_FULL_STEPS};
pub use generators::{EnvelopeState, NoiseState, ToneState};
use units::{Audio, PerMille};

/// The registers, R0 to R15.
pub const REGISTER_COUNT: usize = 16;

/// The largest magnitude [`Ym2149::audio`] returns for a table whose levels
/// lie in 0 to 1000, Fig. 1's per mille of the top level: three channels a
/// whole top level from rest.
pub const AUDIO_FULL_SCALE: i32 = 3 * 1000;

/// A pin's logic level, or a generator's present bit.
pub use signal::Level;

/// The machine. `Default` and [`Ym2149::new`] give the reset state, with
/// `/RESET` released and `/SEL` High, the levels the pins take open.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ym2149 {
    /// The `/RESET` level the last [`Ym2149::step`] showed.
    reset_n: bool,
    /// The `/SEL` level the last [`Ym2149::step`] showed.
    sel_n: bool,
    /// Every register holds the eight bits last written (`YM2149-011`).
    registers: [u8; 16],
    /// Latched register index while the chip-select code is matched.
    address: Option<u8>,
    /// The `/SEL`-Low divide-by-two: whether the next `CLOCK` edge is swallowed.
    halved: bool,
    /// The divide-by-eight master-clock divider's cycle, shared by every generator.
    divider_cycle: u8,
    tone: [ToneState; 3],
    noise: NoiseState,
    envelope: EnvelopeState,
}

/// The clocked machine as observed after an edge or at rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PartState {
    /// The divide-by-eight divider's cycle, 0..=7; the generators advanced when it wrapped to 0.
    pub divider_cycle: usize,
    /// The tone generators of channels A, B and C.
    pub tone: [ToneState; 3],
    /// The noise generator.
    pub noise: NoiseState,
    /// The envelope generator.
    pub envelope: EnvelopeState,
    /// A, B, C as the mixer and level control resolve them right now.
    pub channels: [ChannelState; 3],
}

/// What one rising `CLOCK` edge did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Edge {
    /// Whether this edge was a divided edge, on which every counter advanced.
    pub divided: bool,
}

impl Default for Ym2149 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ym2149 {
    /// The part at power-up, in the state `/RESET` leaves: every register 0,
    /// no address latched, every generator at its reset state, `/RESET`
    /// released and `/SEL` High. A board holds `/RESET` Low on
    /// [`Ym2149::step`] to return it here.
    #[must_use]
    pub fn new() -> Self {
        Self {
            reset_n: true,
            sel_n: true,
            registers: [0; 16],
            address: None,
            halved: false,
            divider_cycle: 0,
            tone: [ToneState::default(); 3],
            noise: NoiseState::default(),
            envelope: EnvelopeState::default(),
        }
    }

    /// The value held by register `index`, 0 to [`REGISTER_COUNT`]` - 1`.
    ///
    /// # Panics
    /// If `index` is not below [`REGISTER_COUNT`].
    #[must_use]
    pub fn register(&self, index: usize) -> u8 {
        self.registers[index]
    }

    /// Whether the last latch selected this chip.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.address.is_some()
    }

    /// One rising `CLOCK` edge, at the `/RESET` and `/SEL` levels the last
    /// [`Ym2149::step`] showed.
    #[must_use]
    pub fn tick(&mut self) -> Edge {
        Edge {
            divided: self.edge(),
        }
    }

    /// `n` rising `CLOCK` edges, as that many [`Ym2149::tick`]s.
    pub fn advance(&mut self, n: usize) {
        for _ in 0..n {
            self.edge();
        }
    }

    /// The signed level of the three channels summed through `table`, which
    /// gives the level of each five-bit code ([`dac::CODE_BIT_COUNT`]); the
    /// catalogue labels only codes 19 to 31, so the table is the caller's.
    ///
    /// Each level is per mille of the top level, Fig. 1's scale (as
    /// [`dac::CATALOGUE_PER_MILLE`]), and the sum is in the same steps. Each
    /// channel is measured from code 1's level, the code a reset leaves (fixed
    /// level 0, mixer level true or false), so the part at rest reads 0 and
    /// full scale is three times `table[31] - table[1]`; the envelope's code 0
    /// reads below rest. A table whose levels lie in 0 to 1000 keeps the sum
    /// within [`AUDIO_FULL_SCALE`], and any sum saturates at `i32`'s range.
    /// The channels leave the part on three pins; this is their unweighted
    /// sum, with nothing of a board's mixing. A pure reading of
    /// [`Ym2149::state`].
    #[must_use]
    pub fn audio(&self, table: &[PerMille; 32]) -> Audio<AUDIO_FULL_SCALE> {
        let level = |code: u8| i64::from(table[usize::from(code & 0x1F)].0);
        let rest = level(1);
        let sum: i64 = self
            .state()
            .channels
            .iter()
            .map(|channel| level(channel.code) - rest)
            .sum();
        Audio(sum.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
    }

    /// One rising `CLOCK` edge, returning whether it was a divided edge.
    fn edge(&mut self) -> bool {
        if !self.reset_n {
            self.reset();
            return false;
        }
        if !self.sel_n {
            self.halved = !self.halved;
            if self.halved {
                return false;
            }
        }
        self.divider_cycle = (self.divider_cycle + 1) & 7;
        let ticked = self.divider_cycle == 0;
        if ticked {
            for channel in 0..3 {
                self.tone[channel].tick(self.tone_period(channel));
            }
            self.noise.tick(self.registers[6] & 0x1F);
            self.envelope.tick(self.envelope_period());
        }
        ticked
    }

    /// The clocked state as it stands, without advancing it.
    #[must_use]
    pub fn state(&self) -> PartState {
        PartState {
            divider_cycle: self.divider_cycle as usize,
            tone: self.tone,
            noise: self.noise,
            envelope: self.envelope,
            channels: [0, 1, 2].map(|channel| {
                ChannelState::resolve(
                    channel,
                    self.registers[7],
                    self.registers[8 + channel] & 0x1F,
                    self.tone[channel].level,
                    self.noise.level(),
                    self.envelope.code(),
                )
            }),
        }
    }

    /// One settled bus transaction. Setup, hold and high-impedance timing are
    /// the caller's; here the pins are already stable. `/RESET` and `/SEL` are
    /// held at the levels shown until the next transaction.
    #[must_use = "the byte a read drives is in `d`, which `outputs()` does not repeat"]
    pub fn step(&mut self, pins: Inputs) -> Outputs {
        self.reset_n = pins.reset_n.is_high();
        self.sel_n = pins.sel_n.is_high();
        if pins.reset_n.is_low() {
            self.reset();
            return self.outputs();
        }
        let function =
            BusFunction::decode(pins.bdir.is_high(), pins.bc2.is_high(), pins.bc1.is_high());
        let mut data = None;
        match function {
            BusFunction::Inactive => {}
            BusFunction::LatchAddress => {
                let matched = pins.a9_n.is_low() && pins.a8.is_high() && pins.d & 0xF0 == 0;
                self.address = matched.then_some(pins.d & 0x0F);
            }
            BusFunction::Read => {
                if let Some(index) = self.address {
                    data = Some(match index {
                        14 => pins.ioa,
                        15 => pins.iob,
                        _ => self.registers[usize::from(index)],
                    });
                }
            }
            BusFunction::Write => {
                if let Some(index) = self.address {
                    self.write(index, pins.d);
                }
            }
        }
        Outputs {
            d: data,
            ..self.outputs()
        }
    }

    /// What the part's outputs stand at now, between transactions: the bus
    /// released and the ports as the registers leave them. [`Ym2149::step`]
    /// returns the same with the byte a read drives.
    #[must_use]
    pub fn outputs(&self) -> Outputs {
        Outputs {
            d: None,
            ioa: (self.registers[7] & 0x40 != 0).then_some(self.registers[14]),
            iob: (self.registers[7] & 0x80 != 0).then_some(self.registers[15]),
        }
    }

    fn tone_period(&self, channel: usize) -> u16 {
        u16::from(self.registers[2 * channel])
            | u16::from(self.registers[2 * channel + 1] & 0x0F) << 8
    }

    fn envelope_period(&self) -> u16 {
        u16::from(self.registers[11]) | u16::from(self.registers[12]) << 8
    }

    fn write(&mut self, index: u8, value: u8) {
        self.registers[usize::from(index)] = value;
        if index == 13 {
            self.envelope = EnvelopeState::triggered(value);
        }
    }

    /// Everything `/RESET` clears (`YM2149-009`), and the generators' dividers,
    /// which are this crate's (`YM2149-OPEN-05`); the pins' levels are the
    /// caller's and stay.
    fn reset(&mut self) {
        *self = Self {
            reset_n: self.reset_n,
            sel_n: self.sel_n,
            ..Self::new()
        };
    }
}
