//! The on-chip peripheral port — the one thing this part has that a 6502 does
//! not.
//!
//! Two eight-bit registers at the bottom of the address space: the direction
//! register at `$0000` and the output register at `$0001` (`MOS6510-018`), bit
//! by bit programmable (`MOS6510-019`).

/// The port's output side: its two registers (`MOS6510-018`).
///
/// [`Default`] is both zero, every line an input.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Port {
    /// The latched output data: the output register, at `$0001`.
    pub data: u8,
    /// The data direction register, at `$0000`, 1 for output
    /// (`MOS6510-019`).
    pub direction: u8,
}

impl Port {
    /// The direction register's address (`MOS6510-018`).
    pub(crate) const DIRECTION_ADDRESS: u16 = 0x0000;
    /// The output register's address (`MOS6510-018`).
    pub(crate) const DATA_ADDRESS: u16 = 0x0001;

    /// Both registers zero. `RES` returns the registers here too.
    ///
    /// Zero is this crate's choice under `MOS6510-OPEN-04`: it makes every
    /// line an input, which is the state that cannot drive anything.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            data: 0,
            direction: 0,
        }
    }

    /// The level each line stands at, with the outside world holding the
    /// lines at `pins`: the output register where the direction register makes
    /// the line an output, the outside world's level where it makes it an
    /// input (`MOS6510-020`).
    #[must_use]
    pub(crate) const fn lines(self, pins: u8) -> u8 {
        (self.data & self.direction) | (pins & !self.direction)
    }

    /// What a read of `address` yields, with the outside world holding the
    /// lines at `pins`, or `None` if the address is not one of the two
    /// registers.
    #[must_use]
    pub(crate) const fn read(self, address: u16, pins: u8) -> Option<u8> {
        match address {
            Self::DIRECTION_ADDRESS => Some(self.direction),
            Self::DATA_ADDRESS => Some(self.lines(pins)),
            _ => None,
        }
    }

    /// Take a write to `address`, and say whether it landed in a register.
    pub(crate) const fn write(&mut self, address: u16, value: u8) -> bool {
        match address {
            Self::DIRECTION_ADDRESS => {
                self.direction = value;
                true
            }
            Self::DATA_ADDRESS => {
                self.data = value;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_output_line_reads_back_the_output_register_and_an_input_line_reads_the_pin() {
        let mut port = Port::new();
        port.write(Port::DIRECTION_ADDRESS, 0b0000_1111);
        port.write(Port::DATA_ADDRESS, 0b1010_1010);
        let pins = 0b0101_0101;
        assert_eq!(port.read(Port::DATA_ADDRESS, pins), Some(0b0101_1010));
        assert_eq!(port.read(Port::DIRECTION_ADDRESS, pins), Some(0b0000_1111));
        assert_eq!(port.read(2, pins), None);
    }
}
