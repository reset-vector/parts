//! The coding tables, the chirp and the interpolation fractions: the
//! crate's definitions (`TMS5220C-OPEN-01` to `TMS5220C-OPEN-04`), and the frame's field
//! widths (`TMS5220C-019`).

/// The parameter fields' widths in bits, energy first (`TMS5220C-019`).
pub(crate) const FIELD_BITS: [u8; 12] = [4, 6, 5, 5, 4, 4, 4, 4, 4, 3, 3, 3];

/// The energy table, in steps of the lattice's 14-bit word, indexed by
/// the 4-bit code; code 15 is the stop
/// code and its entry is not spoken (`TMS5220C-OPEN-01`).
pub(crate) const ENERGY: [u16; 16] = [
    0, 52, 87, 123, 174, 246, 348, 491, 694, 981, 1385, 1957, 2764, 3904, 5514, 7789,
];

/// The pitch table, in samples, indexed by the 6-bit code; 0 is unvoiced
/// (`TMS5220C-OPEN-01`).
pub(crate) const PITCH: [u8; 64] = [
    0, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37,
    38, 39, 40, 41, 42, 44, 46, 48, 50, 52, 53, 56, 58, 60, 62, 65, 68, 70, 72, 76, 78, 80, 84, 86,
    91, 94, 98, 101, 105, 109, 114, 118, 122, 127, 132, 137, 142, 148, 153, 159,
];

/// A coefficient as the coding table prints it, in hundred-thousandths,
/// turned into the crate's Q9 word: `round(v × 512)` (`TMS5220C-OPEN-02`).
const fn q9(v: i32) -> i16 {
    let scaled = v * 512;
    let half = if scaled < 0 { -50_000 } else { 50_000 };
    ((scaled + half) / 100_000) as i16
}

/// The `K1` table, Q9, indexed by the 5-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K1: [i16; 32] = [
    q9(-97850),
    q9(-97270),
    q9(-97070),
    q9(-96680),
    q9(-96290),
    q9(-95900),
    q9(-95310),
    q9(-94140),
    q9(-93360),
    q9(-92580),
    q9(-91600),
    q9(-90620),
    q9(-89650),
    q9(-88280),
    q9(-86910),
    q9(-85350),
    q9(-80420),
    q9(-74058),
    q9(-66019),
    q9(-56116),
    q9(-44296),
    q9(-30706),
    q9(-15735),
    q9(-5),
    q9(15725),
    q9(30696),
    q9(44288),
    q9(56109),
    q9(66013),
    q9(74054),
    q9(80416),
    q9(85350),
];

/// The `K2` table, Q9, 5-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K2: [i16; 32] = [
    q9(-64000),
    q9(-58999),
    q9(-53500),
    q9(-47507),
    q9(-41039),
    q9(-34129),
    q9(-26830),
    q9(-19209),
    q9(-11350),
    q9(-3345),
    q9(4702),
    q9(12690),
    q9(20515),
    q9(28087),
    q9(35325),
    q9(42163),
    q9(48553),
    q9(54464),
    q9(59878),
    q9(64796),
    q9(69227),
    q9(73190),
    q9(76714),
    q9(79828),
    q9(82567),
    q9(84965),
    q9(87057),
    q9(88875),
    q9(90451),
    q9(91813),
    q9(92988),
    q9(98830),
];

/// The `K3` table, Q9, 4-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K3: [i16; 16] = [
    q9(-86000),
    q9(-75467),
    q9(-64933),
    q9(-54400),
    q9(-43867),
    q9(-33333),
    q9(-22800),
    q9(-12267),
    q9(-1733),
    q9(8800),
    q9(19333),
    q9(29867),
    q9(40400),
    q9(50933),
    q9(61467),
    q9(72000),
];

/// The `K4` table, Q9, 4-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K4: [i16; 16] = [
    q9(-64000),
    q9(-53145),
    q9(-42289),
    q9(-31434),
    q9(-20579),
    q9(-9723),
    q9(1132),
    q9(11987),
    q9(22843),
    q9(33698),
    q9(44553),
    q9(55409),
    q9(66264),
    q9(77119),
    q9(87975),
    q9(98830),
];

/// The `K5` table, Q9, 4-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K5: [i16; 16] = [
    q9(-64000),
    q9(-54933),
    q9(-45867),
    q9(-36800),
    q9(-27733),
    q9(-18667),
    q9(-9600),
    q9(-533),
    q9(8533),
    q9(17600),
    q9(26667),
    q9(35733),
    q9(44800),
    q9(53867),
    q9(62933),
    q9(72000),
];

/// The `K6` table, Q9, 4-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K6: [i16; 16] = [
    q9(-50000),
    q9(-41333),
    q9(-32667),
    q9(-24000),
    q9(-15333),
    q9(-6667),
    q9(2000),
    q9(10667),
    q9(19333),
    q9(28000),
    q9(36667),
    q9(45333),
    q9(54000),
    q9(62667),
    q9(71333),
    q9(80000),
];

