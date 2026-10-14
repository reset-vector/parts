//! The machine-cycle sequencer: what the part shows on its pins, one T-state at
//! a time, with no knowledge of instructions.
//!
//! An executor asks for a machine cycle as a [`BusOp`]; the sequencer drives
//! the pins for each of its T-states in turn, honours `WAIT`, and hands back
//! the byte it sampled.
//!
//! # T-state granularity
//!
//! This sequencer advances one whole T-state per step, so a transition the part
//! makes *within* a T-state cannot be placed (`Z80-OPEN-02`). Of three such
//! transitions, two are not modelled and one is:
//!
//! - `Z80-013` — `MREQ` falls half a clock after the address appears in T1 — is
//!   **not modelled**: `MREQ` is asserted for the whole of T1.
//! - `Z80-016` — `WR` rises half a T-state before the buses change — is **not
//!   modelled**: `WR` is deasserted at the end of T3 together with the address
//!   and the data.
//! - `Z80-015` — the opcode is sampled on the same rising edge of T3 that
//!   deasserts `RD` and `MREQ` — **is modelled**, because both fall on a whole-T
//!   boundary and the ordering is what the claim asserts: [`Seq::end_t`] latches
//!   the byte the caller drove during T2 and only then stops asserting the read.
//!   `sampling_precedes_the_deassert` is the test.

use crate::pins::{Address, Ctrl, Pins};

/// One machine cycle, as an executor asks for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum BusOp {
    /// An opcode fetch: four T-states, refresh on the bus in T3-T4.
    ///
    /// `Z80-003`, `Z80-004`. `refresh` is `I * 256 + R`, and it is the caller's
    /// job to pass `R` as it stood *before* the fetch incremented it
    /// (`Z80-029`).
    Fetch {
        /// Driven on the address bus in T1-T2.
        pc: u16,
        /// Driven on the address bus in T3-T4.
        refresh: u16,
    },
    /// A memory read that is not an opcode fetch: three T-states (`Z80-005`).
    MemRead(u16),
    /// A memory write: three T-states (`Z80-005`).
    MemWrite(u16, u8),
    /// An I/O read: four T-states, one of them an automatic wait (`Z80-006`).
    IoRead(u16),
    /// An I/O write: four T-states, one of them an automatic wait (`Z80-006`).
    IoWrite(u16, u8),
    /// T-states in which the part makes no bus request at all.
    Internal(u8),
    /// An interrupt acknowledge: a special M1 asserting `IORQ` in place of
    /// `MREQ`, with two automatic wait states (`Z80-071`).
    ///
    /// `M1` together with `IORQ` is the only combination that means this; `M1`
    /// with `MREQ` is an ordinary opcode fetch.
    IntAck(u16),
}

impl BusOp {
    /// How many T-states the cycle takes before any `WAIT` is counted.
    const fn t_states(self) -> u8 {
        match self {
            Self::Fetch { .. } | Self::IoRead(_) | Self::IoWrite(..) => 4,
            Self::MemRead(_) | Self::MemWrite(..) => 3,
            Self::IntAck(_) => 5,
            Self::Internal(n) => n,
        }
    }

    /// The T-state at whose end `WAIT` decides whether the cycle is extended.
    ///
    /// `Z80-007`. For memory cycles that is T2; for I/O it is the automatic wait
    /// state the part inserts between T2 and T3, which is why the sample lands
    /// one T later and why `Z80-006` counts four.
    const fn wait_t(self) -> Option<u8> {
        match self {
            Self::Fetch { .. } | Self::MemRead(_) | Self::MemWrite(..) => Some(2),
            Self::IoRead(_) | Self::IoWrite(..) => Some(3),
            Self::IntAck(_) => Some(4),
            Self::Internal(_) => None,
        }
    }

    /// The T-state at whose end the byte on the data bus is taken.
    ///
    /// For a fetch this is T2, so that the opcode is in hand before `RD` and
    /// `MREQ` are dropped entering T3 (`Z80-014`, `Z80-015`). For the other
    /// reads it is the cycle's last T-state. A write samples nothing.
    const fn sample_t(self) -> Option<u8> {
        match self {
            Self::Fetch { .. } => Some(2),
            Self::MemRead(_) => Some(3),
            Self::IoRead(_) => Some(4),
            Self::IntAck(_) => Some(5),
            Self::MemWrite(..) | Self::IoWrite(..) | Self::Internal(_) => None,
        }
    }
}

