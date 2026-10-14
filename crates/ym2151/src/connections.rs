//! The eight connections of Fig. 2.9, as data (`YM2151-020`).

use super::Operator::{self, C1, C2, M1, M2};

/// Where each operator's modulation comes from and whether it reaches the
/// output, for connection `CON` (`YM2151-020`, Fig. 2.9). Sources are
/// named by operator; `M1` always has the feedback path instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Connection {
    /// What modulates `C1`.
    pub(crate) c1_from: &'static [Operator],
    /// What modulates `M2`.
    pub(crate) m2_from: &'static [Operator],
    /// What modulates `C2`.
    pub(crate) c2_from: &'static [Operator],
    /// Which operators sum into the channel's output.
    pub(crate) carriers: &'static [Operator],
}

/// The eight connections of Fig. 2.9 (`YM2151-020`).
pub(crate) const CONNECTIONS: [Connection; 8] = [
    Connection {
        c1_from: &[M1],
        m2_from: &[C1],
        c2_from: &[M2],
        carriers: &[C2],
    },
    Connection {
        c1_from: &[],
        m2_from: &[M1, C1],
        c2_from: &[M2],
        carriers: &[C2],
    },
    Connection {
        c1_from: &[],
        m2_from: &[C1],
        c2_from: &[M1, M2],
        carriers: &[C2],
    },
    Connection {
        c1_from: &[M1],
        m2_from: &[],
        c2_from: &[C1, M2],
        carriers: &[C2],
    },
    Connection {
        c1_from: &[M1],
        m2_from: &[],
        c2_from: &[M2],
        carriers: &[C1, C2],
    },
    Connection {
        c1_from: &[M1],
        m2_from: &[M1],
        c2_from: &[M1],
        carriers: &[C1, M2, C2],
    },
    Connection {
        c1_from: &[M1],
        m2_from: &[],
        c2_from: &[],
        carriers: &[C1, M2, C2],
    },
    Connection {
        c1_from: &[],
        m2_from: &[],
        c2_from: &[],
        carriers: &[M1, C1, M2, C2],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eight_connections_of_fig_2_9() {
        type Wiring = (
            &'static [Operator],
            &'static [Operator],
            &'static [Operator],
            &'static [Operator],
        );
        let expect: [Wiring; 8] = [
            (&[M1], &[C1], &[M2], &[C2]),
            (&[], &[M1, C1], &[M2], &[C2]),
            (&[], &[C1], &[M1, M2], &[C2]),
            (&[M1], &[], &[C1, M2], &[C2]),
            (&[M1], &[], &[M2], &[C1, C2]),
            (&[M1], &[M1], &[M1], &[C1, M2, C2]),
            (&[M1], &[], &[], &[C1, M2, C2]),
            (&[], &[], &[], &[M1, C1, M2, C2]),
        ];
        for (con, (c1, m2, c2, carriers)) in expect.iter().enumerate() {
            let c = CONNECTIONS[con];
            assert_eq!(
                (c.c1_from, c.m2_from, c.c2_from, c.carriers),
                (*c1, *m2, *c2, *carriers),
                "CON {con}"
            );
        }
    }
}
