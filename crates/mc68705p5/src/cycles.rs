//! Table 7's cycles (`MC68705P5-009`).

/// Instruction cycles for `opcode`, or `None` where Table 7's cell is blank
/// (`MC68705P5-009`, `MC68705P5-OPEN-01`).
#[must_use]
pub(crate) const fn cycles(opcode: u8) -> Option<u8> {
    let low = opcode & 0x0F;
    let rmw = !matches!(low, 0x1 | 0x2 | 0x5 | 0xB | 0xE);
    let n = match opcode >> 4 {
        0x0 => 10,
        0x1 => 7,
        0x2 => 4,
        0x3 | 0x7 if rmw => 6,
        0x4 | 0x5 if rmw => 4,
        0x6 if rmw => 7,
        0x8 => match low {
            0x0 => 9,
            0x1 => 6,
            0x3 => 11,
            _ => return None,
        },
        0x9 => match low {
            0x7..=0xD | 0xF => 2,
            _ => return None,
        },
        0xA => match low {
            0x7 | 0xC | 0xF => return None,
            0xD => 8,
            _ => 2,
        },
        hi @ 0xB..=0xF => {
            let base = match hi {
                0xB | 0xF => 4,
                0xC | 0xE => 5,
                _ => 6,
            };
            match low {
                0x7 | 0xF => base + 1,
                0xC => base - 1,
                0xD => base + 3,
                _ => base,
            }
        }
        _ => return None,
    };
    Some(n)
}
