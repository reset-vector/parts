//! The eight connections, as data (`YM2203-013`).

use super::Operator::{self, Op1, Op2, Op3, Op4};

/// Where each operator's modulation comes from, and which reach the output,
/// for one algorithm (`YM2203-013`). Operator 1 has the feedback path
/// instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Connection {
    pub(crate) op2_from: &'static [Operator],
    pub(crate) op3_from: &'static [Operator],
    pub(crate) op4_from: &'static [Operator],
    pub(crate) carriers: &'static [Operator],
}

/// The eight connections (`YM2203-013`).
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
