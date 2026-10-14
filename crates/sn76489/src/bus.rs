//! The three control lines and the eight data lines, as levels.
//!
//! **A field is a level, not a condition.** `CE` and `WE` are drawn with bars
//! over them (`SN76489-001`), so their fields end `_n`.

use crate::Level;

/// What the outside world holds the part's bus pins at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// Pin 6. High is the state in which the part is *not* selected.
    /// The sheet's package drawing calls this pin `OE` and its pin table `CE`
    /// (`SN76489-003`).
    pub ce_n: Level,
    /// Pin 5. Its falling edge while `ce_n` is Low is the
    /// strobe.
    pub we_n: Level,
    /// `D0` to `D7`, with `D0` in bit 7 — the sheet's own most significant bit
    /// (`SN76489-002`).
    pub d: u8,
}

impl Inputs {
    /// The idle levels the function table's second condition names: `WE` and
    /// `CE` pulled High (`SN76489-015`).
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            ce_n: Level::High,
            we_n: Level::High,
            d: 0,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives back over one bus transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Outputs {
    /// Pin 4. It is open collector, so High is the level an
    /// external pull-up reaches when the part releases it (`SN76489-014`).
    pub ready: Level,
}
