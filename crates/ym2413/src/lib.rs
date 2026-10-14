//! The Yamaha YM2413 (OPLL) at its pins.
//!
//! **One [`Ym2413::tick`] is one rising edge of the master clock**, and the
//! caller shows the part its bus on every one — `D0`–`D7`, `A0`, `/CS`, `/WE`
//! and `/IC` as an [`Inputs`] word. The bus is **not** asynchronous to the
//! clock: the manual states the wait a write owes in master clocks, not in
//! nanoseconds (`YM2413-007`, `YM2413-008`). A write is taken on the edge on
//! which its mode is first shown, before that edge's slot runs, and that edge
//! is the first of the clocks the wait counts; holding the same mode on later
//! edges takes nothing more. [`Edge`] reports what the edge did,
//! including the write it took.
//!
//! Integer only, deterministic, and free of any machine around the part: no bus
//! decoding beyond the three control pins, no memory map and no resampling. The
//! part is time-multiplexed, so what leaves it is a stream of per-slot words and
//! not a summed sample; [`Ym2413::audio`] is the one documented sum of them.
//!
//! **Only the user tone sounds.** A channel that selects a ROM instrument (1 to
//! 15), and every rhythm voice, outputs silence with
//! [`SlotOutput::instrument_missing`] set: the crate holds none of the
//! Instruments ROM's parameters (`YM2413-OPEN-13`).
//!
//! **Two of the user tone's switches are decoded and not applied**, in either
//! slot, so setting them changes no output: AM, whose depth is not stated
//! (`YM2413-043`), and EG-TYP, so percussive and sustained tones share one
//! envelope law (`YM2413-OPEN-16`). [`Ym2413::tone`] still reports both.
//!
//! **What the crate chooses**, each an open item: a write that breaks the wait
//! is taken and reported (`YM2413-OPEN-01`); a write to an undefined address
//! is dropped (`YM2413-OPEN-02`); `/IC` clears the address latch
//! (`YM2413-OPEN-05`) and starts the frame (`YM2413-OPEN-08`); the envelope
//! steps 0.375 dB (`YM2413-OPEN-09`) at a rate scaling of the crate's own
//! (`YM2413-OPEN-10`); a full-scale modulator deviates the phase by π
//! (`YM2413-OPEN-12`); vibrato is a signed triangle peaking at a 128th of
//! the increment (`YM2413-OPEN-14`); a reset of any width clears in full
//! (`YM2413-OPEN-03`); and the noise register is all ones after one
//! (`YM2413-OPEN-15`).
//!
//! # Examples
//!
//! A user tone on channel 0, keyed, sounds within a few frames:
//!
//! ```
//! use ym2413::reg::{FNUM_LOWER, KEY_BLOCK};
//! use ym2413::{AUDIO_FULL_SCALE, CYCLES_PER_SAMPLE, Inputs, Level, Ym2413};
//!
//! let mut part = Ym2413::new();
//! # let poke = |part: &mut Ym2413, address: u8, value: u8| {
//! #     part.write(Level::Low, address);
//! #     part.write(Level::High, value);
//! # };
//! // The user tone: MUL 1 on both slots, the carrier attacking at once.
//! for (address, value) in [(0x00, 0x01), (0x01, 0x01), (0x04, 0xF0), (0x05, 0xF0)] {
//!     poke(&mut part, address, value);
//! }
//! poke(&mut part, FNUM_LOWER, 0x40);
//! poke(&mut part, KEY_BLOCK, 0b0001_1001); // key on, block 4
//!
//! let mut loudest = 0;
//! for _ in 0..CYCLES_PER_SAMPLE * 8 {
//!     let _ = part.tick(Inputs::idle());
//!     loudest = loudest.max(part.audio().0.abs());
//! }
//! assert!(loudest > 0 && loudest <= AUDIO_FULL_SCALE);
//! ```

#![no_std]

#[cfg(test)]
extern crate std;

mod bus;
mod eg;
mod frame;
#[allow(
    dead_code,
    reason = "the rhythm voices that read it are not modelled (YM2413-OPEN-13)"
)]
mod noise;
mod op;
mod pg;
mod registers;
mod tone;

