//! The part's state as a fixed block of bytes, and back again.
//!
//! The part's state is the register file of `Z80-017`, `WZ`, the interrupt
//! flip-flops and modes, `HALT` and the latched `NMI` edge; this module only
//! writes that state down and reads it back. It exists because a state that
//! cannot leave the crate cannot be saved by anything outside it, and the
//! crate is deliberately free of any machine that might want to. No behaviour
//! depends on it: `running_is_unaffected_by_a_round_trip` is the test that
//! says so.
//!
//! The layout is this crate's and is versioned by its first byte. It is not a
//! stable format and nothing promises it across versions; a reader that does
//! not recognise the version gets [`StateError::Layout`] rather than a wrong
//! answer.

use crate::exec::{Index, STAGE_BYTES, Stage};
use crate::regs::{InterruptMode, Pending, Registers};
use crate::seq::{BUS_OP_BYTES, BusOp, SEQ_BYTES, Seq};
use crate::{Level, State, Z80};

/// The layout version [`Z80::save_state`] writes.
const VERSION: u8 = 3;

/// The bytes [`Registers`] takes.
const REGS_BYTES: usize = 31;

/// The bytes the [`State`] block takes: a tag, the clocks `RESET` has been
/// held, a cycle.
const STATE_BLOCK_BYTES: usize = 2 + BUS_OP_BYTES;

/// The size of the image [`Z80::save_state`] returns and [`Z80::restore_state`]
/// reads.
pub const STATE_LEN: usize =
    1 + REGS_BYTES + SEQ_BYTES + STATE_BLOCK_BYTES + STAGE_BYTES + 1 + 2 + 1 + 3;

/// A state image this crate cannot read.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum StateError {
    /// The version byte, or a tag inside the block, is not one this crate
    /// writes, or the machine cycle the block holds is one no run reaches.
    Layout,
}

impl core::fmt::Display for StateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("state image layout mismatch")
    }
}

impl core::error::Error for StateError {}

/// `again`, in the byte that carries the part's odds and ends.
const AGAIN: u8 = 1 << 0;
/// `Registers::halted`.
const HALTED: u8 = 1 << 1;
/// `Registers::nmi_pending`.
const NMI_LATCHED: u8 = 1 << 2;
/// `NMI` asserted on the previous tick: `Registers::nmi_n` Low.
const NMI_PREVIOUS: u8 = 1 << 3;
/// `int_line`.
const INT_LINE: u8 = 1 << 4;
/// `Pending::Int` in `Registers::pending`.
const INT_PENDING: u8 = 1 << 5;
/// `Pending::Busreq` in `Registers::pending`.
const BUS_REQUEST: u8 = 1 << 6;

/// `mask` when `set`, and nothing otherwise.
const fn bit(set: bool, mask: u8) -> u8 {
    if set { mask } else { 0 }
}

impl Registers {
    /// The register file as bytes, in the order it is declared, but for the
    /// four latches the odds-and-ends byte carries.
    const fn encode(&self) -> [u8; REGS_BYTES] {
        let [ix_lo, ix_hi] = self.ix.to_le_bytes();
        let [iy_lo, iy_hi] = self.iy.to_le_bytes();
        let [sp_lo, sp_hi] = self.sp.to_le_bytes();
        let [pc_lo, pc_hi] = self.pc.to_le_bytes();
        let [wz_lo, wz_hi] = self.wz.to_le_bytes();
        [
            self.a,
            self.f,
            self.b,
            self.c,
            self.d,
            self.e,
            self.h,
            self.l,
            self.a_alt,
            self.f_alt,
            self.b_alt,
            self.c_alt,
            self.d_alt,
            self.e_alt,
            self.h_alt,
            self.l_alt,
            self.i,
            self.r,
            ix_lo,
            ix_hi,
            iy_lo,
            iy_hi,
            sp_lo,
            sp_hi,
            pc_lo,
            pc_hi,
            wz_lo,
            wz_hi,
            self.q,
            bit(self.iff1, 1) | bit(self.iff2, 2),
            self.im.number(),
        ]
    }

