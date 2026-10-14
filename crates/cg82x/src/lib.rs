//! Midway's `CG820`–`CG823`, the four customs `U0`–`U3` of the Ms. Pac-Man
//! auxiliary board, at their pins, as one part.
//!
//! Every item below names the claim or the open item it implements.
//!
//! # Examples
//!
//! ```
//! use cg82x::{Cg82x, Inputs, Level};
//!
//! let mut part = Cg82x::new();
//! // In the add-on state a memory cycle at `3000` selects `U7`.
//! assert_eq!(part.step(Inputs::memory_access(0x3000)).cse2_n, Level::Low);
//! // A cycle at `0038` returns the part to the Pac-Man state...
//! let _ = part.step(Inputs::memory_access(0x0038));
//! // ...where `3000` selects nothing.
//! assert_eq!(part.step(Inputs::memory_access(0x3000)).cse2_n, Level::High);
//! ```
//!
//! # The shape
//!
//! The four packages are one part here: what each does on its own, and
//! what the nets between them carry, is `CG82X-OPEN-01`. So the crate has the
//! pins that leave the four:
//!
//! - in: the Z80's `A3`–`A15`, `/MREQ` and `/RFSH`, and nothing else
//!   (`CG82X-001`). [`Inputs::a`] carries all sixteen lines and `A0`–`A2` are
//!   never read;
//! - out: `/CSE0`, `/CSE1` and `/CSE2`, the chip selects of `U5`, `U6` and
//!   `U7`; `RA3`, `RA4`, `RA5`, `RA8`, `RA9` and `RA10`, six of `U5`'s
//!   eleven address pins; and `/JRFSH`, which the board sends to the Pac-Man
//!   board in place of the Z80's `/RFSH`.
//!
//! One bit of state (`CG82X-002`), which [`Cg82x::save_state`] carries. The
//! part has no clock: the host
//! calls [`Cg82x::step`] whenever an input moves, and a memory cycle — `/MREQ`
//! Low with `/RFSH` High — at a trigger window moves the state as soon as it
//! is seen (`CG82X-004`, `CG82X-005`, `CG82X-OPEN-08`), before the outputs are
//! formed, so that cycle is answered as the new state answers it
//! (`CG82X-007`). Nothing distinguishes a read from a write, there being no
//! pin for either (`CG82X-003`).
//!
//! The selects and `/JRFSH` are drawn with a bar, so `Level::Low` is a select
//! asserted, or `/JRFSH` asserted.
//!
//! **What the crate chooses**, each an open item: a refresh cycle neither
//! selects nor moves the state (`CG82X-OPEN-04`); `/JRFSH` is Low under any
//! select and the Z80's `/RFSH` otherwise (`CG82X-OPEN-05`); `RA3`–`RA10`
//! follow the address wherever no window applies, and the selects are High
//! outside a memory cycle (`CG82X-OPEN-06`); and the add-on state selects
//! `U5` below `3000` at the 38 windows only (`CG82X-OPEN-07`).
//!
//! # What is not here
//!
//! Any figure in nanoseconds; a power-on state other than the one
//! [`Cg82x::new`] chooses (`CG82X-OPEN-03`); the package pin numbers, which are
//! the board drawing's and listed on each field only as a reader's aid; and
//! the board's own wiring of `U5`–`U7`, which is the board's.

#![no_std]

pub use signal::Level;

/// The bit of [`Outputs::ra`] that carries `RA3`. The part drives six of
/// `U5`'s address pins, and [`Outputs::ra`] packs them into its low six bits in
/// the order `RA3`, `RA4`, `RA5`, `RA8`, `RA9`, `RA10`, so a bit's position is
/// not its pin's number.
pub const RA3_MASK: u8 = 1 << 0;
/// `RA4`.
pub const RA4_MASK: u8 = 1 << 1;
/// `RA5`.
pub const RA5_MASK: u8 = 1 << 2;
/// `RA8`.
pub const RA8_MASK: u8 = 1 << 3;
/// `RA9`.
pub const RA9_MASK: u8 = 1 << 4;
/// `RA10`.
pub const RA10_MASK: u8 = 1 << 5;

/// The part's one bit (`CG82X-002`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum State {
    /// No add-on ROM is selected anywhere; the Pac-Man board answers.
    PacMan,
    /// The add-on ROMs answer the windows of `CG82X-008` to `CG82X-011`.
    AddOn,
}

