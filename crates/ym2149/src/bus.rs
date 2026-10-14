use crate::Level;

/// The bus control decoder's eight rows (`YM2149-006`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BusFunction {
    Inactive,
    LatchAddress,
    Read,
    Write,
}

impl BusFunction {
    #[must_use]
    pub(crate) fn decode(bdir: bool, bc2: bool, bc1: bool) -> Self {
        match (bdir, bc2, bc1) {
            (false, false, false) | (false, true, false) | (true, false, true) => Self::Inactive,
            (false, false, true) | (true, false, false) | (true, true, true) => Self::LatchAddress,
            (false, true, true) => Self::Read,
            (true, true, false) => Self::Write,
        }
    }
}

/// Every pin the bus side of the part samples in one transaction.
///
/// `d` is DA7..DA0 as the bus master drives it: the address during a latch,
/// the byte during a write, ignored during a read. `ioa`/`iob` are the
/// resolved IOA7–IOA0/IOB7–IOB0 pin levels, which a read of R14/R15 returns.
/// `sel_n` is `/SEL`: High passes `CLOCK` as the master clock, Low halves it
/// (`YM2149-003`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// The BDIR pin.
    pub bdir: Level,
    /// The BC2 pin.
    pub bc2: Level,
    /// The BC1 pin.
    pub bc1: Level,
    /// The /A9 pin.
    pub a9_n: Level,
    /// The A8 pin.
    pub a8: Level,
    /// DA7–DA0 as the bus master drives them, DA7 the top bit.
    pub d: u8,
    /// IOA7–IOA0 as the board resolves them, IOA7 the top bit.
    pub ioa: u8,
    /// IOB7–IOB0 as the board resolves them, IOB7 the top bit.
    pub iob: u8,
    /// The /RESET pin.
    pub reset_n: Level,
    /// The /SEL pin.
    pub sel_n: Level,
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// Bus idle, the part deselected, ports pulled high, reset released,
    /// `/SEL` High as its pull-up leaves it open (`YM2149-005`).
    ///
    /// `a9_n` High and `a8` Low deselect the part: a latch under them takes no
    /// address. `a9_n` Low and `a8` High select it, and they are also the
    /// levels the on-chip pull-down and pull-up give the two pins left open,
    /// so a board that does not decode them presents those (`YM2149-005`,
    /// `YM2149-007`); [`Inputs::latch_access`] latches under them. No transaction
    /// starts until BDIR, BC2 and BC1 leave the inactive code.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            bdir: Level::Low,
            bc2: Level::Low,
            bc1: Level::Low,
            a9_n: Level::High,
            a8: Level::Low,
            d: 0,
            ioa: 0xFF,
            iob: 0xFF,
            reset_n: Level::High,
            sel_n: Level::High,
        }
    }

    /// A latch access of the address `d` under the common address code, `/A9`
    /// Low and A8 High; a board that decodes the two pins sets them on the
    /// word.
    #[must_use]
    pub const fn latch_access(d: u8) -> Self {
        Self {
            bc1: Level::High,
            a9_n: Level::Low,
            a8: Level::High,
            d,
            ..Self::idle()
        }
    }

    /// A write access of `d` to the latched register.
    #[must_use]
    pub const fn write_access(d: u8) -> Self {
        Self {
            bdir: Level::High,
            bc2: Level::High,
            d,
            ..Self::idle()
        }
    }

    /// A read access of the latched register.
    #[must_use]
    pub const fn read_access() -> Self {
        Self {
            bc2: Level::High,
            bc1: Level::High,
            ..Self::idle()
        }
    }

    /// This word with BDIR at `bdir`.
    #[must_use]
    pub const fn with_bdir(self, bdir: Level) -> Self {
        Self { bdir, ..self }
    }

    /// This word with BC2 at `bc2`.
    #[must_use]
    pub const fn with_bc2(self, bc2: Level) -> Self {
        Self { bc2, ..self }
    }

    /// This word with BC1 at `bc1`.
    #[must_use]
    pub const fn with_bc1(self, bc1: Level) -> Self {
        Self { bc1, ..self }
    }

    /// This word with IOA7–IOA0 at `ioa`.
    #[must_use]
    pub const fn with_ioa(self, ioa: u8) -> Self {
        Self { ioa, ..self }
    }

    /// This word with IOB7–IOB0 at `iob`.
    #[must_use]
    pub const fn with_iob(self, iob: u8) -> Self {
        Self { iob, ..self }
    }

    /// This word with /RESET at `reset_n`.
    #[must_use]
    pub const fn with_reset_n(self, reset_n: Level) -> Self {
        Self { reset_n, ..self }
    }

    /// This word with /SEL at `sel_n`.
    #[must_use]
    pub const fn with_sel_n(self, sel_n: Level) -> Self {
        Self { sel_n, ..self }
    }
}

/// What the part drives on its output pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Outputs {
    /// DA7..DA0: `Some` while the part drives them, on a read with a latched
    /// address; `None` is the bus at high impedance.
    pub d: Option<u8>,
    /// IOA7–IOA0: R14 while R7 bit 6 sets port A to output; `None` while it
    /// is an input.
    pub ioa: Option<u8>,
    /// IOB7–IOB0: R15 while R7 bit 7 sets port B to output; `None` while it
    /// is an input.
    pub iob: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::{BusFunction, Inputs};

    #[test]
    fn each_port_copy_sets_its_own_group_alone() {
        let word = Inputs::idle().with_ioa(0x12);
        assert_eq!((word.ioa, word.iob), (0x12, 0xFF));
        let word = Inputs::idle().with_iob(0x34);
        assert_eq!((word.ioa, word.iob), (0xFF, 0x34));
    }

    /// `YM2149-006`: the eight codes of BDIR, BC2, BC1 in order.
    #[test]
    fn all_eight_bus_control_codes_decode_to_their_function() {
        let table = [
            BusFunction::Inactive,
            BusFunction::LatchAddress,
            BusFunction::Inactive,
            BusFunction::Read,
            BusFunction::LatchAddress,
            BusFunction::Inactive,
            BusFunction::Write,
            BusFunction::LatchAddress,
        ];
        for (code, expected) in table.into_iter().enumerate() {
            let (bdir, bc2, bc1) = (code & 4 != 0, code & 2 != 0, code & 1 != 0);
            assert_eq!(
                BusFunction::decode(bdir, bc2, bc1),
                expected,
                "code {code:03b}"
            );
        }
    }
}
