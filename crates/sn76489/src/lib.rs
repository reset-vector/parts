//! The Texas Instruments SN76489AN digital complex sound generator at its pins.
//!
//! Three verbs, because the bus is asynchronous to the clock:
//! [`Sn76489::step`] is one settled bus transaction, [`Sn76489::tick`] is
//! one rising edge of the clock input, and [`Sn76489::advance`] is any number
//! of them. [`Sn76489::write`]
//! is the transaction a caller with no bus makes, which is what a file format
//! carrying one data byte and no register address leaves it with.
//!
//! **The part is parameterised, and it is still one die.** [`Parameters`]
//! carries the three numbers that differ between the parts answering to this
//! type number, and [`Member::Sn76489an`]'s are that die's. Any setting
//! on those three axes is honoured. Three things its relatives do are not
//! modelled: the T6W28's stereo, the NCR8496's XNOR noise feedback, and a
//! frequency of 0 read as `0x400`; here it holds the output (`SN76489-018`).
//!
//! **Nothing here is analogue.** The part is modelled as far as the four digital
//! levels its summing buffer is given (`SN76489-033`), and [`dac`] turns an
//! attenuation code into an amplitude without touching the machine.
//!
//! **What the crate chooses**, each an open item: power-on with every
//! attenuator off, every period zero and the latch unset (`SN76489-OPEN-02`);
//! every write of the noise control register resetting the shift register
//! (`SN76489-OPEN-03`); a write taking effect at the strobe
//! (`SN76489-OPEN-04`); a noise counter of its own (`SN76489-OPEN-07`); and
//! [`Member::Sn76494n`]'s noise parameters taken as the SN76489AN's
//! (`SN76489-OPEN-06`).
//!
//! # Examples
//!
//! ```
//! use core::num::NonZeroUsize;
//! use sn76489::{Member, Parameters, Sn76489};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // A setting from a file's header: fifteen stages, taps 0x0003, divide by 16.
//! let stages = NonZeroUsize::new(15).ok_or("a register of no stages")?;
//! let divider = NonZeroUsize::new(16).ok_or("a divider of nothing")?;
//! let parameters = Parameters::new(stages, 0x0003, divider)?;
//! assert_eq!(parameters, Member::Sn76489an.parameters());
//! let mut psg = Sn76489::new(parameters);
//! // Tone 0 at period 1 and no attenuation.
//! for byte in [0x81, 0x00, 0x90] {
//!     psg.write(byte);
//! }
//! psg.advance(16);
//! assert_eq!(psg.state().channels[0].attenuation_steps, 0);
//! assert!(psg.audio().0 >= 0);
//! # Ok(())
//! # }
//! ```

#![no_std]

use core::fmt;
use core::num::NonZeroUsize;
use units::Audio;

mod bus;
pub mod dac;
mod noise;
mod tone;

pub use bus::{Inputs, Outputs};
pub use noise::NoiseState;
pub use tone::ToneState;

/// How many clock cycles a load occupies, and therefore how long `READY` is
/// held low after `CE`'s leading edge or a strobe (`SN76489-014`).
///
/// The sheet says *approximately* 32, a typical, so this is a reading of it and not a
/// guarantee the part makes.
pub const WRITE_TYP_CYCLES: usize = 32;

/// A pin's logic level, or a generator's present bit.
pub use signal::Level;

/// How many control registers the part holds (`SN76489-007`).
pub const REGISTER_COUNT: usize = 8;

/// The largest value [`Sn76489::channel_audio`] returns: an unattenuated
/// channel, the largest value an `i16` sample carries, held as an `i32`.
/// The scale is this crate's choice.
pub const CHANNEL_FULL_SCALE: i32 = 32_767;

/// The largest value [`Sn76489::audio`] returns: four channels at
/// [`CHANNEL_FULL_SCALE`]. It returns nothing below 0.
pub const AUDIO_FULL_SCALE: i32 = 4 * CHANNEL_FULL_SCALE;

/// The values of a channel's four-bit attenuation code, in the steps
/// [`ChannelState::attenuation_steps`] counts.
pub mod attenuation {
    /// The code that turns a channel off (`SN76489-019`).
    pub const OFF: u32 = 0x0F;
}

