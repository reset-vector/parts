//! The part's outer edge: the pins, as the outside world sees them during one
//! clock cycle.

use core::fmt;

/// The control signals, plus nothing else.
///
/// `MOS6502-001` names the forty pins and gives each a direction. Several are
/// **active Low** on the part; here a **set bit means the condition it names
/// holds**, and the empty set is every pin at its idle level. An active-Low
/// pin's flag bears the pin's name and is set while the pin is Low
/// ([`Ctrl::RES`], [`Ctrl::IRQ`], [`Ctrl::NMI`], [`Ctrl::SO`]); an active-High
/// pin's flag bears its name plus `_LOW` when set means Low
/// ([`Ctrl::RDY_LOW`]). Each constant's doc says which pin level that is.
///
/// The clock, power and ground of `MOS6502-001` are not here: `Ø0 IN` is the
/// `tick` itself, `Ø1 OUT` and `Ø2 OUT` are that same tick seen from outside
/// (`MOS6502-002`), and the supply pins carry no information. Pins 5, 35 and 36
/// are `N.C.`
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u8);

impl Ctrl {
    /// No condition present: a read cycle, nothing requested.
    pub const NONE: Self = Self(0);

    /// Out. `R/W` is Low: [`Pins::d`] holds a byte for the addressed device
    /// (`MOS6502-012`). Absent, the cycle is a read.
    pub const WRITE: Self = Self(1 << 0);

    /// Out. `SYNC` is High: this cycle is an op-code fetch (`MOS6502-003`).
    ///
    /// It is a real pin — pin 7 — and it is what the 6502 has where the
    /// MCS6501 has `BA` (`MOS6502-002`).
    pub const SYNC: Self = Self(1 << 1);

    /// In. `RES` is Low: the part is held in reset and writes nothing
    /// (`MOS6502-014`).
    pub const RES: Self = Self(1 << 3);

    /// In. `IRQ` is Low: a maskable interrupt is requested. A level, not an
    /// edge (`MOS6502-020`).
    pub const IRQ: Self = Self(1 << 4);

    /// In. `NMI` is Low: a non-maskable interrupt is requested.
    ///
    /// The part responds to the High-to-Low transition and to nothing else
    /// (`MOS6502-016`), and the line must be Low for at least two clock cycles
    /// (`MOS6502-017`).
    pub const NMI: Self = Self(1 << 5);

    /// In. `RDY` is Low: the part is not to run this cycle (`MOS6502-005`).
    ///
    /// A write cycle runs anyway and the part stops in the read cycle after it,
    /// which is the manufacturer's own sentence and not this crate's choice.
    pub const RDY_LOW: Self = Self(1 << 6);

    /// In. `S.O.` is Low.
    ///
    /// The part sets the overflow flag on the pin's **negative transition**
    /// (`MOS6502-006`), so a caller holds this bit for as long as it holds the
    /// pin Low and the flag is set once.
    pub const SO: Self = Self(1 << 7);

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
        const NAMES: [(Ctrl, &str); 7] = [
            (Ctrl::WRITE, "WRITE"),
            (Ctrl::SYNC, "SYNC"),
            (Ctrl::RES, "RES"),
            (Ctrl::IRQ, "IRQ"),
            (Ctrl::NMI, "NMI"),
            (Ctrl::RDY_LOW, "RDY_LOW"),
            (Ctrl::SO, "SO"),
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
/// [`Ctrl::IRQ`], [`Ctrl::NMI`], [`Ctrl::RDY_LOW`], [`Ctrl::SO`], and
/// [`Pins::d`] when the part is reading — and receives back the pins the
/// part drives: the address bus, `R/W`, `SYNC`, and the data bus on a write.
///
/// `Copy`, with no lifetime and no callback: a bus resolved through a callback
/// is a machine leaking into the chip.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub struct Pins {
    /// The sixteen address outputs, `AB0`–`AB15`.
    ///
    /// The part always drives a known address (`MOS6502-009`), which is why the
    /// package has no valid-memory-address output.
    pub address: u16,
    /// The eight bidirectional data lines, `DB0`–`DB7`: `None` while nothing
    /// drives them.
    ///
    /// The part returns `Some` only during a write. A board answers a read
    /// with `Some(byte)` ([`Pins::with_d`]); a read presented `None` takes
    /// `0x00`.
    pub d: Option<u8>,
    /// The control signals.
    pub ctrl: Ctrl,
}

impl Pins {
    /// All lines quiet, the address bus at zero and the data bus undriven.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            address: 0,
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

    /// This word with the signals in `ctrl` also set.
    #[must_use]
    pub const fn with(mut self, ctrl: Ctrl) -> Self {
        self.ctrl = self.ctrl.with(ctrl);
        self
    }
}
