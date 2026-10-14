//! The Motorola MC3340 electronic attenuator, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The law is the sheet's graph, transcribed.** Figure 3 gives attenuation
//! against the DC voltage on pin 2 as three curves, one per supply
//! (`MC3340-011` to `MC3340-013`), with no curve between supplies
//! (`MC3340-OPEN-05`), and Figure 4 gives it against a resistor
//! from pin 2 to ground (`MC3340-015`). Each is a table here, of [`ControlRow`]s
//! and of [`ResistorRow`]s,
//! and [`attenuation_cb`] and [`attenuation_at_resistor_cb`] interpolate
//! **linearly between adjacent points** — the crate's choice, not the
//! sheet's — and hold the end point's value past either end
//! (`MC3340-OPEN-06`).
//!
//! **Units.** Voltages are microvolts. Attenuation is in centibels, tenths
//! of a decibel, which is the transcription's own resolution. A gain is a
//! voltage ratio in parts per billion; [`ratio_ppb`] turns centibels into one
//! from a 200-entry table of 10^(−r/200) and never touches a float.
//!
//! **Two forms.** [`Mc3340::drive`] is the law without time: output
//! from input and control in the same instant. [`Stepped`] is the same law
//! with one sample of latency, for a host that probes `IN` and writes `OUT`
//! once a sample; the sheet says nothing about how fast the part is
//! (`MC3340-OPEN-07`).
//!
//! **No frequency.** The transfer is flat at every frequency, with or without
//! a capacitor on pin 6 (`MC3340-OPEN-04`).
//!
//! **The DC levels are the caller's.** The sheet prints no DC level for pin
//! 1 or pin 7 with no signal (`MC3340-OPEN-02`, `MC3340-OPEN-03`). Pin 1 is
//! taken as its signal, its departure from its own DC level, which the
//! transfer does not see; pin 7 is its voltage, about a DC level the caller
//! declares in [`Mc3340::new`], and Figure 6's swing is centred on that
//! level (`MC3340-OPEN-09`).
//!
//! # Examples
//!
//! ```
//! use mc3340::{Corner, Curve, Inputs, Mc3340, swing_at_supply};
//! use units::Microvolts;
//!
//! // The typical part at 16 V, its output resting at 6 V inside Figure 6's
//! // swing there.
//! let dc = Microvolts(6_000_000);
//! let swing = swing_at_supply(dc, Curve::Vcc16V.test_supply_uv())?;
//! let part = Mc3340::at_corner(Curve::Vcc16V, dc, swing, Corner::Typ)?;
//!
//! // 100 mV of signal at full gain, 13 dB, is 446.684 mV on pin 7.
//! let pins = Inputs {
//!     input_signal_uv: Microvolts(100_000),
//!     control_uv: Microvolts(0),
//! };
//! assert_eq!(part.drive(pins).output_uv, Microvolts(6_446_684));
//!
//! // At 6.5 V on pin 2 the curve reads 80.6 dB down, and 42 µV are left.
//! let attenuated = Inputs {
//!     control_uv: Microvolts(6_500_000),
//!     ..pins
//! };
//! assert_eq!(part.drive(attenuated).output_uv, Microvolts(6_000_042));
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

#![no_std]

use units::{Centibels, Microvolts, Ohms, PartsPerBillion, Picoamps, Picofarads};

/// How many pins the package presents (`MC3340-001`).
pub const PIN_COUNT: usize = 8;

/// The signal input (`MC3340-002`).
pub const INPUT_PIN: u8 = 1;
/// The control (`MC3340-002`).
pub const CONTROL_PIN: u8 = 2;
/// Ground (`MC3340-002`).
pub const GND_PIN: u8 = 3;
/// The roll-off pin (`MC3340-002`, `MC3340-019`).
pub const ROLL_OFF_PIN: u8 = 6;
/// The output (`MC3340-002`).
pub const OUTPUT_PIN: u8 = 7;
/// The supply (`MC3340-002`).
pub const VCC_PIN: u8 = 8;

/// The operating supply, lowest and highest, in microvolts: 9.0 V to 18 V
/// (`MC3340-003`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(9_000_000), Microvolts(18_000_000));
/// The absolute maximum supply, in microvolts: 20 V (`MC3340-003`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(20_000_000);

/// The maximum input voltage, RMS, in microvolts: 0.5 Vrms (`MC3340-004`).
pub const INPUT_MAX_RMS_UV: Microvolts = Microvolts(500_000);

/// The voltage gain at full gain, minimum and typical, in centibels: 11 dB
/// and 13 dB (`MC3340-005`).
pub const GAIN_MIN_TYP_CB: (Centibels, Centibels) = (Centibels(110), Centibels(130));

/// The same two gains as voltage ratios in parts per billion, minimum and
/// typical: [`ratio_ppb`] of each (`MC3340-005`).
pub const GAIN_MIN_TYP_PPB: (PartsPerBillion, PartsPerBillion) =
    (ratio_ppb(GAIN_MIN_TYP_CB.0), ratio_ppb(GAIN_MIN_TYP_CB.1));

/// The span of attenuation from maximum gain, the sheet's attenuation
/// range, minimum and typical, in centibels: 70 dB and 80 dB (`MC3340-006`).
pub const ATTENUATION_SPAN_MIN_TYP_CB: (Centibels, Centibels) = (Centibels(700), Centibels(800));
/// The control at which that span is stated, in microvolts: 6.5 V, at
/// `VCC` 16 V (`MC3340-006`).
pub const ATTENUATION_SPAN_TEST_CONTROL_UV: Microvolts = Microvolts(6_500_000);

