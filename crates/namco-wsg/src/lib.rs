//! Namco's 3-channel wavetable sound generator — a **section of the Pac-Man game
//! board**, not a package — at its own edge.
//!
//! The section's digital edge is the `74LS273` at board position `2M`: eight
//! bits per channel slot, four of them the wave table's output nibble and four the
//! volume nibble straight out of the register file (`NAMCO-WSG-024`). **This
//! crate stops there**, because on the board the multiplication of the two and
//! the summing of the three channels happen past that latch, in a resistor network
//! and four `CD4066` switches (`NAMCO-WSG-026`, `NAMCO-WSG-027`). A consumer
//! wanting one PCM number folds them itself: [`NamcoWsg::audio`] sums the three
//! channels through a table of levels the consumer supplies.
//!
//! [`NamcoWsg::sample`] advances **one whole 96 kHz period** rather than one of
//! the sixteen nibble slots inside it; a slot-granular core is outside this
//! crate. The sequencer's table gives the sixteen slots in order
//! (`NAMCO-WSG-035` to `NAMCO-WSG-040`) and what each of its four outputs
//! drives (`NAMCO-WSG-009`, `NAMCO-WSG-041` to `NAMCO-WSG-043`). The adder's
//! carry input, the sixth flip-flop of the `74LS174`, the two multiplexer
//! selects and the three lines that carry a waveform select into the wave PROM
//! are `NAMCO-WSG-050`, `NAMCO-WSG-051`, `NAMCO-WSG-049` and `NAMCO-WSG-045`;
//! which input a select level chooses, that the two inversions cancel, and
//! where the two devices the file positions take differ are `NAMCO-WSG-052`,
//! `NAMCO-WSG-053` and `NAMCO-WSG-055`.
//!
//! [`NamcoWsg::write`] stores a register unconditionally, which the sequencer
//! allows (`NAMCO-WSG-039`), and [`NamcoWsg::sample`]'s three pairs come in the
//! order the latch presents them, three to a cycle of the file
//! (`NAMCO-WSG-044`).
//!
//! **What the crate chooses**, an open item: a 512-byte wave image is read
//! over its first 256 bytes (`NAMCO-WSG-OPEN-04`).
//!
//! # Examples
//!
//! ```
//! use namco_wsg::{NamcoWsg, ROM_NIBBLES, Select};
//! use units::Audio;
//!
//! // A wave table whose every entry is 9.
//! let mut wsg = NamcoWsg::with_image(&[9; ROM_NIBBLES])?;
//! wsg.write(Select::new(0x15), 0xF); // channel 0's volume
//! let latched = wsg.sample();
//! assert_eq!((latched[0].wave, latched[0].volume), (9, 0xF));
//!
//! // A consumer's table of the stage past the latch: volume times sample.
//! let mut levels = [[Audio(0); 16]; 16];
//! for (volume, row) in levels.iter_mut().enumerate() {
//!     for (wave, level) in row.iter_mut().enumerate() {
//!         *level = Audio((volume * wave) as i32);
//!     }
//! }
//! assert_eq!(wsg.audio(&levels), Audio(15 * 9));
//! # Ok::<(), namco_wsg::ImageError>(())
//! ```

#![no_std]

use units::{Audio, Hertz};

/// The levels of the lines a [`NamcoWsg::write`] selects a register by:
/// `AB0`–`AB3`, the address both halves of the file take, in bits 0–3, and
/// in bit 4 the line choosing the half whose write enable the write reaches,
/// so a select and a data byte cannot be swapped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The levels `levels`, bit 0 `AB0`; bits above 4 are no line of the
    /// section and are dropped.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0x1F)
    }

    /// The levels, bit 0 `AB0`, bit 4 the half.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// Nibble registers the section presents, `0x00`–`0x1F` (`NAMCO-WSG-003`).
pub const REGISTER_COUNT: usize = 32;

/// Channels (`NAMCO-WSG-015`).
pub const CHANNEL_COUNT: usize = 3;

