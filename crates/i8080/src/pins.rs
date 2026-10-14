//! The part's outer edge: the pins, as the outside world sees them during one
//! state.

use core::fmt;

/// The status word the part puts on the data lines during `T1`, one constant
/// per machine cycle type.
///
/// `I8080-012` fixes the bit positions and `I8080-013` fixes these ten bytes.
/// The manual prints the chart as bits and never as a byte, so each constant
/// below names the column of Table 2-1 it was read from.
pub mod status {
    /// `D0`, `INTA` — an interrupt is being acknowledged.
    pub const INTA: u8 = 1 << 0;
    /// `D1`, `W̄Ō`, active Low — **1 for a read or input**, 0 for a write or
    /// output. The one bit in the word that asserts by being clear.
    pub const WO_N: u8 = 1 << 1;
    /// `D2`, `STACK` — the address bus holds the stack pointer.
    pub const STACK: u8 = 1 << 2;
    /// `D3`, `HLTA` — a halt instruction is being acknowledged.
    pub const HLTA: u8 = 1 << 3;
    /// `D4`, `OUT` — the address bus holds an output device number.
    pub const OUT: u8 = 1 << 4;
    /// `D5`, `M1` — this cycle fetches the first byte of an instruction.
    pub const M1: u8 = 1 << 5;
    /// `D6`, `INP` — the address bus holds an input device number.
    pub const INP: u8 = 1 << 6;
    /// `D7`, `MEMR` — the data bus will be used for memory read data.
    pub const MEMR: u8 = 1 << 7;

    /// Column 1 — instruction fetch.
    pub const FETCH: u8 = MEMR | M1 | WO_N;
    /// Column 2 — memory read.
    pub const MEM_READ: u8 = MEMR | WO_N;
    /// Column 3 — memory write. Every bit clear, `WO_N` included.
    pub const MEM_WRITE: u8 = 0;
    /// Column 4 — stack read.
    pub const STACK_READ: u8 = MEMR | STACK | WO_N;
    /// Column 5 — stack write.
    pub const STACK_WRITE: u8 = STACK;
    /// Column 6 — input read.
    pub const INPUT_READ: u8 = INP | WO_N;
    /// Column 7 — output write.
    pub const OUTPUT_WRITE: u8 = OUT;
    /// Column 8 — interrupt acknowledge.
    pub const INT_ACK: u8 = INTA | WO_N | M1;
    /// Column 9 — halt acknowledge.
    pub const HALT_ACK: u8 = MEMR | HLTA | WO_N;
    /// Column 10 — interrupt acknowledge while halted.
    pub const INT_ACK_HALT: u8 = INTA | WO_N | HLTA | M1;
}

/// The control signals, plus nothing else.
///
/// `I8080-002` names the forty pins and gives each a direction. A **set flag
/// means the condition it names holds**, and the empty set is every pin at its
/// idle level. `WR` is **active Low** on the part, and its flag set means
/// asserted (Low); `READY` is active High, and its flag is
/// [`Ctrl::READY_LOW`], set when the pin is Low. The inversion happens at the package pin, which this type does not
/// model.
///
/// The clock, power and ground of `I8080-002` and `I8080-003` are not here: a
/// state is the [`crate::I8080::tick`] itself (`I8080-010`) and the supply pins
/// carry no information.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Ctrl(u16);

impl Ctrl {
    /// Nothing asserted in either direction.
    pub const NONE: Self = Self(0);

    /// Out. `SYNC` is High: this is the first state of a machine cycle, and
    /// [`Pins::d`] holds the status word (`I8080-011`, `I8080-015`).
    pub const SYNC: Self = Self(1 << 0);

    /// Out. `DBIN` is High: the data bus is in the input mode and the board
    /// should drive [`Pins::d`] (`I8080-018`).
    pub const DBIN: Self = Self(1 << 1);

    /// Out. `WR` is Low: [`Pins::d`] holds a byte for the addressed device
    /// (`I8080-019`).
    pub const WR: Self = Self(1 << 2);