/// The most pin 2 sinks with no input signal, in picoamperes: 2.0 mA
/// (`MC3340-007`).
pub const CONTROL_SINK_MAX_PA: Picoamps = Picoamps(2_000_000_000);

/// The maker's circuit: the capacitor from pin 6 to ground, in picofarads
/// (`MC3340-016`).
pub const APPLICATION_ROLL_OFF_PF: Picofarads = Picofarads(620);

/// One point of a Figure 3 curve: at a control voltage, the attenuation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct ControlRow {
    /// The control on pin 2, in microvolts.
    pub uv: Microvolts,
    /// The attenuation from maximum gain, in centibels.
    pub attenuation_cb: Centibels,
}

const fn p(control_uv: i32, attenuation_cb: i32) -> ControlRow {
    ControlRow {
        uv: Microvolts(control_uv),
        attenuation_cb: Centibels(attenuation_cb),
    }
}

/// One point of Figure 4: at a resistor from pin 2 to ground, the
/// attenuation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct ResistorRow {
    /// The resistor, in ohms.
    pub control_ohms: Ohms,
    /// The attenuation from maximum gain, in centibels.
    pub attenuation_cb: Centibels,
}

const fn r(control_ohms: u64, attenuation_cb: i32) -> ResistorRow {
    ResistorRow {
        control_ohms: Ohms(control_ohms),
        attenuation_cb: Centibels(attenuation_cb),
    }
}

/// Figure 3 at `VCC` 8.0 V (`MC3340-011`).
const CURVE_8V: [ControlRow; 15] = [
    p(1_500_000, 3),
    p(1_867_000, 12),
    p(2_000_000, 30),
    p(2_063_000, 43),
    p(2_138_000, 73),
    p(2_194_000, 93),
    p(2_302_000, 132),
    p(2_469_000, 193),
    p(2_705_000, 293),
    p(2_951_000, 393),
    p(3_208_000, 493),
    p(3_486_000, 593),
    p(3_780_000, 693),
    p(4_095_000, 793),
    p(4_398_000, 883),
];

/// Figure 3 at `VCC` 12 V (`MC3340-012`).
const CURVE_12V: [ControlRow; 20] = [
    p(1_500_000, 0),
    p(2_750_000, 0),
    p(2_930_000, 3),
    p(3_046_000, 9),
    p(3_077_000, 12),
    p(3_175_000, 22),
    p(3_241_000, 32),
    p(3_288_000, 43),
    p(3_350_000, 63),
    p(3_388_000, 83),
    p(3_406_000, 93),
    p(3_496_000, 143),
    p(3_584_000, 193),
    p(3_811_000, 293),
    p(4_058_000, 393),
    p(4_319_000, 493),
    p(4_598_000, 593),
    p(4_896_000, 693),
    p(5_215_000, 793),
    p(5_520_000, 883),
];

/// Figure 3 at `VCC` 16 V (`MC3340-013`).
const CURVE_16V: [ControlRow; 17] = [
    p(1_500_000, 0),
    p(3_930_000, 0),
    p(4_058_000, 6),
    p(4_116_000, 12),
    p(4_195_000, 22),
    p(4_254_000, 32),
    p(4_301_000, 43),
    p(4_371_000, 63),
    p(4_445_000, 93),
    p(4_673_000, 193),
    p(4_930_000, 293),
    p(5_201_000, 393),
    p(5_489_000, 493),
    p(5_793_000, 593),
    p(6_114_000, 693),
    p(6_455_000, 793),
    p(6_500_000, 806),
];

/// Figure 4, a resistor from pin 2 to ground, in ohms (`MC3340-015`).
pub const RESISTOR_ROWS: [ResistorRow; 11] = [
    r(4_000, 0),
    r(5_000, 40),
    r(6_000, 80),
    r(7_000, 134),
    r(8_000, 200),
    r(9_000, 270),
    r(10_000, 347),
    r(15_000, 551),
    r(20_000, 680),
    r(25_000, 779),
    r(30_000, 860),
];

/// One point of Figure 6: at a supply, the output swing, peak to peak.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct SwingRow {
    /// The supply, in microvolts.
    pub test_supply_uv: Microvolts,
    /// The swing, peak to peak, in microvolts.
    pub pp_uv: Microvolts,
}

const fn s(test_supply_uv: i32, pp_uv: i32) -> SwingRow {
    SwingRow {
        test_supply_uv: Microvolts(test_supply_uv),
        pp_uv: Microvolts(pp_uv),
    }
}

/// Figure 6: the output swing, peak to peak, against supply (`MC3340-017`).
pub const SWING_ROWS: [SwingRow; 11] = [
    s(8_000_000, 5_000_000),
    s(9_000_000, 6_000_000),
    s(10_000_000, 6_500_000),
    s(11_000_000, 7_000_000),
    s(12_000_000, 7_310_000),
    s(13_000_000, 7_550_000),
    s(14_000_000, 7_760_000),
    s(15_000_000, 8_000_000),
    s(16_000_000, 8_180_000),
    s(17_000_000, 8_390_000),
    s(18_000_000, 8_600_000),
];