/// The levels at the part's inputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The Z80's address bus; `A0`–`A2` enter no package and are ignored.
    pub a: u16,
    /// `/MREQ` (`U1` pin 11, `U3` pin 9).
    pub mreq_n: Level,
    /// The Z80's `/RFSH` (`U1` pin 9, `U3` pin 8).
    pub rfsh_n: Level,
}

impl Inputs {
    /// A memory cycle at `a`: `/MREQ` Low, `/RFSH` High.
    #[must_use]
    pub const fn memory_access(a: u16) -> Self {
        Self {
            a,
            mreq_n: Level::Low,
            rfsh_n: Level::High,
        }
    }

    /// The bus at rest at `0000`: `/MREQ` and `/RFSH` High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: 0,
            mreq_n: Level::High,
            rfsh_n: Level::High,
        }
    }

    const fn is_cycle(self) -> bool {
        self.mreq_n.is_low() && self.rfsh_n.is_high()
    }
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

/// The levels at the part's outputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `/CSE0`, `U5`'s select (`U3` pin 15).
    pub cse0_n: Level,
    /// `/CSE1`, `U6`'s select (`U3` pin 16).
    pub cse1_n: Level,
    /// `/CSE2`, `U7`'s select (`U3` pin 17).
    pub cse2_n: Level,
    /// `RA3`, `RA4`, `RA5`, `RA8`, `RA9` and `RA10` in bits 0 to 5, at the
    /// masks [`RA3_MASK`], [`RA4_MASK`], [`RA5_MASK`], [`RA8_MASK`], [`RA9_MASK`] and [`RA10_MASK`] (`U2`
    /// pins 15, 16, 14, 13; `U3` pins 14, 13), High where the bit is set. Bit
    /// `n` is not `RAn`.
    pub ra: u8,
    /// `/JRFSH`, to the Pac-Man board's `/RFSH` (`U3` pin 18).
    pub jrfsh_n: Level,
}

/// The 38 windows below `3000` the add-on state answers from `U5`, each with
/// the levels of `RA3`–`RA10` the part drives there (`CG82X-011`).
const PATCHES: [(u16, u8); 38] = [
    (0x0410, RA4_MASK),
    (0x08e0, RA3_MASK | RA4_MASK | RA10_MASK),
    (0x0a30, RA3_MASK | RA4_MASK | RA10_MASK),
    (0x0c20, RA9_MASK | RA10_MASK),
    (0x0e58, RA4_MASK | RA9_MASK | RA10_MASK),
    (0x0ea8, RA3_MASK | RA4_MASK | RA10_MASK),
    (0x1000, RA9_MASK),
    (0x1008, RA3_MASK),
    (0x1348, RA4_MASK),
    (0x1688, RA4_MASK),
    (0x16b0, RA4_MASK | RA10_MASK),
    (0x16d8, RA4_MASK),
    (0x16f8, RA4_MASK | RA10_MASK),
    (0x19a8, RA4_MASK | RA9_MASK),
    (0x19b8, RA4_MASK | RA9_MASK | RA10_MASK),
    (0x2060, RA4_MASK | RA10_MASK),
    (0x2108, RA3_MASK | RA4_MASK),
    (0x21a0, RA9_MASK | RA10_MASK),
    (0x2298, RA9_MASK),
    (0x23e0, RA4_MASK | RA9_MASK),
    (0x2418, 0),
    (0x2448, RA3_MASK | RA4_MASK),
    (0x2470, RA10_MASK),
    (0x2488, 0),
    (0x24b0, RA10_MASK),
    (0x24d8, 0),
    (0x24f8, RA10_MASK),
    (0x2748, RA3_MASK),
    (0x2780, RA3_MASK),
    (0x27b8, RA3_MASK | RA10_MASK),
    (0x2800, RA4_MASK | RA9_MASK),
    (0x2b20, RA10_MASK),
    (0x2b30, RA3_MASK | RA10_MASK),
    (0x2bf0, RA3_MASK | RA10_MASK),
    (0x2cc0, RA3_MASK),
    (0x2cd8, RA9_MASK),
    (0x2cf0, RA9_MASK | RA10_MASK),
    (0x2d60, RA9_MASK | RA10_MASK),
];

