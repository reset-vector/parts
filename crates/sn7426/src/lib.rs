//! The Texas Instruments SN7426 quadruple 2-input NAND gate with
//! high-voltage open-collector outputs, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7426-014`), so the whole of its logic is
//! [`drive`]: a function from the levels on the eight inputs to what the part
//! does to the four outputs. There is no clock and no `tick`.
//!
//! **An output is the level it leaves a pulled-up line at** (`SN7426-004`).
//! The part sinks a pin or lets go of it, so an output is `Low` sinking and
//! `High` released, and a released pin's High is the pull-up's. What a pin
//! then reads is resolved by [`pin_uv`] for a pull-up the consumer names — its
//! rail and its resistance — in one of two cases: the ideal part, or the
//! limits the data sheet guarantees: a released pin leaking the 12 V limit up
//! to 12 V and the 15 V one above it (`SN7426-OPEN-02`), and a sinking pin at
//! the one `VOL` maximum at every current up to 16 mA (`SN7426-OPEN-03`).
//! An input pin is High or Low and nothing between (`SN7426-OPEN-04`).
//!
//! # Examples
//!
//! ```
//! use sn7426::{Case, Inputs, Level, RailResistor, drive, pin_uv};
//! use units::{Microvolts, Ohms};
//!
//! // Gate 1 has both inputs High and sinks; gates 2 to 4 release their pins.
//! let y = drive(Inputs { a: 0b0001, b: 0b0001 });
//! assert_eq!(y, 0b1110);
//! // Pulled to 12 V through 10 kΩ, a released pin reads the rail.
//! let pull = RailResistor { rail_uv: Microvolts(12_000_000), resistance_ohms: Ohms(10_000) };
//! let y2 = Level::from(y & 0b0010 != 0);
//! assert_eq!(pin_uv(y2, pull, Case::Ideal), Some(Microvolts(12_000_000)));
//! ```

#![no_std]

use units::{Microvolts, Ohms, Picoamps, Picoseconds};

/// How many gates the package holds (`SN7426-001`).
pub const GATE_COUNT: usize = 4;

/// The four gates, in the low four bits of a byte; bit 0 is gate 1.
pub const GATE_MASK: u8 = 0x0F;

/// How many pins the package presents (`SN7426-002`).
pub const PIN_COUNT: usize = 14;

/// The highest voltage an output may be pulled to (`SN7426-005`).
pub const VOH_MAX_UV: Microvolts = Microvolts(15_000_000);

/// The most current a Low output is specified to sink (`SN7426-008`).
pub const IOL_MAX_PA: Picoamps = Picoamps(16_000_000_000);

/// The highest a Low output reads while sinking up to [`IOL_MAX_PA`]
/// (`SN7426-008`).
pub const VOL_MAX_UV: Microvolts = Microvolts(400_000);

/// One `IOH` row: the most an off output leaks with its pin at a voltage
/// (`SN7426-009`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IohRow {
    /// The pin voltage the row is stated at.
    pub voh_uv: Microvolts,
    /// The leakage limit there.
    pub max_pa: Picoamps,
}

/// The two `IOH` rows, at 12 V and at 15 V (`SN7426-009`).
pub const IOH_ROWS: [IohRow; 2] = [
    IohRow {
        voh_uv: Microvolts(12_000_000),
        max_pa: Picoamps(50_000_000),
    },
    IohRow {
        voh_uv: VOH_MAX_UV,
        max_pa: Picoamps(1_000_000_000),
    },
];

/// The supply range, `(min, max)` (`SN7426-006`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(4_750_000), Microvolts(5_250_000));
/// The nominal supply (`SN7426-006`).
pub const SUPPLY_TYP_UV: Microvolts = Microvolts(5_000_000);

/// The lowest input voltage guaranteed High (`SN7426-007`).
pub const VIH_MIN_UV: Microvolts = Microvolts(2_000_000);
/// The highest input voltage guaranteed Low (`SN7426-007`).
pub const VIL_MAX_UV: Microvolts = Microvolts(800_000);

/// Propagation delay to a rising output, typical and maximum, at a 1 kΩ,
/// 15 pF load (`SN7426-012`).
pub const TPLH_TYP_MAX_PS: (Picoseconds, Picoseconds) = (Picoseconds(16_000), Picoseconds(24_000));
/// Propagation delay to a falling output, typical and maximum, at a 1 kΩ,
/// 15 pF load (`SN7426-012`).
pub const TPHL_TYP_MAX_PS: (Picoseconds, Picoseconds) = (Picoseconds(11_000), Picoseconds(17_000));

/// A pin's level, as [`gate`] takes its inputs and returns its output. At an
/// output, `Low` is the transistor on and the pin sunk; `High` the transistor
/// off and the pin released, so the High is the pull-up's (`SN7426-004`).
pub use signal::Level;