/// Which of Figure 3's three curves, named by its supply (`MC3340-010`).
///
/// There is no curve for any other supply (`MC3340-OPEN-05`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Curve {
    /// `VCC` = 8.0 V, below the operating range (`MC3340-003`, `MC3340-011`).
    Vcc8V,
    /// `VCC` = 12 V (`MC3340-012`).
    Vcc12V,
    /// `VCC` = 16 V, the electrical characteristics' own supply (`MC3340-013`).
    Vcc16V,
}

impl Curve {
    /// The three curves, lowest supply first.
    pub const ALL: [Self; 3] = [Self::Vcc8V, Self::Vcc12V, Self::Vcc16V];

    /// The curve's points.
    #[must_use]
    pub const fn rows(self) -> &'static [ControlRow] {
        match self {
            Self::Vcc8V => &CURVE_8V,
            Self::Vcc12V => &CURVE_12V,
            Self::Vcc16V => &CURVE_16V,
        }
    }

    /// The supply the curve is drawn at, in microvolts.
    #[must_use]
    pub const fn test_supply_uv(self) -> Microvolts {
        match self {
            Self::Vcc8V => Microvolts(8_000_000),
            Self::Vcc12V => Microvolts(12_000_000),
            Self::Vcc16V => Microvolts(16_000_000),
        }
    }

    /// The curve drawn at exactly this supply, or `None`: the sheet draws no
    /// other (`MC3340-OPEN-05`).
    #[must_use]
    pub const fn at_test_supply_uv(supply_uv: Microvolts) -> Option<Self> {
        match supply_uv.0 {
            8_000_000 => Some(Self::Vcc8V),
            12_000_000 => Some(Self::Vcc12V),
            16_000_000 => Some(Self::Vcc16V),
            _ => None,
        }
    }
}

/// Between two adjacent points `(a_at, a_cb)` and `(b_at, b_cb)` with
/// `a_at < at <= b_at`: linear, rounded half up. Every table here rises
/// strictly in its control and never falls in its value.
const fn between(a_at: i64, a_cb: Centibels, b_at: i64, b_cb: Centibels, at: i64) -> Centibels {
    let span = b_at - a_at;
    let rise = (b_cb.0 - a_cb.0) as i64;
    let step = (rise * (at - a_at) + span / 2) / span;
    Centibels(a_cb.0 + step as i32)
}

/// A Figure 3 curve at `control_uv`: linear between adjacent points, rounded
/// half up, and the end point's value past either end.
const fn on_curve(points: &[ControlRow], control_uv: Microvolts) -> Centibels {
    let at = control_uv.0 as i64;
    if control_uv.0 <= points[0].uv.0 {
        return points[0].attenuation_cb;
    }
    let mut i = 1;
    while i < points.len() {
        let (a, b) = (points[i - 1], points[i]);
        if control_uv.0 <= b.uv.0 {
            return between(
                a.uv.0 as i64,
                a.attenuation_cb,
                b.uv.0 as i64,
                b.attenuation_cb,
                at,
            );
        }
        i += 1;
    }
    points[points.len() - 1].attenuation_cb
}

/// Attenuation from maximum gain, in centibels, with `control_uv` on pin 2
/// at the curve's supply (`MC3340-010` to `MC3340-013`).
#[must_use]
pub const fn attenuation_cb(curve: Curve, control_uv: Microvolts) -> Centibels {
    on_curve(curve.rows(), control_uv)
}

/// Attenuation from maximum gain, in centibels, with `control_ohms` from pin
/// 2 to ground (`MC3340-015`). Linear in ohms between Figure 4's points,
/// whose axis is logarithmic.
#[must_use]
pub const fn attenuation_at_resistor_cb(control_ohms: Ohms) -> Centibels {
    let points = &RESISTOR_ROWS;
    if control_ohms.0 <= points[0].control_ohms.0 {
        return points[0].attenuation_cb;
    }
    let mut i = 1;
    while i < points.len() {
        let (a, b) = (points[i - 1], points[i]);
        if control_ohms.0 <= b.control_ohms.0 {
            let at = control_ohms.0 as i64;
            return between(
                a.control_ohms.0 as i64,
                a.attenuation_cb,
                b.control_ohms.0 as i64,
                b.attenuation_cb,
                at,
            );
        }
        i += 1;
    }
    points[points.len() - 1].attenuation_cb
}

