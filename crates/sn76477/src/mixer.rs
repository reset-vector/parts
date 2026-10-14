//! The mixer.
//!
//! Three logic inputs choose one of eight rows, and seven of those rows are the
//! logical AND of one, two or three generator outputs — `SN76477-025` and
//! `SN76477-024`. **It does not sum**, in both TI documents' own words, so two
//! sounds at once are a board's problem and not this part's.

/// A row of the mixer's table — `SN76477-025`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Mix {
    /// `C L, B L, A L`.
    Vco,
    /// `C L, B L, A H`.
    Slf,
    /// `C L, B H, A L`.
    Noise,
    /// `C L, B H, A H`.
    VcoNoise,
    /// `C H, B L, A L`.
    SlfNoise,
    /// `C H, B L, A H`.
    SlfVcoNoise,
    /// `C H, B H, A L`.
    SlfVco,
    /// `C H, B H, A H`.
    Inhibit,
}

impl Mix {
    /// The row three logic levels select. `a` is pin 26, `b` pin 25, `c` pin 27,
    /// which is the order the table's columns are **not** printed in.
    #[must_use]
    pub(crate) const fn select(a: bool, b: bool, c: bool) -> Self {
        match (c, b, a) {
            (false, false, false) => Self::Vco,
            (false, false, true) => Self::Slf,
            (false, true, false) => Self::Noise,
            (false, true, true) => Self::VcoNoise,
            (true, false, false) => Self::SlfNoise,
            (true, false, true) => Self::SlfVcoNoise,
            (true, true, false) => Self::SlfVco,
            (true, true, true) => Self::Inhibit,
        }
    }

    /// The mixer output for one set of generator outputs.
    ///
    /// `Inhibit` is low. The data sheet gives that row the word and nothing
    /// else, and `SN76477-OPEN-10` is what it does not say.
    #[must_use]
    pub(crate) const fn level(self, slf: bool, vco: bool, noise: bool) -> bool {
        match self {
            Self::Vco => vco,
            Self::Slf => slf,
            Self::Noise => noise,
            Self::VcoNoise => vco && noise,
            Self::SlfNoise => slf && noise,
            Self::SlfVcoNoise => slf && vco && noise,
            Self::SlfVco => slf && vco,
            Self::Inhibit => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Mix;

    /// `SN76477-024`, `SN76477-025`: the whole table, written from its rows by
    /// (C, B, A) as printed, over every combination of the three generators.
    #[test]
    fn the_whole_table() {
        type Row = ((bool, bool, bool), fn(bool, bool, bool) -> bool);
        let table: [Row; 8] = [
            ((false, false, false), |_slf, vco, _n| vco),
            ((false, false, true), |slf, _vco, _n| slf),
            ((false, true, false), |_slf, _vco, n| n),
            ((false, true, true), |_slf, vco, n| vco && n),
            ((true, false, false), |slf, _vco, n| slf && n),
            ((true, false, true), |slf, vco, n| slf && vco && n),
            ((true, true, false), |slf, vco, _n| slf && vco),
            ((true, true, true), |_slf, _vco, _n| false),
        ];
        for ((c, b, a), expected) in table {
            for bits in 0..8u8 {
                let (slf, vco, noise) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
                assert_eq!(
                    Mix::select(a, b, c).level(slf, vco, noise),
                    expected(slf, vco, noise),
                    "C={c} B={b} A={a} slf={slf} vco={vco} noise={noise}"
                );
            }
        }
        assert_eq!(Mix::select(false, true, false), Mix::Noise);
        assert_eq!(Mix::select(true, false, false), Mix::Slf);
    }
}
