//! What the catalogue says a converter code becomes, and no more
//! (`YM2149-029` to `YM2149-031`).
//!
//! The code is five bits wide ([`CODE_BIT_COUNT`]), the width of
//! [`crate::ChannelState::code`]; `ay8910`'s `dac` module has the same shape
//! for its four-bit code.
//!
//! Fig. 1 normalises the top level to 1 V above a DC level of about 2 V and
//! labels thirteen levels, codes 19 to 31. The nineteen below are drawn and not
//! labelled, and this module does not invent them (`YM2149-OPEN-01`). There is
//! no volt, no load and no output impedance here either (`YM2149-OPEN-02`).

use units::PerMille;

/// Bits in the code each channel hands its converter.
pub const CODE_BIT_COUNT: usize = 5;

/// Fig. 1's labelled levels, as printed, in per mille of the top level above
/// the output's DC level, for codes [`LOWEST_LABELLED_CODE`]..=31.
pub const CATALOGUE_PER_MILLE: [PerMille; 13] = [
    PerMille(125),
    PerMille(149),
    PerMille(177),
    PerMille(210),
    PerMille(250),
    PerMille(297),
    PerMille(354),
    PerMille(420),
    PerMille(500),
    PerMille(595),
    PerMille(707),
    PerMille(841),
    PerMille(1000),
];

/// The lowest code Fig. 1 labels.
pub const LOWEST_LABELLED_CODE: u8 = 19;

/// Fig. 1's level for a five-bit `code`, in per mille as
/// [`CATALOGUE_PER_MILLE`]; `None` where Fig. 1 prints no value. Bits above
/// the fifth are ignored.
#[must_use]
pub const fn code5_per_mille(code: u8) -> Option<PerMille> {
    let code = code & 0x1F;
    if code < LOWEST_LABELLED_CODE {
        None
    } else {
        Some(CATALOGUE_PER_MILLE[(code - LOWEST_LABELLED_CODE) as usize])
    }
}
