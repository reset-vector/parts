//! The graphics data sequencer: one dot of text or bit-map graphics from the
//! mode bits, the matrix word and the shifted dot data.
//!
//! The five documented modes are `MOS6567-027` to `MOS6567-031`; the three
//! invalid ones are `MOS6567-055`, and the foreground rule `MOS6567-056`.

/// The three mode bits (`MOS6567-018`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) struct Mode {
    /// `ECM`, bit 6 of `$11`.
    pub ecm: bool,
    /// `BMM`, bit 5 of `$11`.
    pub bmm: bool,
    /// `MCM`, bit 4 of `$16`.
    pub mcm: bool,
}

/// One dot as the sequencer outputs it: a colour code and whether the dot is
/// foreground for priority and collision (`MOS6567-056`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) struct Pixel {
    /// The colour code, `0..=15`.
    pub colour: u8,
    /// Foreground, as `MOS6567-056` decides it.
    pub foreground: bool,
}

/// Whether a dot is foreground, decided by `MCM` alone (`MOS6567-056`): with
/// `MCM` clear a 1 bit, with it set the pairs 10 and 11.
#[must_use]
pub(crate) const fn foreground(mcm: bool, bit: bool, pair: u8) -> bool {
    if mcm { pair >= 2 } else { bit }
}

/// The dot for one bit (`bit`, the register's top bit) and one pair (`pair`,
/// the two bits latched on an even dot), given the matrix word `c` and the four
/// background colours.
///
/// The five documented modes are `MOS6567-027` to `MOS6567-031`; the three
/// invalid ones are black and still form foreground, `MOS6567-055`.
#[must_use]
pub(crate) fn pixel(mode: Mode, c: u16, bit: bool, pair: u8, background: [u8; 4]) -> Pixel {
    let nybble = ((c >> 8) & 0x0f) as u8;
    let upper = ((c >> 4) & 0x0f) as u8;
    let lower = (c & 0x0f) as u8;
    let colour = match (mode.ecm, mode.bmm, mode.mcm) {
        (false, false, false) => {
            if bit {
                nybble
            } else {
                background[0]
            }
        }
        (false, false, true) => {
            if c & 0x800 == 0 {
                if bit { nybble & 0x07 } else { background[0] }
            } else {
                match pair {
                    0 => background[0],
                    1 => background[1],
                    2 => background[2],
                    _ => nybble & 0x07,
                }
            }
        }
        (false, true, false) => {
            if bit {
                upper
            } else {
                lower
            }
        }
        (false, true, true) => match pair {
            0 => background[0],
            1 => upper,
            2 => lower,
            _ => nybble,
        },
        (true, false, false) => {
            if bit {
                nybble
            } else {
                background[((c >> 6) & 0x03) as usize]
            }
        }
        _ => 0,
    };
    Pixel {
        colour,
        foreground: foreground(mode.mcm, bit, pair),
    }
}
