//! The signal words the chip crates share that are not quantities: a pin's
//! logic level, that level where a part may leave it undetermined, four bits.
//!
//! Each is defined here once, so one part's output is another's input with no
//! conversion, and a part's crate re-exports what its pins carry. What a level
//! or a nibble means on a given pin is that part's to say; nothing here
//! computes a part's behaviour.
//!
//! # Examples
//!
//! ```
//! use signal::{Level, Logic, Nibble};
//!
//! assert_eq!(Level::from(true), Level::High);
//! assert_eq!(Logic::from(Level::Low), Logic::Low);
//! assert_eq!(Logic::default(), Logic::Undetermined);
//! assert_eq!(Level::try_from(Logic::High), Ok(Level::High));
//! assert_eq!(Nibble::new(0x5A).value(), 0xA);
//! ```

#![no_std]

/// A logic level a pin stands at, or that a part holds where its sheet gives
/// it as one.
///
/// There is no `Default`: which level a pin stands at before anything drives
/// it is the part's to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// Low.
    Low,
    /// High.
    High,
}

impl Level {
    /// `High` for `true` and `Low` for `false`.
    #[must_use]
    pub const fn from_bool(high: bool) -> Self {
        if high { Self::High } else { Self::Low }
    }

    /// Whether the level is [`Level::High`].
    #[must_use]
    pub const fn is_high(self) -> bool {
        matches!(self, Self::High)
    }

    /// Whether the level is [`Level::Low`].
    #[must_use]
    pub const fn is_low(self) -> bool {
        matches!(self, Self::Low)
    }
}

impl core::ops::Not for Level {
    type Output = Self;

    /// The other level.
    fn not(self) -> Self {
        match self {
            Self::Low => Self::High,
            Self::High => Self::Low,
        }
    }
}

impl From<bool> for Level {
    /// `High` for `true` and `Low` for `false`.
    fn from(high: bool) -> Self {
        Self::from_bool(high)
    }
}

/// A logic level, or none, where the part does not determine one: a level
/// between the input thresholds, a state the sheet gives none for, a
/// power-up the sheet does not fix.
///
/// [`Logic::Undetermined`] is not a third level, and not a released line,
/// which is `None` on the pin. A part whose pins always stand at a level
/// takes [`Level`] instead. `Default` is `Undetermined`, a part at power-up
/// that no sheet fixes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Logic {
    /// Low.
    Low,
    /// High.
    High,
    /// No level the part determines.
    #[default]
    Undetermined,
}

impl Logic {
    /// `High` for `true` and `Low` for `false`.
    #[must_use]
    pub const fn from_bool(high: bool) -> Self {
        if high { Self::High } else { Self::Low }
    }

    /// Whether the level is [`Logic::High`]: false for
    /// [`Logic::Undetermined`], so `!is_high()` is not [`Logic::is_low`].
    #[must_use]
    pub const fn is_high(self) -> bool {
        matches!(self, Self::High)
    }

    /// Whether the level is [`Logic::Low`]: false for
    /// [`Logic::Undetermined`].
    #[must_use]
    pub const fn is_low(self) -> bool {
        matches!(self, Self::Low)
    }
}

impl core::ops::Not for Logic {
    type Output = Self;

    /// The other level, and `Undetermined` for `Undetermined`, as an
    /// inverter's output is.
    fn not(self) -> Self {
        match self {
            Self::Low => Self::High,
            Self::High => Self::Low,
            Self::Undetermined => Self::Undetermined,
        }
    }
}

impl From<bool> for Logic {
    /// `High` for `true` and `Low` for `false`.
    fn from(high: bool) -> Self {
        Self::from_bool(high)
    }
}

impl From<Level> for Logic {
    /// The same level, determined.
    fn from(level: Level) -> Self {
        match level {
            Level::Low => Self::Low,
            Level::High => Self::High,
        }
    }
}

/// A [`Logic::Undetermined`] read where a [`Level`] was wanted: the error of
/// `Level::try_from(Logic)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct UndeterminedError;