/// Nibbles in the wave table PROM at `1M`, its size (`NAMCO-WSG-016`).
pub const ROM_NIBBLES: usize = 256;

/// Waveforms the table holds, of [`POSITIONS_PER_WAVEFORM`] nibbles each
/// (`NAMCO-WSG-018`), and the count the image divides into
/// (`NAMCO-WSG-032`).
pub const WAVEFORM_COUNT: usize = 8;

/// Nibbles in one waveform: the wave index is five bits (`NAMCO-WSG-014`).
pub const POSITIONS_PER_WAVEFORM: usize = 32;

/// The section's own sample rate in hertz (`NAMCO-WSG-029`).
pub const SAMPLE_RATE_HZ: Hertz = Hertz(96_000);

/// Nibble slots in one sample frame (`NAMCO-WSG-028`).
pub const SLOTS_PER_FRAME: usize = 16;

/// Bits in a channel's accumulator, counting the nibble two of the three channels
/// have no register for (`NAMCO-WSG-015`).
const PHASE_BIT_COUNT: u32 = 20;

/// The accumulator's modulus.
const ACCUMULATOR_MASK: u32 = (1 << PHASE_BIT_COUNT) - 1;

/// How far right the accumulator is shifted to leave the five-bit wave index.
const INDEX_SHIFT: u32 = PHASE_BIT_COUNT - 5;

/// A nibble.
const NIBBLE: u8 = 0x0F;

/// A pin's level, as a getter named for the pin returns it (`sound_on()`)
/// and a setter takes it (`set_sound_on(level)`).
pub use signal::Level;

/// The largest magnitude [`NamcoWsg::audio`] returns for a table whose
/// entries lie in `i16`'s range: three channels of −32 768.
pub const AUDIO_FULL_SCALE: i32 = 3 * 32_768;

/// Bytes [`NamcoWsg::save_state`] writes.
pub const STATE_LEN: usize = REGISTER_COUNT + CHANNEL_COUNT * 2 + 1;

/// Where one channel's nibbles sit in the register file.
///
/// Both halves of the file are addressed by the same four lines
/// (`NAMCO-WSG-004`), so index *i* of one half and index *i* of the other are
/// the pair the adder sees in one slot: accumulator against frequency for the
/// first five, waveform against volume for the sixth. The layout below is the
/// register map of Alessandro Scotti's note *Namco 3-channel Waveform Sound
/// Generator* (`NAMCO-WSG-036`).
struct Layout {
    /// Register of the accumulator's least significant *stored* nibble.
    accumulator: usize,
    /// Register of the frequency's least significant *stored* nibble.
    frequency: usize,
    /// How many nibbles of each the file holds: five for channel 0, four for the
    /// others (`NAMCO-WSG-015`, `NAMCO-WSG-036`).
    nibbles: u32,
    /// Which nibble of the twenty-bit value the first stored one is. Channels 1
    /// and 2 have no register for nibble 0 and it is a hardwired zero.
    first: u32,
    /// Register of the waveform select.
    waveform: usize,
    /// Register of the volume.
    volume: usize,
}

/// The three channels, in the order the register map lays them out.
const CHANNELS_LAYOUT: [Layout; CHANNEL_COUNT] = [
    Layout {
        accumulator: 0x00,
        frequency: 0x10,
        nibbles: 5,
        first: 0,
        waveform: 0x05,
        volume: 0x15,
    },
    Layout {
        accumulator: 0x06,
        frequency: 0x16,
        nibbles: 4,
        first: 1,
        waveform: 0x0A,
        volume: 0x1A,
    },
    Layout {
        accumulator: 0x0B,
        frequency: 0x1B,
        nibbles: 4,
        first: 1,
        waveform: 0x0F,
        volume: 0x1F,
    },
];