use eg::Envelope;
use pg::Phase;
use units::Audio;

use bus::BusMode;
pub use bus::{WAIT_ADDRESS_CYCLES, WAIT_DATA_CYCLES, Wait, WriteMode};
pub use frame::{CHANNEL_COUNT, CYCLES_PER_SAMPLE, CYCLES_PER_SLOT, Role, SLOT_COUNT};
use frame::{Frame, channel_of, role_of};
pub use op::Word;
use registers::Registers;
pub use signal::Level;
pub use tone::{SlotTone, Tone};

/// The register addresses (`YM2413-015`).
pub mod reg {
    pub use crate::registers::{FNUM_LOWER, INST_VOL, KEY_BLOCK, RHYTHM, TEST, USER_TONE};
}

/// The largest magnitude [`Ym2413::audio`] returns: nine melody carriers at
/// ±255 (`YM2413-083`), the most words other than zero that ever stand handed
/// to a pin at once, rhythm mode switched on or off included (`YM2413-084`).
pub const AUDIO_FULL_SCALE: i32 = 9 * 255;

/// Register addresses [`Ym2413::register`] reads, `$00`–`$3F`, of which the
/// map defines 37 (`YM2413-015`).
pub const REGISTER_COUNT: usize = 0x40;

/// Which analogue pin a slot's word is bound for (`YM2413-084`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pin {
    /// The melody output.
    Mo,
    /// The rhythm output, which carries slots 13–18 in rhythm mode.
    Ro,
}

/// One slot's own state between its passes through the operator cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Slot {
    phase: Phase,
    envelope: Envelope,
    /// The word this slot last produced, which is what feeds a carrier's phase
    /// or the modulator's own feedback path.
    output: i32,
    /// The word this slot last handed a pin: its word when it ran as a
    /// carrier or bound for `RO`, and zero when its last word went to neither
    /// (`YM2413-084`, `YM2413-085`).
    pin_word: i32,
    /// The key state this slot last saw, so an edge can retrigger it.
    keyed: bool,
    /// What this slot last handed the converter, which [`Ym2413::sample`]
    /// returns for a slot that completed before it was called; `None` until
    /// the slot first runs after `/IC`.
    last: Option<SlotOutput>,
}

impl Slot {
    /// A slot as `/IC` leaves it.
    const CLEARED: Self = Self {
        phase: Phase::ZERO,
        envelope: Envelope::IDLE,
        output: 0,
        pin_word: 0,
        keyed: false,
        last: None,
    };
}

/// The bus pins the caller shows the part on every master clock
/// (`YM2413-001`, `YM2413-004`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// `D0`–`D7` as driven by the bus master; read on a write.
    pub d: u8,
    /// `A0`: Low selects the address latch, High the data.
    pub a0: Level,
    /// `/CS`.
    pub cs_n: Level,
    /// `/WE`.
    pub we_n: Level,
    /// `/IC`: Low holds the part in initial clear (`YM2413-011`).
    pub ic_n: Level,
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The bus idle: nothing selected, `/IC` High.
    #[must_use]
    pub const fn idle() -> Self {
        Inputs {
            d: 0,
            a0: Level::Low,
            cs_n: Level::High,
            we_n: Level::High,
            ic_n: Level::High,
        }
    }

    /// A write: the address latch when `a0` is Low, data when High.
    #[must_use]
    pub const fn write_access(a0: Level, d: u8) -> Self {
        Inputs {
            d,
            a0,
            cs_n: Level::Low,
            we_n: Level::Low,
            ic_n: Level::High,
        }
    }

    /// The same pins with `/IC` at `ic_n`; Low holds the part in initial
    /// clear.
    #[must_use]
    pub const fn with_ic_n(self, ic_n: Level) -> Self {
        Inputs { ic_n, ..self }
    }

    /// The bus condition `/CS`, `/WE` and `A0` select (`YM2413-004`).
    #[must_use]
    fn mode(self) -> BusMode {
        BusMode::decode(self.cs_n, self.we_n, self.a0)
    }
}

