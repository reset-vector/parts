use signal::Level;

/// The four operating modes `/CS`, `/WE` and `A0` select (`YM2413-004`).
///
/// The manual's Table II-1 lists four rows and leaves one combination out of
/// its own numbering: `/CS` high is inactive whatever the other two do, so the
/// decode below folds that row in rather than inventing a fifth mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum BusMode {
    /// `/CS` high. The data bus is at high impedance.
    #[default]
    Inactive,
    /// `/CS` low, `/WE` low, `A0` low. The byte on the bus is a register address.
    AddressWrite,
    /// `/CS` low, `/WE` low, `A0` high. The byte on the bus enters the register
    /// the last address write selected.
    DataWrite,
    /// `/CS` low, `/WE` high, `A0` low. The part drives test data on `D0`, `D1`;
    /// the manual marks it "Normally not used".
    TestRead,
    /// `/CS` low, `/WE` high, `A0` high. The manual's "Bus inhibited": the bus
    /// is at high impedance and its contents are, in its words, meaningless.
    BusInhibited,
}

impl BusMode {
    /// Decode the three control pins. All three are as the bus master drives
    /// them; `cs_n` and `we_n` are active-Low, matching the pin names.
    #[must_use]
    pub(crate) fn decode(cs_n: Level, we_n: Level, a0: Level) -> Self {
        use Level::{High, Low};
        match (cs_n, we_n, a0) {
            (High, _, _) => Self::Inactive,
            (Low, Low, Low) => Self::AddressWrite,
            (Low, Low, High) => Self::DataWrite,
            (Low, High, Low) => Self::TestRead,
            (Low, High, High) => Self::BusInhibited,
        }
    }

    /// The write this mode is, where the bus master drives the data bus.
    #[must_use]
    pub(crate) fn write_mode(self) -> Option<WriteMode> {
        match self {
            Self::AddressWrite => Some(WriteMode::Address),
            Self::DataWrite => Some(WriteMode::Data),
            Self::Inactive | Self::TestRead | Self::BusInhibited => None,
        }
    }

    /// Whether the part's `D0`–`D7` sit at high impedance in this mode
    /// (`YM2413-009`).
    #[cfg(test)]
    #[must_use]
    pub(crate) fn floats(self) -> bool {
        matches!(self, Self::Inactive | Self::BusInhibited)
    }
}

/// The two operating modes that write (`YM2413-004`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WriteMode {
    /// `/CS` low, `/WE` low, `A0` low. The byte on the bus is a register address.
    Address,
    /// `/CS` low, `/WE` low, `A0` high. The byte on the bus enters the register
    /// the last address write selected.
    Data,
}

/// Master clock cycles a write owes after an address write (`YM2413-007`).
pub const WAIT_ADDRESS_CYCLES: usize = 12;

/// Master clock cycles a write owes after a data write (`YM2413-008`).
pub const WAIT_DATA_CYCLES: usize = 84;

/// The wait a write owes the part before the next bus operation, counted in
/// master clocks (`YM2413-007`, `YM2413-008`).
///
/// The manual states the two figures and says only that "if the wait time is
/// not observed data will be uncertain". This type is the accounting and not a
/// gate: the machine takes every write it is handed and reports whether the
/// wait was owed. What the part does with a write inside the wait is
/// `YM2413-OPEN-01`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Wait {
    remaining: usize,
}

impl Wait {
    /// No wait owed.
    pub(crate) const NONE: Self = Self { remaining: 0 };

    /// Charge the wait a mode owes on a write, replacing any wait still owed.
    pub(crate) fn charge(&mut self, mode: WriteMode) {
        self.remaining = match mode {
            WriteMode::Address => WAIT_ADDRESS_CYCLES,
            WriteMode::Data => WAIT_DATA_CYCLES,
        };
    }

    /// One master clock elapses.
    pub(crate) fn tick(&mut self) {
        self.remaining = self.remaining.saturating_sub(1);
    }

    /// Master clock cycles still owed before the bus may be used again.
    #[must_use]
    pub fn remaining_cycles(self) -> usize {
        self.remaining
    }

    /// Whether a bus operation right now would violate the stated wait.
    #[must_use]
    pub fn is_owed(self) -> bool {
        self.remaining > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_pins_select_five_modes() {
        use Level::{High, Low};
        for we_n in [Low, High] {
            for a0 in [Low, High] {
                assert_eq!(BusMode::decode(High, we_n, a0), BusMode::Inactive);
            }
        }
        assert_eq!(BusMode::decode(Low, Low, Low), BusMode::AddressWrite);
        assert_eq!(BusMode::decode(Low, Low, High), BusMode::DataWrite);
        assert_eq!(BusMode::decode(Low, High, Low), BusMode::TestRead);
        assert_eq!(BusMode::decode(Low, High, High), BusMode::BusInhibited);

        assert_eq!(BusMode::AddressWrite.write_mode(), Some(WriteMode::Address));
        assert_eq!(BusMode::DataWrite.write_mode(), Some(WriteMode::Data));
        assert_eq!(BusMode::TestRead.write_mode(), None);

        assert!(BusMode::Inactive.floats());
        assert!(BusMode::BusInhibited.floats());
        assert!(!BusMode::TestRead.floats());
    }
}
