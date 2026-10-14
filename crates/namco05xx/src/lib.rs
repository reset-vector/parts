//! The Namco `05xx` star generator, at its pins.
//!
//! The part's star field is described by its *statistics*. **The mechanism
//! that produces them here is the crate's definition**, and every choice
//! below names the open item it stands under.
//!
//! **What the crate chooses**, each an open item: the shift register's width,
//! taps and seed (`NAMCO05XX-OPEN-01`); how dense a candidate is
//! (`NAMCO05XX-OPEN-02`); how the sets are chosen (`NAMCO05XX-OPEN-03`);
//! which line carries which colour bit (`NAMCO05XX-OPEN-04`); `Q1` and `Q2`
//! ignored and the sequence running on beneath `Q5` (`NAMCO05XX-OPEN-06`); the
//! window taken at construction (`NAMCO05XX-OPEN-07`); the register reloaded
//! every frame (`NAMCO05XX-OPEN-08`); the part gating itself on pin 5
//! (`NAMCO05XX-OPEN-10`); and the field standing still while `Q0` is Low
//! (`NAMCO05XX-OPEN-11`).
//!
//! # Examples
//!
//! ```
//! use namco05xx::{Inputs, Level, Namco05xx, Window, q};
//!
//! let mut part = Namco05xx::new(Window {
//!     pixel_range: (16, 271),
//!     line_range: (0, 223),
//! });
//! // One frame of 232 lines of 288 pixels, every pixel transparent.
//! let mut lit = 0;
//! for line in 0..232 {
//!     for pixel in 0..288 {
//!         let out = part.tick(Inputs {
//!             hsync_n: Level::from_bool(pixel >= 8),
//!             vsync_n: Level::from_bool(line > 0 || pixel >= 8),
//!             q: q::Q5_MASK,
//!             transparent: Level::High,
//!         });
//!         if out.colour.is_some() {
//!             lit += 1;
//!         }
//!     }
//! }
//! assert!((100..=120).contains(&lit));
//! ```
//!
//! # The shape
//!
//! One clock reaches the crate, `6MHZ` at pin 1, and everything happens on it:
//!
//! - [`Namco05xx::tick`] is one pixel period. Its [`Inputs`] carry the two
//!   syncs, the six control lines the `LS259` holds, and pin 5's statement
//!   that the video pixel about to be drawn is transparent. It returns what
//!   the part drives for that pixel.
//! - [`Outputs`] is pins 6–11 and pin 20: a colour, and the palette PROM
//!   held off while that colour is on its bus (`NAMCO05XX-007`,
//!   `NAMCO05XX-SIB-04`).
//! - [`Namco05xx::new`] is the only state the part starts from here.
//!   **No reset pin is drawn** (`NAMCO05XX-SIB-01`, `NAMCO05XX-OPEN-09`), so
//!   the crate offers none.
//!
//! # What is not here
//!
//! Pin 3, which [`Inputs`] does not carry — what the board's `4H·2H·1H` NAND
//! does inside the box is `NAMCO05XX-OPEN-05`. Any figure in nanoseconds.

#![no_std]

pub use signal::Level;

/// The bits of [`Inputs::q`], the six lines the `LS259` at `5K` holds.
pub mod q {
    /// `Q0`, pin 13 — High while the field moves (`NAMCO05XX-004`).
    pub const Q0_MASK: u8 = 1 << 0;

    /// `Q3` and `Q4`, pins 22 and 23 — the two-bit code that picks the set
    /// (`NAMCO05XX-006`).
    pub const Q3_Q4_MASK: u8 = (1 << 3) | (1 << 4);

    /// `Q5`, pin 19 — no star is lit while it is Low (`NAMCO05XX-008`). The
    /// sequence runs on beneath the blank, which is the crate's choice
    /// (`NAMCO05XX-OPEN-06`).
    pub const Q5_MASK: u8 = 1 << 5;
}

/// How many colours the six lines reach (`NAMCO05XX-003`).
pub const COLOUR_COUNT: usize = 64;

/// How many sets the two lines at `Q3`/`Q4` choose between.
pub const SELECT_COUNT: usize = 4;