/// What the section drives out for one channel: the `74LS273` at `2M` at the
/// end of a sample frame.
///
/// Both fields are nibbles. They are **not** a product: the board multiplies
/// them past this latch and this crate does not (`NAMCO-WSG-027`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Outputs {
    /// The wave table nibble, latch bits `5D`–`8D` (`NAMCO-WSG-024`).
    pub wave: u8,
    /// The volume nibble, latch bits `1D`–`4D`, straight from the register
    /// file. A volume rises with loudness: each set bit closes one switch of the
    /// stage past the latch, and 0 opens them all and mutes the channel; the
    /// step is that stage's, which [`NamcoWsg::audio`] takes as a table
    /// (`NAMCO-WSG-026`).
    pub volume: u8,
}

/// A wave image the position cannot hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum ImageError {
    /// The image is `len` bytes long, not `expected` ([`ROM_NIBBLES`]) or
    /// twice it, whose first half the position reads.
    Len {
        /// The length the image had.
        len: usize,
        /// The length the position reads.
        expected: usize,
    },
}

impl core::fmt::Display for ImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Len { len, expected } => write!(
                f,
                "a wave image of {len} bytes: the position takes {expected} or twice it"
            ),
        }
    }
}

impl core::error::Error for ImageError {}

/// A state image this crate cannot read.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum StateError {
    /// A byte is wider than [`NamcoWsg::save_state`] writes it: every byte of
    /// the image is a nibble but the last, which is a boolean.
    Layout,
}

impl core::fmt::Display for StateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("state image layout mismatch")
    }
}

impl core::error::Error for StateError {}

/// The section.
///
/// The wave table is a constructor input because on the board it is a PROM in a
/// socket at `1M` and not part of the logic that reads it (`NAMCO-WSG-016`).
/// The words of the `PM1-3` PROM are this section's all the same, and
/// `NAMCO-WSG-032` to `NAMCO-WSG-034` are what they come to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NamcoWsg {
    /// The two `74LS89`/`82S25` halves, one nibble per entry
    /// (`NAMCO-WSG-003`). The accumulators live in here, which is why a
    /// [`NamcoWsg::sample`] writes registers.
    registers: [u8; REGISTER_COUNT],
    /// The PROM at `1M`, one nibble per entry.
    wave: [u8; ROM_NIBBLES],
    /// What the `2M` latch holds.
    latched: [Outputs; CHANNEL_COUNT],
    /// The `SOUND ON` net, `true` for the level that lets the latch hold
    /// (`NAMCO-WSG-025`).
    sound_on: bool,
}

impl NamcoWsg {
    /// The section with `wave` in the socket at `1M`, one nibble per entry.
    fn new(wave: [u8; ROM_NIBBLES]) -> Self {
        let mut wave = wave;
        for entry in &mut wave {
            *entry &= NIBBLE;
        }
        Self {
            registers: [0; REGISTER_COUNT],
            wave,
            latched: [Outputs::default(); CHANNEL_COUNT],
            sound_on: true,
        }
    }

    /// The section with a wave image of 256 or 512 bytes in the socket at
    /// `1M`, one nibble per entry.
    ///
    /// Anything above bit 3 of an entry is masked away: the PROM is four bits
    /// wide and the position's outputs `D0`–`D3` are all that reach the latch.
    ///
    /// **512 bytes take the first 256.** The layout marks the position as
    /// taking an `IM 5623` where the schematic prints `MB7052`
    /// (`NAMCO-WSG-019`); which of the two the board carries is
    /// `NAMCO-WSG-OPEN-04`, and either is read over its first 256 bytes here.
    ///
    /// # Errors
    ///
    /// [`ImageError::Len`] for any other length.
    ///
    /// # Examples
    ///
    /// ```
    /// use namco_wsg::{ImageError, NamcoWsg};
    ///
    /// # fn main() -> Result<(), ImageError> {
    /// let wsg = NamcoWsg::with_image(&[0x0F; 256])?;
    /// assert_eq!(wsg.save_state(), NamcoWsg::with_image(&[0x0F; 512])?.save_state());
    /// assert!(matches!(NamcoWsg::with_image(&[0; 128]), Err(ImageError::Len { .. })));
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_image(image: &[u8]) -> Result<Self, ImageError> {
        if image.len() != ROM_NIBBLES && image.len() != ROM_NIBBLES * 2 {
            return Err(ImageError::Len {
                len: image.len(),
                expected: ROM_NIBBLES,
            });
        }
        let mut wave = [0u8; ROM_NIBBLES];
        wave.copy_from_slice(&image[..ROM_NIBBLES]);
        Ok(Self::new(wave))
    }

