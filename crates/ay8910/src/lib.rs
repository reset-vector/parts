//! The General Instrument AY-3-8910 programmable sound generator at its digital pins.
//!
//! Three verbs, because the bus is asynchronous to the clock (`AY8910-004`):
//! [`Ay8910::step`] is one settled bus transaction, [`Ay8910::tick`] is one
//! rising edge of the external clock, and [`Ay8910::advance`] is any number of
//! them; the two clock verbs read `/RESET` as the last [`Ay8910::step`] showed
//! it. Nothing here is analogue; the two voltage tables are
//! [`dac::OUTPUT_B_ROWS`], two loads of one measurement, and neither ever
//! touches the machine. [`Ay8910::audio`] sums the three channels
//! through a table the caller chooses. [`Ay8910::save_state`] is the whole
//! machine as forty-two bytes, for a consumer that has to save it.
//!
//! **What the crate chooses**, an open item: a register bit with no storage
//! reads as zero (`AY8910-OPEN-01`).
//!
//! # Examples
//!
//! ```
//! use ay8910::{Ay8910, Inputs, dac::OUTPUT_B_ROWS};
//!
//! let mut psg = Ay8910::new();
//! // Tone and noise off on every channel, channel A at fixed volume 15.
//! for (register, value) in [(7, 0x3F), (8, 0x0F)] {
//!     let _ = psg.step(Inputs::latch_access(register));
//!     let _ = psg.step(Inputs::write_access(value));
//! }
//! assert_eq!(psg.step(Inputs::read_access()).d, Some(0x0F));
//! psg.advance(64);
//! assert_eq!(psg.state().channels[0].code, 15);
//! assert_eq!(psg.audio(&OUTPUT_B_ROWS[0].uv).0, 925_000);
//! ```

#![no_std]

mod bus;
mod channel;
pub mod dac;
mod envelope;
mod noise;
mod state;
mod tone;

use bus::BusFunction;
pub use bus::{Inputs, Outputs};
pub use channel::{ChannelState, VOLUME_FULL_STEPS};
pub use envelope::EnvelopeState;
pub use noise::NoiseState;
pub use state::STATE_LEN;
pub use tone::ToneState;
use units::Microvolts;

/// The registers, R0 to R15.
pub const REGISTER_COUNT: usize = 16;

/// The largest magnitude [`Ay8910::audio`] returns given either table in
/// [`dac`]: three channels at code 15 into 9.830 kΩ.
pub const AUDIO_FULL_SCALE_UV: Microvolts =
    Microvolts(3 * (dac::OUTPUT_B_ROWS[1].uv[15].0 - dac::OUTPUT_B_ROWS[1].uv[0].0));

/// A pin's logic level, or a generator's present bit.
pub use signal::Level;

/// Write masks per register: only the documented functional bits are stored,
/// and a read returns every other bit as zero (`AY8910-OPEN-01`).
const MASKS: [u8; 16] = [
    0xFF, 0x0F, 0xFF, 0x0F, 0xFF, 0x0F, 0x1F, 0xFF, 0x1F, 0x1F, 0x1F, 0xFF, 0xFF, 0x0F, 0xFF, 0xFF,
];

/// The machine. `Default` and [`Ay8910::new`] give the reset state, with
/// `/RESET` released.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ay8910 {
    /// The `/RESET` level the last [`Ay8910::step`] showed, which the clock
    /// verbs tick on.
    reset_n: bool,
    registers: [u8; 16],
    /// Latched register index while the common address code is selected.
    address: Option<u8>,
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
    /// A, B, C as the mixer and amplitude logic resolve them right now.
    pub channels: [ChannelState; 3],
}

/// What one rising external clock edge did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Edge {
    /// Whether this edge was a divided edge, on which every counter advanced.
    pub divided: bool,
}

impl Default for Ay8910 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ay8910 {
    /// The part at power-up, in the state its reset leaves: every register
    /// 0, no address latched, every generator in its reset state, and `/RESET` released.
    /// A board holds `/RESET` Low on [`Ay8910::step`] to return it here.
    #[must_use]
    pub fn new() -> Self {
        Self {
            reset_n: true,
            registers: [0; 16],
            address: None,
            divider_cycle: 0,
            tone: [ToneState::default(); 3],
            noise: NoiseState::default(),
            envelope: EnvelopeState::default(),
        }
    }

    /// The masked value held by register `index`, 0 to [`REGISTER_COUNT`]` - 1`.
    ///
    /// # Panics
    /// If `index` is not below [`REGISTER_COUNT`].
    #[must_use]
    pub fn register(&self, index: usize) -> u8 {
        self.registers[index]
    }