/// The machine cycle in progress.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) struct Seq {
    op: BusOp,
    /// One-based index of the T-state now being driven; `0` before the first.
    ///
    /// A wait state is this index standing still, not a value of its own.
    t: u8,
    /// The last byte sampled off the data bus.
    data: u8,
}

impl Seq {
    /// A sequencer about to drive T1 of `op` on its first [`Seq::end_t`].
    pub(crate) const fn new(op: BusOp) -> Self {
        Self { op, t: 0, data: 0 }
    }

    /// Begins `op` at its T1, which [`Seq::drive`] reports immediately.
    pub(crate) const fn start(&mut self, op: BusOp) {
        debug_assert!(
            op.t_states() >= 1,
            "a machine cycle is at least one T-state"
        );
        debug_assert!(
            !matches!(op, BusOp::Internal(n) if n > LONGEST_INTERNAL),
            "no instruction idles longer than LONGEST_INTERNAL in one cycle"
        );
        self.op = op;
        self.t = 1;
    }

    /// Ends the T-state in progress and begins the next.
    ///
    /// Returns `Some(byte)` when that was the cycle's last T-state: the cycle is
    /// over, `byte` is what it sampled — zero for a cycle that samples nothing —
    /// and the caller must [`Seq::start`] the next cycle before driving again.
    pub(crate) fn end_t(&mut self, inp: Pins) -> Option<u8> {
        if self.t == 0 {
            self.t = 1;
            return None;
        }
        if self.op.wait_t() == Some(self.t) && inp.ctrl.contains(Ctrl::WAIT) {
            return None;
        }
        if self.op.sample_t() == Some(self.t) {
            self.data = inp.d.unwrap_or(0);
        }
        self.t += 1;
        if self.t > self.op.t_states() {
            Some(self.data)
        } else {
            None
        }
    }

    /// Whether the index stands one past the cycle's last T-state, where
    /// [`Seq::end_t`] leaves it once the cycle is over and before the next
    /// [`Seq::start`].
    pub(crate) const fn is_past_end(&self) -> bool {
        self.t > self.op.t_states()
    }

    /// Whether the T-state now in progress is this cycle's last.
    ///
    /// `Z80-008`'s sample point: the rising edge that begins the last clock
    /// period of a machine cycle.
    pub(crate) const fn on_last_t(&self) -> bool {
        self.t == self.op.t_states()
    }

    /// The last byte sampled off the data bus.
    pub(crate) const fn sampled(&self) -> u8 {
        self.data
    }

    /// Whether the T-state now in progress is this cycle's first.
    pub(crate) const fn opening(&self) -> bool {
        self.t == 1
    }

    /// The pins the part drives during the T-state now in progress.
    pub(crate) fn drive(&self) -> Pins {
        match self.op {
            BusOp::Fetch { pc, refresh } => match self.t {
                1 | 2 => Pins {
                    address: Some(Address::Stable(pc)),
                    d: None,
                    ctrl: Ctrl::M1 | Ctrl::MREQ | Ctrl::RD,
                },
                3 => Pins {
                    address: Some(Address::Stable(refresh)),
                    d: None,
                    ctrl: Ctrl::RFSH | Ctrl::MREQ,
                },
                _ => Pins {
                    address: Some(Address::Stable(refresh)),
                    d: None,
                    ctrl: Ctrl::RFSH,
                },
            },
            BusOp::MemRead(address) => Pins {
                address: Some(Address::Stable(address)),
                d: None,
                ctrl: Ctrl::MREQ | Ctrl::RD,
            },
            BusOp::MemWrite(address, data) => Pins {
                address: Some(Address::Stable(address)),
                d: Some(data),
                ctrl: if self.t >= 2 {
                    Ctrl::MREQ | Ctrl::WR
                } else {
                    Ctrl::MREQ
                },
            },
            BusOp::IoRead(address) => Pins {
                address: Some(Address::Stable(address)),
                d: None,
                ctrl: if self.t >= 2 {
                    Ctrl::IORQ | Ctrl::RD
                } else {
                    Ctrl::NONE
                },
            },
            BusOp::IoWrite(address, data) => Pins {
                address: Some(Address::Stable(address)),
                d: Some(data),
                ctrl: if self.t >= 2 {
                    Ctrl::IORQ | Ctrl::WR
                } else {
                    Ctrl::NONE
                },
            },
            BusOp::IntAck(address) => Pins {
                address: Some(Address::Stable(address)),
                d: None,
                ctrl: if self.t >= 3 {
                    Ctrl::M1 | Ctrl::IORQ
                } else {
                    Ctrl::M1
                },
            },
            BusOp::Internal(_) => Pins {
                address: Some(Address::Undetermined),
                ..Pins::idle()
            },
        }
    }
}