    /// Out. `WAIT` is High: the part is in a `TW` state because `READY` was
    /// Low when it was sampled (`I8080-023`).
    pub const WAIT: Self = Self(1 << 3);

    /// Out. `HLDA` is High: a hold request has been acknowledged
    /// (`I8080-033`, `I8080-034`).
    pub const HLDA: Self = Self(1 << 4);

    /// Out. `INTE` is High: the interrupt enable flip-flop is set
    /// (`I8080-030`).
    pub const INTE: Self = Self(1 << 5);

    /// In. `READY` is Low: the addressed device has not yet got valid data,
    /// or not yet taken the byte, and the part waits (`I8080-023`). Clear,
    /// `READY` is High and the part runs.
    pub const READY_LOW: Self = Self(1 << 7);

    /// In. `HOLD` is High: an external device asks for the buses
    /// (`I8080-033`).
    pub const HOLD: Self = Self(1 << 8);

    /// In. `INT` is High: an interrupt is requested (`I8080-029`).
    pub const INT: Self = Self(1 << 9);

    /// In. `RESET` is High: the part is being reset (`I8080-026`). It must be
    /// held for three states before it takes effect (`I8080-027`).
    pub const RESET: Self = Self(1 << 10);

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
        const NAMES: [(Ctrl, &str); 10] = [
            (Ctrl::SYNC, "SYNC"),
            (Ctrl::DBIN, "DBIN"),
            (Ctrl::WR, "WR"),
            (Ctrl::WAIT, "WAIT"),
            (Ctrl::HLDA, "HLDA"),
            (Ctrl::INTE, "INTE"),
            (Ctrl::READY_LOW, "READY_LOW"),
            (Ctrl::HOLD, "HOLD"),
            (Ctrl::INT, "INT"),
            (Ctrl::RESET, "RESET"),
        ];
        if self.0 == 0 {
            return f.write_str("quiet");
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
    /// The address.
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

/// The pin state for one state, in both directions.
///
/// A **state** is the unit here, not a machine cycle and not an instruction:
/// `I8080-010` defines it as the interval between two rising edges of `Ø1`, and
/// three to five of them make a machine cycle. The caller presents the pins the
/// outside world drives — [`Ctrl::READY_LOW`], [`Ctrl::HOLD`], [`Ctrl::INT`],
/// [`Ctrl::RESET`], and [`Pins::d`] while [`Ctrl::DBIN`] asks for a byte —
/// and receives back the pins the part drives.
///
/// `Copy`, with no lifetime and no callback: a bus resolved through a callback
/// is a machine leaking into the chip.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub struct Pins {
    /// The sixteen address outputs.
    ///
    /// Loaded in `T1` and stable through `T3` (`I8080-017`). In the state
    /// that enters a hold, where `HLDA` rises and the lines float only in the
    /// next (`I8080-034`), the address the part drives again when the hold
    /// ends: the program counter from a halt, otherwise the address of the
    /// machine cycle the hold defers. `None` while the part has released the
    /// bus ([`crate::I8080::is_released`]), and [`Address::Undetermined`] in a
    /// state `RESET` holds, where no document says what the lines carry.
    pub address: Option<Address>,
    /// The eight bidirectional data lines: `None` while nothing drives them.
    ///
    /// In `T1`, while [`Ctrl::SYNC`] is High, the part drives the status word
    /// (`I8080-011`), whose bits are in [`status`]. During an output cycle it
    /// drives the byte being written from `T2` to the end of the machine cycle
    /// (`I8080-020`). In every other state it returns `None`. During an input
    /// cycle a caller answers with `Some(byte)`; a read that finds `None`
    /// takes 0.
    pub d: Option<u8>,
    /// The control signals.
    pub ctrl: Ctrl,
}

impl Pins {
    /// Every input at its idle level: no reset, no hold, no interrupt, and
    /// `READY` High — a board that always answers at once. The same as
    /// [`Pins::default`].
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
}
