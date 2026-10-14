//! The part's outer edge: what crosses the pins in each half of one `Ø0`
//! cycle.
//!
//! The part is a bus master in the first half of every cycle (`MOS6567-009`)
//! and, when it has said so three cycles earlier, in the second half too
//! (`MOS6567-010`), so one cycle is two calls: [`crate::Mos6567::phase1`] announces what the part
//! drives and asks for, and [`crate::Mos6567::phase2`] takes the answer, the
//! processor's access, and returns the second-half request and the cycle's
//! video. Every request is answered in the call after it was made, because a
//! sprite's data address depends on the pointer fetched in the half-cycle
//! before it (`MOS6567-026`).

use core::fmt;

use crate::Level;

/// What one of the part's first-phase memory accesses is for.
///
/// The letters are Bauer's (`MOS6567-049`); a board serves every purpose the
/// same way and may ignore this field. The first phase never reads the video
/// matrix, which is a second-phase access ([`Phase2Purpose`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Phase1Purpose {
    /// A character generator or bit map read (`MOS6567-024`, `MOS6567-025`).
    Graphics,
    /// A sprite's data pointer, from the last eight matrix locations
    /// (`MOS6567-026`), for the sprite numbered `0..=7`.
    SpritePointer(usize),
    /// One of a sprite's three data bytes for this line (`MOS6567-026`),
    /// for the sprite numbered `0..=7`.
    SpriteData(usize),
    /// A dynamic RAM refresh row (`MOS6567-014`, `MOS6567-059`).
    Refresh,
    /// A first phase with nothing to fetch, which the part spends reading
    /// `$3FFF` (`MOS6567-049`).
    Idle,
}

/// What one of the part's second-phase memory accesses is for: the two
/// fetches the part takes the processor's half of the cycle for
/// (`MOS6567-010`, `MOS6567-049`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Phase2Purpose {
    /// A video matrix and colour RAM read, twelve bits wide (`MOS6567-023`).
    Matrix,
    /// One of a sprite's three data bytes for this line (`MOS6567-026`),
    /// for the sprite numbered `0..=7`.
    SpriteData(usize),
}

/// One memory access the part makes in a cycle's first phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Phase1Access {
    /// `A13`–`A00`, the fourteen-bit address, demultiplexed. The pins carry
    /// it as a row and a column under `RAS` and `CAS` (`MOS6567-013`); this
    /// crate does not model the strobes.
    pub a: u16,
    /// What the access is for.
    pub purpose: Phase1Purpose,
}

/// One memory access the part makes in a cycle's second phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Phase2Access {
    /// `A13`–`A00`, the fourteen-bit address, as [`Phase1Access::a`].
    pub a: u16,
    /// What the access is for.
    pub purpose: Phase2Purpose,
}

/// The processor's control signals at the part, set when the condition the
/// flag names holds.
///
/// A flag set means the condition it names holds, and the empty set is every
/// pin at its idle level. `CS` is active Low, so [`Ctrl::CS`] set is the pin
/// Low; [`Ctrl::WRITE`] set is `R/W` Low.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u8);

impl Ctrl {
    /// Every pin at its idle level: not selected, `R/W` High.
    pub const NONE: Self = Self(0);
    /// `CS` Low: the processor accesses a register in this cycle's second
    /// phase (`MOS6567-015`).
    pub const CS: Self = Self(1 << 0);
    /// `R/W` Low: the access is a write (`MOS6567-015`).
    pub const WRITE: Self = Self(1 << 1);

    /// Whether every flag in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// These flags with those in `other` also set.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// These flags with those in `other` cleared.
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
        match (self.contains(Self::CS), self.contains(Self::WRITE)) {
            (false, false) => f.write_str("none"),
            (true, false) => f.write_str("CS"),
            (false, true) => f.write_str("WRITE"),
            (true, true) => f.write_str("CS|WRITE"),
        }
    }
}

/// What the outside world presents at the start of a cycle.
///
/// [`Phase1Inputs::default`] holds `LP` High and the data lines at zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Phase1Inputs {
    /// The level at `LP`, active Low. The part acts on a High-to-Low
    /// transition, once per frame (`MOS6567-022`).
    pub lp_n: Level,
    /// `DB11`–`DB0`: the twelve bits the board returned for the second-phase
    /// fetch the previous [`Phase2Outputs`] asked for, if it asked for one.
    /// Only the low twelve bits are read. Named for the access, since the
    /// lines carried it in the half before this call.
    pub access_d: u16,
}

impl Phase1Inputs {
    /// The part idle; see [`Phase1Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            lp_n: Level::High,
            access_d: 0,
        }
    }
}

impl Default for Phase1Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives at the start of a cycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Phase1Outputs {
    /// The level at `BA`: Low when the part will take a second
    /// phase three cycles on, and the processor should finish what it can
    /// (`MOS6567-010`).
    pub ba: Level,
    /// The level `AEC` holds through this cycle's second phase: Low when it
    /// stays Low, so the processor
    /// must not drive the buses and [`Phase2Outputs::access`] will carry the part's
    /// access (`MOS6567-010`). `AEC` is Low in the first phase of every cycle
    /// (`MOS6567-009`) and that half is not a field: it is this call.
    pub aec: Level,
    /// The part's first-phase access, one every cycle (`MOS6567-009`,
    /// `MOS6567-049`). The board answers it in the next
    /// [`Phase2Inputs::access_d`].
    pub access: Phase1Access,
}

