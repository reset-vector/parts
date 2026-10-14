//! The part's outer edge: the pins, as the outside world sees them during one
//! `Ø2` cycle.

use core::fmt;

use units::Microvolts;

/// The DC level the sheet says a signal at `EXT IN` rides at, in microvolts:
/// 6 V (`MOS6581-048`). A caller with nothing connected presents this.
pub const EXT_IN_DC_UV: Microvolts = Microvolts(6_000_000);

/// The DC level `AUDIO OUT` rides at, in microvolts: 6 V (`MOS6581-049`).
/// A silent part holds the pin here.
pub const AUDIO_OUT_DC_UV: Microvolts = Microvolts(6_000_000);

/// The control signals, plus nothing else.
///
/// `MOS6581-001` names the twenty-eight pins. A **set flag means the
/// condition it names holds**, and the empty set is every pin at its idle
/// level. `CS` and `RES` are active Low and their flags, named for the pin,
/// mean asserted (Low); `WRITE` is `R/W` Low; `POTX_LOW` and `POTY_LOW`
/// mean the part is pulling that pot pin Low, to ground.
///
/// `Ø2` is not here: it is the `tick` itself. Power and ground carry no
/// information. The address, the data bus and the four analogue levels are
/// the other fields of [`Pins`].
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u8);

impl Ctrl {
    /// No condition present: not selected, not in reset, neither pot pin
    /// sunk.
    pub const NONE: Self = Self(0);

    /// In. `CS` is Low: the bus access in this cycle is this part's
    /// (`MOS6581-005`). Absent, the part ignores `R/W`, `A4`–`A0` and the
    /// data bus.
    pub const CS: Self = Self(1 << 0);

    /// In. `R/W` is Low: the access is a write, [`Pins::d`] holds the byte
    /// (`MOS6581-006`). Absent, the access is a read.
    pub const WRITE: Self = Self(1 << 1);

    /// In. `RES` is Low: the part is held in reset (`MOS6581-009`).
    pub const RES: Self = Self(1 << 2);

    /// Out. The part is sinking `POT X` this cycle, discharging the board's
    /// capacitor (`MOS6581-047`, `MOS6581-065`). A board modelling the
    /// capacitor holds the pin at ground while this is set.
    pub const POTX_LOW: Self = Self(1 << 3);

    /// Out. The part is sinking `POT Y` this cycle, on the same terms as
    /// [`Ctrl::POTX_LOW`].
    pub const POTY_LOW: Self = Self(1 << 4);

    /// Whether every signal in `other` is set here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any signal in `other` is set here.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// This set with the signals in `other` also set.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// This set with the signals in `other` cleared.
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
        const NAMES: [(Ctrl, &str); 5] = [
            (Ctrl::CS, "CS"),
            (Ctrl::WRITE, "WRITE"),
            (Ctrl::RES, "RES"),
            (Ctrl::POTX_LOW, "POTX_LOW"),
            (Ctrl::POTY_LOW, "POTY_LOW"),
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

/// The levels at `A4`–`A0`: which register a [`Pins::read_access`] or
/// [`Pins::write_access`] reaches (`MOS6581-007`), so a register address and
/// a data byte cannot be swapped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The levels `levels` at `A4`–`A0`, bit 0 `A0`; the bits above `A4`
    /// are dropped.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0x1f)
    }

    /// The levels at `A4`–`A0`, bit 0 `A0`.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// The pin state for one `Ø2` cycle, in both directions.
///
/// The caller presents what the outside world drives — the select and
/// direction lines, the register address, the data byte on a write, and the
/// levels at `EXT IN`, `POT X` and `POT Y` — and receives back what the part
/// drives: the data byte on a read, the level at `AUDIO OUT`, and whether it
/// is sinking either pot pin.
///
/// Every analogue level is in **microvolts at the pin**, the unit `lm324`,
/// `ne555` and `sn7414` already speak, because the sheet states the one gain
/// it states in volts (`MOS6581-048`) and a level at a pin is what a
/// consumer takes as a sample. `Copy`, with no lifetime and no callback.
///
/// `Default` is [`Pins::idle`], so `Pins { a, ..Default::default() }`
/// leaves `EXT IN` at its DC level.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Pins {
    /// `A4`–`A0`, the register address, `0`–`31` (`MOS6581-007`). Only the
    /// low five bits are read.
    pub a: u8,
    /// `D7`–`D0`.
    ///
    /// On the way in, the byte the outside drives, `None` when nothing
    /// drives it; a write cycle with `None` writes `0x00`. On the way out,
    /// `Some` only during a read the part is selected for (`MOS6581-008`),
    /// `None` (high impedance) otherwise.
    pub d: Option<u8>,
    /// In. The level at `EXT IN`, pin 26, in microvolts. The signal is
    /// expected to ride at [`EXT_IN_DC_UV`] (`MOS6581-048`), and what the
    /// part mixes in is the excursion from that level.
    pub ext_in_uv: Microvolts,
    /// In. The level at `POT X`, pin 24, in microvolts: the voltage on the
    /// board's capacitor (`MOS6581-047`).
    pub potx_uv: Microvolts,
    /// In. The level at `POT Y`, pin 23, on the same terms.
    pub poty_uv: Microvolts,
    /// Out. The level at `AUDIO OUT`, pin 27, into the sheet's 1 kΩ source
    /// resistor, in microvolts: [`AUDIO_OUT_DC_UV`] plus the composite
    /// (`MOS6581-049`, `MOS6581-OPEN-13`).
    pub audio_out_uv: Microvolts,
    /// The control signals.
    pub ctrl: Ctrl,
}

impl Default for Pins {
    /// [`Pins::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

impl Pins {
    /// Not selected, not in reset, `EXT IN` at its DC level, both pot pins at
    /// ground.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: 0,
            d: None,
            ext_in_uv: EXT_IN_DC_UV,
            potx_uv: Microvolts(0),
            poty_uv: Microvolts(0),
            audio_out_uv: Microvolts(0),
            ctrl: Ctrl::NONE,
        }
    }

    /// [`Pins::idle`] with a read of register `a` in this cycle.
    #[must_use]
    pub const fn read_access(a: Select) -> Self {
        let mut pins = Self::idle();
        pins.a = a.levels();
        pins.ctrl = pins.ctrl.with(Ctrl::CS);
        pins
    }

    /// [`Pins::idle`] with a write of `d` to register `a` in this cycle.
    #[must_use]
    pub const fn write_access(a: Select, d: u8) -> Self {
        let mut pins = Self::read_access(a);
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

    /// This word with the level at `EXT IN` replaced.
    #[must_use]
    pub const fn with_ext_in_uv(mut self, ext_in_uv: Microvolts) -> Self {
        self.ext_in_uv = ext_in_uv;
        self
    }

    /// This word with the levels at the two pot pins replaced.
    #[must_use]
    pub const fn with_pots_uv(mut self, potx_uv: Microvolts, poty_uv: Microvolts) -> Self {
        self.potx_uv = potx_uv;
        self.poty_uv = poty_uv;
        self
    }
}