    /// One nibble of the register file, as the CPU's write port reaches it.
    ///
    /// `a` selects the register: bits 0–3 are the levels of `AB0`–`AB3`, the
    /// address both halves take (`NAMCO-WSG-005`), and bit 4 chooses the half
    /// whose write enable the write reaches, `0x10`–`0x1F` the second. `d` is the
    /// data bus, of which only `DB0`–`DB3`
    /// are stored: the multiplexer at `3K` takes those four and the file is
    /// four bits wide, so nothing above bit 3 of a bus write is stored anywhere
    /// in the section (`NAMCO-WSG-006`).
    ///
    /// **That it stores unconditionally is the board's, not a convention.**
    /// The half holding the accumulators has one write enable and it is the
    /// sequencer's (`NAMCO-WSG-009`), so a write reaches it only where the
    /// sequencer allows one; the sequencer's table allows one at the same point
    /// of every slot and asserts nothing else while the strobe is low
    /// (`NAMCO-WSG-039`). What that model does not carry is the stall
    /// (`NAMCO-WSG-OPEN-09`), which is below this crate's granularity.
    pub fn write(&mut self, a: Select, d: u8) {
        self.registers[usize::from(a.levels())] = d & NIBBLE;
    }

    /// What register `index` holds, `index` as bits 4–0 of
    /// [`NamcoWsg::write`]'s select. The section drives nothing back onto the
    /// data bus (`NAMCO-WSG-048`); this is for a consumer's save image and for
    /// a test.
    ///
    /// # Panics
    ///
    /// `index` of [`REGISTER_COUNT`] or more.
    #[must_use]
    pub fn register(&self, index: usize) -> u8 {
        self.registers[index]
    }

    /// Drives `SOUND ON`: High, the section sounding.
    ///
    /// Low holds the `2M` latch's asynchronous clear, and every latched bit
    /// is then zero — volume as well as sample (`NAMCO-WSG-025`).
    pub fn set_sound_on(&mut self, level: Level) {
        self.sound_on = level.is_high();
        if level.is_low() {
            self.latched = [Outputs::default(); CHANNEL_COUNT];
        }
    }

    /// The level `SOUND ON` is held at, as [`NamcoWsg::set_sound_on`] last drove it.
    #[must_use]
    pub const fn sound_on(&self) -> Level {
        Level::from_bool(self.sound_on)
    }

    /// What the section drives now, the `2M` latch's contents, one channel to
    /// an entry in the order [`NamcoWsg::sample`] returns them.
    #[must_use]
    pub const fn outputs(&self) -> [Outputs; CHANNEL_COUNT] {
        self.latched
    }

