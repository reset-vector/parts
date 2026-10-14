//! The quantisation widths and their adaptation, `msm5205`'s
//! (`MSM6258-OPEN-01`).

/// The quantisation widths: `msm5205`'s `STEP_TABLE` (`MSM6258-OPEN-01`).
pub(crate) const STEP_TABLE: [u16; 49] = [
    16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130,
    143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796,
    876, 963, 1060, 1166, 1282, 1411, 1552,
];

/// What a code's magnitude adds to the adaptation index: `msm5205`'s
/// `STEP_ADJUST` (`MSM6258-OPEN-01`).
pub(crate) const STEP_ADJUST: [i8; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// The highest adaptation index.
pub(crate) const LAST_STEP_INDEX: u8 = 48;

#[cfg(test)]
mod tests {
    use super::{LAST_STEP_INDEX, STEP_ADJUST, STEP_TABLE};

    #[test]
    fn the_widths_and_adjustments_are_msm5205s() {
        let mut x: u64 = 16 * 1_000_000_000_000;
        for (k, &step) in STEP_TABLE.iter().enumerate() {
            assert_eq!(x / 1_000_000_000_000, u64::from(step), "k = {k}");
            x = x * 11 / 10;
        }
        assert_eq!(STEP_TABLE.len(), usize::from(LAST_STEP_INDEX) + 1);
        for (magnitude, &adjust) in (0i8..).zip(STEP_ADJUST.iter()) {
            let rule = if magnitude < 4 {
                -1
            } else {
                2 * (magnitude - 3)
            };
            assert_eq!(adjust, rule, "magnitude {magnitude}");
        }
    }
}