/// The levels at the part's inputs for one `6MHZ` period.
///
/// [`Inputs::default`] holds both syncs High, `Q5` High and every other `Q`
/// line Low (the field standing, the part free to light a star), and the pixel
/// not transparent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `HSYNC`, pin 2, active Low: a line begins at its falling edge.
    pub hsync_n: Level,
    /// `VSYNC`, pin 4, active Low: a frame begins at its falling edge.
    pub vsync_n: Level,
    /// The six lines the `LS259` at `5K` holds, as the board wires them: bit
    /// *n* is `Qn`, which is pin 13, 14, 15, 22, 23, 19 for *n* = 0–5
    /// (`NAMCO05XX-SIB-02`), a set bit High. No star is lit while `Q5` is Low
    /// (`NAMCO05XX-008`). `Q1` and `Q2` (bits 1 and 2) are accepted and
    /// ignored: nothing the part drives depends on them, and what they do is
    /// `NAMCO05XX-OPEN-06`.
    pub q: u8,
    /// Pin 5: High when the pixel the video side is about to draw is the
    /// transparent one, which is the only pixel a star may replace
    /// (`NAMCO05XX-007`, `NAMCO05XX-SIB-03`).
    pub transparent: Level,
}

impl Inputs {
    /// The part idle; see [`Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            hsync_n: Level::High,
            vsync_n: Level::High,
            q: q::Q5_MASK,
            transparent: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The levels this part drives for one pixel.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct Outputs {
    /// Pins 6–11 while a star is lit, the six lines as a value 0–63: bit *n*
    /// is pin 11 − *n* (`NAMCO05XX-003`, `NAMCO05XX-OPEN-04`). `None` while
    /// they are released: they sit on the palette PROM's output lines
    /// (`NAMCO05XX-SIB-04`), which drive that bus when no star is lit.
    pub colour: Option<u8>,
}

impl Outputs {
    /// Whether pin 20 holds the palette PROM's chip select off, so that a lit
    /// star's colour is on its bus (`NAMCO05XX-007`, `NAMCO05XX-SIB-04`): the
    /// condition, which holds exactly while [`Outputs::colour`] is `Some`, and
    /// not the pin's level.
    #[must_use]
    pub const fn is_palette_off(self) -> bool {
        self.colour.is_some()
    }
}

/// Where the field is allowed to be, in pixels and lines counted from the
/// falling edges of the two syncs.
///
/// Whether the part counts the raster itself or is gated from the board is
/// `NAMCO05XX-OPEN-07`, so the window is taken at construction, as `mos6581`
/// takes its board's capacitance. The host is what aligns it. Pixel 0 is the
/// period in which `HSYNC` is first seen low, and line 0 begins at the period
/// in which `VSYNC` is first seen low, which also restarts the pixel count: a
/// `VSYNC` that falls mid-line makes the rest of that line line 0, so a host
/// that wants whole lines lets it fall with `HSYNC`.
///
/// The crate counts pixels and lines in sixteen bits, wrapping, so a bound
/// past 65 535 is never reached. A count wraps to 0 when its sync stays away
/// for 65 536 periods (pixels) or lines, and the window then opens again: a
/// host that stops `HSYNC` sees the field return every 65 536 periods.
///
/// On Galaga the window is pixels 16–271 of the 288, over the 224 visible
/// lines (`NAMCO05XX-002`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Window {
    /// The first and the last pixel of a line the field may reach, both
    /// inclusive.
    pub pixel_range: (usize, usize),
    /// The first and the last line, both inclusive.
    pub line_range: (usize, usize),
}

impl Window {
    /// How many pixels the field covers in a frame; none when a range's last
    /// lies before its first. Saturates at `usize::MAX`.
    #[must_use]
    pub const fn pixel_count(self) -> usize {
        span(self.pixel_range).saturating_mul(span(self.line_range))
    }
}

/// How many of `first..=last` there are.
const fn span((first, last): (usize, usize)) -> usize {
    if last < first {
        0
    } else {
        (last - first).saturating_add(1)
    }
}

/// The shift register's taps, and the value it is loaded with at the head of a
/// frame — **the crate's definition**, `NAMCO05XX-OPEN-01`. Sixteen bits with
/// feedback from 16, 14, 13 and 11 is a maximal pair, so the register walks
/// every non-zero value once in 65,535 steps, and that is what puts the
/// pattern's repeat beyond any period a frame counter reaches
/// (`NAMCO05XX-005`).
const SEED: u16 = 0xACE1;

/// The value the low eight bits must hold for a pixel to be a candidate —
/// the crate's definition, `NAMCO05XX-OPEN-02`. One value in 256 candidates
/// is halved again by the set, and the count lies inside the band of
/// `NAMCO05XX-001`.
const LIT: u8 = 0x2D;