    /// A channel's phase accumulator, as the register file holds it, in the
    /// twenty bits the wave index is taken from.
    ///
    /// # Panics
    ///
    /// If `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn phase(&self, channel: usize) -> u32 {
        self.gather(&CHANNELS_LAYOUT[channel], Half::Accumulator)
    }

    /// A channel's frequency register: the phase increment each period adds
    /// to its accumulator, on the same twenty bits.
    ///
    /// # Panics
    ///
    /// If `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn increment(&self, channel: usize) -> u32 {
        self.gather(&CHANNELS_LAYOUT[channel], Half::Frequency)
    }

    /// Advances one 96 kHz period and returns what the `2M` latch then holds.
    ///
    /// Each channel's frequency is added into its accumulator in the register
    /// file (`NAMCO-WSG-011`, `NAMCO-WSG-012`), the accumulator's top five bits
    /// index the waveform its select names (`NAMCO-WSG-014`,
    /// `NAMCO-WSG-018`), and the nibble that comes back is latched beside the
    /// channel's volume nibble.
    ///
    /// The three pairs come back in the order the board's one latch presents
    /// them: the sequencer produces one per-channel event per run of accumulator
    /// slots, in run order (`NAMCO-WSG-038`). They are a period's worth of
    /// output and never three simultaneous ones — the section has one
    /// `74LS273`, and a consumer that sums them is modelling the node they meet
    /// at.
    pub fn sample(&mut self) -> [Outputs; CHANNEL_COUNT] {
        for (channel, layout) in CHANNELS_LAYOUT.iter().enumerate() {
            let sum = self
                .gather(layout, Half::Accumulator)
                .wrapping_add(self.gather(layout, Half::Frequency))
                & ACCUMULATOR_MASK;
            self.scatter(layout, sum);
            let index = (sum >> INDEX_SHIFT) & 0x1F;
            let select = u32::from(self.registers[layout.waveform] & 0x07);
            let at = (select << 5) | index;
            self.latched[channel] = Outputs {
                wave: self.wave[at as usize],
                volume: self.registers[layout.volume],
            };
        }
        if !self.sound_on {
            self.latched = [Outputs::default(); CHANNEL_COUNT];
        }
        self.latched
    }

    /// What the `2M` latch holds now, as one signed sample: the sum over the
    /// three channels of `levels[volume][sample]`, each channel's two nibbles
    /// indexing the table.
    ///
    /// The section forms no product and no sum (`NAMCO-WSG-027`); both are the
    /// stage past the latch (`NAMCO-WSG-026`), so the table is the consumer's
    /// model of that stage, in the unit of the sum. An entry is meant to lie
    /// within a third of [`AUDIO_FULL_SCALE`], −32 768 to 32 767, and then the
    /// sum is within it; entries past it saturate the sum at `i32`'s ends
    /// rather than wrap. The volume nibble gates the stage's switches
    /// (`NAMCO-WSG-026`), so a table whose row 0 is 0 rests at 0: the section
    /// from power-up or reset with no volume written, or with `SOUND ON`
    /// clearing the latch.
    #[must_use]
    pub fn audio(&self, levels: &[[Audio<AUDIO_FULL_SCALE>; 16]; 16]) -> Audio<AUDIO_FULL_SCALE> {
        self.latched
            .iter()
            .map(|l| levels[usize::from(l.volume & NIBBLE)][usize::from(l.wave & NIBBLE)])
            .fold(Audio(0), |sum, level| sum + level)
    }

    /// The nibbles of one half of a channel's pair, as one twenty-bit value.
    fn gather(&self, layout: &Layout, half: Half) -> u32 {
        let base = match half {
            Half::Accumulator => layout.accumulator,
            Half::Frequency => layout.frequency,
        };
        let mut value = 0u32;
        for n in 0..layout.nibbles {
            let nibble = u32::from(self.registers[base + n as usize]);
            value |= nibble << (4 * (layout.first + n));
        }
        value
    }

    /// Writes a twenty-bit accumulator back into the nibbles that hold it.
    ///
    /// The nibble channels 1 and 2 have no register for is dropped, which costs
    /// nothing: their frequency has no register for it either, so nothing ever
    /// carries into bit 4 (`NAMCO-WSG-015`).
    fn scatter(&mut self, layout: &Layout, value: u32) {
        for n in 0..layout.nibbles {
            let shift = 4 * (layout.first + n);
            let nibble = ((value >> shift) & 0x0F) as u8;
            self.registers[layout.accumulator + n as usize] = nibble;
        }
    }

    /// The whole of the section's state, as [`STATE_LEN`] bytes.
    ///
    /// The register file carries the accumulators, so this is the file, the
    /// latch and the enable and nothing else.
    #[must_use]
    pub fn save_state(&self) -> [u8; STATE_LEN] {
        let mut out = [0u8; STATE_LEN];
        out[..REGISTER_COUNT].copy_from_slice(&self.registers);
        for (channel, latched) in self.latched.iter().enumerate() {
            out[REGISTER_COUNT + channel * 2] = latched.wave;
            out[REGISTER_COUNT + channel * 2 + 1] = latched.volume;
        }
        out[STATE_LEN - 1] = u8::from(self.sound_on);
        out
    }

    /// The inverse of [`NamcoWsg::save_state`]. The wave table is not in the
    /// image: it is a PROM and a save cannot change one.
    ///
    /// # Errors
    ///
    /// [`StateError::Layout`] where a byte before the last is not a nibble, or
    /// the last, the enable, is not a boolean, because such an image is not one
    /// this crate wrote. The length cannot be wrong: the image is an array.
    ///
    /// # Examples
    ///
    /// ```
    /// use namco_wsg::{NamcoWsg, StateError};
    ///
    /// # fn main() -> Result<(), Box<dyn core::error::Error>> {
    /// let mut wsg = NamcoWsg::with_image(&[0; 256])?;
    /// let mut image = wsg.save_state();
    /// wsg.restore_state(&image)?;
    /// image[0] = 0x10;
    /// assert_eq!(wsg.restore_state(&image), Err(StateError::Layout));
    /// # Ok(())
    /// # }
    /// ```
    pub fn restore_state(&mut self, image: &[u8; STATE_LEN]) -> Result<(), StateError> {
        if image[..STATE_LEN - 1].iter().any(|byte| *byte > NIBBLE) || image[STATE_LEN - 1] > 1 {
            return Err(StateError::Layout);
        }
        self.registers.copy_from_slice(&image[..REGISTER_COUNT]);
        for (channel, latched) in self.latched.iter_mut().enumerate() {
            latched.wave = image[REGISTER_COUNT + channel * 2];
            latched.volume = image[REGISTER_COUNT + channel * 2 + 1];
        }
        self.sound_on = image[STATE_LEN - 1] == 1;
        Ok(())
    }
}