/// The windows a cycle at which leaves the part in [`State::PacMan`]
/// (`CG82X-005`). Each is the first of eight addresses: a cycle at `a` is in
/// the window `a & !7` (`CG82X-006`).
const CLEARS: [u16; 7] = [0x0038, 0x03b0, 0x1600, 0x2120, 0x3ff0, 0x8000, 0x97f0];

/// The window a cycle at which leaves the part in [`State::AddOn`]
/// (`CG82X-004`), eight addresses as each of [`CLEARS`].
const SET_WINDOW: u16 = 0x3ff8;

/// The state a cycle at `a` leaves the part in, if `a` is in a trigger
/// window; `None` elsewhere (`CG82X-006`).
const fn trigger(a: u16) -> Option<State> {
    let w = a & !7;
    if w == SET_WINDOW {
        return Some(State::AddOn);
    }
    let mut i = 0;
    while i < CLEARS.len() {
        if CLEARS[i] == w {
            return Some(State::PacMan);
        }
        i += 1;
    }
    None
}

/// `RA3`–`RA10` from `A4`, `A3`, `A10`, `A9`, `A5`, `A8` (`CG82X-009`), and
/// what the part drives wherever no table entry applies (`CG82X-OPEN-06`).
const fn ra_in_order(a: u16) -> u8 {
    let a = a as u32;
    ((a >> 4 & 1)
        | (a >> 3 & 1) << 1
        | (a >> 10 & 1) << 2
        | (a >> 9 & 1) << 3
        | (a >> 5 & 1) << 4
        | (a >> 8 & 1) << 5) as u8
}

const fn patch(a: u16) -> Option<u8> {
    let w = a & !7;
    let mut i = 0;
    while i < PATCHES.len() {
        if PATCHES[i].0 == w {
            return Some(PATCHES[i].1);
        }
        i += 1;
    }
    None
}

/// One of the part's three chip selects.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Select {
    /// `/CSE0`, `U5`'s select.
    Cse0,
    /// `/CSE1`, `U6`'s select.
    Cse1,
    /// `/CSE2`, `U7`'s select.
    Cse2,
}

/// Which select a cycle at `a` asserts in `state`, if any (`CG82X-008` to
/// `CG82X-012`).
const fn select(state: State, a: u16) -> Option<Select> {
    if matches!(state, State::PacMan) {
        return None;
    }
    if patch(a).is_some() {
        return Some(Select::Cse0);
    }
    match a {
        0x3000..=0x3fff => Some(Select::Cse2),
        0x8000..=0x87ff => Some(Select::Cse0),
        0x8800..=0x97ff => Some(Select::Cse1),
        _ => None,
    }
}

/// The outputs for `inputs` in `state`, with no change of state.
const fn answer(state: State, inputs: Inputs) -> Outputs {
    let ra = match patch(inputs.a) {
        Some(ra) => ra,
        None => ra_in_order(inputs.a),
    };
    let chosen = if inputs.is_cycle() {
        select(state, inputs.a)
    } else {
        None
    };
    Outputs {
        cse0_n: Level::from_bool(!matches!(chosen, Some(Select::Cse0))),
        cse1_n: Level::from_bool(!matches!(chosen, Some(Select::Cse1))),
        cse2_n: Level::from_bool(!matches!(chosen, Some(Select::Cse2))),
        ra,
        jrfsh_n: Level::from_bool(chosen.is_none() && inputs.rfsh_n.is_high()),
    }
}

/// The length of [`Cg82x::save_state`]'s image.
pub const STATE_LEN: usize = 1;

/// A state image this crate cannot read.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum StateError {
    /// The byte is not one [`Cg82x::save_state`] writes.
    Layout,
}

impl core::fmt::Display for StateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("state image layout mismatch")
    }
}

impl core::error::Error for StateError {}

/// The four customs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Cg82x {
    state: State,
    /// The levels the last step moved the pins to.
    inputs: Inputs,
}

impl Default for Cg82x {
    fn default() -> Self {
        Self::new()
    }
}

impl Cg82x {
    /// The part in the add-on state, this crate's choice under
    /// `CG82X-OPEN-03`: the part has no reset pin, and a board that needs a
    /// known state runs a memory cycle at a trigger window (`CG82X-004`,
    /// `CG82X-005`).
    #[must_use]
    pub const fn new() -> Self {
        Self::with_state(State::AddOn)
    }