/// One step of the register.
const fn step(r: u16) -> u16 {
    let fb = (r ^ (r >> 2) ^ (r >> 3) ^ (r >> 5)) & 1;
    (r >> 1) | (fb << 15)
}

/// One step backwards, which is what moving the field by a pixel costs
/// (`NAMCO05XX-004`): every pixel then shows what the pixel before it showed.
const fn step_back(r: u16) -> u16 {
    let lost = ((r >> 15) ^ (r >> 1) ^ (r >> 2) ^ (r >> 4)) & 1;
    (r << 1) | lost
}

/// The set a candidate belongs to, and the two sets the code at `Q3`/`Q4`
/// shows — the crate's definition, `NAMCO05XX-OPEN-03`.
///
/// The code is read as Gray, so its four values sit in a cycle; a candidate's
/// own two bits name a position in that cycle, and the field shows the code's
/// position and the one after it. A step of the code therefore retires one
/// position and admits one: the half of the field that stands and the half
/// that is exchanged (`NAMCO05XX-006`).
const fn position_of(gray: u8) -> u8 {
    let high = (gray >> 1) & 1;
    (high << 1) | (high ^ (gray & 1))
}

/// The Namco `05xx` at its pins.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Namco05xx {
    window: Window,
    /// The register at the head of the frame; one step back a frame while the
    /// field is moving.
    head: u16,
    /// The register at the pixel about to be drawn.
    reg: u16,
    pixel: u16,
    line: u16,
    hsync_n: Level,
    vsync_n: Level,
    /// What the last period drove.
    outputs: Outputs,
}

impl Namco05xx {
    /// A part at power-on, driving the window given.
    #[must_use]
    pub const fn new(window: Window) -> Self {
        Self {
            window,
            head: SEED,
            reg: SEED,
            pixel: 0,
            line: 0,
            hsync_n: Level::High,
            vsync_n: Level::High,
            outputs: Outputs { colour: None },
        }
    }

    /// The window this part was built with.
    #[must_use]
    pub const fn window(&self) -> Window {
        self.window
    }

    /// One `6MHZ` period at pin 1, with `inputs` the levels at the part's
    /// other inputs during it.
    ///
    /// Pin 3 is not in [`Inputs`]: what it does inside the box is
    /// `NAMCO05XX-OPEN-05`, and a pin this crate would ignore is better absent
    /// than accepted and dropped.
    ///
    /// What comes back [`Namco05xx::outputs`] reads again.
    pub fn tick(&mut self, inputs: Inputs) -> Outputs {
        self.outputs = self.period(inputs);
        self.outputs
    }

