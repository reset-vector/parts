//! One machine cycle, state by state.
//!
//! What `SYNC`, `DBIN`, `WR` and `WAIT` do across `T1` to `T5`, and where a
//! `TW` goes.

use crate::pins::status;

/// The ten machine cycle types of `I8080-013`, plus the one the manual names
/// but does not count: `DAD`'s internal cycles, which raise no `SYNC` and touch
/// no bus (`I8080-016`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum CycleKind {
    Fetch,
    MemRead,
    MemWrite,
    StackRead,
    StackWrite,
    Input,
    Output,
    IntAck,
    IntAckHalt,
    Halt,
    /// Not one of the ten. `I8080-016`: no `SYNC`, no `READY`, no bus.
    Internal,
}

impl CycleKind {
    /// The status word this cycle puts on the data lines in `T1`
    /// (`I8080-013`), or `None` for the internal cycle, which puts out no
    /// `SYNC` and therefore no status.
    pub(crate) const fn status(self) -> Option<u8> {
        Some(match self {
            Self::Fetch => status::FETCH,
            Self::MemRead => status::MEM_READ,
            Self::MemWrite => status::MEM_WRITE,
            Self::StackRead => status::STACK_READ,
            Self::StackWrite => status::STACK_WRITE,
            Self::Input => status::INPUT_READ,
            Self::Output => status::OUTPUT_WRITE,
            Self::IntAck => status::INT_ACK,
            Self::IntAckHalt => status::INT_ACK_HALT,
            Self::Halt => status::HALT_ACK,
            Self::Internal => return None,
        })
    }

    /// Whether data enters the part during this cycle, which is exactly the set
    /// `I8080-018` says raises `DBIN` — with the two interrupt
    /// acknowledgements, which are fetches by another name.
    pub(crate) const fn is_input(self) -> bool {
        matches!(
            self,
            Self::Fetch | Self::MemRead | Self::StackRead | Self::Input | Self::IntAck
        ) || matches!(self, Self::IntAckHalt)
    }

    /// Whether the part drives data during this cycle, which is the set
    /// `I8080-019` says pulls `WR` Low.
    pub(crate) const fn is_output(self) -> bool {
        matches!(self, Self::MemWrite | Self::StackWrite | Self::Output)
    }

    /// Whether the cycle waits on `READY`. Everything that touches the bus
    /// does; `I8080-016` says `DAD`'s two do not, and the halt cycle is not a
    /// transfer either.
    pub(crate) const fn needs_ready(self) -> bool {
        !matches!(self, Self::Internal | Self::Halt)
    }
}

/// Where inside a machine cycle the part stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum At {
    T1,
    T2,
    /// The wait state `READY` Low asks for (`I8080-023`). Any number of them
    /// sit between `T2` and `T3`.
    Tw,
    T3,
    T4,
    T5,
}

/// The machine cycle in progress.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) struct Cycle {
    pub(crate) kind: CycleKind,
    /// What the address lines carry from `T1` to `T3` (`I8080-017`).
    pub(crate) addr: u16,
    /// The byte the part drives from `T2` onwards during an output cycle
    /// (`I8080-020`).
    pub(crate) out: u8,
    /// How many numbered states this cycle has: three normally, four or five
    /// where the instruction needs them (`I8080-021`). Wait states are not
    /// counted here.
    pub(crate) states: u8,
    pub(crate) at: At,
}

impl Cycle {
    pub(crate) const fn new(kind: CycleKind, addr: u16, out: u8, states: u8) -> Self {
        Self {
            kind,
            addr,
            out,
            states,
            at: At::T1,
        }
    }

    /// The state after this one within the same machine cycle, or `None` when
    /// the cycle is over.
    ///
    /// `ready` is the level presented while the state now ending was on the
    /// pins, which is how `I8080-023` says `READY` is sampled.
    pub(crate) const fn next(self, ready: bool) -> Option<At> {
        match self.at {
            At::T1 => Some(At::T2),
            At::T2 | At::Tw => {
                if self.states < 3 {
                    None
                } else if ready || !self.kind.needs_ready() {
                    Some(At::T3)
                } else {
                    Some(At::Tw)
                }
            }
            At::T3 => {
                if self.states >= 4 {
                    Some(At::T4)
                } else {
                    None
                }
            }
            At::T4 => {
                if self.states >= 5 {
                    Some(At::T5)
                } else {
                    None
                }
            }
            At::T5 => None,
        }
    }
}