/// 10^(−r/200) in units of 10^−24, for `r` in 0..200: one centibel apart,
/// fine enough that [`ratio_ppb`] rounds every gain it carries to the
/// nearest part from it. Computed from the exponential and written out, so
/// that no float is evaluated here.
const DECADE_E24: [u128; 200] = [
    1_000_000_000_000_000_000_000_000,
    988_553_094_656_938_840_285_248,
    977_237_220_955_810_682_697_076,
    966_050_878_989_813_374_276_736,
    954_992_586_021_435_949_723_959,
    944_060_876_285_923_380_364_380,
    933_254_300_796_991_043_532_097,
    922_571_427_154_763_160_030_738,
    912_010_839_355_909_742_120_959,
    901_571_137_605_956_885_892_463,
    891_250_938_133_745_529_953_109,
    881_048_873_008_014_087_144_537,
    870_963_589_956_080_637_510_827,
    860_993_752_184_600_649_041_245,
    851_138_038_202_376_467_817_126,
    841_395_141_645_195_091_152_742,
    831_763_771_102_671_006_166_691,
    822_242_649_947_071_141_989_649,
    812_830_516_164_099_246_541_279,
    803_526_122_185_617_258_528_753,
    794_328_234_724_281_502_065_918,
    785_235_634_610_071_785_119_829,
    776_247_116_628_691_733_893_701,
    767_361_489_361_818_943_285_663,
    758_577_575_029_183_768_753_767,
    749_894_209_332_455_827_302_184,
    741_310_241_300_917_514_913_390,
    732_824_533_138_904_084_618_225,
    724_435_960_074_990_063_551_434,
    716_143_410_212_902_018_815_621,
    707_945_784_384_137_910_802_215,
    699_841_996_002_273_498_824_371,
    691_830_970_918_936_487_533_684,
    683_911_647_281_429_323_649_819,
    676_082_975_391_981_771_058_992,
    668_343_917_568_614_608_362_574,
    660_693_448_007_596_006_509_646,
    653_130_552_647_472_355_255_327,
    645_654_229_034_655_515_878_214,
    638_263_486_190_548_683_892_867,
    630_957_344_480_193_249_434_360,
    623_734_835_482_419_244_598_528,
    616_595_001_861_482_166_320_348,
    609_536_897_240_169_160_390_365,
    602_559_586_074_357_746_971_303,
    595_662_143_529_010_460_509_798,
    588_843_655_355_588_967_267_099,
    582_103_217_770_871_411_842_782,
    575_439_937_337_156_930_062_038,
    568_852_930_843_841_449_464_457,
    562_341_325_190_349_080_394_951,
    555_904_257_270_403_580_379_501,
    549_540_873_857_624_552_093_684,
    543_250_331_492_433_210_823_946,
    537_031_796_370_252_730_903_581,
    530_884_444_230_988_352_200_462,
    524_807_460_249_772_597_364_312,
    518_800_038_928_961_118_230_439,
    512_861_383_991_364_855_546_333,
    506_990_708_274_704_360_059_565,
    501_187_233_627_272_285_001_554,
    495_450_190_804_790_220_143_608,
    489_778_819_368_446_195_910_309,
    484_172_367_584_099_342_531_851,
    478_630_092_322_638_343_922_088,
    473_151_258_961_480_478_903_271,
    467_735_141_287_198_193_582_090,
    462_381_021_399_260_298_134_655,
    457_088_189_614_875_029_000_314,
    451_855_944_374_922_363_535_280,
    446_683_592_150_963_118_556_251,
    441_570_447_353_312_506_930_773,
    436_515_832_240_165_967_463_835,
    431_519_076_827_765_222_807_882,
    426_579_518_801_592_658_004_524,
    421_696_503_428_582_248_569_013,
    416_869_383_470_335_401_771_092,
    412_097_519_097_330_207_965_975,
    407_380_277_804_112_730_504_656,
    402_717_034_325_459_092_920_881,
    398_107_170_553_497_250_770_252,
    393_550_075_455_777_462_702_083,
    389_045_144_994_280_601_093_704,
    384_591_782_045_353_566_886_509,
    380_189_396_320_561_196_149_706,
    375_837_404_288_444_167_377_711,
    371_535_229_097_172_538_616_558,
    367_282_300_498_084_662_229_444,
    363_078_054_770_101_342_467_371,
    358_921_934_645_005_217_023_368,
    354_813_389_233_575_458_433_219,
    350_751_873_952_568_004_557_410,
    346_736_850_452_531_639_452_911,
    342_767_786_546_450_356_734_514,
    338_844_156_139_202_547_048_274,
    334_965_439_157_827_659_548_773,
    331_131_121_482_591_094_301_700,
    327_340_694_878_838_188_337_918,
    323_593_656_929_628_262_678_675,
    319_889_510_969_139_801_047_861,
    316_227_766_016_837_933_199_889,
    312_607_936_712_395_496_834_412,
    309_029_543_251_359_051_955_131,
    305_492_111_321_551_320_272_579,
    301_995_172_040_201_619_863_115,
    298_538_261_891_795_961_791_328,
    295_120_922_666_638_570_793_493,
    291_742_701_400_116_686_418_179,
    288_403_150_312_660_594_239_197,
    285_101_826_750_390_928_908_048,
    281_838_293_126_445_381_910_192,
    278_612_116_862_977_036_943_793,
    275_422_870_333_816_644_863_121,
    272_270_130_807_791_238_133_269,
    269_153_480_392_691_571_739_917,
    266_072_505_979_880_963_499_131,
    263_026_799_189_538_191_728_980,
    260_015_956_316_527_192_288_364,
    257_039_578_276_886_380_070_035,
    254_097_270_554_930_502_165_319,
    251_188_643_150_958_011_108_503,
    248_313_310_529_557_026_869_813,
    245_470_891_568_503_035_608_272,
    242_661_009_508_241_551_629_748,
    239_883_291_901_949_046_531_740,
    237_137_370_566_165_526_165_175,
    234_422_881_531_992_211_814_752,
    231_739_464_996_847_856_903_329,
    229_086_765_276_777_304_572_408,
    226_464_430_759_305_964_689_660,
    223_872_113_856_833_961_195_495,
    221_309_470_960_563_772_233_398,
    218_776_162_394_955_256_222_611,
    216_271_852_372_702_026_936_224,
    213_796_208_950_223_209_751_941,
    211_348_903_983_664_679_556_035,
    208_929_613_085_403_948_312_223,
    206_538_015_581_052_937_065_435,
    204_173_794_466_952_933_144_457,
    201_836_636_368_156_098_566_087,
    199_526_231_496_887_960_135_246,
    197_242_273_611_485_375_489_097,
    194_984_459_975_804_532_357_053,
    192_752_491_319_093_600_610_939,
    190_546_071_796_324_718_268_801,
    188_364_908_948_980_053_500_089,
    186_208_713_666_286_744_867_178,
    184_077_200_146_895_581_536_576,
    181_970_085_860_998_344_010_368,
    179_887_091_512_878_784_072_457,
    177_827_941_003_892_280_122_542,
    175_792_361_395_869_260_891_264,
    173_780_082_874_937_546_699_960,
    171_790_838_715_758_812_955_551,
    169_824_365_246_174_435_462_538,
    167_880_401_812_256_031_397_195,
    165_958_690_743_756_063_431_023,
    164_058_977_319_953_927_518_459,
    162_181_009_735_892_997_284_779,
    160_324_539_069_004_149_770_990,
    158_489_319_246_111_348_520_210,
    156_675_107_010_814_910_631_305,
    154_881_661_891_248_134_467_162,
    153_108_746_168_203_014_193_529,
    151_356_124_843_620_816_246_407,
    149_623_565_609_443_341_188_029,
    147_910_838_816_820_742_219_935,
    146_217_717_445_671_818_882_833,
    144_543_977_074_592_751_193_148,
    142_889_395_851_110_285_651_559,
    141_253_754_462_275_430_215_561,
    139_636_836_105_593_760_462_158,
    138_038_426_460_288_483_784_271,
    136_458_313_658_892_452_571_133,
    134_896_288_259_165_360_924_795,
    133_352_143_216_332_402_567_593,
    131_825_673_855_640_710_204_737,
    130_316_677_845_229_938_727_797,
    128_824_955_169_313_396_284_265,
    127_350_308_101_666_168_401_186,
    125_892_541_179_416_721_042_395,
    124_451_461_177_138_508_704_764,
    123_026_877_081_238_153_424_154,
    121_618_600_064_636_799_871_959,
    120_226_443_461_741_290_583_261,
    118_850_222_743_701_843_773_012,
    117_489_755_493_952_954_172_207,
    116_144_861_384_034_274_856_970,
    114_815_362_149_688_275_154_622,
    113_501_081_567_231_506_397_172,
    112_201_845_430_196_343_559_104,
    110_917_481_526_240_106_667_617,
    109_647_819_614_318_501_314_371,
    108_392_691_402_120_352_633_001,
    107_151_930_523_760_641_740_830,
    105_925_372_517_728_887_880_928,
    104_712_854_805_089_953_346_450,
    103_514_216_667_934_381_727_562,
    102_329_299_228_075_413_096_628,
    101_157_945_425_989_852_444_093,
];

