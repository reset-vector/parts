//! The part's outer edge: what the outside world drives and sees during one
//! E cycle.
//!
//! The forty pins of `MC6803-002` reduce to four kinds here: the bus (port 3's
//! multiplexed byte, port 4's high byte, `R/W`), the three control inputs, and
//! the two data ports. `AS`, `E`, the crystal pins and the supplies carry nothing a cycle-stepped model needs:
//! `E` is the tick itself (`MC6803-003`) and `AS` is a fixed phase of it
//! (`MC6803-006`).

use core::fmt;

/// The control signals.
///
/// A **set flag means the condition it names holds**, and the empty set is
/// every pin at its idle level. Several pins are **active Low**: an
/// active-low pin's flag bears the pin's name and set means asserted (Low).
/// Each constant's doc says which pin level it is. The inversion happens at the package pin, which this type does not
/// model.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u8);

impl Ctrl {
    /// No condition present: a read cycle, nothing requested.
    pub const NONE: Self = Self(0);

    /// Out. `R/W` (`SC2`) is Low: [`Pins::d`] holds a byte for the
    /// addressed device (`MC6803-006`). Absent, the cycle is a read.
    pub const WRITE: Self = Self(1 << 0);

    /// In. `RESET` is Low: the part is held in reset (`MC6803-009`).
    pub const RESET: Self = Self(1 << 4);

    /// In. `NMI` is Low. The part responds to the High-to-Low transition
    /// between two samples and to nothing else (`MC6803-032`).
    pub const NMI: Self = Self(1 << 5);

    /// In. `IRQ1` is Low: a maskable interrupt is requested. A level, not an
    /// edge (`MC6803-032`).
    pub const IRQ1: Self = Self(1 << 6);

    /// Every signal in `other` is present here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
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
        const NAMES: [(Ctrl, &str); 4] = [
            (Ctrl::WRITE, "WRITE"),
            (Ctrl::RESET, "RESET"),
            (Ctrl::NMI, "NMI"),
            (Ctrl::IRQ1, "IRQ1"),
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

/// The pin state for one E cycle, in both directions.
///
/// The caller presents what the outside world drives — [`Ctrl::RESET`],
/// [`Ctrl::NMI`], [`Ctrl::IRQ1`], the levels on the two data ports, and
/// [`Pins::d`] when the part is reading an external address — and receives
/// back what the part drives: the address, `R/W`, the data bus on a write, and
/// the levels of the port lines it has been told to drive.
///
/// `Copy`, with no lifetime and no callback: a bus resolved through a callback
/// is a machine leaking into the chip.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Pins {
    /// The sixteen address lines: `A0`–`A7` on port 3 while `AS` is High and
    /// `A8`–`A15` on port 4 (`MC6803-006`). Every cycle carries one
    /// (`MC6803-007`).
    pub address: u16,
    /// The eight data lines, `D0`–`D7` on port 3 while `E` is High: `None`
    /// while nothing drives them.
    ///
    /// The part returns `Some` only during a write. During a read of an
    /// external address a caller answers with `Some(byte)`, and a read that
    /// finds `None` takes 0; during an internal read the part ignores it
    /// (`MC6803-008`).
    pub d: Option<u8>,
    /// The control signals.
    pub ctrl: Ctrl,
    /// The eight lines `P10`–`P17`.
    ///
    /// In: the levels at the pins, read for every bit whose direction is
    /// input. Out: the levels the part drives on the bits whose direction is
    /// output, with input bits echoed back as presented (`MC6803-038`).
    /// [`crate::Registers::port_1`] says which is which.
    pub p1: u8,
    /// The five lines `P20`–`P24`, in bits 0–4; bits 5–7 are never driven and
    /// are ignored on the way in (`MC6803-039`).
    ///
    /// `P20` is also the timer's input-capture edge (`MC6803-044`), `P21` the
    /// timer output level when its direction is output (`MC6803-043`), and
    /// `P22`–`P24` the serial interface's clock, receive and transmit lines
    /// when it is enabled (`MC6803-047`, `MC6803-048`).
    pub p2: u8,
}

/// `P22 P21 P20` at `010`: operating mode 2 (`MC6803-004`).
const MODE_2: u8 = 0b010;

impl Default for Pins {
    /// [`Pins::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

impl Pins {
    /// All lines quiet: both buses at zero, no request, port 1 Low and port
    /// 2 holding `P22 P21 P20` at `010`, so that a reset under these pins
    /// latches operating mode 2, the mode [`crate::Registers`]' `Default`
    /// stands in (`MC6803-004`).
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            address: 0,
            d: None,
            ctrl: Ctrl::NONE,
            p1: 0,
            p2: MODE_2,
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

    /// This word with the signals in `ctrl` cleared.
    #[must_use]
    pub const fn without(mut self, ctrl: Ctrl) -> Self {
        self.ctrl = self.ctrl.without(ctrl);
        self
    }

    /// This word with the port 1 levels replaced.
    #[must_use]
    pub const fn with_p1(mut self, levels: u8) -> Self {
        self.p1 = levels;
        self
    }

    /// This word with the port 2 levels replaced.
    #[must_use]
    pub const fn with_p2(mut self, levels: u8) -> Self {
        self.p2 = levels & 0x1f;
        self
    }
}