/// A channel's block and F-Number (`YM2413-038`, `YM2413-039`). The register
/// layout closes it, so it is exhaustive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fnum {
    /// The three-bit block, `$2n` `D3`–`D1`.
    pub block: u8,
    /// The nine-bit F-Number: eight bits from `$1n` and bit 8 from `$2n` `D0`.
    pub fnum: u16,
}

/// The machine. `Default` and [`Ym2413::new`] give the state `/IC` low leaves.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ym2413 {
    registers: Registers,
    /// The address a data write will land on, latched by an address write.
    address: u8,
    /// The bus condition the last edge was shown, so a write is taken once.
    bus: BusMode,
    wait: Wait,
    frame: Frame,
    slots: [Slot; SLOT_COUNT],
    /// The low-frequency oscillator's sample counter, shared by every slot
    /// (`YM2413-042`, `YM2413-043`).
    lfo: u32,
}

/// What the part drives out for one slot: the word it handed the converter
/// on the way to `MO` or `RO`, with where in the frame it stands. It is not
/// a pin word: beside the word it carries the slot's place in the frame,
/// which no pin shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SlotOutput {
    /// The slot, 0..18.
    pub slot: usize,
    /// The channel it belongs to (`YM2413-031`).
    pub channel: usize,
    /// Whether it is that channel's modulator or its carrier (`YM2413-032`).
    pub role: Role,
    /// The pin the word is bound for (`YM2413-084`), or `None` for a
    /// melody modulator's word, which reaches no pin.
    pub pin: Option<Pin>,
    /// The signed nine-bit word (`YM2413-083`). A melody modulator's word
    /// reaches no pin (`pin` is `None`); it is here because it is what
    /// modulates the carrier.
    ///
    /// This collapses the part's two zeros into one; `code` keeps them apart.
    pub value: i32,
    /// The same word as the converter is handed it: a sign and a magnitude,
    /// which is not the same set of values as `value` (`YM2413-095`).
    pub code: op::Word,
    /// Whether this slot's channel selected an instrument whose parameters
    /// this crate does not hold — a ROM instrument or a rhythm voice — in
    /// which case `value` is zero and nothing was substituted (`YM2413-OPEN-13`).
    pub instrument_missing: bool,
}

/// What one master-clock edge did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Edge {
    /// The slot that completed on this edge, on the last of its four master
    /// clocks, and `None` on the other three (`YM2413-034`).
    pub slot: Option<usize>,
    /// The word that slot handed the converter, where one completed.
    pub output: Option<SlotOutput>,
    /// The write this edge took, where it took one.
    pub write: Option<BusWrite>,
    /// Master clock cycles still owed to the bus before the next write is within the
    /// manual's stated wait (`YM2413-007`, `YM2413-008`).
    pub wait_remaining_cycles: usize,
}

/// What a write did, and whether it was inside the wait the part is owed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct BusWrite {
    /// Which write it was.
    pub mode: WriteMode,
    /// Whether the address written is one the register map defines. A write to
    /// an address it does not define is dropped (`YM2413-OPEN-02`).
    pub defined: bool,
    /// Whether the stated wait had **not** elapsed when this write was taken.
    ///
    /// The machine takes the write either way. The manual says only that data
    /// "will be uncertain" (`YM2413-OPEN-01`).
    pub wait_violated: bool,
}