/// [`DECADE_E24`] rounded to the nearest part per billion.
const DECADE_PPB: [i64; 200] = {
    let mut out = [0; 200];
    let mut r = 0;
    while r < 200 {
        let unit = 10_u128.pow(15);
        out[r] = ((DECADE_E24[r] + unit / 2) / unit) as i64;
        r += 1;
    }
    out
};

/// The highest gain whose ratio fits [`PartsPerBillion`], in centibels:
/// 199.2 dB.
const RATIO_PPB_TOP_CB: i32 = 1_992;

/// A gain in centibels as a voltage ratio in parts per billion: 10^(g/200),
/// rounded to the nearest part, so zero far enough below unity, and
/// `i64::MAX` above +199.2 dB, where the ratio no longer fits.
#[must_use]
pub const fn ratio_ppb(gain_cb: Centibels) -> PartsPerBillion {
    if gain_cb.0 > RATIO_PPB_TOP_CB {
        return PartsPerBillion(i64::MAX);
    }
    let n = -(gain_cb.0 as i64);
    let decades = n.div_euclid(200);
    if decades >= 10 {
        return PartsPerBillion(0);
    }
    let divisor = 10_u128.pow((15 + decades) as u32);
    let base = DECADE_E24[n.rem_euclid(200) as usize];
    PartsPerBillion(((base + divisor / 2) / divisor) as i64)
}

/// Figure 6 at `supply_uv`, peak to peak, in microvolts: linear between its
/// points, rounded half up, and `None` outside 8 V to 18 V where it draws
/// nothing (`MC3340-017`).
const fn figure_6_pp_uv(supply_uv: Microvolts) -> Option<i32> {
    if supply_uv.0 < SWING_ROWS[0].test_supply_uv.0
        || supply_uv.0 > SWING_ROWS[SWING_ROWS.len() - 1].test_supply_uv.0
    {
        return None;
    }
    let at = supply_uv.0 as i64;
    let mut i = 1;
    while i < SWING_ROWS.len() {
        let (a, b) = (SWING_ROWS[i - 1], SWING_ROWS[i]);
        if at <= b.test_supply_uv.0 as i64 {
            let span = b.test_supply_uv.0 as i64 - a.test_supply_uv.0 as i64;
            let rise = b.pp_uv.0 as i64 - a.pp_uv.0 as i64;
            let step = (rise * (at - a.test_supply_uv.0 as i64) + span / 2) / span;
            return Some(a.pp_uv.0 + step as i32);
        }
        i += 1;
    }
    Some(SWING_ROWS[SWING_ROWS.len() - 1].pp_uv.0)
}

