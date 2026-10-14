//! The part's outer edge: the pins, as the outside world sees them during one
//! `Ø2` cycle.

use core::fmt;

/// The control signals, plus nothing else.
///
/// `MOS6526-001` names the forty pins. A **set flag means the condition it
/// names holds**, and the empty set is every pin at its idle level. An
/// active-Low pin's flag bears the pin's name and set means asserted (Low):
/// `CS`, `RES`, `FLAG`, `IRQ`, `PC`. An active-High pin's flag bears its name
/// and set means High where the pin idles Low (`TOD`), and its name plus
/// `_LOW` and set means Low where it idles High (`CNT_LOW`, `SP_LOW`).
/// `WRITE` is `R/W` Low. Each constant's doc says which pin level it is.
///
/// `Ø2` is not here: it is the `tick` itself. Power and ground carry no
/// information. `RS3`–`RS0`, the data bus and the two ports are the other
/// fields of [`Pins`].
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u16);

impl Ctrl {
    /// No condition present: not selected, `TOD` Low, every other line High.
    pub const NONE: Self = Self(0);

    /// In. `CS` is Low: the bus access in this cycle is this part's
    /// (`MOS6526-004`). Absent, the part ignores `R/W`, `RS3`–`RS0` and the
    /// data bus.
    pub const CS: Self = Self(1 << 0);

    /// In. `R/W` is Low: the access is a write, [`Pins::d`] holds the byte
    /// (`MOS6526-005`). Absent, the access is a read.
    pub const WRITE: Self = Self(1 << 1);

    /// In. `RES` is Low: the part is held in reset (`MOS6526-009`).
    pub const RES: Self = Self(1 << 2);

    /// In. `FLAG` is Low. The part acts on the High-to-Low transition and not
    /// on the level (`MOS6526-019`).
    pub const FLAG: Self = Self(1 << 3);

    /// In. `TOD` is High (`MOS6526-033`). The part counts the Low-to-High
    /// transition, which is `MOS6526-OPEN-04`'s choice.
    pub const TOD: Self = Self(1 << 4);

    /// In and out. `CNT` is Low.
    ///
    /// On the way in, the outside world pulls the line Low; absent, nobody
    /// does and it is High, the pin being open drain (`MOS6526-011`). On the
    /// way out, the line stands Low: the outside pulls it Low, or the part
    /// does when the serial port is in output mode (`MOS6526-040`).
    pub const CNT_LOW: Self = Self(1 << 5);

    /// In and out. `SP` is Low, on the same terms as [`Ctrl::CNT_LOW`]
    /// (`MOS6526-011`, `MOS6526-039`, `MOS6526-041`).
    pub const SP_LOW: Self = Self(1 << 6);

    /// Out. `IRQ` is Low: an enabled interrupt stands in the data register
    /// (`MOS6526-008`, `MOS6526-043`).
    pub const IRQ: Self = Self(1 << 7);

    /// Out. `PC` is Low: port B was read or written in the previous cycle
    /// (`MOS6526-018`).
    pub const PC: Self = Self(1 << 8);

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
        const NAMES: [(Ctrl, &str); 9] = [
            (Ctrl::CS, "CS"),
            (Ctrl::WRITE, "WRITE"),
            (Ctrl::RES, "RES"),
            (Ctrl::FLAG, "FLAG"),
            (Ctrl::TOD, "TOD"),
            (Ctrl::CNT_LOW, "CNT_LOW"),
            (Ctrl::SP_LOW, "SP_LOW"),
            (Ctrl::IRQ, "IRQ"),
            (Ctrl::PC, "PC"),
        ];
        if self.0 == 0 {
            return f.write_str("none");
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

/// The levels of the select pins `RS3`–`RS0`, `RS3` in bit 3 and `RS0` in
/// bit 0, as [`Pins::read_access`] and [`Pins::write_access`] take them
/// (`MOS6526-006`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The select at `levels`, masked to `RS3`–`RS0` (bits 3–0); the other
    /// bits are ignored.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0x0f)
    }

    /// The levels of `RS3`–`RS0`, in bits 3–0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// The pin state for one `Ø2` cycle, in both directions.
///
/// The caller presents what the outside world drives — the select and
/// direction lines, the register address, the data byte on a write, the levels
/// on `FLAG`, `TOD`, `CNT` and `SP`, and the levels the port lines are held at —
/// and receives back what the part drives: the data byte on a read, the levels
/// the port lines stand at, and `IRQ`, `PC`, `CNT` and `SP`.
///
/// `Copy`, with no lifetime and no callback: a bus resolved through a callback
/// is a machine leaking into the chip. `Default` is [`Pins::idle`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Pins {
    /// `RS3`–`RS0`, the register address, `0`–`15` (`MOS6526-006`). Only the
    /// low four bits are read.
    pub rs: u8,
    /// `DB7`–`DB0`.
    ///
    /// On the way in, the byte the outside drives, `None` when nothing
    /// drives it; a write cycle with `None` writes `0x00`. On the way out,
    /// `Some` only during a read the part is selected for (`MOS6526-007`),
    /// `None` (high impedance) otherwise.
    pub d: Option<u8>,
    /// `PA7`–`PA0`.
    ///
    /// On the way in, the level the outside world holds each line at — a set
    /// bit is High, and a line nobody drives is High by the part's own pull-up
    /// (`MOS6526-010`), so a caller with nothing connected passes `0xFF`. On
    /// the way out, the level each line stands at: the output register's bit
    /// ANDed with the outside's level where the direction register makes the
    /// line an output, and the outside's level alone where it makes it an
    /// input (`MOS6526-015`, `MOS6526-016`).
    pub pa: u8,
    /// `PB7`–`PB0`, on the same terms as [`Pins::pa`], with the two timer
    /// outputs overriding bits 6 and 7 when switched on (`MOS6526-023`).
    pub pb: u8,
    /// The control signals.
    pub ctrl: Ctrl,
}

impl Default for Pins {
    fn default() -> Self {
        Self::idle()
    }
}

impl Pins {
    /// Not selected, every open-drain line pulled High, `TOD` Low, `FLAG` High,
    /// every port line undriven.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            rs: 0,
            d: None,
            pa: 0xff,
            pb: 0xff,
            ctrl: Ctrl::NONE,
        }
    }

    /// [`Pins::idle`] with a read of the register `rs` selects in this cycle.
    #[must_use]
    pub const fn read_access(rs: Select) -> Self {
        let mut pins = Self::idle();
        pins.rs = rs.levels();
        pins.ctrl = pins.ctrl.with(Ctrl::CS);
        pins
    }

    /// [`Pins::idle`] with a write of `d` to the register `rs` selects in this
    /// cycle.
    #[must_use]
    pub const fn write_access(rs: Select, d: u8) -> Self {
        let mut pins = Self::read_access(rs);
        pins.d = Some(d);
        pins.ctrl = pins.ctrl.with(Ctrl::WRITE);
        pins
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

    /// This word with the port A lines the outside world holds replaced.
    #[must_use]
    pub const fn with_pa(mut self, pa: u8) -> Self {
        self.pa = pa;
        self
    }

    /// This word with the port B lines the outside world holds replaced.
    #[must_use]
    pub const fn with_pb(mut self, pb: u8) -> Self {
        self.pb = pb;
        self
    }
}