/// The three numbers that differ between the parts answering to this type
/// number.
///
/// These express only the noise register's length, its white-noise tap mask
/// and the clock divider, and they are a struct of parameters and not an enum
/// of members because a caller takes all three from a file's header, where a
/// setting need name no part. A [`Member`] gives the settings of the parts a
/// document names. No preset is given for the SN76496, SN76489A, TMS9919, SN94624, NCR8496 or
/// T6W28, and no setting reproduces the NCR8496's XNOR feedback, T6W28 or Game
/// Gear stereo, an output sign inversion, or a period of 0 played as 0x400.
///
/// Two of them — the register's length and its white-noise tap mask — are
/// the caller's: TI's nine pages describe the noise source's mechanism and
/// give neither number (`SN76489-022`), and they are given per machine
/// (`SN76489-031`). The third, the clock divider, has both its settings in a
/// manufacturer's document (`SN76489-005`, `SN76489-SIB-01`).
///
/// **The fields are private and [`Parameters::new`] is the only way to build one
/// that is not a [`Member`]'s**, because a consumer takes all three from a
/// file's declared header and two of them index a shift: a length of zero
/// underflows and one above [`Parameters::MAX_NOISE_STAGE_COUNT`] shifts a `u16` past
/// its width, which panics in a debug build and is silently wrong in a release
/// one. The type refuses a zero length and a zero divider; refusing the rest
/// at construction is the only place the caller still knows which file it came
/// from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Parameters {
    noise_stage_count: NonZeroUsize,
    noise_tap_mask: u16,
    clock_divider_cycles: NonZeroUsize,
}

/// Why [`Parameters::new`] refused three numbers: each names the one that is
/// not a shift register this crate holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParametersError {
    /// `noise_stage_count` is above [`Parameters::MAX_NOISE_STAGE_COUNT`], wider
    /// than the register this crate holds.
    NoiseStagesAboveMax,
}

impl fmt::Display for ParametersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoiseStagesAboveMax => "a noise register wider than sixteen stages",
        })
    }
}

impl core::error::Error for ParametersError {}

impl Parameters {
    /// The longest noise register this crate represents, which is the width of
    /// the `u16` it is held in. Not a statement about any die: TI's sheet gives
    /// no length at all (`SN76489-022`).
    pub const MAX_NOISE_STAGE_COUNT: usize = 16;

    /// A setting of `noise_stage_count` stages, white-noise taps
    /// `noise_tap_mask`, and the clock input divided by
    /// `clock_divider_cycles`.
    ///
    /// A tap mask is not checked. A tap above the highest stage is a bit that
    /// never sets, which is a file describing a register it does not have and
    /// not a value this crate cannot hold.
    ///
    /// # Errors
    ///
    /// [`ParametersError::NoiseStagesAboveMax`] for a `noise_stage_count`
    /// above [`Parameters::MAX_NOISE_STAGE_COUNT`], a register this crate does
    /// not hold.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use sn76489::{Parameters, ParametersError};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let divider = NonZeroUsize::new(16).ok_or("a divider of nothing")?;
    /// let fifteen = NonZeroUsize::new(15).ok_or("a register of no stages")?;
    /// let parameters = Parameters::new(fifteen, 0x0003, divider)?;
    /// # let _ = parameters;
    ///
    /// // A register wider than this crate holds is refused.
    /// let wide = NonZeroUsize::new(Parameters::MAX_NOISE_STAGE_COUNT + 1).ok_or("no stages")?;
    /// assert_eq!(
    ///     Parameters::new(wide, 0x0003, divider),
    ///     Err(ParametersError::NoiseStagesAboveMax)
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub const fn new(
        noise_stage_count: NonZeroUsize,
        noise_tap_mask: u16,
        clock_divider_cycles: NonZeroUsize,
    ) -> Result<Self, ParametersError> {
        if noise_stage_count.get() > Self::MAX_NOISE_STAGE_COUNT {
            return Err(ParametersError::NoiseStagesAboveMax);
        }
        Ok(Self {
            noise_stage_count,
            noise_tap_mask,
            clock_divider_cycles,
        })
    }

    /// Stages in the noise shift register. 15 on [`Member::Sn76489an`].
    #[must_use]
    pub const fn noise_stage_count(self) -> NonZeroUsize {
        self.noise_stage_count
    }

    /// Which stages the white-noise feedback takes the exclusive-OR of, as a
    /// mask over the register. `0x0003` on [`Member::Sn76489an`].
    #[must_use]
    pub const fn noise_tap_mask(self) -> u16 {
        self.noise_tap_mask
    }

    /// What the clock input is divided by before any counter sees it. 16 on
    /// [`Member::Sn76489an`].
    #[must_use]
    pub const fn clock_divider_cycles(self) -> NonZeroUsize {
        self.clock_divider_cycles
    }

    /// The register's reset state: every stage clear but the highest
    /// (`SN76489-030`).
    ///
    /// The shift is in range for every `Parameters` that exists: [`Parameters::new`]
    /// is the only way to build one and it refuses a length this would not fit.
    #[must_use]
    pub(crate) const fn noise_reset(self) -> u16 {
        1 << (self.noise_stage_count.get() - 1)
    }
}