/// Which of the sheet's figures to take: its minimum or its typical
/// (`MC3340-005`, `MC3340-017`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Corner {
    /// The table's minimum: a gain of 11 dB. Figure 6 draws none.
    Min,
    /// The typical part: a gain of 13 dB, and Figure 6's one curve.
    Typ,
}

/// The two voltages pin 7 cannot pass, in microvolts (`MC3340-017`).
pub use units::Swing;

/// Figure 6's swing at `supply_uv`, centred on `output_dc_uv`: its
/// peak to peak linear between the figure's points, rounded half up, and
/// the odd microvolt above the level, saturating at the range of an
/// `i32` (`MC3340-017`). That the swing is centred is the crate's
/// assumption (`MC3340-OPEN-09`).
///
/// The figure is one curve, taken as the typical part's: it draws no
/// minimum.
///
/// # Errors
///
/// [`SwingError::SupplyOutsideCurve`] outside 8 V to 18 V, where it draws
/// nothing.
///
/// # Examples
///
/// ```
/// use mc3340::{SwingError, swing_at_supply};
/// use units::Microvolts;
///
/// let dc = Microvolts(6_000_000);
/// let swing = swing_at_supply(dc, Microvolts(12_000_000))?;
/// let (low, high) = swing.range_uv();
/// assert!(low < dc && dc < high);
///
/// // Figure 6 stops at 18 V.
/// assert_eq!(
///     swing_at_supply(dc, Microvolts(20_000_000)),
///     Err(SwingError::SupplyOutsideCurve)
/// );
/// # Ok::<(), SwingError>(())
/// ```
pub const fn swing_at_supply(
    output_dc_uv: Microvolts,
    supply_uv: Microvolts,
) -> Result<Swing, SwingError> {
    let Some(pp_uv) = figure_6_pp_uv(supply_uv) else {
        return Err(SwingError::SupplyOutsideCurve);
    };
    let below = pp_uv / 2;
    let swing = Swing::declared(
        Microvolts(output_dc_uv.0.saturating_sub(below)),
        Microvolts(output_dc_uv.0.saturating_add(pp_uv - below)),
    );
    Ok(swing.expect("figure 6 draws every swing above zero"))
}

/// Why [`swing_at_supply`] has no swing to give.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum SwingError {
    /// The supply is outside 8 V to 18 V, where Figure 6 draws nothing.
    SupplyOutsideCurve,
}

impl core::fmt::Display for SwingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::SupplyOutsideCurve => "a supply outside figure 6's 8 V to 18 V",
        })
    }
}

impl core::error::Error for SwingError {}

/// Why [`Mc3340::new`] refuses a part.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Mc3340Error {
    /// The declared DC level of pin 7 is outside the swing, so the output
    /// could not stand at it with no signal.
    DcOutsideSwing,
    /// The gain is zero, so no input would reach pin 7.
    ZeroGain,
    /// The gain is below zero, which would invert the signal.
    NegativeGain,
}

impl core::fmt::Display for Mc3340Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::DcOutsideSwing => "a DC level outside the swing",
            Self::ZeroGain => "a gain of zero",
            Self::NegativeGain => "a gain below zero",
        })
    }
}

impl core::error::Error for Mc3340Error {}

/// The two input pins, in microvolts: pin 1's signal, its departure from
/// the DC level it rests at, and pin 2 from ground.
///
/// Pin 1's own DC level is on no page (`MC3340-OPEN-03`) and the transfer
/// does not see it, so the pin is taken as its signal alone.
/// [`Inputs::default`] is the part idle on every curve: no signal on pin 1,
/// so the output stands at its DC level; and pin 2 at 0 V, below every
/// curve's first point, where the attenuation holds at that point's figure.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `INPUT`, pin 1, its departure from its DC level.
    pub input_signal_uv: Microvolts,
    /// `CONTROL`, pin 2.
    pub control_uv: Microvolts,
}

impl Inputs {
    /// `INPUT` at its DC level, `CONTROL` at ground.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            input_signal_uv: Microvolts(0),
            control_uv: Microvolts(0),
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The output pin, in microvolts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct Outputs {
    /// `OUTPUT`, pin 7: its DC level ([`Mc3340::output_dc_uv`]) plus the
    /// signal the part puts on it.
    pub output_uv: Microvolts,
}

/// One part: a curve, a gain, the caller's DC level for pin 7 and a swing
/// (`MC3340-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Mc3340 {
    curve: Curve,
    output_dc_uv: Microvolts,
    swing: Swing,
    gain_ppb: PartsPerBillion,
}