    /// The part holding `state`, for a board that knows which it is in.
    #[must_use]
    pub const fn with_state(state: State) -> Self {
        Self {
            state,
            inputs: Inputs::idle(),
        }
    }

    /// The state the part holds (`CG82X-002`).
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// The part's one bit (`CG82X-002`) as a state image: 0 where the Pac-Man
    /// board answers, 1 where the add-on ROMs do.
    #[must_use]
    pub const fn save_state(&self) -> [u8; STATE_LEN] {
        [match self.state {
            State::PacMan => 0,
            State::AddOn => 1,
        }]
    }

    /// The inverse of [`Cg82x::save_state`].
    ///
    /// # Errors
    ///
    /// [`StateError::Layout`] where the byte is neither 0 nor 1, because such
    /// an image is not one this crate wrote.
    ///
    /// # Examples
    ///
    /// ```
    /// use cg82x::{Cg82x, StateError};
    ///
    /// # fn main() -> Result<(), StateError> {
    /// let mut part = Cg82x::new();
    /// part.restore_state(&[1])?;
    /// assert_eq!(part.save_state(), [1]);
    /// assert_eq!(part.restore_state(&[2]), Err(StateError::Layout));
    /// # Ok(())
    /// # }
    /// ```
    pub const fn restore_state(&mut self, image: &[u8; STATE_LEN]) -> Result<(), StateError> {
        self.state = match image[0] {
            0 => State::PacMan,
            1 => State::AddOn,
            _ => return Err(StateError::Layout),
        };
        Ok(())
    }

    /// The pins move to `inputs`: a memory cycle at a trigger window moves
    /// the state first, then the outputs are formed. The part has no clock;
    /// call this whenever an input moves. What comes back
    /// [`Cg82x::outputs`] reads again.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        if inputs.is_cycle()
            && let Some(next) = trigger(inputs.a)
        {
            self.state = next;
        }
        self.inputs = inputs;
        self.outputs()
    }

    /// The outputs the held state forms from the levels the last step moved
    /// the pins to; before the first, from [`Inputs::idle`].
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        answer(self.state, self.inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_in_order_below_3000() {
        assert!(PATCHES.windows(2).all(|p| p[0].0 < p[1].0));
        assert!(
            PATCHES
                .iter()
                .all(|&(w, ra)| w < 0x3000 && w & 7 == 0 && ra & (RA5_MASK | RA8_MASK) == 0)
        );
    }

    #[test]
    fn idle_pins_leave_the_state_alone() {
        let mut part = Cg82x::with_state(State::PacMan);
        let out = part.step(Inputs {
            a: SET_WINDOW,
            ..Inputs::idle()
        });
        assert_eq!(part.state, State::PacMan);
        assert!(
            [out.cse0_n, out.cse1_n, out.cse2_n, out.jrfsh_n]
                .iter()
                .all(|l| l.is_high())
        );
    }

    #[test]
    fn a_cycle_anywhere_in_a_window_moves_the_state_there() {
        for &w in &CLEARS {
            for a in w..w + 8 {
                let mut part = Cg82x::with_state(State::AddOn);
                let _ = part.step(Inputs::memory_access(a));
                assert_eq!(part.state(), State::PacMan, "{a:04x}");
            }
        }
        for a in SET_WINDOW..SET_WINDOW + 8 {
            let mut part = Cg82x::with_state(State::PacMan);
            let _ = part.step(Inputs::memory_access(a));
            assert_eq!(part.state(), State::AddOn, "{a:04x}");
        }
        let mut part = Cg82x::with_state(State::PacMan);
        let _ = part.step(Inputs::memory_access(SET_WINDOW + 8));
        assert_eq!(part.state(), State::PacMan);
    }

    /// `outputs()` forms again what the last step returned, the state the
    /// step moved included.
    #[test]
    fn outputs_reads_back_the_last_step() {
        let mut part = Cg82x::new();
        assert_eq!(part.outputs(), answer(State::AddOn, Inputs::idle()));
        for a in [CLEARS[0], 0x0000, SET_WINDOW, 0x0000, 0x2000] {
            let out = part.step(Inputs::memory_access(a));
            assert_eq!(part.outputs(), out, "{a:04x}");
        }
        let out = part.step(Inputs::idle());
        assert_eq!(part.outputs(), out);
    }
}