/// The parts a document gives the [`Parameters`] of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Member {
    /// The SN76489AN itself: fifteen stages, taps `0x0003`, divide by sixteen
    /// (`SN76489-005`, `SN76489-031`).
    Sn76489an,
    /// The setting `SN76489-SIB-01` describes: the same part with the
    /// divide-by-eight stage deleted. Its noise parameters are the
    /// SN76489AN's, because the footnote that names this part says nothing
    /// about its shift register — `SN76489-OPEN-06`.
    Sn76494n,
    /// The setting `SN76489-SIB-02` describes: the PSG Sega integrated into its
    /// VDP chips. **Not the SN76489AN.**
    SegaVdp,
}

impl Member {
    /// This member's three numbers.
    #[must_use]
    pub const fn parameters(self) -> Parameters {
        const FIFTEEN: NonZeroUsize = NonZeroUsize::new(15).unwrap();
        const SIXTEEN: NonZeroUsize = NonZeroUsize::new(16).unwrap();
        const TWO: NonZeroUsize = NonZeroUsize::new(2).unwrap();
        let (noise_stage_count, noise_tap_mask, clock_divider_cycles) = match self {
            Self::Sn76489an => (FIFTEEN, 0x0003, SIXTEEN),
            Self::Sn76494n => (FIFTEEN, 0x0003, TWO),
            Self::SegaVdp => (SIXTEEN, 0x0009, SIXTEEN),
        };
        Parameters {
            noise_stage_count,
            noise_tap_mask,
            clock_divider_cycles,
        }
    }
}

/// The machine. [`Sn76489::new`] takes the [`Parameters`] it is one of.
///
/// **The state it starts in is a choice, not a reading.** The part has no reset
/// pin, and under `SN76489-OPEN-02` this starts with every attenuator off,
/// every period register zero, no register latched and the divider at cycle
/// zero. A caller that knows better sets the
/// registers by writing them, which is the only way in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sn76489 {
    parameters: Parameters,
    /// Ten-bit period registers for the three tone channels.
    period: [u16; 3],
    /// Four-bit attenuation codes, channels 0..=2 the tones and 3 the noise.
    attenuation: [u8; 4],
    /// The three-bit noise control register: bit 2 the mode, bits 1..=0 the
    /// rate (`SN76489-023`, `SN76489-025`).
    noise_control: u8,
    /// The latched register address (`SN76489-008`), unset until a latch byte
    /// arrives.
    address: Option<u8>,
    /// Clock cycles left on the current load (`SN76489-014`).
    busy: usize,
    /// The level `WE` was last shown, so a strobe is an edge and not a level.
    we_high: bool,
    /// The level `CE` was last shown, so its leading edge is an edge.
    ce_high: bool,
    /// The divider's cycle, 0 up to `clock_divider_cycles - 1` (`SN76489-005`).
    divider_cycle: usize,
    tone: [ToneState; 3],
    noise: NoiseState,
}

/// What one channel is doing right now.
///
/// `level` is the generator's bit before the attenuator, so a channel whose
/// `attenuation_steps` is [`attenuation::OFF`] still reports the bit its counter or its shift
/// register is producing. The summing buffer past the attenuators is
/// `SN76489-033` and is not modelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ChannelState {
    /// The generator's output bit.
    pub level: Level,
    /// The four-bit attenuation register, in steps of 2 dB of power
    /// (`SN76489-034`), 0 to 15; 15 ([`attenuation::OFF`]) is silence, not a
    /// fifteenth step (`SN76489-019`).
    pub attenuation_steps: u32,
}

/// The clocked machine as observed after an edge or at rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PartState {
    /// The divider's cycle; the generators advanced on the edge that wrapped it
    /// to zero.
    pub divider_cycle: usize,
    /// The three tone generators.
    pub tone: [ToneState; 3],
    /// The noise generator.
    pub noise: NoiseState,
    /// Channels 0..=2 are the tone generators and 3 is the noise generator, in
    /// the order `SN76489-009` reads out of a register address.
    pub channels: [ChannelState; 4],
}

/// What one clock edge did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Edge {
    /// Whether this edge was a divided edge, on which every counter advanced.
    pub divided: bool,
}

impl Sn76489 {
    /// A part of the given [`Parameters`], in the start state the type's
    /// documentation describes.
    #[must_use]
    pub fn new(parameters: Parameters) -> Self {
        Self {
            parameters,
            period: [0; 3],
            attenuation: [attenuation::OFF as u8; 4],
            noise_control: 0,
            address: None,
            busy: 0,
            we_high: true,
            ce_high: true,
            divider_cycle: 0,
            tone: [ToneState::default(); 3],
            noise: NoiseState::reset(parameters),
        }
    }