impl core::fmt::Display for UndeterminedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("the part determines no level")
    }
}

impl core::error::Error for UndeterminedError {}

impl TryFrom<Logic> for Level {
    type Error = UndeterminedError;

    /// The same level, or [`UndeterminedError`] where the part determines none.
    ///
    /// ```
    /// use signal::{Level, Logic};
    ///
    /// assert_eq!(Level::try_from(Logic::High), Ok(Level::High));
    /// assert!(Level::try_from(Logic::Undetermined).is_err());
    /// ```
    fn try_from(logic: Logic) -> Result<Self, UndeterminedError> {
        match logic {
            Logic::Low => Ok(Self::Low),
            Logic::High => Ok(Self::High),
            Logic::Undetermined => Err(UndeterminedError),
        }
    }
}

/// Four bits, in bits 3–0 of a `u8`: every one of the sixteen values is one.
///
/// It orders and formats as its value: `{:x}`, `{:X}`, `{:o}` and `{:b}` take
/// the same flags as on a `u8`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Nibble(u8);

impl Nibble {
    /// The nibble `bits`' low four bits make.
    #[must_use]
    pub const fn new(bits: u8) -> Self {
        Self(bits & 0xF)
    }

    /// Its value, 0 to 15.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl From<Nibble> for u8 {
    /// Its value, 0 to 15.
    fn from(nibble: Nibble) -> Self {
        nibble.0
    }
}

/// Formats as the value, through `u8`'s own impl so every flag holds.
macro_rules! nibble_fmt {
    ($($trait:ident),*) => {$(
        impl core::fmt::$trait for Nibble {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::$trait::fmt(&self.0, f)
            }
        }
    )*};
}

nibble_fmt!(LowerHex, UpperHex, Octal, Binary);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_is_high_for_true() {
        assert_eq!(Level::from(true), Level::High);
        assert_eq!(Level::from(false), Level::Low);
        assert!(Level::High.is_high());
        assert!(!Level::Low.is_high());
        assert!(Level::Low.is_low());
        assert!(!Level::High.is_low());
    }

    #[test]
    fn not_gives_the_other_level_and_keeps_undetermined() {
        assert_eq!(!Level::Low, Level::High);
        assert_eq!(!Level::High, Level::Low);
        assert_eq!(!Logic::Low, Logic::High);
        assert_eq!(!Logic::High, Logic::Low);
        assert_eq!(!Logic::Undetermined, Logic::Undetermined);
        assert!(!Logic::Undetermined.is_low());
        assert!(!Logic::Undetermined.is_high());
    }

    #[test]
    fn a_logic_level_is_high_for_true_and_undetermined_by_default() {
        assert_eq!(Logic::from(true), Logic::High);
        assert_eq!(Logic::from(false), Logic::Low);
        assert!(!Logic::Undetermined.is_high());
        assert_eq!(Logic::default(), Logic::Undetermined);
        assert_eq!(Logic::from(Level::High), Logic::High);
    }

    #[test]
    fn a_determined_logic_level_converts_to_a_level_and_undetermined_does_not() {
        assert_eq!(Level::try_from(Logic::Low), Ok(Level::Low));
        assert_eq!(Level::try_from(Logic::High), Ok(Level::High));
        assert_eq!(Level::try_from(Logic::Undetermined), Err(UndeterminedError));
        for level in [Level::Low, Level::High] {
            assert_eq!(Level::try_from(Logic::from(level)), Ok(level));
        }
    }

    #[test]
    fn a_nibble_orders_converts_and_formats_as_its_value() {
        extern crate std;
        use std::format;
        let a = Nibble::new(0xA);
        assert!(Nibble::new(3) < a);
        assert_eq!(u8::from(a), 0xA);
        assert_eq!(format!("{a:x} {a:X} {a:o} {a:#06b}"), "a A 12 0b1010");
    }

    #[test]
    fn a_nibble_keeps_the_low_four_bits() {
        assert_eq!(Nibble::new(0xFF).value(), 0xF);
        assert_eq!(Nibble::default().value(), 0);
    }
}