impl Mc3340 {
    /// A part on `curve` whose pin 7 stands at `output_dc_uv` with no signal,
    /// which no page states (`MC3340-OPEN-02`): a caller declares it. The
    /// output is held to `swing` ([`swing_at_supply`] for Figure 6's, or a
    /// caller's own), and its gain with no attenuation is `gain_ppb`, a
    /// voltage ratio in parts per billion (`MC3340-005`).
    ///
    /// # Errors
    ///
    /// [`Mc3340Error::ZeroGain`] or [`Mc3340Error::NegativeGain`] where
    /// `gain_ppb` is zero or below, which no part has, and
    /// [`Mc3340Error::DcOutsideSwing`] where
    /// `output_dc_uv` is below or above the swing; a level at either limit is
    /// inside it. Checked in that order.
    ///
    /// # Examples
    ///
    /// ```
    /// use mc3340::{Curve, Mc3340, Mc3340Error, Swing};
    /// use units::{Microvolts, PartsPerBillion};
    ///
    /// let swing = Swing::declared(Microvolts(1_000_000), Microvolts(11_000_000))
    ///     .ok_or("a swing with no width")?;
    /// let dc = Microvolts(6_000_000);
    /// // A specimen's own gain, four times.
    /// let part = Mc3340::new(Curve::Vcc12V, dc, swing, PartsPerBillion(4_000_000_000))?;
    /// assert_eq!(part.gain_ppb(), PartsPerBillion(4_000_000_000));
    ///
    /// // No part has a gain of zero.
    /// assert_eq!(
    ///     Mc3340::new(Curve::Vcc12V, dc, swing, PartsPerBillion(0)),
    ///     Err(Mc3340Error::ZeroGain)
    /// );
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub const fn new(
        curve: Curve,
        output_dc_uv: Microvolts,
        swing: Swing,
        gain_ppb: PartsPerBillion,
    ) -> Result<Self, Mc3340Error> {
        if gain_ppb.0 == 0 {
            return Err(Mc3340Error::ZeroGain);
        }
        if gain_ppb.0 < 0 {
            return Err(Mc3340Error::NegativeGain);
        }
        let (low, high) = swing.range_uv();
        if output_dc_uv.0 < low.0 || output_dc_uv.0 > high.0 {
            return Err(Mc3340Error::DcOutsideSwing);
        }
        Ok(Self {
            curve,
            output_dc_uv,
            swing,
            gain_ppb,
        })
    }

    /// As [`Mc3340::new`], at one of the sheet's gains ([`GAIN_MIN_TYP_PPB`],
    /// `MC3340-005`).
    ///
    /// # Errors
    ///
    /// As [`Mc3340::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use mc3340::{Corner, Curve, GAIN_MIN_TYP_PPB, Mc3340, swing_at_supply};
    /// use units::Microvolts;
    ///
    /// let dc = Microvolts(6_000_000);
    /// let swing = swing_at_supply(dc, Curve::Vcc16V.test_supply_uv())?;
    /// let part = Mc3340::at_corner(Curve::Vcc16V, dc, swing, Corner::Min)?;
    /// assert_eq!(part.gain_ppb(), GAIN_MIN_TYP_PPB.0);
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub const fn at_corner(
        curve: Curve,
        output_dc_uv: Microvolts,
        swing: Swing,
        corner: Corner,
    ) -> Result<Self, Mc3340Error> {
        let gain_ppb = match corner {
            Corner::Min => GAIN_MIN_TYP_PPB.0,
            Corner::Typ => GAIN_MIN_TYP_PPB.1,
        };
        Self::new(curve, output_dc_uv, swing, gain_ppb)
    }

    /// The curve.
    #[must_use]
    pub const fn curve(self) -> Curve {
        self.curve
    }

    /// The declared DC level of pin 7 with no signal, in microvolts.
    #[must_use]
    pub const fn output_dc_uv(self) -> Microvolts {
        self.output_dc_uv
    }

    /// The swing.
    #[must_use]
    pub const fn swing(self) -> Swing {
        self.swing
    }

    /// The voltage gain with no attenuation, in parts per billion; the gain
    /// at a control is this times [`ratio_ppb`] of less [`attenuation_cb`]
    /// there (`MC3340-009`).
    #[must_use]
    pub const fn gain_ppb(self) -> PartsPerBillion {
        self.gain_ppb
    }

    /// Pin 7 with pins 1 and 2 at these levels in the same instant: the
    /// output's DC level plus the gain, less the attenuation, times the
    /// input's signal, rounded half away from zero to the microvolt once,
    /// at the end, and held to the swing (`MC3340-005`, `MC3340-009`,
    /// `MC3340-017`).
    #[must_use]
    pub const fn drive(self, inputs: Inputs) -> Outputs {
        let n = attenuation_cb(self.curve, inputs.control_uv).0 as i64;
        let decades = n.div_euclid(200);
        let fraction = DECADE_PPB[n.rem_euclid(200) as usize];
        let product = inputs.input_signal_uv.0 as i128 * self.gain_ppb.0 as i128 * fraction as i128;
        let divisor = 1_000_000_000_000_000_000_i128 * 10_i128.pow(decades as u32);
        let half = divisor / 2;
        let moved = if product >= 0 {
            (product + half) / divisor
        } else {
            (product - half) / divisor
        };
        let (low, high) = self.swing.range_uv();
        let out = self.output_dc_uv.0 as i128 + moved;
        let output_uv = if out <= low.0 as i128 {
            low
        } else if out >= high.0 as i128 {
            high
        } else {
            Microvolts(out as i32)
        };
        Outputs { output_uv }
    }
}

/// The part sampled with one sample of latency: a consumer reads pin 1
/// ([`INPUT_PIN`]), less its DC level, and pin 2 ([`CONTROL_PIN`]) from its probes, calls
/// [`Stepped::sample`], and writes what it returns onto pin 7
/// ([`OUTPUT_PIN`]) — the output the previous sample's pins made.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Stepped {
    part: Mc3340,
    next_sample_uv: Microvolts,
}

