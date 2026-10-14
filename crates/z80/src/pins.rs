//! The part's outer edge: the pins, as the outside world sees them during one
//! T-state.

use core::fmt;

/// The control outputs the part releases on a bus acknowledge, with the
/// address and data buses: its tristate group.
///
/// `Z80-002`. Whether `RFSH` belongs here is `Z80-OPEN-01`, under which this
/// crate leaves it out.
pub const TRISTATE_CONTROLS: Ctrl = Ctrl(Ctrl::MREQ.0 | Ctrl::IORQ.0 | Ctrl::RD.0 | Ctrl::WR.0);

/// The thirteen control signals, plus nothing else.
///
/// `Z80-001` names them and gives each a direction. On the part every one is
/// **active Low**; here a **set flag means asserted** (the pin Low), and the
/// empty set is every pin at its idle level, High. The inversion happens at
/// the package pin, which this type does not model — a board that wants the
/// electrical level takes the complement of these bits.
///
/// The clock, power and ground of `Z80-001` are not here: the clock is the
/// `tick` itself and the supply pins carry no information.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u16);

impl Ctrl {
    /// No signal asserted.
    pub const NONE: Self = Self(0);

    /// Out. This machine cycle is an opcode fetch.
    pub const M1: Self = Self(1 << 0);
    /// Out. The address bus holds a valid memory address.
    pub const MREQ: Self = Self(1 << 1);
    /// Out. The low half of the address bus holds a valid I/O port address.
    pub const IORQ: Self = Self(1 << 2);
    /// Out. The addressed device should place its byte on the data bus.
    pub const RD: Self = Self(1 << 3);
    /// Out. The data bus holds a byte for the addressed device to store.
    pub const WR: Self = Self(1 << 4);
    /// Out. The address bus holds a refresh address (`Z80-004`).
    pub const RFSH: Self = Self(1 << 5);
    /// Out. The CPU has executed a `HALT` and is waiting for an interrupt.
    pub const HALT: Self = Self(1 << 6);
    /// Out. The CPU has released the buses in answer to [`Ctrl::BUSREQ`].
    pub const BUSACK: Self = Self(1 << 7);

    /// In. The addressed device is not ready; the CPU extends the cycle (`Z80-007`).
    pub const WAIT: Self = Self(1 << 8);
    /// In. A maskable interrupt is requested. A level, not an edge (`Z80-070`).
    pub const INT: Self = Self(1 << 9);
    /// In. A non-maskable interrupt is requested. Edge-triggered (`Z80-066`).
    pub const NMI: Self = Self(1 << 10);
    /// In. Another device asks for the buses (`Z80-008`).
    pub const BUSREQ: Self = Self(1 << 11);
    /// In. Reset. Must be held three full clock cycles to take (`Z80-010`).
    pub const RESET: Self = Self(1 << 12);

    /// Every signal in `other` is asserted here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// At least one signal in `other` is asserted here.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// This word with the signals in `other` also asserted.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// This word with the signals in `other` deasserted.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl core::ops::BitOr for Ctrl {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.with(rhs)
    }
}

impl core::ops::BitOrAssign for Ctrl {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.with(rhs);
    }
}

/// Named, so a failing assertion prints signals rather than a hex word.
impl fmt::Debug for Ctrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const NAMES: [(Ctrl, &str); 13] = [
            (Ctrl::M1, "M1"),
            (Ctrl::MREQ, "MREQ"),
            (Ctrl::IORQ, "IORQ"),
            (Ctrl::RD, "RD"),
            (Ctrl::WR, "WR"),
            (Ctrl::RFSH, "RFSH"),
            (Ctrl::HALT, "HALT"),
            (Ctrl::BUSACK, "BUSACK"),
            (Ctrl::WAIT, "WAIT"),
            (Ctrl::INT, "INT"),
            (Ctrl::NMI, "NMI"),
            (Ctrl::BUSREQ, "BUSREQ"),
            (Ctrl::RESET, "RESET"),
        ];
        if self.0 == 0 {
            return f.write_str("-");
        }
        let mut first = true;
        for (bit, name) in NAMES {
            if self.contains(bit) {
                if !first {
                    f.write_str("|")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        Ok(())
    }
}

/// What the part drives on `A15`–`A0` while it has not released them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Address {
    /// The address, valid for the cycle the strobes name.
    Stable(u16),
    /// Driven, at a level the part does not determine.
    Undetermined,
}

impl Address {
    /// The address, where it is [`Address::Stable`].
    #[must_use]
    pub const fn stable(self) -> Option<u16> {
        match self {
            Self::Stable(a) => Some(a),
            Self::Undetermined => None,
        }
    }
}

/// The pin state for one T-state, in both directions.
///
/// The caller presents the pins the outside world drives — [`Ctrl::WAIT`],
/// [`Ctrl::INT`], [`Ctrl::NMI`], [`Ctrl::BUSREQ`], [`Ctrl::RESET`], and `data`
/// when the CPU is reading — and receives back the pins the CPU drives.
///
/// `Copy`, with no lifetime and no callback: a bus resolved through a callback
/// is a machine leaking into the chip.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub struct Pins {
    /// The sixteen address outputs: `None` while the part has released them,
    /// as it has while it drives [`Ctrl::BUSACK`] and after a tick the caller
    /// presented [`Ctrl::RESET`] to (`Z80-011`, `Z80-002`), and
    /// [`Address::Undetermined`] in an internal T-state, which drives no
    /// strobe.
    pub address: Option<Address>,
    /// The eight bidirectional data lines: `None` while nothing drives them.
    ///
    /// The CPU returns `Some` only during a write. A caller answers a read with
    /// `Some(byte)`; a read that finds `None` takes 0.
    pub d: Option<u8>,
    /// The thirteen control signals.
    pub ctrl: Ctrl,
}

impl Pins {
    /// All lines quiet and both buses undriven.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            address: None,
            d: None,
            ctrl: Ctrl::NONE,
        }
    }

    /// This word with `d` driven to `d` — how a board answers a read.
    #[must_use]
    pub const fn with_d(mut self, d: u8) -> Self {
        self.d = Some(d);
        self
    }

    /// This word with the signals in `ctrl` also asserted.
    #[must_use]
    pub const fn with(mut self, ctrl: Ctrl) -> Self {
        self.ctrl = self.ctrl.with(ctrl);
        self
    }
}