    /// The parameters this part was built with.
    #[must_use]
    pub const fn parameters(&self) -> Parameters {
        self.parameters
    }

    /// The value held by control register `index`, 0 to
    /// [`REGISTER_COUNT`]` - 1`, in the register's own width (`SN76489-007`).
    ///
    /// Nothing on the part reads back — this is for a test and for a consumer
    /// saving state, not a pin.
    ///
    /// # Panics
    /// If `index` is not below [`REGISTER_COUNT`].
    #[must_use]
    pub const fn register(&self, index: usize) -> u16 {
        assert!(index < REGISTER_COUNT, "no such register");
        let channel = index >> 1;
        if index & 1 == 1 {
            self.attenuation[channel] as u16
        } else if channel == 3 {
            self.noise_control as u16
        } else {
            self.period[channel]
        }
    }

    /// What the address latch holds (`SN76489-008`): the register address a
    /// data byte would reach, or `None` while it is empty, until a latch byte
    /// has arrived, when a data byte reaches no register (`SN76489-011`).
    #[must_use]
    pub const fn latch(&self) -> Option<u8> {
        self.address
    }

    /// What the part drives now, from the levels it was last shown: `READY`
    /// Low from `CE`'s leading edge or a strobe until the load's clock cycles
    /// have run, whatever `CE` does meanwhile; unclocked, that is
    /// `SN76489-015`'s dependence on `CE`.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            ready: Level::from_bool(self.busy == 0),
        }
    }

    /// One settled bus transaction (`SN76489-014`, `SN76489-015`).
    ///
    /// A strobe is the falling edge of `WE` while `CE` is asserted, which is
    /// the sheet's *when `CE` is true, the `WE` signal strobes the contents of
    /// the data bus*. `CE`'s leading edge pulls `READY` low and the transfer
    /// completing releases it (`SN76489-014`), so it is held low for
    /// [`WRITE_TYP_CYCLES`] clock edges after that edge and again after the strobe,
    /// and never for longer because `CE` stays Low. The write takes effect at
    /// the strobe, this crate's choice under `SN76489-OPEN-04`.
    pub fn step(&mut self, pins: Inputs) -> Outputs {
        if pins.ce_n.is_low() && self.ce_high {
            self.busy = WRITE_TYP_CYCLES;
        }
        self.ce_high = pins.ce_n.is_high();
        let strobe = pins.ce_n.is_low() && pins.we_n.is_low() && self.we_high;
        self.we_high = pins.we_n.is_high();
        if strobe {
            self.write(pins.d);
        }
        self.outputs()
    }

    /// The byte `d` written, as if strobed (`SN76489-008`, `SN76489-011`).
    ///
    /// The part latches its own register address, so a byte is the whole of a
    /// transaction and there is no address to supply beside it.
    pub fn write(&mut self, d: u8) {
        self.busy = WRITE_TYP_CYCLES;
        if d & 0x80 == 0 {
            let Some(index) = self.address else { return };
            self.put(index, u16::from(d & 0x3F), true);
            return;
        }
        let index = (d >> 4) & 0x07;
        self.address = Some(index);
        self.put(index, u16::from(d & 0x0F), false);
    }

    /// Puts `value` into control register `index`.
    ///
    /// `high` distinguishes the two byte forms: a data byte supplies the six
    /// high bits of a ten-bit period (`SN76489-012`) and the low bits of
    /// anything narrower (`SN76489-013`), where a latch byte supplies the low
    /// four of a period and the whole of anything narrower.
    fn put(&mut self, index: u8, value: u16, high: bool) {
        let channel = (index >> 1) as usize;
        if index & 1 == 1 {
            self.attenuation[channel] = (value as u8) & 0x0F;
        } else if channel == 3 {
            self.noise_control = (value as u8) & 0x07;
            self.noise.reset_register(self.parameters);
        } else if high {
            self.period[channel] = (value << 4) | (self.period[channel] & 0x0F);
        } else {
            self.period[channel] = (self.period[channel] & 0x3F0) | value;
        }
    }

    /// `n` rising edges of the clock input without the readout
    /// (`SN76489-005`); [`Sn76489::state`] is where they left the part.
    pub fn advance(&mut self, n: usize) {
        for _ in 0..n {
            self.edge();
        }
    }

    /// One rising edge of the clock input, returning whether it was a divided
    /// edge (`SN76489-005`).
    fn edge(&mut self) -> bool {
        self.busy = self.busy.saturating_sub(1);
        self.divider_cycle += 1;
        if self.divider_cycle < self.parameters.clock_divider_cycles().get() {
            return false;
        }
        self.divider_cycle = 0;
        for channel in 0..3 {
            self.tone[channel].tick(self.period[channel]);
        }
        self.noise
            .tick(self.noise_reload(), self.noise_control, self.parameters);
        true
    }

    /// One rising edge of the clock input. [`Sn76489::state`] reads the machine
    /// after it.
    #[must_use]
    pub fn tick(&mut self) -> Edge {
        Edge {
            divided: self.edge(),
        }
    }

    /// Channel `ch`'s contribution on [`dac::LADDER`]'s scale, within
    /// [`CHANNEL_FULL_SCALE`]: its attenuation's rung while its generator's
    /// bit is High, 0 while it is Low or its attenuator is
    /// [`attenuation::OFF`]. Channels 0 to 2 are the tone generators and 3
    /// the noise generator.
    ///
    /// # Panics
    ///
    /// Panics if `ch` is 4 or more.
    #[must_use]
    pub fn channel_audio(&self, ch: usize) -> Audio<CHANNEL_FULL_SCALE> {
        Self::audio_of(self.state().channels[ch])
    }

    /// One channel's contribution, from its state.
    fn audio_of(channel: ChannelState) -> Audio<CHANNEL_FULL_SCALE> {
        if channel.level.is_high() {
            Audio(i32::from(
                dac::LADDER[channel.attenuation_steps as usize & 0x0F],
            ))
        } else {
            Audio(0)
        }
    }

    /// The four channels' [`Sn76489::channel_audio`] summed.
    ///
    /// One-sided: 0 at rest, which the part starts in, and never below it; at
    /// most [`AUDIO_FULL_SCALE`], four times [`CHANNEL_FULL_SCALE`]. A pure
    /// reading of [`Sn76489::state`].
    #[must_use]
    pub fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        let channels = self.state().channels;
        let sum: Audio<CHANNEL_FULL_SCALE> = channels.into_iter().map(Self::audio_of).sum();
        Audio(sum.0)
    }

    /// The clocked state as it stands, without advancing it.
    #[must_use]
    pub fn state(&self) -> PartState {
        PartState {
            divider_cycle: self.divider_cycle,
            tone: self.tone,
            noise: self.noise,
            channels: [0, 1, 2, 3].map(|channel| ChannelState {
                level: if channel == 3 {
                    self.noise.level()
                } else {
                    self.tone[channel].level
                },
                attenuation_steps: self.attenuation[channel] as u32,
            }),
        }
    }

    /// What the noise counter reloads with: three fixed values, or the third
    /// tone channel's period register (`SN76489-023`, `SN76489-OPEN-07`).
    const fn noise_reload(&self) -> u16 {
        match self.noise_control & 0x03 {
            0 => 0x10,
            1 => 0x20,
            2 => 0x40,
            _ => self.period[2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nz(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).expect("a test's count is not zero")
    }

    #[test]
    fn a_length_the_register_cannot_hold_is_refused_rather_than_shifted() {
        assert_eq!(
            Parameters::new(nz(Parameters::MAX_NOISE_STAGE_COUNT + 1), 0x0003, nz(16)),
            Err(ParametersError::NoiseStagesAboveMax)
        );
        assert_eq!(
            Parameters::new(nz(255), 0x0003, nz(16)),
            Err(ParametersError::NoiseStagesAboveMax)
        );

        assert_eq!(
            Parameters::new(nz(1), 0x0003, nz(1)).unwrap().noise_reset(),
            0x0001
        );
        let widest =
            Parameters::new(nz(Parameters::MAX_NOISE_STAGE_COUNT), 0x0009, nz(16)).unwrap();
        assert_eq!(widest.noise_reset(), 0x8000);
    }

    #[test]
    fn each_refusal_reads_as_its_reason() {
        extern crate std;
        assert_eq!(
            std::format!("{}", ParametersError::NoiseStagesAboveMax),
            "a noise register wider than sixteen stages"
        );
    }

    #[test]
    fn each_member_is_its_stages_taps_and_divider() {
        for (parameters, stages, taps, divider) in [
            (Member::Sn76489an, 15, 0x0003, 16),
            (Member::Sn76494n, 15, 0x0003, 2),
            (Member::SegaVdp, 16, 0x0009, 16),
        ] {
            assert_eq!(
                Parameters::new(nz(stages), taps, nz(divider)),
                Ok(parameters.parameters()),
                "the member and the constructor disagree"
            );
        }
    }
}