impl Stepped {
    /// A part whose output starts at its declared DC level.
    #[must_use]
    pub const fn new(part: Mc3340) -> Self {
        Self {
            part,
            next_sample_uv: part.output_dc_uv,
        }
    }

    /// The part.
    #[must_use]
    pub const fn part(&self) -> Mc3340 {
        self.part
    }

    /// What the next [`Stepped::sample`] will return, in microvolts.
    #[must_use]
    pub const fn next_sample_uv(&self) -> Microvolts {
        self.next_sample_uv
    }

    /// Takes this sample's pin 1 and pin 2 and returns pin 7 for this
    /// sample, which is what the previous sample's pins made.
    #[must_use]
    pub const fn sample(&mut self, inputs: Inputs) -> Outputs {
        let out = self.next_sample_uv;
        self.next_sample_uv = self.part.drive(inputs).output_uv;
        Outputs { output_uv: out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_pins_hold_the_output_at_its_dc_level_on_every_curve() {
        for curve in Curve::ALL {
            for dc in [0, 6_000_000].map(Microvolts) {
                let swing = swing_at_supply(dc, curve.test_supply_uv())
                    .expect("every curve's supply is inside Figure 6");
                let part = Mc3340::at_corner(curve, dc, swing, Corner::Typ)
                    .expect("Figure 6's swing is centred on the DC level");
                assert_eq!(part.drive(Inputs::default()).output_uv, dc);
                for control_uv in (0..7_000_000).step_by(500_000).map(Microvolts) {
                    let pins = Inputs {
                        control_uv,
                        ..Default::default()
                    };
                    assert_eq!(part.drive(pins).output_uv, dc);
                }
            }
        }
    }

    #[test]
    fn a_gain_of_zero_or_below_is_refused_before_the_dc_level() {
        let swing = Swing::declared(Microvolts(1_000), Microvolts(2_000)).expect("ordered");
        let new = |gain| Mc3340::new(Curve::Vcc12V, Microvolts(0), swing, PartsPerBillion(gain));
        assert_eq!(new(0), Err(Mc3340Error::ZeroGain));
        assert_eq!(new(-1), Err(Mc3340Error::NegativeGain));
        assert_eq!(new(1), Err(Mc3340Error::DcOutsideSwing));
    }

    #[test]
    fn a_dc_level_outside_the_swing_is_refused() {
        let swing = Swing::declared(Microvolts(1_000), Microvolts(2_000)).expect("ordered");
        let new = |dc| Mc3340::new(Curve::Vcc12V, Microvolts(dc), swing, GAIN_MIN_TYP_PPB.1);
        assert_eq!(new(999), Err(Mc3340Error::DcOutsideSwing));
        assert_eq!(new(2_001), Err(Mc3340Error::DcOutsideSwing));
        for dc in [1_000, 1_500, 2_000] {
            let part = new(dc).expect("inside the swing");
            assert_eq!(part.drive(Inputs::default()).output_uv, Microvolts(dc));
            assert_eq!(
                Stepped::new(part).sample(Inputs::default()).output_uv,
                Microvolts(dc)
            );
        }
    }

    #[test]
    fn the_ratio_fits_to_its_top_and_saturates_past_it() {
        assert_eq!(
            ratio_ppb(Centibels(RATIO_PPB_TOP_CB)),
            PartsPerBillion(9_120_108_393_559_097_421)
        );
        assert_eq!(ratio_ppb(Centibels(130)), PartsPerBillion(4_466_835_922));
        assert_eq!(
            ratio_ppb(Centibels(RATIO_PPB_TOP_CB + 1)),
            PartsPerBillion(i64::MAX)
        );
        assert_eq!(ratio_ppb(Centibels(i32::MAX)), PartsPerBillion(i64::MAX));
        assert_eq!(
            ratio_ppb(Centibels(1_800)),
            PartsPerBillion(1_000_000_000_000_000_000)
        );
        assert_eq!(ratio_ppb(Centibels(i32::MIN)), PartsPerBillion(0));
    }

    #[test]
    fn the_sheets_largest_gain_and_widest_signal_do_not_overflow() {
        let wide = Swing::declared(Microvolts(i32::MIN), Microvolts(i32::MAX)).unwrap();
        let part = Mc3340::new(
            Curve::Vcc16V,
            Microvolts(0),
            wide,
            PartsPerBillion(i64::MAX),
        )
        .expect("inside the swing");
        for signal in [i32::MAX, i32::MIN] {
            let pins = Inputs {
                input_signal_uv: Microvolts(signal),
                control_uv: Microvolts(0),
            };
            let out = part.drive(pins).output_uv;
            assert_eq!(out, Microvolts(signal));
        }
    }

    #[test]
    fn figure_6_is_refused_off_its_supplies_and_a_swing_of_no_width_is_refused() {
        let supply = Curve::Vcc12V.test_supply_uv();
        assert!(swing_at_supply(Microvolts(0), supply).is_ok());
        for off in [7_999_999, 18_000_001].map(Microvolts) {
            assert_eq!(
                swing_at_supply(Microvolts(0), off),
                Err(SwingError::SupplyOutsideCurve)
            );
        }
        for dc in [i32::MIN, i32::MAX].map(Microvolts) {
            let (low, high) = swing_at_supply(dc, supply)
                .expect("a saturated limit still leaves a width")
                .range_uv();
            assert!(low < high);
        }
        assert_eq!(Swing::declared(Microvolts(5), Microvolts(5)), None);
    }
}
