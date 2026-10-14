//! The part's outer edge: the pins, as the outside world sees them during one
//! clock cycle.

use core::fmt;

/// The control signals, plus nothing else.
///
/// `MOS6510-001` names the forty pins and gives each a direction. Several are
/// **active Low** on the part; here a **set bit means the condition it names
/// holds**, and the empty set is every pin at its idle level. An active-Low
/// pin's flag bears the pin's name and is set while the pin is Low
/// ([`Ctrl::RES`], [`Ctrl::IRQ`], [`Ctrl::NMI`]); an active-High pin's flag
/// bears its name plus `_LOW` when set means Low ([`Ctrl::AEC_LOW`],
/// [`Ctrl::RDY_LOW`]). Each constant's doc says which pin level that is.
///
/// The clock, power and ground of `MOS6510-001` are not here: the clock is the
/// `tick` itself and the supply pins carry no information. `Ø2 OUT` is not here
/// either, for the same reason — it is this crate's `tick` seen from outside.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u8);

impl Ctrl {
    /// No condition present: a read cycle, buses driven, nothing requested.
    pub const NONE: Self = Self(0);

    /// Out. `R/W` is Low: [`Pins::d`] holds a byte for the addressed device
    /// (`MOS6510-006`). Absent, the cycle is a read.
    pub const WRITE: Self = Self(1 << 0);

    /// In. `RES` is Low: the part is held in reset (`MOS6510-010`).
    pub const RES: Self = Self(1 << 2);

    /// In. `IRQ` is Low: a maskable interrupt is requested. A level, not an
    /// edge (`MOS6510-012`).
    pub const IRQ: Self = Self(1 << 3);

    /// In. `NMI` is Low: a non-maskable interrupt is requested.
    ///
    /// The pin is `MOS6510-014`. Where it vectors and whether it is edge
    /// triggered is `MOS6510-OPEN-08`: this crate's choice is to latch a
    /// High-to-Low edge and vector through `$FFFA`.
    pub const NMI: Self = Self(1 << 4);

    /// In. `AEC` is Low: the outside world has taken the buses, and the part
    /// releases them (`MOS6510-004`).
    pub const AEC_LOW: Self = Self(1 << 5);

    /// In. `RDY` is Low. This crate stalls before a read cycle and runs a write
    /// cycle to completion, its choice under `MOS6510-OPEN-03`.
    pub const RDY_LOW: Self = Self(1 << 6);

    /// Every signal in `other` is present here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// At least one signal in `other` is present here.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// This word with the signals in `other` also set.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// This word with the signals in `other` cleared.
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
        const NAMES: [(Ctrl, &str); 6] = [
            (Ctrl::WRITE, "WRITE"),
            (Ctrl::RES, "RES"),
            (Ctrl::IRQ, "IRQ"),
            (Ctrl::NMI, "NMI"),
            (Ctrl::AEC_LOW, "AEC_LOW"),
            (Ctrl::RDY_LOW, "RDY_LOW"),
        ];
        if self.0 == 0 {
            return f.write_str("read");
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

/// The pin state for one clock cycle, in both directions.
///
/// The caller presents the pins the outside world drives — [`Ctrl::RES`],
/// [`Ctrl::IRQ`], [`Ctrl::NMI`], [`Ctrl::AEC_LOW`], [`Ctrl::RDY_LOW`], the
/// [`Pins::p`] lines the direction register makes inputs, and [`Pins::d`]
/// when the part is reading — and receives back the pins the part drives.
///
/// `Copy`, with no lifetime and no callback: a bus resolved through a callback
/// is a machine leaking into the chip.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub struct Pins {
    /// The sixteen address outputs, `A0`–`A15`: `None` while
    /// [`Ctrl::AEC_LOW`] is presented, the part having released the bus
    /// (`MOS6510-004`, [`crate::Mos6510::is_released`]).
    pub address: Option<u16>,
    /// The eight bidirectional data lines, `DB0`–`DB7`: `None` while nothing
    /// drives them.
    ///
    /// The part returns `Some` only during a write. A board answers a read
    /// with `Some(byte)` ([`Pins::with_d`]); a read presented `None` takes
    /// `0x00`.
    pub d: Option<u8>,
    /// The control signals.
    pub ctrl: Ctrl,
    /// The peripheral port lines, `P0` upward.
    ///
    /// On the way in, the level the outside world holds each line at; on the
    /// way out, the level the line stands at — the output register's bit where
    /// the direction register makes the line an output, and the caller's own
    /// bit where it makes it an input (`MOS6510-020`).
    ///
    /// Eight bits, because both registers are eight bits wide
    /// (`MOS6510-018`). Which of them reach a package pin is the variant's
    /// business (`MOS6510-002`).
    pub p: u8,
}

impl Pins {
    /// All lines quiet, both buses undriven, and every port line Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            address: None,
            d: None,
            ctrl: Ctrl::NONE,
            p: 0,
        }
    }

    /// This word with `d` driven to `d` — how a board answers a read.
    #[must_use]
    pub const fn with_d(mut self, d: u8) -> Self {
        self.d = Some(d);
        self
    }

    /// This word with the signals in `ctrl` also set.
    #[must_use]
    pub const fn with(mut self, ctrl: Ctrl) -> Self {
        self.ctrl = self.ctrl.with(ctrl);
        self
    }

    /// This word with the port lines the outside world holds replaced.
    #[must_use]
    pub const fn with_p(mut self, levels: u8) -> Self {
        self.p = levels;
        self
    }
}