/// The `K7` table, Q9, 4-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K7: [i16; 16] = [
    q9(-60000),
    q9(-50667),
    q9(-41333),
    q9(-32000),
    q9(-22667),
    q9(-13333),
    q9(-4000),
    q9(5333),
    q9(14667),
    q9(24000),
    q9(33333),
    q9(42667),
    q9(52000),
    q9(61333),
    q9(70667),
    q9(80000),
];

/// The `K8` table, Q9, 3-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K8: [i16; 8] = [
    q9(-50000),
    q9(-31429),
    q9(-12857),
    q9(5714),
    q9(24286),
    q9(42857),
    q9(61429),
    q9(80000),
];

/// The `K9` table, Q9, 3-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K9: [i16; 8] = [
    q9(-50000),
    q9(-34286),
    q9(-18571),
    q9(-2857),
    q9(12857),
    q9(28571),
    q9(44286),
    q9(60000),
];

/// The `K10` table, Q9, 3-bit code (`TMS5220C-OPEN-01`).
pub(crate) const K10: [i16; 8] = [
    q9(-40000),
    q9(-25714),
    q9(-11429),
    q9(2857),
    q9(17143),
    q9(31429),
    q9(45714),
    q9(60000),
];

/// The chirp, signed eight-bit, one entry a sample from the start of a
/// pitch period; zero beyond the last (`TMS5220C-OPEN-03`).
pub(crate) const CHIRP: [i8; 41] = [
    0x00,
    0x2A,
    0xD4_u8 as i8,
    0x32,
    0xB2_u8 as i8,
    0x12,
    0x25,
    0x14,
    0x02,
    0xE1_u8 as i8,
    0xC5_u8 as i8,
    0x02,
    0x5F,
    0x5A,
    0x05,
    0x0F,
    0x26,
    0xFC_u8 as i8,
    0xA5_u8 as i8,
    0xA5_u8 as i8,
    0xD6_u8 as i8,
    0xDD_u8 as i8,
    0xDC_u8 as i8,
    0xFC_u8 as i8,
    0x25,
    0x2B,
    0x22,
    0x21,
    0x0F,
    0xFF_u8 as i8,
    0xF8_u8 as i8,
    0xEE_u8 as i8,
    0xED_u8 as i8,
    0xEF_u8 as i8,
    0xF7_u8 as i8,
    0xF6_u8 as i8,
    0xFA_u8 as i8,
    0x00,
    0x03,
    0x02,
    0x01,
];

/// The interpolation divisors for an eight-interval frame, `IC1`–`IC7` then
/// `IC0` (`TMS5220C-OPEN-04`). A shorter frame uses the tail.
pub(crate) const INTERPOLATION_DIVISORS: [i32; 8] = [8, 8, 8, 4, 4, 2, 2, 1];

#[cfg(test)]
mod tests {
    use super::{
        CHIRP, ENERGY, FIELD_BITS, INTERPOLATION_DIVISORS, K1, K2, K3, K4, K5, K6, K7, K8, K9, K10,
        PITCH,
    };

    /// `TMS5220C-019`: twelve fields, and with the repeat bit fifty bits.
    #[test]
    fn the_field_widths_make_a_fifty_bit_frame() {
        assert_eq!(FIELD_BITS, [4, 6, 5, 5, 4, 4, 4, 4, 4, 3, 3, 3]);
        assert_eq!(
            FIELD_BITS.iter().map(|&b| u32::from(b)).sum::<u32>() + 1,
            50
        );
    }

    /// `TMS5220C-023`, `TMS5220C-OPEN-01` to `TMS5220C-OPEN-04`: the tables' shapes, their ends,
    /// and the coefficients rising with the code.
    #[test]
    fn the_tables_have_their_shapes_and_ends() {
        assert_eq!(ENERGY.len(), 16);
        assert_eq!(PITCH.len(), 64);
        assert_eq!((K1.len(), K2.len()), (32, 32));
        assert_eq!(
            (K3.len(), K4.len(), K5.len(), K6.len(), K7.len()),
            (16, 16, 16, 16, 16)
        );
        assert_eq!((K8.len(), K9.len(), K10.len()), (8, 8, 8));
        assert_eq!(ENERGY[0], 0);
        assert_eq!(ENERGY[14], 5514);
        assert_eq!(PITCH[1], 15);
        assert_eq!(PITCH[63], 159);
        assert!(PITCH.windows(2).skip(1).all(|w| w[1] > w[0]));
        assert_eq!(K1[0], -501);
        assert_eq!(K2[31], 506);
        assert_eq!(K4[15], 506);
        assert_eq!(K1[23], 0);
        assert_eq!(K10[0], -205);
        for table in [
            &K1[..],
            &K2[..],
            &K3[..],
            &K4[..],
            &K5[..],
            &K6[..],
            &K7[..],
        ] {
            assert!(table.windows(2).all(|w| w[1] > w[0]));
        }
        assert_eq!(CHIRP.len(), 41);
        assert_eq!(CHIRP[12], 0x5F);
        assert_eq!(CHIRP[40], 1);
        assert_eq!(INTERPOLATION_DIVISORS, [8, 8, 8, 4, 4, 2, 2, 1]);
    }
}