/// The longest run of internal T-states any instruction makes in one machine
/// cycle.
pub(crate) const LONGEST_INTERNAL: u8 = 5;

/// The five bytes [`BusOp::encode`] writes, and [`Seq::encode`] embeds.
pub(crate) const BUS_OP_BYTES: usize = 5;

/// The seven bytes [`Seq::encode`] writes.
pub(crate) const SEQ_BYTES: usize = BUS_OP_BYTES + 2;

impl BusOp {
    /// The cycle as bytes, for `Z80::save_state`: a tag and two words.
    ///
    /// This is a readout, not a behaviour: nothing reads it back except
    /// [`BusOp::decode`], whose round trip
    /// `every_stage_and_cycle_shape_round_trips` asserts.
    pub(crate) const fn encode(self) -> [u8; BUS_OP_BYTES] {
        let (tag, a, b) = match self {
            Self::Fetch { pc, refresh } => (0, pc, refresh),
            Self::MemRead(address) => (1, address, 0),
            Self::MemWrite(address, data) => (2, address, data as u16),
            Self::IoRead(address) => (3, address, 0),
            Self::IoWrite(address, data) => (4, address, data as u16),
            Self::Internal(n) => (5, n as u16, 0),
            Self::IntAck(address) => (6, address, 0),
        };
        let [a_lo, a_hi] = a.to_le_bytes();
        let [b_lo, b_hi] = b.to_le_bytes();
        [tag, a_lo, a_hi, b_lo, b_hi]
    }

    /// The inverse of [`BusOp::encode`]; `None` for a tag no version wrote, or
    /// for an internal cycle of a length no instruction runs: none of no
    /// T-states, which [`Seq::start`] refuses, and none past
    /// [`LONGEST_INTERNAL`], whose T-state index [`Seq::end_t`] would count
    /// up past the `u8` it is held in.
    pub(crate) const fn decode(bytes: [u8; BUS_OP_BYTES]) -> Option<Self> {
        let [tag, a_lo, a_hi, b_lo, b_hi] = bytes;
        let a = u16::from_le_bytes([a_lo, a_hi]);
        let b = u16::from_le_bytes([b_lo, b_hi]);
        Some(match tag {
            0 => Self::Fetch { pc: a, refresh: b },
            1 => Self::MemRead(a),
            2 => Self::MemWrite(a, b as u8),
            3 => Self::IoRead(a),
            4 => Self::IoWrite(a, b as u8),
            5 if a >= 1 && a <= LONGEST_INTERNAL as u16 => Self::Internal(a as u8),
            6 => Self::IntAck(a),
            _ => return None,
        })
    }
}

impl Seq {
    /// The sequencer as bytes: the cycle, the T-state index, the sampled byte.
    pub(crate) const fn encode(&self) -> [u8; SEQ_BYTES] {
        let [a, b, c, d, e] = self.op.encode();
        [a, b, c, d, e, self.t, self.data]
    }