/// The levels of the register select pins `A05`–`A00`, `A05` in bit 5 and
/// `A00` in bit 0, as [`crate::Mos6567::read`], [`crate::Mos6567::write`],
/// [`Phase2Inputs::read_access`] and [`Phase2Inputs::write_access`] take them
/// (`MOS6567-015`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The select at `levels`, masked to `A05`–`A00` (bits 5–0); the other
    /// bits are ignored.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0x3f)
    }

    /// The levels of `A05`–`A00`, in bits 5–0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// What the outside world presents for the second half of a cycle.
///
/// [`Phase2Inputs::default`] is no processor access and the data lines at
/// zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Phase2Inputs {
    /// `DB11`–`DB0` as the board drove them in the first phase: its answer to
    /// [`Phase1Outputs::access`]. Only the low twelve bits are read; a
    /// graphics, pointer, data or refresh access uses the low eight. Named
    /// for the access, as [`Phase1Inputs::access_d`] is; [`Phase2Inputs::d`] is
    /// the same lines in this half.
    pub access_d: u16,
    /// `A05`–`A00`, the register select of the processor's access
    /// (`MOS6567-015`). Only the low six bits are read, and only with
    /// [`Ctrl::CS`] set.
    pub a: u8,
    /// `DB7`–`DB0` as the processor drives them in this second phase: the
    /// byte of a write. Read only with [`Ctrl::CS`] and [`Ctrl::WRITE`] set.
    pub d: u8,
    /// `CS` and `R/W`. Ignored in a cycle where [`Phase1Outputs::aec`] was
    /// Low, which is `MOS6567-OPEN-10`'s choice.
    pub ctrl: Ctrl,
}

impl Phase2Inputs {
    /// The part idle; see [`Phase2Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            access_d: 0,
            a: 0,
            d: 0,
            ctrl: Ctrl::NONE,
        }
    }

    /// [`Phase2Inputs::default`] with a read of the register `a` selects in
    /// this cycle's second phase; the board's answer to the first phase is
    /// set in [`Phase2Inputs::access_d`].
    #[must_use]
    pub const fn read_access(a: Select) -> Self {
        Self {
            access_d: 0,
            a: a.levels(),
            d: 0,
            ctrl: Ctrl::CS,
        }
    }

    /// [`Phase2Inputs::default`] with a write of `d` to the register `a`
    /// selects in this cycle's second phase.
    #[must_use]
    pub const fn write_access(a: Select, d: u8) -> Self {
        Self {
            access_d: 0,
            a: a.levels(),
            d,
            ctrl: Ctrl::CS.with(Ctrl::WRITE),
        }
    }
}

impl Default for Phase2Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The level at `SYNC/LUM` through one dot clock (`MOS6567-044`,
/// `MOS6567-045`).
///
/// The pin's level is `MOS6567-OPEN-09`, so this is the unit the part is
/// supplied in: the interval the dot falls in, or the colour code whose
/// luminance the pin carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SyncLum {
    /// Inside a sync or equalisation pulse.
    Sync,
    /// At blanking level.
    Blank,
    /// The luminance of colour code `0..=15` (`MOS6567-032`).
    Luminance(u8),
}

impl Default for SyncLum {
    fn default() -> Self {
        Self::Luminance(0)
    }
}

/// What `COLOR` carries through one dot clock (`MOS6567-044`,
/// `MOS6567-046`).
///
/// As [`SyncLum`], the interval the dot falls in or the colour code whose
/// chrominance the pin carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Colour {
    /// Nothing: a blanking interval or a sync pulse.
    Blank,
    /// The colour reference burst.
    Burst,
    /// The chrominance of colour code `0..=15` (`MOS6567-032`).
    Chroma(u8),
}

impl Default for Colour {
    fn default() -> Self {
        Self::Chroma(0)
    }
}

/// What the part drives through the second half of a cycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Phase2Outputs {
    /// The part's second-phase access, present exactly when
    /// [`Phase1Outputs::aec`] was Low. The board answers it in the next
    /// [`Phase1Inputs::access_d`].
    pub access: Option<Phase2Access>,
    /// `DB7`–`DB0` as the part drives them for a register read; `None`
    /// (high impedance) when the cycle carried no read.
    pub d: Option<u8>,
    /// The level at `IRQ`, active Low: Low while an enabled
    /// interrupt latch is set (`MOS6567-016`, `MOS6567-020`).
    pub irq_n: Level,
    /// `SYNC/LUM` through the eight dot clocks of this cycle, first dot first
    /// (`MOS6567-005`, `MOS6567-044`).
    pub sync_lum: [SyncLum; 8],
    /// `COLOR` through the same eight dot clocks.
    pub colour: [Colour; 8],
}
