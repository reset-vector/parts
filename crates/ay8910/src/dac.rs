//! The two analogue tables, `AY8910-045` and `AY8910-054`: pin B's DC voltage
//! per four-bit code, one specimen, one load to VSS each, integer microvolts
//! above VSS. Nominal reproductions of one measurement, not a transfer function
//! for the part. Each reads a code and nothing else.
//!
//! The code is four bits wide ([`CODE_BIT_COUNT`]), the width of
//! [`crate::ChannelState::code`]; `ym2149`'s `dac` module has the same shape
//! for its five-bit code.
//!
//! **There is no interpolation between them and there is not going to be:**
//! any other load is `AY8910-OPEN-09`.

use units::{Microvolts, Ohms};

/// Bits in the code each channel hands its converter.
pub const CODE_BIT_COUNT: usize = 4;

/// One measurement of output B: the pin's DC voltage per code into one load
/// to VSS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutputBRow {
    /// The load to VSS.
    pub load_ohms: Ohms,
    /// The pin's level above VSS for codes 0..=15.
    pub uv: [Microvolts; 16],
}

impl OutputBRow {
    /// The pin voltage for a four-bit `code`; higher bits are ignored.
    #[must_use]
    pub const fn code4_uv(&self, code: u8) -> Microvolts {
        self.uv[(code & 0x0F) as usize]
    }
}

/// A row's sixteen levels, as printed in microvolts.
const fn levels(uv: [i32; 16]) -> [Microvolts; 16] {
    let mut out = [Microvolts(0); 16];
    let mut code = 0;
    while code < 16 {
        out[code] = Microvolts(uv[code]);
        code += 1;
    }
    out
}

/// Couriersud's July 2014 measurement of one specimen, the sixteen-point
/// lookup into 983 Ω (`AY8910-045`) and then the same measurement's second
/// load, 9.830 kΩ (`AY8910-054`).
pub const OUTPUT_B_ROWS: [OutputBRow; 2] = [
    OutputBRow {
        load_ohms: Ohms(983),
        uv: levels([
            0, 10_600, 15_000, 22_200, 32_000, 46_600, 66_500, 103_900, 123_700, 198_600, 280_300,
            354_800, 470_200, 603_000, 753_000, 925_000,
        ]),
    },
    OutputBRow {
        load_ohms: Ohms(9_830),
        uv: levels([
            0, 99_800, 137_700, 196_000, 270_800, 371_900, 493_800, 691_000, 779_000, 1_066_000,
            1_301_000, 1_474_000, 1_687_000, 1_887_000, 2_074_000, 2_251_000,
        ]),
    },
];