/// Which half of the register file a gather reads.
#[derive(Clone, Copy)]
enum Half {
    Accumulator,
    Frequency,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wave table whose entry *n* is *n* modulo sixteen, so a sample reports
    /// the index that fetched it.
    fn ramp() -> [u8; ROM_NIBBLES] {
        let mut wave = [0u8; ROM_NIBBLES];
        for (at, entry) in wave.iter_mut().enumerate() {
            *entry = (at % 16) as u8;
        }
        wave
    }

    #[test]
    fn a_bus_write_keeps_four_bits() {
        let mut wsg = NamcoWsg::new(ramp());
        wsg.write(Select::new(0x15), 0xF7);
        assert_eq!(wsg.register(0x15), 0x07);
    }

    #[test]
    fn the_state_image_round_trips() {
        let mut wsg = NamcoWsg::new(ramp());
        wsg.write(Select::new(0x10), 0x8);
        wsg.write(Select::new(0x15), 0xF);
        let _ = wsg.sample();
        let image = wsg.save_state();

        let mut other = NamcoWsg::new(ramp());
        other
            .restore_state(&image)
            .expect("an image this crate wrote");
        let again = other.save_state();
        assert_eq!(image, again);
    }

    #[test]
    fn a_state_image_of_bytes_is_refused() {
        let mut wsg = NamcoWsg::new(ramp());
        let mut image = [0u8; STATE_LEN];
        image[0] = 0x10;
        assert_eq!(wsg.restore_state(&image), Err(StateError::Layout));

        let mut image = [0u8; STATE_LEN];
        image[STATE_LEN - 1] = 2;
        assert_eq!(wsg.restore_state(&image), Err(StateError::Layout));
    }
}