    /// The inverse of [`Registers::encode`], or `None` for an interrupt mode
    /// the part has not got.
    const fn decode(b: [u8; REGS_BYTES]) -> Option<Self> {
        let Some(im) = InterruptMode::from_number(b[30]) else {
            return None;
        };
        Some(Self {
            a: b[0],
            f: b[1],
            b: b[2],
            c: b[3],
            d: b[4],
            e: b[5],
            h: b[6],
            l: b[7],
            a_alt: b[8],
            f_alt: b[9],
            b_alt: b[10],
            c_alt: b[11],
            d_alt: b[12],
            e_alt: b[13],
            h_alt: b[14],
            l_alt: b[15],
            i: b[16],
            r: b[17],
            ix: u16::from_le_bytes([b[18], b[19]]),
            iy: u16::from_le_bytes([b[20], b[21]]),
            sp: u16::from_le_bytes([b[22], b[23]]),
            pc: u16::from_le_bytes([b[24], b[25]]),
            wz: u16::from_le_bytes([b[26], b[27]]),
            q: b[28],
            iff1: b[29] & 1 != 0,
            iff2: b[29] & 2 != 0,
            im,
            halted: false,
            nmi_n: Level::High,
            nmi_pending: false,
            pending: None,
        })
    }
}

impl State {
    /// What the part is doing besides executing, with the clocks `RESET` has
    /// been held, as bytes.
    const fn encode(self, held: u8) -> [u8; STATE_BLOCK_BYTES] {
        let (tag, op) = match self {
            Self::Starting => (0, [0; BUS_OP_BYTES]),
            Self::Running => (1, [0; BUS_OP_BYTES]),
            Self::BusReleased(op) => (3, op.encode()),
        };
        [tag, held, op[0], op[1], op[2], op[3], op[4]]
    }

    /// The inverse of [`State::encode`]; the clocks are `b[1]`.
    const fn decode(b: [u8; STATE_BLOCK_BYTES]) -> Option<Self> {
        Some(match b[0] {
            0 => Self::Starting,
            1 => Self::Running,
            3 => match BusOp::decode([b[2], b[3], b[4], b[5], b[6]]) {
                Some(op) => Self::BusReleased(op),
                None => return None,
            },
            _ => return None,
        })
    }
}

/// Copies `src` into `dst` at `at`, and reports where the next field starts.
fn put<const N: usize>(dst: &mut [u8; STATE_LEN], at: usize, src: [u8; N]) -> usize {
    dst[at..at + N].copy_from_slice(&src);
    at + N
}

/// Takes `N` bytes out of `src` at `at`, and reports where the next field starts.
fn take<const N: usize>(src: &[u8; STATE_LEN], at: usize) -> ([u8; N], usize) {
    let mut out = [0u8; N];
    out.copy_from_slice(&src[at..at + N]);
    (out, at + N)
}

impl Z80 {
    /// The whole of the part's state, as an image.
    ///
    /// Every field goes in: the register file, the machine cycle in flight and
    /// how far into it the part is, the instruction and its step, the prefix,
    /// the latched interrupt edge and the sampled levels. A part restored from
    /// the image runs on identically, mid-instruction included.
    #[must_use]
    pub fn save_state(&self) -> [u8; STATE_LEN] {
        let mut image = [0u8; STATE_LEN];
        let out = &mut image;
        let mut at = put(out, 0, [VERSION]);
        at = put(out, at, self.regs.encode());
        at = put(out, at, self.seq.encode());
        at = put(out, at, self.state.encode(self.reset_held));
        at = put(out, at, self.stage.encode());
        at = put(out, at, [self.index.encode()]);
        at = put(out, at, self.indexed.to_le_bytes());
        at = put(
            out,
            at,
            [bit(self.again, AGAIN)
                | bit(self.regs.halted, HALTED)
                | bit(self.regs.nmi_pending, NMI_LATCHED)
                | bit(self.regs.nmi_n.is_low(), NMI_PREVIOUS)
                | bit(self.int_line, INT_LINE)
                | bit(self.regs.pending == Some(Pending::Busreq), BUS_REQUEST)
                | bit(self.regs.pending == Some(Pending::Int), INT_PENDING)],
        );
        at = put(out, at, [self.lo, self.hi, self.q_previous]);
        debug_assert_eq!(at, STATE_LEN, "every byte of the block is written");
        image
    }