impl Default for Ym2413 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ym2413 {
    /// A part with every register cleared, as after `/IC` low (`YM2413-011`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            registers: Registers::CLEARED,
            address: 0,
            bus: BusMode::Inactive,
            wait: Wait::NONE,
            frame: Frame::START,
            slots: [Slot::CLEARED; SLOT_COUNT],
            lfo: 0,
        }
    }

    /// One rising edge of the master clock, with the bus as `pins` show it.
    ///
    /// With `/IC` Low the part is cleared — every register zero, the address
    /// latch too (`YM2413-OPEN-05`), the frame at its start (`YM2413-OPEN-08`)
    /// — and nothing else happens on the edge (`YM2413-011`). The
    /// manual asks for at least 80 master clocks of `/IC` (`YM2413-012`); the
    /// crate clears on every such edge, and `YM2413-OPEN-03` is what a shorter
    /// pulse does on the part.
    ///
    /// Otherwise a write mode shown for the first time is taken, then on every
    /// fourth edge a slot completes, its phase and envelope advance one sample,
    /// and the word it hands the converter is reported (`YM2413-034`).
    #[must_use]
    pub fn tick(&mut self, pins: Inputs) -> Edge {
        if pins.ic_n.is_low() {
            *self = Self::default();
            return Edge {
                slot: None,
                output: None,
                write: None,
                wait_remaining_cycles: self.wait.remaining_cycles(),
            };
        }
        let mode = pins.mode();
        let write = mode
            .write_mode()
            .filter(|_| mode != self.bus)
            .map(|write| self.take_write(write, pins.d));
        self.bus = mode;
        self.wait.tick();
        let slot = self.frame.advance();
        let output = slot.map(|slot| {
            let output = self.run_slot(slot);
            self.slots[slot].last = Some(output);
            output
        });
        if slot == Some(SLOT_COUNT - 1) {
            self.lfo = self.lfo.wrapping_add(1);
        }
        Edge {
            slot,
            output,
            write,
            wait_remaining_cycles: self.wait.remaining_cycles(),
        }
    }

    /// One bus write, as [`Ym2413::tick`] takes it when the pins first show a
    /// write mode, without the bus: with `a0` Low, `d` is latched as the
    /// register address the next data write reaches (`YM2413-005`); with
    /// `a0` High, `d` is written to that register (`YM2413-006`). The wait
    /// the write owes the bus is charged as there (`YM2413-007`,
    /// `YM2413-008`).
    pub fn write(&mut self, a0: Level, d: u8) {
        let mode = if a0.is_high() {
            WriteMode::Data
        } else {
            WriteMode::Address
        };
        self.take_write(mode, d);
    }

    /// An address write (`YM2413-005`) or a data write (`YM2413-006`).
    fn take_write(&mut self, mode: WriteMode, value: u8) -> BusWrite {
        let wait_violated = self.wait.is_owed();
        let defined = if mode == WriteMode::Address {
            self.address = value;
            registers::mask(self.address).is_some()
        } else {
            self.registers.write(self.address, value)
        };
        self.wait.charge(mode);
        BusWrite {
            mode,
            defined,
            wait_violated,
        }
    }

    /// Runs with the bus idle to the end of the frame, and returns the frame's
    /// 18 per-slot words in slot order (`YM2413-029`, `YM2413-030`).
    ///
    /// From the start of a frame this is [`Ym2413::tick`] 72 times with
    /// [`Inputs::idle`] and nothing else — the same machine, the same words,
    /// one call instead of 72. It exists because the frame is a unit the part
    /// itself has, and because a caller that takes whole frames need not pay
    /// for 72 calls to do it.
    ///
    /// From inside a frame (after a [`Ym2413::tick`], or a write placed there)
    /// it runs only to the frame's end, and a slot that completed before the
    /// call is given as the word it handed the converter then, which `tick`
    /// reported; a slot that has not run since `/IC` is given as a zero word.
    #[must_use]
    pub fn sample(&mut self) -> [SlotOutput; SLOT_COUNT] {
        loop {
            let _ = self.tick(Inputs::idle());
            if self.frame.clock_index() == 0 {
                break;
            }
        }
        core::array::from_fn(|slot| {
            self.slots[slot].last.unwrap_or(SlotOutput {
                slot,
                channel: channel_of(slot),
                role: role_of(slot),
                pin: self.pin_of(slot),
                value: 0,
                code: op::Word::default(),
                instrument_missing: false,
            })
        })
    }

    /// The tone a channel's instrument selection resolves to, or `None` when it
    /// selects the Instruments ROM (`YM2413-026`, `YM2413-OPEN-13`).
    ///
    /// In rhythm mode channels 6 to 8 carry the percussion (`YM2413-071`),
    /// whose tones are the ROM's (`YM2413-079`), and `$37` and `$38`'s high
    /// nibbles are volumes rather than an instrument selection (`YM2413-028`),
    /// so those channels resolve to `None` whatever `$36`–`$38` hold.
    ///
    /// `channel` is zero-based, `0..9`.
    ///
    /// # Panics
    ///
    /// Panics if `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn tone(&self, channel: usize) -> Option<Tone> {
        assert!(
            channel < CHANNEL_COUNT,
            "channel {channel} is not on the part"
        );
        self.tone_of(channel)
    }

    /// [`Ym2413::tone`] for a channel already known to be on the part.
    fn tone_of(&self, channel: usize) -> Option<Tone> {
        if self.registers.is_rhythm_mode() && channel >= 6 {
            return None;
        }
        (self.registers.instrument(channel) == 0)
            .then(|| Tone::from_bytes(self.registers.user_tone()))
    }

    /// The pin a slot's word is bound for (`YM2413-084`): `RO` for the top
    /// six slots in rhythm mode, `MO` for a melody carrier, and `None` for a
    /// melody modulator, whose word reaches no pin.
    ///
    /// # Panics
    ///
    /// Panics if `slot` is not below [`SLOT_COUNT`].
    #[must_use]
    pub fn pin_of(&self, slot: usize) -> Option<Pin> {
        assert!(slot < SLOT_COUNT, "slot {slot} is outside the frame");
        if self.registers.is_rhythm_mode() && slot >= 12 {
            Some(Pin::Ro)
        } else if role_of(slot) == Role::Carrier {
            Some(Pin::Mo)
        } else {
            None
        }
    }

    /// The part's sound as one signed sample, mono, at rest 0: the unweighted
    /// sum of the word each slot last handed a pin — a carrier's to `MO`, and
    /// in rhythm mode a word bound for `RO` (`YM2413-084`). A word is bound to
    /// its pin as the slot hands it over (`YM2413-085`), so a slot whose last
    /// word reached no pin adds nothing until it next runs, whatever rhythm
    /// mode has since made of it.
    ///
    /// The unit is the word's own: each word is signed nine bits, ±255 at full
    /// scale (`YM2413-083`, `YM2413-095`). Only a melody carrier's word is
    /// other than zero here, since the rhythm voices sound silence, so the sum
    /// stays within nine of them, [`AUDIO_FULL_SCALE`]. `MO` and `RO` are
    /// summed as equals; a board's own weighting of the two pins is not
    /// applied, and no output stage is modelled, so this is the internal sum.
    /// A slot's word is the one it last handed over, so mid-frame the sum
    /// mixes two frames' words.
    #[must_use]
    pub fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        Audio(self.slots.iter().map(|slot| slot.pin_word).sum())
    }

    /// One pass through the operator cell, for the slot that just completed.
    fn run_slot(&mut self, slot: usize) -> SlotOutput {
        let channel = channel_of(slot);
        let role = role_of(slot);
        let pin = self.pin_of(slot);
        let Some(tone) = self.tone_of(channel) else {
            self.slots[slot].output = 0;
            self.slots[slot].pin_word = 0;
            return SlotOutput {
                slot,
                channel,
                role,
                pin,
                value: 0,
                code: op::Word::default(),
                instrument_missing: true,
            };
        };
        let half = tone.slot(role);

        let keyed = self.registers.key_on(channel);
        if keyed != self.slots[slot].keyed {
            self.slots[slot].keyed = keyed;
            if keyed {
                self.slots[slot].envelope.key_on();
                self.slots[slot].phase.reset();
            } else {
                self.slots[slot].envelope.key_off();
            }
        }

        let fnum = self.registers.fnum(channel);
        let block = self.registers.block(channel);
        let increment = pg::increment(fnum, block, half.mul);
        let increment = if half.vib {
            increment.saturating_add_signed(pg::vibrato_offset(increment, self.lfo))
        } else {
            increment
        };
        self.slots[slot].phase.advance(increment);

        let key_scale = eg::rate_key_scale(block, fnum & 0x100 != 0, half.ksr);
        let release = eg::release_field(self.registers.sustain_on(channel), half.rr);
        let rates = [
            eg::rate(half.ar, key_scale),
            eg::rate(half.dr, key_scale),
            eg::rate(release, key_scale),
        ];
        self.slots[slot]
            .envelope
            .advance(rates, eg::sustain_level(half.sl));

        let level = match role {
            Role::Modulator => eg::total_level(tone.tl),
            Role::Carrier => eg::volume(self.registers.volume(channel)),
        };
        let attenuation = self.slots[slot]
            .envelope
            .attenuation_db16()
            .saturating_add(level)
            .saturating_add(eg::level_key_scale(
                half.ksl,
                block,
                u8::try_from(fnum >> 5).expect("four bits"),
            ));

        let source = match role {
            Role::Modulator => self.slots[slot].output,
            Role::Carrier => self.slots[slot - 3].output,
        };
        let depth = match role {
            Role::Modulator => op::feedback_depth(tone.feedback),
            Role::Carrier => pg::PHASE_MODULUS / 2,
        };
        let phase = self.slots[slot]
            .phase
            .raw()
            .wrapping_add(op::modulation(source, depth))
            % pg::PHASE_MODULUS;
        let index = u16::try_from(phase >> pg::PHASE_FRACTION_BITS).expect("ten bits");

        let code = op::operate(index, attenuation, half.half_wave);
        let value = code.value();
        self.slots[slot].output = value;
        self.slots[slot].pin_word = if pin.is_some() { value } else { 0 };
        SlotOutput {
            slot,
            channel,
            role,
            pin,
            value,
            code,
            instrument_missing: false,
        }
    }

    /// The stored byte at register `i`, masked as it was written: only the
    /// bits the map defines are kept, and an address the map does not define
    /// reads zero (`YM2413-015`). The part has no readback path; this is the
    /// crate's view of its register file. An address at `$40` or above
    /// latches whole and reaches no register (`YM2413-OPEN-02`).
    ///
    /// # Panics
    ///
    /// Panics if `i` is not below [`REGISTER_COUNT`].
    #[must_use]
    pub fn register(&self, i: usize) -> u8 {
        assert!(i < REGISTER_COUNT, "register {i:#x} is not in the file");
        self.registers.get(i as u8)
    }

    /// The user tone register's eight bytes, `$00`–`$07` (`YM2413-016`).
    #[must_use]
    pub fn user_tone(&self) -> [u8; 8] {
        self.registers.user_tone()
    }

    /// Whether rhythm mode is selected — `$0E` `D5` (`YM2413-069`).
    #[must_use]
    pub fn is_rhythm_mode(&self) -> bool {
        self.registers.is_rhythm_mode()
    }

    /// The five rhythm instrument on/off bits, `$0E` `D4`–`D0` in the manual's
    /// order `BD SD TOM TOP-CY HH` (`YM2413-070`).
    #[must_use]
    pub fn rhythm_keys(&self) -> u8 {
        self.registers.rhythm_keys()
    }

    /// A channel's block and F-Number (`YM2413-038`, `YM2413-039`).
    ///
    /// # Panics
    ///
    /// Panics if `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn fnum(&self, channel: usize) -> Fnum {
        assert!(
            channel < CHANNEL_COUNT,
            "channel {channel} is not on the part"
        );
        Fnum {
            block: self.registers.block(channel),
            fnum: self.registers.fnum(channel),
        }
    }

    /// A channel's key bit, `$2n` `D4` (`YM2413-053`).
    ///
    /// # Panics
    ///
    /// Panics if `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn is_keyed(&self, channel: usize) -> bool {
        assert!(
            channel < CHANNEL_COUNT,
            "channel {channel} is not on the part"
        );
        self.registers.key_on(channel)
    }

    /// A channel's sustain bit, `$2n` `D5` (`YM2413-052`).
    ///
    /// # Panics
    ///
    /// Panics if `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn is_sustained(&self, channel: usize) -> bool {
        assert!(
            channel < CHANNEL_COUNT,
            "channel {channel} is not on the part"
        );
        self.registers.sustain_on(channel)
    }

    /// A channel's instrument selection, `$3n` `D7`–`D4` (`YM2413-026`).
    ///
    /// # Panics
    ///
    /// Panics if `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn instrument(&self, channel: usize) -> u8 {
        assert!(
            channel < CHANNEL_COUNT,
            "channel {channel} is not on the part"
        );
        self.registers.instrument(channel)
    }

    /// A channel's four-bit `VOL` field, `$3n` `D3`–`D0`, as an attenuation:
    /// 0 is the loudest and each step 3 dB more, to 45 dB at 15
    /// (`YM2413-027`).
    ///
    /// # Panics
    ///
    /// Panics if `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn attenuation_steps(&self, channel: usize) -> u32 {
        assert!(
            channel < CHANNEL_COUNT,
            "channel {channel} is not on the part"
        );
        u32::from(self.registers.volume(channel))
    }

    /// The address a data write would land on.
    #[must_use]
    pub fn address(&self) -> u8 {
        self.address
    }

    /// The master clock within the audio frame, `0..72`; slot
    /// `frame_cycle() / CYCLES_PER_SLOT` is the one executing (`YM2413-034`).
    #[must_use]
    pub const fn frame_cycle(&self) -> usize {
        self.frame.clock_index()
    }

    /// The wait the bus is owed right now.
    #[must_use]
    pub fn wait(&self) -> Wait {
        self.wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_write_is_taken_once() {
        let mut part = Ym2413::new();
        assert!(
            part.tick(Inputs::write_access(Level::Low, 0x30))
                .write
                .is_some()
        );
        assert!(
            part.tick(Inputs::write_access(Level::High, 0x11))
                .write
                .is_some()
        );
        let held = part.tick(Inputs::write_access(Level::High, 0x22));
        assert_eq!(held.write, None);
        assert_eq!(part.register(0x30), 0x11);
    }

    #[test]
    fn a_write_without_the_bus_is_the_write_tick_takes() {
        let (mut bus, mut direct) = (Ym2413::new(), Ym2413::new());
        let _ = bus.tick(Inputs::write_access(Level::Low, 0x30));
        let _ = bus.tick(Inputs::write_access(Level::High, 0x11));
        direct.write(Level::Low, 0x30);
        direct.write(Level::High, 0x11);
        assert_eq!(direct.register(0x30), bus.register(0x30));
        assert_eq!(direct.register(0x30), 0x11);
    }

    /// A keyed user tone on channel 0, as the crate root's example has it.
    fn keyed() -> Ym2413 {
        let mut part = Ym2413::new();
        for (address, value) in [
            (0x00, 0x01),
            (0x01, 0x01),
            (0x04, 0xF0),
            (0x05, 0xF0),
            (0x10, 0x40),
            (0x20, 0b0001_1001),
        ] {
            part.write(Level::Low, address);
            part.write(Level::High, value);
        }
        part
    }

    #[test]
    fn sample_from_inside_a_frame_runs_to_its_end() {
        let mut ticked = keyed();
        for _ in 0..CYCLES_PER_SAMPLE * 3 {
            let _ = ticked.tick(Inputs::idle());
        }
        let mut stepped = ticked.clone();
        let words: std::vec::Vec<SlotOutput> = (0..CYCLES_PER_SAMPLE)
            .filter_map(|_| stepped.tick(Inputs::idle()).output)
            .collect();
        for ticks in [1, 6] {
            let mut sampled = ticked.clone();
            for _ in 0..ticks {
                let _ = sampled.tick(Inputs::idle());
            }
            assert_eq!(sampled.sample().to_vec(), words, "{ticks} ticks in");
            assert_eq!(sampled, stepped, "{ticks} ticks in");
        }
    }

    #[test]
    fn sample_after_a_write_inside_a_frame_takes_it() {
        let mut part = keyed();
        for _ in 0..CYCLES_PER_SLOT * 5 {
            let _ = part.tick(Inputs::idle());
        }
        let _ = part.tick(Inputs::write_access(Level::Low, 0x30));
        let _ = part.tick(Inputs::idle());
        let _ = part.tick(Inputs::write_access(Level::High, 0x07));
        let frame = part.sample();
        assert_eq!(part.frame_cycle(), 0);
        assert_eq!(part.attenuation_steps(0), 7);
        assert!(frame.iter().enumerate().all(|(i, w)| w.slot == i));
    }

    #[test]
    #[should_panic(expected = "channel 9")]
    fn a_channel_off_the_part_panics() {
        let _ = Ym2413::new().fnum(CHANNEL_COUNT);
    }
}