    /// Whether the last latched address selected this chip.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.address.is_some()
    }

    /// One rising edge of the external clock, at the `/RESET` level the last
    /// [`Ay8910::step`] showed.
    #[must_use]
    pub fn tick(&mut self) -> Edge {
        Edge {
            divided: self.edge(),
        }
    }

    /// `n` rising edges of the external clock, as that many
    /// [`Ay8910::tick`]s.
    ///
    /// A caller that advances the part faster than it reads it — one native
    /// sample is eight edges and one readout — reads [`Ay8910::state`] once
    /// after.
    pub fn advance(&mut self, n: usize) {
        for _ in 0..n {
            self.edge();
        }
    }

    /// The signed level of the three channels summed through `table`, which
    /// gives the voltage of each four-bit code ([`dac::CODE_BIT_COUNT`]), a
    /// row of [`dac::OUTPUT_B_ROWS`] or the caller's own.
    /// Each channel is measured from code 0's level, the code a reset leaves,
    /// so the part at rest reads 0 and full scale is three times
    /// `table[15] - table[0]`, [`AUDIO_FULL_SCALE_UV`] for either table in
    /// [`dac`]. A sum past `i32`'s range saturates. The channels leave the part on three pins; this is their
    /// unweighted sum, with nothing of a board's mixing. A pure reading of
    /// [`Ay8910::state`].
    #[must_use]
    pub fn audio(&self, table: &[Microvolts; 16]) -> Microvolts {
        let level = |code: u8| i64::from(table[usize::from(code & 0x0F)].0);
        let rest = level(0);
        let sum: i64 = self
            .state()
            .channels
            .iter()
            .map(|channel| level(channel.code) - rest)
            .sum();
        Microvolts(sum.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
    }

    /// One rising edge of the external clock, returning whether it was a
    /// divided edge.
    fn edge(&mut self) -> bool {
        if !self.reset_n {
            self.reset();
            return false;
        }
        self.divider_cycle = (self.divider_cycle + 1) & 7;
        let ticked = self.divider_cycle == 0;
        if ticked {
            for channel in 0..3 {
                self.tone[channel].tick(self.tone_period(channel));
            }
            self.noise.tick(self.registers[6]);
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
                    self.registers[8 + channel],
                    self.tone[channel].level,
                    self.noise.level(),
                    self.envelope.code(),
                )
            }),
        }
    }

    /// One settled bus transaction. Setup, hold and tristate timing are the
    /// caller's; here the pins are already stable. `/RESET` is held at the
    /// level shown until the next transaction.
    #[must_use = "the byte a read drives is in `d`, which `outputs()` does not repeat"]
    pub fn step(&mut self, pins: Inputs) -> Outputs {
        self.reset_n = pins.reset_n.is_high();
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
                let common = pins.a9_n.is_low() && pins.a8.is_high() && pins.d & 0xF0 == 0;
                self.address = common.then_some(pins.d & 0x0F);
            }
            BusFunction::Read => {
                if let Some(index) = self.address
                    && pins.test2.is_low()
                {
                    data = Some(match index {
                        14 => pins.ioa,
                        15 => pins.iob,
                        _ => self.registers[usize::from(index)],
                    });
                }
            }
            BusFunction::Write => {
                if let Some(index) = self.address
                    && pins.test2.is_low()
                {
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
    /// released, the ports and TEST1 as the registers and the envelope leave
    /// them. [`Ay8910::step`] returns the same with the byte a read drives.
    #[must_use]
    pub fn outputs(&self) -> Outputs {
        Outputs {
            d: None,
            ioa: (self.registers[7] & 0x40 != 0).then_some(self.registers[14]),
            iob: (self.registers[7] & 0x80 != 0).then_some(self.registers[15]),
            test1: self.envelope.test1,
        }
    }

    fn tone_period(&self, channel: usize) -> u16 {
        u16::from(self.registers[2 * channel]) | u16::from(self.registers[2 * channel + 1]) << 8
    }

    fn envelope_period(&self) -> u16 {
        u16::from(self.registers[11]) | u16::from(self.registers[12]) << 8
    }

    fn write(&mut self, index: u8, value: u8) {
        let index = usize::from(index);
        self.registers[index] = value & MASKS[index];
        if index == 13 {
            self.envelope.trigger(value);
        }
    }

    /// Everything the reset pin clears, from any verb; the pin's level is
    /// the caller's and stays.
    fn reset(&mut self) {
        self.registers = [0; 16];
        self.address = None;
        self.divider_cycle = 0;
        self.tone = [ToneState::default(); 3];
        self.noise = NoiseState::default();
        self.envelope = EnvelopeState::default();
    }
}
