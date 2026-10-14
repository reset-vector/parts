//! The eight algorithms, as data (`YM2612-010`).

use super::Operator::{self, Op1, Op2, Op3, Op4};

/// Where each operator's modulation comes from, and which reach the output,
/// for one algorithm (`YM2612-010`). Operator 1 has the feedback path
/// instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Connection {
    pub(crate) op2_from: &'static [Operator],
    pub(crate) op3_from: &'static [Operator],
    pub(crate) op4_from: &'static [Operator],
    pub(crate) carriers: &'static [Operator],
}

/// The eight algorithms of the manual's p. 6 (`YM2612-010`).
pub(crate) const CONNECTIONS: [Connection; 8] = [
    Connection {
        op2_from: &[Op1],
        op3_from: &[Op2],
        op4_from: &[Op3],
        carriers: &[Op4],
    },
    Connection {
        op2_from: &[],
        op3_from: &[Op1, Op2],
        op4_from: &[Op3],
        carriers: &[Op4],
    },
    Connection {
        op2_from: &[],
        op3_from: &[Op2],
        op4_from: &[Op1, Op3],
        carriers: &[Op4],
    },
    Connection {
        op2_from: &[Op1],
        op3_from: &[],
        op4_from: &[Op2, Op3],
        carriers: &[Op4],
    },
    Connection {
        op2_from: &[Op1],
        op3_from: &[],
        op4_from: &[Op3],
        carriers: &[Op2, Op4],
    },
    Connection {
        op2_from: &[Op1],
        op3_from: &[Op1],
        op4_from: &[Op1],
        carriers: &[Op2, Op3, Op4],
    },
    Connection {
        op2_from: &[Op1],
        op3_from: &[],
        op4_from: &[],
        carriers: &[Op2, Op3, Op4],
    },
    Connection {
        op2_from: &[],
        op3_from: &[],
        op4_from: &[],
        carriers: &[Op1, Op2, Op3, Op4],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eight_algorithms() {
        let carriers = CONNECTIONS.map(|a| a.carriers);
        assert_eq!(
            carriers,
            [
                &[Op4][..],
                &[Op4],
                &[Op4],
                &[Op4],
                &[Op2, Op4],
                &[Op2, Op3, Op4],
                &[Op2, Op3, Op4],
                &[Op1, Op2, Op3, Op4],
            ]
        );
        assert_eq!(
            (
                CONNECTIONS[0].op2_from,
                CONNECTIONS[0].op3_from,
                CONNECTIONS[0].op4_from
            ),
            (&[Op1][..], &[Op2][..], &[Op3][..])
        );
        assert_eq!(CONNECTIONS[1].op3_from, &[Op1, Op2]);
        assert_eq!(
            (CONNECTIONS[2].op3_from, CONNECTIONS[2].op4_from),
            (&[Op2][..], &[Op1, Op3][..])
        );
        assert_eq!(
            (CONNECTIONS[3].op2_from, CONNECTIONS[3].op4_from),
            (&[Op1][..], &[Op2, Op3][..])
        );
        assert_eq!(
            (CONNECTIONS[4].op2_from, CONNECTIONS[4].op4_from),
            (&[Op1][..], &[Op3][..])
        );
        for m in [
            CONNECTIONS[5].op2_from,
            CONNECTIONS[5].op3_from,
            CONNECTIONS[5].op4_from,
        ] {
            assert_eq!(m, &[Op1]);
        }
        assert_eq!(CONNECTIONS[6].op2_from, &[Op1]);
        for a in &CONNECTIONS {
            for (to, from) in [(Op2, a.op2_from), (Op3, a.op3_from), (Op4, a.op4_from)] {
                assert!(from.iter().all(|&f| (f as u8) < (to as u8)));
            }
        }
    }
}