    /// The inverse of [`Seq::encode`]; `None` for a T-state index no run
    /// reaches.
    ///
    /// That is anything past one beyond the cycle's last: [`Seq::end_t`] leaves
    /// the index there when a bus request holds the next cycle back, or when
    /// `settle` leaves the part yet to start its first, and an index further on
    /// would count up until it overflowed.
    pub(crate) const fn decode(bytes: [u8; SEQ_BYTES]) -> Option<Self> {
        let [a, b, c, d, e, t, data] = bytes;
        match BusOp::decode([a, b, c, d, e]) {
            Some(op) if t <= op.t_states() + 1 => Some(Self { op, t, data }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BusOp, Seq};
    use crate::pins::{Ctrl, Pins};
    use std::vec::Vec;

    /// Runs one cycle to completion, returning the pins driven in each T-state
    /// and the byte the cycle sampled.
    ///
    /// `input` is what the outside world presents on each tick, by tick index.
    fn run(op: BusOp, mut input: impl FnMut(usize) -> Pins) -> (Vec<Pins>, u8) {
        let mut seq = Seq::new(op);
        let mut driven = Vec::new();
        for tick in 0..64 {
            if let Some(byte) = seq.end_t(input(tick)) {
                return (driven, byte);
            }
            driven.push(seq.drive());
        }
        panic!("cycle did not finish in 64 T-states");
    }

    /// The outside world holding every input line quiet.
    fn quiet(_: usize) -> Pins {
        Pins::idle()
    }

    /// The outside world answering every read with `byte`.
    fn answering(byte: u8) -> impl FnMut(usize) -> Pins {
        move |_| Pins::idle().with_d(byte)
    }

    #[test]
    fn fetch_is_four_t_states() {
        let (driven, _) = run(
            BusOp::Fetch {
                pc: 0x1234,
                refresh: 0x0000,
            },
            quiet,
        );
        assert_eq!(driven.len(), 4);
    }

    #[test]
    fn fetch_reads_at_pc_then_shows_the_refresh_address() {
        let op = BusOp::Fetch {
            pc: 0x1234,
            refresh: 0x4B07,
        };
        let (driven, _) = run(op, quiet);

        for t in [0, 1] {
            assert_eq!(
                driven[t].address,
                Some(crate::Address::Stable(0x1234)),
                "T{}",
                t + 1
            );
            assert_eq!(
                driven[t].ctrl,
                Ctrl::M1 | Ctrl::MREQ | Ctrl::RD,
                "T{}",
                t + 1
            );
        }
        for t in [2, 3] {
            assert_eq!(
                driven[t].address,
                Some(crate::Address::Stable(0x4B07)),
                "T{}",
                t + 1
            );
            assert!(driven[t].ctrl.contains(Ctrl::RFSH), "T{}", t + 1);
            assert!(!driven[t].ctrl.contains(Ctrl::RD), "T{}", t + 1);
            assert!(!driven[t].ctrl.contains(Ctrl::M1), "T{}", t + 1);
        }
        assert!(
            driven[2].ctrl.contains(Ctrl::MREQ),
            "the refresh read is in T3"
        );
    }

    #[test]
    fn sampling_precedes_the_deassert() {
        let op = BusOp::Fetch {
            pc: 0x0000,
            refresh: 0x0000,
        };
        let (driven, byte) = run(op, answering(0xC9));
        assert_eq!(byte, 0xC9);
        assert!(
            driven[1].ctrl.contains(Ctrl::RD),
            "read still asserted when sampled"
        );
        assert!(
            !driven[2].ctrl.contains(Ctrl::RD),
            "and dropped immediately after"
        );
    }

    #[test]
    fn a_fetch_ignores_the_data_bus_after_t2() {
        let op = BusOp::Fetch {
            pc: 0x0000,
            refresh: 0x0000,
        };
        let (_, byte) = run(op, |tick| {
            Pins::idle().with_d(if tick <= 2 { 0x3E } else { 0xFF })
        });
        assert_eq!(byte, 0x3E);
    }

    #[test]
    fn memory_read_is_three_t_states() {
        let (driven, byte) = run(BusOp::MemRead(0x8000), answering(0x5A));
        assert_eq!(driven.len(), 3);
        assert_eq!(byte, 0x5A);
        for (t, pins) in driven.iter().enumerate() {
            assert_eq!(
                pins.address,
                Some(crate::Address::Stable(0x8000)),
                "T{}",
                t + 1
            );
            assert!(pins.ctrl.contains(Ctrl::MREQ | Ctrl::RD), "T{}", t + 1);
            assert!(!pins.ctrl.contains(Ctrl::M1), "a plain read is not a fetch");
        }
    }

    #[test]
    fn memory_write_is_three_t_states_and_asserts_wr_from_t2() {
        let (driven, _) = run(BusOp::MemWrite(0x8000, 0xA5), quiet);
        assert_eq!(driven.len(), 3);
        assert_eq!(
            driven[0].ctrl,
            Ctrl::MREQ,
            "T1 requests memory but does not write"
        );
        for t in [1, 2] {
            assert!(driven[t].ctrl.contains(Ctrl::MREQ | Ctrl::WR), "T{}", t + 1);
            assert_eq!(driven[t].d, Some(0xA5), "T{}", t + 1);
        }
    }

    #[test]
    fn io_cycle_is_four_t_states_with_the_request_from_t2() {
        let (driven, byte) = run(BusOp::IoRead(0x00FE), answering(0x1F));
        assert_eq!(driven.len(), 4);
        assert_eq!(byte, 0x1F);
        assert_eq!(
            driven[0].ctrl,
            Ctrl::NONE,
            "T1 puts the port address up only"
        );
        for t in [1, 2, 3] {
            assert!(driven[t].ctrl.contains(Ctrl::IORQ | Ctrl::RD), "T{}", t + 1);
            assert_eq!(
                driven[t].address,
                Some(crate::Address::Stable(0x00FE)),
                "T{}",
                t + 1
            );
        }
    }

    #[test]
    fn io_write_drives_data_for_the_whole_cycle() {
        let (driven, _) = run(BusOp::IoWrite(0x00FE, 0x07), quiet);
        assert_eq!(driven.len(), 4);
        for (t, pins) in driven.iter().enumerate() {
            assert_eq!(pins.d, Some(0x07), "T{}", t + 1);
        }
        assert!(driven[3].ctrl.contains(Ctrl::IORQ | Ctrl::WR));
    }

    #[test]
    fn wait_extends_a_memory_read_at_t2() {
        let (driven, byte) = run(BusOp::MemRead(0x4000), |tick| {
            let pins = Pins::idle().with_d(0x77);
            if (2..4).contains(&tick) {
                pins.with(Ctrl::WAIT)
            } else {
                pins
            }
        });
        assert_eq!(driven.len(), 5);
        assert_eq!(byte, 0x77);
        for (t, pins) in driven.iter().enumerate() {
            assert!(
                pins.ctrl.contains(Ctrl::MREQ | Ctrl::RD),
                "the read is held through the wait, T{}",
                t + 1
            );
        }
    }

    #[test]
    fn wait_extends_a_fetch_at_t2_and_not_at_t3() {
        let op = BusOp::Fetch {
            pc: 0x0000,
            refresh: 0x0000,
        };
        let (at_t2, _) = run(op, |tick| {
            let pins = Pins::idle();
            if tick == 2 {
                pins.with(Ctrl::WAIT)
            } else {
                pins
            }
        });
        assert_eq!(at_t2.len(), 5);

        let (at_t3, _) = run(op, |tick| {
            let pins = Pins::idle();
            if tick == 3 {
                pins.with(Ctrl::WAIT)
            } else {
                pins
            }
        });
        assert_eq!(at_t3.len(), 4);
    }

    #[test]
    fn wait_extends_an_io_cycle_at_its_automatic_wait_state() {
        let (driven, _) = run(BusOp::IoRead(0x0001), |tick| {
            let pins = Pins::idle();
            if tick == 3 {
                pins.with(Ctrl::WAIT)
            } else {
                pins
            }
        });
        assert_eq!(driven.len(), 5);
    }

    #[test]
    fn internal_cycles_make_no_bus_request() {
        let (driven, byte) = run(BusOp::Internal(5), answering(0xFF));
        assert_eq!(driven.len(), 5);
        assert_eq!(byte, 0, "an internal cycle samples nothing");
        for pins in &driven {
            assert_eq!(pins.ctrl, Ctrl::NONE);
        }
    }

    #[test]
    fn wait_is_ignored_by_an_internal_cycle() {
        let (driven, _) = run(BusOp::Internal(2), |_| Pins::idle().with(Ctrl::WAIT));
        assert_eq!(driven.len(), 2);
    }
}