    /// What the part drives as the last `6MHZ` period left it; before the
    /// first, no star.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        self.outputs
    }

    fn period(&mut self, inputs: Inputs) -> Outputs {
        let Inputs {
            hsync_n,
            vsync_n,
            q,
            transparent,
        } = inputs;
        let frame = self.vsync_n.is_high() && vsync_n.is_low();
        let line = self.hsync_n.is_high() && hsync_n.is_low();
        self.hsync_n = hsync_n;
        self.vsync_n = vsync_n;

        if frame {
            self.line = 0;
            if q & q::Q0_MASK != 0 {
                self.head = step_back(self.head);
            }
            self.reg = self.head;
            self.pixel = 0;
        } else if line {
            self.line = self.line.wrapping_add(1);
            self.pixel = 0;
        } else {
            self.pixel = self.pixel.wrapping_add(1);
        }

        let (pixel, line) = (usize::from(self.pixel), usize::from(self.line));
        let inside = pixel >= self.window.pixel_range.0
            && pixel <= self.window.pixel_range.1
            && line >= self.window.line_range.0
            && line <= self.window.line_range.1;
        if !inside {
            return Outputs::default();
        }

        let r = self.reg;
        self.reg = step(r);

        let candidate = (r & 0xFF) as u8 == LIT;
        let shown = {
            let seat = ((r >> 8) & 3) as u8;
            let here = position_of((q & q::Q3_Q4_MASK) >> 3);
            seat == here || seat == (here + 1) & 3
        };
        if candidate && shown && transparent.is_high() && q & q::Q5_MASK != 0 {
            Outputs {
                colour: Some(((r >> 10) & 0x3F) as u8),
            }
        } else {
            Outputs::default()
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// The window on Galaga (`NAMCO05XX-002`).
    const GALAGA: Window = Window {
        pixel_range: (16, 271),
        line_range: (0, 223),
    };

    #[test]
    fn the_register_walks_every_non_zero_value() {
        let mut r = SEED;
        for _ in 0..65_534 {
            r = step(r);
            assert_ne!(r, SEED, "the register returned early");
        }
        assert_eq!(step(r), SEED, "the register's period is not 65,535");
    }

    #[test]
    fn each_q_mask_is_named_for_the_pins_it_selects() {
        assert_eq!(q::Q0_MASK, 1 << 0);
        assert_eq!(q::Q3_Q4_MASK, (1 << 3) | (1 << 4));
        assert_eq!(q::Q5_MASK, 1 << 5);
    }

    #[test]
    fn a_step_back_undoes_a_step() {
        let mut r = SEED;
        for _ in 0..1000 {
            assert_eq!(step_back(step(r)), r);
            r = step(r);
        }
    }

    #[test]
    fn the_gray_code_walks_the_cycle_one_position_at_a_time() {
        assert_eq!(position_of(0b00), 0);
        assert_eq!(position_of(0b01), 1);
        assert_eq!(position_of(0b11), 2);
        assert_eq!(position_of(0b10), 3);
    }

    /// The count's bound is a property of the register's whole cycle, and this
    /// is where that is established: every phase the head can ever take,
    /// against the band of `NAMCO05XX-001`.
    #[test]
    fn every_phase_of_the_register_lands_inside_the_band() {
        const PERIOD: usize = 65_535;
        let window = GALAGA.pixel_count();
        for code in [0b00u8, 0b01, 0b11, 0b10] {
            let here = position_of(code);
            let mut shown = Vec::with_capacity(PERIOD);
            let mut r = SEED;
            for _ in 0..PERIOD {
                let seat = ((r >> 8) & 3) as u8;
                shown.push((r & 0xFF) as u8 == LIT && (seat == here || seat == (here + 1) & 3));
                r = step(r);
            }
            assert_eq!(r, SEED, "the sequence did not close");

            let mut count: u32 = shown[..window].iter().filter(|s| **s).count() as u32;
            let (mut low, mut high) = (count, count);
            for phase in 1..PERIOD {
                if shown[phase - 1] {
                    count -= 1;
                }
                if shown[(phase + window - 1) % PERIOD] {
                    count += 1;
                }
                low = low.min(count);
                high = high.max(count);
            }
            assert!(
                (100..=120).contains(&low) && (100..=120).contains(&high),
                "code {code:02b} lights {low}-{high} stars across the cycle, outside 100-120"
            );
        }
    }

    /// `NAMCO05XX-OPEN-11`: the field standing still is this crate's choice,
    /// guarded here.
    #[test]
    fn the_field_stands_while_q0_is_low() {
        let mut part = Namco05xx::new(GALAGA);
        let mut frame = |q| {
            let mut lit = Vec::new();
            for line in 0..232u16 {
                for pixel in 0..288u16 {
                    let out = part.tick(Inputs {
                        hsync_n: Level::from_bool(pixel >= 8),
                        vsync_n: Level::from_bool(line > 0 || pixel >= 8),
                        q,
                        transparent: Level::High,
                    });
                    if let Some(colour) = out.colour {
                        lit.push((line, pixel, colour));
                    }
                }
            }
            lit
        };
        let first = frame(q::Q5_MASK);
        assert!(!first.is_empty());
        for _ in 0..8 {
            assert_eq!(first, frame(q::Q5_MASK), "the field moved with Q0 Low");
        }
    }

    #[test]
    fn a_window_with_a_bound_before_its_first_covers_nothing() {
        assert_eq!(GALAGA.pixel_count(), 256 * 224);
        let backward = Window {
            pixel_range: (16, 15),
            ..GALAGA
        };
        assert_eq!(backward.pixel_count(), 0);
        let whole = Window {
            pixel_range: (0, 65_535),
            line_range: (0, 0),
        };
        assert_eq!(whole.pixel_count(), 65_536);
    }

    /// `outputs()` reads back what each period drove, a lit star among them.
    #[test]
    fn outputs_reads_back_the_last_period() {
        let mut part = Namco05xx::new(GALAGA);
        assert_eq!(part.outputs(), Outputs::default());
        let mut lit = false;
        for i in 0..400_000u32 {
            let inputs = Inputs {
                hsync_n: Level::from_bool(i % 384 != 0),
                vsync_n: Level::from_bool(i % (384 * 264) != 0),
                transparent: Level::High,
                ..Inputs::default()
            };
            let out = part.tick(inputs);
            assert_eq!(part.outputs(), out);
            lit |= out.colour.is_some();
        }
        assert!(lit, "a star was lit");
    }
}