    /// Replaces the whole of the part's state with what `image` holds.
    ///
    /// # Errors
    ///
    /// [`StateError::Layout`] if the version byte is not one this crate writes,
    /// a tag inside the block is not, its flags hold both a bus request and an
    /// accepted `INT`, which exclude one another, or the machine cycle it holds is one no
    /// run reaches (a T-state index more than one past the cycle's last, or
    /// one past it while the part runs, its bus neither released to a request
    /// nor awaiting its first cycle; an internal cycle
    /// longer than any instruction makes). The part is left untouched when that
    /// happens; an image it accepts never makes it panic. Nothing else is
    /// checked: an instruction step past the instruction's last is accepted,
    /// and the part ends that instruction when the cycle in hand ends.
    ///
    /// # Examples
    ///
    /// ```
    /// use z80::{Pins, Z80};
    ///
    /// let mut cpu = Z80::new();
    /// let _ = cpu.tick(Pins::idle());
    /// let mut copy = Z80::new();
    /// copy.restore_state(&cpu.save_state())?;
    /// assert_eq!(copy, cpu);
    ///
    /// let mut foreign = cpu.save_state();
    /// foreign[0] ^= 0xFF; // a version byte this crate does not write
    /// assert!(copy.restore_state(&foreign).is_err());
    /// assert_eq!(copy, cpu, "a refused image leaves the part untouched");
    /// # Ok::<(), z80::StateError>(())
    /// ```
    pub fn restore_state(&mut self, image: &[u8; STATE_LEN]) -> Result<(), StateError> {
        let (version, at) = take::<1>(image, 0);
        if version[0] != VERSION {
            return Err(StateError::Layout);
        }
        let (regs, at) = take::<REGS_BYTES>(image, at);
        let (seq, at) = take::<SEQ_BYTES>(image, at);
        let (state, at) = take::<STATE_BLOCK_BYTES>(image, at);
        let (stage, at) = take::<STAGE_BYTES>(image, at);
        let (index, at) = take::<1>(image, at);
        let (indexed, at) = take::<2>(image, at);
        let (flags, at) = take::<1>(image, at);
        let (odds, at) = take::<3>(image, at);
        debug_assert_eq!(at, STATE_LEN, "every byte of the block is read");

        let regs = Registers::decode(regs).ok_or(StateError::Layout)?;
        let seq = Seq::decode(seq).ok_or(StateError::Layout)?;
        let held = state[1];
        let state = State::decode(state).ok_or(StateError::Layout)?;
        let stage = Stage::decode(stage).ok_or(StateError::Layout)?;
        let index = Index::decode(index[0]).ok_or(StateError::Layout)?;
        if matches!(state, State::Running) && seq.is_past_end() {
            return Err(StateError::Layout);
        }
        let pending = match (flags[0] & BUS_REQUEST != 0, flags[0] & INT_PENDING != 0) {
            (false, false) => None,
            (true, false) => Some(Pending::Busreq),
            (false, true) => Some(Pending::Int),
            (true, true) => return Err(StateError::Layout),
        };

        self.regs = regs;
        self.seq = seq;
        self.state = state;
        self.reset_held = held;
        self.stage = stage;
        self.index = index;
        self.indexed = u16::from_le_bytes(indexed);
        self.again = flags[0] & AGAIN != 0;
        self.regs.halted = flags[0] & HALTED != 0;
        self.regs.nmi_pending = flags[0] & NMI_LATCHED != 0;
        self.regs.nmi_n = Level::from_bool(flags[0] & NMI_PREVIOUS == 0);
        self.int_line = flags[0] & INT_LINE != 0;
        self.regs.pending = pending;
        self.lo = odds[0];
        self.hi = odds[1];
        self.q_previous = odds[2];
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{STATE_LEN, StateError};
    use crate::exec::{Index, Kind, Stage, decode, decode_cb, decode_ed};
    use crate::pins::{Ctrl, Pins};
    use crate::seq::{BusOp, LONGEST_INTERNAL};
    use crate::{State, Z80};
    use std::vec;
    use std::vec::Vec;

    /// A memory image that exercises prefixes, blocks and interrupts rather
    /// than the run of `NOP`s an all-zero one gives.
    fn scrambled() -> Vec<u8> {
        let mut mem = vec![0u8; 0x1_0000];
        let mut x: u32 = 0x1234_5678;
        for byte in &mut mem {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *byte = (x >> 16) as u8;
        }
        mem
    }

    /// Runs `cpu` for `ticks` T-states over `mem`, answering every read, and
    /// calls `each` after every tick.
    fn drive(cpu: &mut Z80, mem: &mut [u8], ticks: usize, mut each: impl FnMut(&mut Z80)) {
        let mut inp = Pins::idle();
        for t in 0..ticks {
            let out = cpu.tick(inp);
            let ctrl = out.ctrl;
            let request = ctrl.contains(Ctrl::MREQ) || ctrl.contains(Ctrl::IORQ);
            assert_eq!(cpu.is_reading(), request && ctrl.contains(Ctrl::RD));
            assert_eq!(cpu.is_writing(), request && ctrl.contains(Ctrl::WR));
            assert_eq!(
                cpu.is_acknowledging_interrupt(),
                ctrl.contains(Ctrl::M1 | Ctrl::IORQ)
            );
            let mut next = Pins::idle();
            if t % 977 == 0 {
                next = next.with(Ctrl::INT);
            }
            if t % 4001 == 0 {
                next = next.with(Ctrl::NMI);
            }
            if cpu.is_reading() && out.ctrl.contains(Ctrl::MREQ) {
                next = next.with_d(
                    mem[usize::from(
                        out.address
                            .and_then(crate::Address::stable)
                            .expect("the cycle drives an address"),
                    )],
                );
            } else if cpu.is_reading() && out.ctrl.contains(Ctrl::IORQ)
                || cpu.is_acknowledging_interrupt()
            {
                next = next.with_d(0xFF);
            }
            if cpu.is_writing() && out.ctrl.contains(Ctrl::MREQ) {
                mem[usize::from(
                    out.address
                        .and_then(crate::Address::stable)
                        .expect("the cycle drives an address"),
                )] = out.d.expect("a write drives d");
            }
            inp = next;
            each(cpu);
        }
    }

    #[test]
    fn every_kind_round_trips() {
        let tables: [fn(u8) -> Kind; 3] = [decode, decode_cb, decode_ed];
        for table in tables {
            for byte in 0..=u8::MAX {
                let kind = table(byte);
                assert_eq!(
                    Kind::decode(kind.encode()),
                    Some(kind),
                    "{kind:?} from byte {byte:#04x}"
                );
            }
        }
        for kind in [Kind::AcceptNmi, Kind::AcceptInt] {
            assert_eq!(Kind::decode(kind.encode()), Some(kind));
        }
    }

    #[test]
    fn every_stage_and_cycle_shape_round_trips() {
        let kind = decode(0x76);
        let stages = [
            Stage::Opcode,
            Stage::CbOpcode,
            Stage::IndexedCbDisplacement,
            Stage::IndexedCbOpcode,
            Stage::EdOpcode,
            Stage::IndexedCbSettle { kind },
            Stage::DisplacementSettle { kind },
            Stage::Displacement { kind },
            Stage::Body { kind, step: 3 },
        ];
        for stage in stages {
            assert_eq!(Stage::decode(stage.encode()), Some(stage), "{stage:?}");
        }
        for index in [Index::None, Index::Ix, Index::Iy] {
            assert_eq!(Index::decode(index.encode()), Some(index));
        }
        let ops = [
            BusOp::Fetch {
                pc: 0x1234,
                refresh: 0x5678,
            },
            BusOp::MemRead(0x9ABC),
            BusOp::MemWrite(0xDEF0, 0x5A),
            BusOp::IoRead(0x00FE),
            BusOp::IoWrite(0x00FE, 0xA5),
            BusOp::Internal(5),
            BusOp::IntAck(0x4321),
        ];
        for op in ops {
            assert_eq!(BusOp::decode(op.encode()), Some(op), "{op:?}");
            let released = State::BusReleased(op);
            assert_eq!(State::decode(released.encode(0)), Some(released));
        }
        for state in [State::Starting, State::Running] {
            assert_eq!(State::decode(state.encode(2)), Some(state), "{state:?}");
        }
    }

    #[test]
    fn a_round_trip_at_every_t_state_restores_an_equal_part() {
        let mut mem = scrambled();
        let mut cpu = Z80::new();
        let mut restored = Z80::new();
        drive(&mut cpu, &mut mem, 20_000, |cpu| {
            restored
                .restore_state(&cpu.save_state())
                .expect("this crate's own image");
            assert_eq!(&restored, &*cpu);
        });
    }

    #[test]
    fn running_is_unaffected_by_a_round_trip() {
        let mut plain_mem = scrambled();
        let mut plain = Z80::new();
        drive(&mut plain, &mut plain_mem, 20_000, |_| {});

        let mut cycled_mem = scrambled();
        let mut cycled = Z80::new();
        drive(&mut cycled, &mut cycled_mem, 20_000, |cpu| {
            let image = cpu.save_state();
            cpu.restore_state(&image).expect("this crate's own image");
        });

        assert_eq!(plain, cycled);
        assert_eq!(plain_mem, cycled_mem, "and it wrote the same bytes");
    }

    #[test]
    fn an_image_from_another_version_is_refused() {
        let mut image = Z80::new().save_state();
        image[0] = 0xFF;
        let mut cpu = Z80::new();
        assert_eq!(cpu.restore_state(&image), Err(StateError::Layout));
    }

    #[test]
    fn a_refused_image_leaves_the_part_alone() {
        let mut mem = scrambled();
        let mut cpu = Z80::new();
        drive(&mut cpu, &mut mem, 500, |_| {});
        let before = cpu.clone();

        let mut image = cpu.save_state();
        image[1 + 31 + 7] = 0x7F;
        assert_eq!(cpu.restore_state(&image), Err(StateError::Layout));
        assert_eq!(cpu, before);
    }

    #[test]
    fn an_image_holding_a_bus_request_and_an_accepted_int_is_refused() {
        let mut image = Z80::new().save_state();
        image[STATE_LEN - 4] |= super::BUS_REQUEST | super::INT_PENDING;
        let mut cpu = Z80::new();
        let before = cpu.clone();
        assert_eq!(cpu.restore_state(&image), Err(StateError::Layout));
        assert_eq!(cpu, before);
        image[STATE_LEN - 4] &= !super::INT_PENDING;
        assert_eq!(cpu.restore_state(&image), Ok(()));
    }

    #[test]
    fn a_cycle_no_run_reaches_is_refused() {
        let image = Z80::new().save_state();
        let seq = 1 + 31;
        let mut cpu = Z80::new();

        let mut late = image;
        late[seq..seq + 5].copy_from_slice(&BusOp::MemRead(0x1234).encode());
        late[seq + 5] = 4;
        assert_eq!(
            cpu.restore_state(&late),
            Ok(()),
            "one past the end is reached"
        );
        let state = seq + 7;
        let mut running = late;
        running[state] = 1;
        assert_eq!(cpu.restore_state(&running), Err(StateError::Layout));
        let mut released = late;
        released[state] = 3;
        released[state + 2..state + 7].copy_from_slice(&BusOp::MemRead(0x1235).encode());
        assert_eq!(cpu.restore_state(&released), Ok(()));
        released[seq + 5] = 5;
        assert_eq!(
            cpu.restore_state(&released),
            Err(StateError::Layout),
            "a released bus holds the index one past the end, no further"
        );
        late[seq + 5] = 5;
        assert_eq!(cpu.restore_state(&late), Err(StateError::Layout));

        let mut empty = image;
        empty[seq..seq + 5].copy_from_slice(&BusOp::Internal(1).encode());
        empty[seq + 1] = 0;
        empty[seq + 5] = 0;
        assert_eq!(cpu.restore_state(&empty), Err(StateError::Layout));
    }

    #[test]
    fn an_internal_cycle_longer_than_any_instruction_runs_is_refused() {
        let image = Z80::new().save_state();
        let seq = 1 + 31;
        let mut cpu = Z80::new();
        let before = cpu.clone();

        let mut long = image;
        long[seq..seq + 5].copy_from_slice(&[5, 0xFF, 0, 0, 0]);
        long[seq + 5] = 0xFF;
        assert_eq!(cpu.restore_state(&long), Err(StateError::Layout));
        assert_eq!(cpu, before);

        long[seq + 1] = LONGEST_INTERNAL + 1;
        long[seq + 5] = 1;
        assert_eq!(cpu.restore_state(&long), Err(StateError::Layout));
        let state = seq + 7;
        let mut released = image;
        released[state] = 3;
        released[state + 2..state + 7].copy_from_slice(&[5, LONGEST_INTERNAL + 1, 0, 0, 0]);
        assert_eq!(cpu.restore_state(&released), Err(StateError::Layout));

        let mut longest = image;
        longest[seq..seq + 5].copy_from_slice(&BusOp::Internal(LONGEST_INTERNAL).encode());
        longest[seq + 5] = LONGEST_INTERNAL + 1;
        assert_eq!(cpu.restore_state(&longest), Ok(()));
    }

    #[test]
    fn a_mutated_image_is_refused_or_runs_without_a_panic() {
        let mut mem = scrambled();
        let mut cpu = Z80::new();
        let mut images = Vec::new();
        let mut t = 0usize;
        drive(&mut cpu, &mut mem, 6_000, |cpu| {
            t += 1;
            if t.is_multiple_of(13) {
                images.push(cpu.save_state());
            }
        });
        let mut x: u32 = 0x9E37_79B9;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let mut accepted = 0usize;
        for (n, base) in images.iter().enumerate() {
            for _ in 0..40 {
                let mut image = *base;
                for _ in 0..=next() % 3 {
                    let at = 1 + next() as usize % (STATE_LEN - 1);
                    image[at] = next() as u8;
                }
                let mut part = Z80::new();
                if part.restore_state(&image).is_err() {
                    continue;
                }
                accepted += 1;
                let _ = part.registers();
                let _ = part.is_at_instruction_boundary();
                let mut settled = part.clone();
                let _ = settled.settle(Pins::idle());
                let mut run_mem = mem.clone();
                drive(&mut part, &mut run_mem, 64, |_| {});
                let mut again = Z80::new();
                assert_eq!(again.restore_state(&part.save_state()), Ok(()), "image {n}");
                assert_eq!(again, part, "image {n}");
            }
        }
        assert!(accepted > 1_000, "the mutations reach running states");
    }

    #[test]
    fn a_step_past_the_instructions_last_is_accepted_and_ends_it() {
        let mut mem = scrambled();
        let mut cpu = Z80::new();
        drive(&mut cpu, &mut mem, 500, |_| {});
        let stage = 1 + 31 + 7 + 7;
        for (kind, step) in [(Kind::Nop, 255), (Kind::CallNn, 200)] {
            let mut image = cpu.save_state();
            image[stage..stage + 4].copy_from_slice(&Stage::Body { kind, step }.encode());
            let mut part = Z80::new();
            assert_eq!(part.restore_state(&image), Ok(()), "{kind:?} at {step}");
            let mut ended = false;
            drive(&mut part, &mut mem.clone(), 16, |cpu| {
                ended |= cpu.is_at_instruction_boundary();
            });
            assert!(ended, "{kind:?} at {step} reaches a boundary");
        }
    }
}