/// The level one gate leaves its pulled-up output pin at, given the levels on
/// its two inputs (`SN7426-003`, `SN7426-004`). [`Level::Low`] is the output transistor on and the pin sunk toward
/// ground, [`Level::High`] the transistor off and the pin left to the pull-up,
/// whose High it is.
#[must_use]
pub const fn gate(a: Level, b: Level) -> Level {
    Level::from_bool(!(a.is_high() && b.is_high()))
}

/// The levels the outside world holds the part's eight input pins at.
///
/// A field is a level per bit, bit 0 for gate 1 through bit 3 for gate 4;
/// a set bit is a High pin, and no level lies between High and Low
/// (`SN7426-OPEN-04`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1A`–`4A`, bit `n` for gate `n + 1`, per [`GATE_MASK`].
    pub a: u8,
    /// `1B`–`4B`, bit `n` for gate `n + 1`.
    pub b: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { a: 0, b: 0 }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The levels the part leaves `1Y`–`4Y` at, one bit per gate per
/// [`GATE_MASK`]: [`gate`] for each, a clear bit sinking and a set bit
/// released. Each output depends on its own two inputs only (`SN7426-001`);
/// bits above [`GATE_MASK`] reach nothing.
#[must_use]
pub fn drive(inputs: Inputs) -> u8 {
    let mut y = 0;
    for n in 0..GATE_COUNT {
        let a = Level::from_bool((inputs.a >> n) & 1 == 1);
        let b = Level::from_bool((inputs.b >> n) & 1 == 1);
        if gate(a, b).is_high() {
            y |= 1 << n;
        }
    }
    y
}

/// A resistor from one output pin to a rail, both the consumer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RailResistor {
    /// The rail.
    pub rail_uv: Microvolts,
    /// The resistance.
    pub resistance_ohms: Ohms,
}

/// Which part [`pin_uv`] resolves an output against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Case {
    /// No leakage when off; the pin at ground when sinking.
    Ideal,
    /// The data sheet's limits: an off output leaking its maximum, a sinking
    /// one at [`VOL_MAX_UV`].
    Limit,
}

/// The sink current in whole microamperes, rounded up.
fn sink_ua(pull: RailResistor) -> Option<u32> {
    if pull.resistance_ohms.0 == 0 {
        return None;
    }
    let rail_uv = u64::try_from(pull.rail_uv.0).ok()?;
    let ua = rail_uv.div_ceil(pull.resistance_ohms.0);
    u32::try_from(ua).ok()
}

/// `IOH` maximum with the pin pulled to `voh_uv`: the 12 V row up to 12 V
/// and the 15 V row above it ([`IOH_ROWS`], `SN7426-009`, `SN7426-OPEN-02`).
/// `None` below zero
/// or above [`VOH_MAX_UV`].
#[must_use]
pub fn ioh_max_pa(voh_uv: Microvolts) -> Option<Picoamps> {
    if !(Microvolts(0)..=VOH_MAX_UV).contains(&voh_uv) {
        None
    } else if voh_uv <= IOH_ROWS[0].voh_uv {
        Some(IOH_ROWS[0].max_pa)
    } else {
        Some(IOH_ROWS[1].max_pa)
    }
}

/// The current a sinking output takes from `pull`, rounded up to a whole
/// microampere, with the pin taken at ground. `None` for a resistance of zero
/// or a rail below zero.
#[must_use]
pub fn sink_pa(pull: RailResistor) -> Option<Picoamps> {
    sink_ua(pull).map(|ua| Picoamps(i64::from(ua) * 1_000_000))
}

/// What an output pin reads, pulled up by `pull`, at `case`, given `y`, the
/// level the part leaves it at: what [`gate`] returns, High released and Low
/// sinking.
///
/// `None` where the sheet guarantees nothing: a rail below zero or above
/// [`VOH_MAX_UV`] (`SN7426-005`), a resistance of zero, or a Low asked to sink
/// more than [`IOL_MAX_PA`] (`SN7426-008`). A released pin at [`Case::Limit`]
/// reads the rail less the leakage limit across the resistor, rounded up to a
/// whole millivolt (`SN7426-009`), never below zero.
#[must_use]
pub fn pin_uv(y: Level, pull: RailResistor, case: Case) -> Option<Microvolts> {
    let leak_ua = u64::try_from(ioh_max_pa(pull.rail_uv)?.0 / 1_000_000).ok()?;
    let sink = sink_ua(pull)?;
    match (y, case) {
        (Level::High, Case::Ideal) => Some(pull.rail_uv),
        (Level::High, Case::Limit) => {
            let drop_mv = leak_ua
                .saturating_mul(pull.resistance_ohms.0)
                .div_ceil(1_000);
            let drop_uv = i32::try_from(drop_mv.saturating_mul(1_000)).unwrap_or(i32::MAX);
            Some(Microvolts(pull.rail_uv.0.saturating_sub(drop_uv).max(0)))
        }
        (Level::Low, _) if i64::from(sink) * 1_000_000 > IOL_MAX_PA.0 => None,
        (Level::Low, Case::Ideal) => Some(Microvolts(0)),
        (Level::Low, Case::Limit) => Some(VOL_MAX_UV.min(pull.rail_uv)),
    }
}
