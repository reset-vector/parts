//! The Texas Instruments SN7442A 4-line BCD to 10-line decimal decoder, at its
//! pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7442A-009`), so the whole of it is
//! [`drive`]: a function from the levels the outside world holds the four input
//! pins at to the ten levels the part drives back. There is no clock here and no
//! `tick`, because the data sheet specifies no edge for one to correspond to.
//!
//! **This crate is a reading of the logic diagram, not of the function table.**
//! The logic diagram of TI's data sheet, `SDLS109` p. 2, prints each gate's four
//! input literals inside it (`SN7442A-012`); the crate holds those ten legends
//! transcribed and [`drive`] is ten NAND gates over them.
//!
//! # Examples
//!
//! Code 3 drives output 3 Low and the other nine High.
//!
//! ```
//! use sn7442a::{DECIMAL_MASK, Inputs, drive};
//!
//! assert_eq!(drive(Inputs { abcd: 3 }).decimal, DECIMAL_MASK & !(1 << 3));
//! ```

#![no_std]

/// How many pins the package presents (`SN7442A-002`).
pub const PIN_COUNT: usize = 16;

/// The four input pins, in the low four bits of a byte.
///
/// Bit 0 is `A` and bit 3 is `D`, which is the weighting of `SN7442A-005` and
/// **not** the order they run round the package, which is `SN7442A-002`.
pub const ABCD_MASK: u8 = 0x0F;

/// The ten output pins, in the low ten bits of a `u16`.
///
/// Bit `n` is the output the sheet numbers `n`. Nothing above bit 9 leaves a pin
/// and every value this crate produces is masked with this. The all-ones value
/// is also the part's own resting state on any invalid code (`SN7442A-007`).
pub const DECIMAL_MASK: u16 = 0x03FF;

/// How many outputs the package carries (`SN7442A-001`).
pub const OUTPUT_COUNT: usize = 10;

/// How many inputs it carries.
pub const INPUT_COUNT: usize = 4;

/// The highest input code the part decodes; every code above it is invalid
/// (`SN7442A-006`, `SN7442A-007`).
pub const LAST_VALID_CODE: u8 = 9;

/// Which form of each input the ten NAND gates take, one entry per output, as
/// drawn on `SDLS109` p. 2.
///
/// A set bit means the gate takes that input's **unbarred** form and a clear bit
/// its barred form, under `ABCD_MASK`. So entry 6 is `0b0110`, which is the
/// legend `Ā B C D̄` printed inside the gate driving output 6 at pin 7. Entry
/// *n* is *n* (`SN7442A-012`).
const GATE_LITERALS: [u8; OUTPUT_COUNT] = [
    0b0000, 0b0001, 0b0010, 0b0011, 0b0100, 0b0101, 0b0110, 0b0111, 0b1000, 0b1001,
];

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** No input of this part is drawn with
/// a bar (`SN7442A-002`), so `true` is simply High on the pin. There is no
/// enable and no strobe here: the package has none (`SN7442A-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The `A` to `D` pins, one bit each per [`ABCD_MASK`], a set bit High:
    /// `A` in bit 0 (weight 1) through `D` in bit 3 (weight 8), the weighting of
    /// `SN7442A-005`, against which a gate legend is compared. Bits above 3
    /// are no pin and are ignored.
    pub abcd: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { abcd: 0 }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives on its ten output pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// The pins the function table heads `DECIMAL OUTPUT`, `0` to `9`, one bit
    /// per output per [`DECIMAL_MASK`], a set bit High.
    pub decimal: u16,
}

/// What the part drives on its ten outputs.
///
/// The outputs are active Low (`SN7442A-006`), so the decoded one is the
/// single clear bit, and on an invalid code there is none (`SN7442A-007`).
/// Both of those fall out of the gates rather than being written as cases: a
/// NAND over four literals is High unless every one of them is, and no invalid
/// code satisfies any of the ten legends.
#[must_use]
pub fn drive(inputs: Inputs) -> Outputs {
    let code = inputs.abcd & ABCD_MASK;
    let mut y = 0;
    for (n, literals) in GATE_LITERALS.iter().enumerate() {
        if gate(code, *literals) {
            y |= 1 << n;
        }
    }
    Outputs { decimal: y }
}

/// One four-input NAND gate: the level its output pin is driven to.
///
/// `literals` says which form of each input this gate takes, per
/// `GATE_LITERALS`; a literal is High exactly where the pin's level matches
/// the form drawn. So all four are High only on the one code, and the gate is
/// Low only there (`SN7442A-012`).
fn gate(code: u8, literals: u8) -> bool {
    code != literals
}

#[cfg(test)]
mod tests {
    use super::{ABCD_MASK, GATE_LITERALS, OUTPUT_COUNT};

    /// One four-input legend per output (`SN7442A-012`), and no legend names a
    /// fifth input.
    #[test]
    fn one_legend_per_output_over_four_inputs() {
        assert_eq!(GATE_LITERALS.len(), OUTPUT_COUNT);
        for literals in GATE_LITERALS {
            assert_eq!(
                literals & !ABCD_MASK,
                0,
                "a gate legend names a fifth input"
            );
        }
    }
}
