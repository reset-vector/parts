//! The MOS 6567 Video Interface Chip, emulated at its pins.
//!
//! The crate depends on `signal` alone, allocates nothing, does no I/O and is
//! `no_std`: a [`Mos6567`] is a plain value that advances one `Ø0` cycle per
//! pair of calls and shows the outside world its pins.
//!
//! # Examples
//!
//! ```
//! use mos6567::{Frame, Level, Mos6567, Phase1Inputs, Phase1Purpose, Phase2Inputs, Select, reg};
//!
//! let mut vic = Mos6567::new(Frame::R8);
//! let mut memory = [0u16; 0x4000];
//! memory[0x3fff] = 0xff;
//! // Switch the display on: DEN, RSEL, YSCROLL 3 (MOS6567-018).
//! let mut pending = None;
//! let mut written = false;
//! for _ in 0..(65 * 263) {
//!     let out1 = vic.phase1(Phase1Inputs { lp_n: Level::High, access_d: pending.unwrap_or(0) });
//!     let answer = memory[usize::from(out1.access.a & 0x3fff)];
//!     let access = if written {
//!         Phase2Inputs::default()
//!     } else {
//!         Phase2Inputs::write_access(Select::new(reg::CR1), 0x1b)
//!     };
//!     written = true;
//!     let out2 = vic.phase2(Phase2Inputs { access_d: answer, ..access });
//!     pending = out2.access.map(|f| memory[usize::from(f.a & 0x3fff)]);
//!     if out1.access.purpose == Phase1Purpose::Idle {
//!         assert_eq!(out1.access.a, 0x3fff, "MOS6567-049");
//!     }
//! }
//! // A frame of cycles later the part stands at cycle 1 of the last line;
//! // the next `phase1` wraps the raster to 0 (MOS6567-053).
//! assert_eq!((vic.raster(), vic.line_cycle()), (262, 1));
//! // The register reads back off the bus too, the raster's bit 8 in bit 7.
//! assert_eq!(vic.read(Select::new(reg::CR1)), 0x80 | 0x1b);
//! ```
//!
//! # The part, not the machine
//!
//! Fourteen address lines, twelve data lines, six register-select lines and
//! the control pins, and nothing about what answers an address
//! (`MOS6567-023`): the character ROM, the colour RAM, the two bank bits and
//! the modulator are a board's. `BA` and `AEC` leave this part as levels; that
//! a Commodore 64 puts a gate between `BA` and `RDY` is the board's.
//!
//! # Two calls per cycle
//!
//! The part is a bus master in the first half of every cycle and sometimes in
//! the second, so [`Mos6567::phase1`] announces `BA`, `AEC` for the second
//! half and the first-half fetch, and [`Mos6567::phase2`] takes that fetch's
//! answer and the processor's register access, and returns the second-half
//! fetch, the read byte, `IRQ` and the cycle's eight dots at `SYNC/LUM` and
//! `COLOR`. **Each fetch is
//! answered in the call after it was made**, so a board holds one word
//! between calls, as `mos6510` holds one byte between ticks.
//!
//! # What this crate chooses
//!
//! Each choice under an open item is marked where it is written. The two a
//! consumer most needs to know: the lines per frame is a [`Frame`] the
//! consumer chooses (`MOS6567-OPEN-02`); and the eight dots of a cycle are
//! generated before that cycle's register write is applied, and an access
//! presented while the part holds `AEC` low is ignored (`MOS6567-OPEN-10`).
//! The others:
//!
//! - Every edge and window sits at the sprite coordinate four dots below the
//!   decode count that makes it (`MOS6567-OPEN-03`).
//! - The sprite rules tied to sprite 0's pointer cycle move by two cycles,
//!   those tied to the line start do not, and a sprite's display ends at its
//!   next pointer cycle, so its twenty-first row is shown (`MOS6567-OPEN-04`).
//! - The X coordinate repeats `$18C`–`$193` over cycles 63 and 64
//!   (`MOS6567-OPEN-05`).
//! - The part starts with every register zero, `DEN` included
//!   (`MOS6567-OPEN-06`).
//! - A write to a collision register is ignored (`MOS6567-OPEN-07`).
//! - A dot is sync in the `HSYNC` window of every line, in the `HEQ1` and
//!   `HEQ2` windows of the `VEQ` lines and across the `VSYNC` lines outside
//!   those windows; blank in `HBLANK` and on the `VBLANK` lines; burst in the
//!   `BURST` window of lines not blanked (`MOS6567-OPEN-08`).
//! - Each dot is a four-bit colour code, which a consumer maps to levels
//!   (`MOS6567-OPEN-09`).
//! - The mode bits are read at every dot as they stand, so a change lands at
//!   the next cycle boundary (`MOS6567-OPEN-11`).
//! - `LP` is sampled once per cycle, in [`Mos6567::phase1`]; the latch takes
//!   the X coordinate of the cycle's end, and the frame's trigger is released
//!   at raster 0 (`MOS6567-OPEN-12`).
//! - A second phase the part wants before `AEC` is low fetches nothing
//!   (`MOS6567-OPEN-13`).
//! - A fetched byte reaches the pin from the fifth dot of its fetch cycle
//!   plus `XSCROLL`, and the border switches at the comparison values with no
//!   further delay (`MOS6567-OPEN-14`).
//! - The raster compare is made at the start of cycle 1 on every line, line 0
//!   included (`MOS6567-OPEN-15`).

#![no_std]

#[cfg(test)]
extern crate std;

use core::num::NonZeroUsize;

mod decode;
mod pins;
mod schedule;
mod sequencer;
mod sprites;

pub use decode::VBLANK_RANGE_RASTERS;
pub use pins::{
    Colour, Ctrl, Phase1Access, Phase1Inputs, Phase1Outputs, Phase1Purpose, Phase2Access,
    Phase2Inputs, Phase2Outputs, Phase2Purpose, Select, SyncLum,
};
pub use schedule::{BAD_LINE_BA_RANGE_CYCLES, CYCLES_PER_LINE, DOTS_PER_CYCLE, DOTS_PER_LINE};
pub use sprites::Sprite;

use sequencer::{Mode, Pixel};
use sprites::SpriteLogic;

use schedule::LAST_CYCLE;

/// The raster lines per frame, the consumer's choice (`MOS6567-OPEN-02`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Frame {
    /// 262 lines: the sheet's `VRESET` decode at 261 (`MOS6567-045`).
    Sheet,
    /// 263 lines: what Bauer reports for the 6567R8 (`MOS6567-048`,
    /// `MOS6567-SIB-05`).
    R8,
}

impl Frame {
    /// The number of raster lines.
    #[must_use]
    pub const fn line_count(self) -> NonZeroUsize {
        let n = match self {
            Self::Sheet => 262,
            Self::R8 => 263,
        };
        NonZeroUsize::new(n).expect("a frame has lines")
    }
}

/// The border unit's two flip-flops as [`Mos6567::border`] shows them
/// (`MOS6567-058`). Not pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[non_exhaustive]
pub struct Border {
    /// The main border flip-flop.
    pub main: bool,
    /// The vertical border flip-flop.
    pub vertical: bool,
}

/// Register addresses, as [`Phase2Inputs::a`] and [`Select::new`] select
/// them.
pub mod reg {
    /// Register `$11`, control register 1 (`MOS6567-018`).
    pub const CR1: u8 = 0x11;
    /// Register `$16`, control register 2 (`MOS6567-018`).
    pub const CR2: u8 = 0x16;
    /// Register `$18`, the memory pointers (`MOS6567-018`).
    pub const MEMPTR: u8 = 0x18;
    /// Register `$19`, the interrupt register (`MOS6567-020`).
    pub const IRR: u8 = 0x19;
    /// Register `$1A`, the interrupt enables (`MOS6567-020`).
    pub const IER: u8 = 0x1a;
}

/// A pin's level, as a getter named for the pin returns it (`irq_n()`).
pub use signal::Level;

/// The interrupt register's bits (`$19`), which the interrupt enable
/// register (`$1A`) masks bit for bit (`MOS6567-020`).
pub mod interrupt {
    /// `IRST`, interrupt bit 0 (`MOS6567-020`).
    pub const IRST: u8 = 0x01;
    /// `IMBC`, interrupt bit 1 (`MOS6567-020`).
    pub const IMBC: u8 = 0x02;
    /// `IMMC`, interrupt bit 2 (`MOS6567-020`).
    pub const IMMC: u8 = 0x04;
    /// `ILP`, interrupt bit 3 (`MOS6567-020`).
    pub const ILP: u8 = 0x08;
}

use interrupt::{ILP, IMBC, IMMC, IRST};
use reg::{CR1, CR2, IER, MEMPTR};

/// `RC8`, bit 7 of `$11`.
const RC8: u8 = 0x80;
/// `ECM`, bit 6 of `$11`.
const ECM: u8 = 0x40;
/// `BMM`, bit 5 of `$11`.
const BMM: u8 = 0x20;
/// `DEN`, bit 4 of `$11` (`MOS6567-018`).
const DEN: u8 = 0x10;
/// `RSEL`, bit 3 of `$11`.
const RSEL: u8 = 0x08;
/// `MCM`, bit 4 of `$16`.
const MCM: u8 = 0x10;
/// `CSEL`, bit 3 of `$16`.
const CSEL: u8 = 0x08;

/// The four interrupt latch bits.
const SOURCES: u8 = IRST | IMBC | IMMC | ILP;

/// The whole part.
///
/// [`Mos6567::new`] is the part with every register zero, this crate's
/// choice under `MOS6567-OPEN-06`; the part has no reset pin.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Mos6567 {
    /// The forty-seven registers as written (`MOS6567-017`). The raster and
    /// light-pen entries are not read back from here.
    regs: [u8; 47],
    frame: Frame,

    /// The cycle within the line, `1..=65`, of the call in flight.
    cycle: u8,
    /// The raster line, `0..lines`.
    raster: u16,
    /// The first `phase1` has run, so cycle 1 increments the raster.
    started: bool,
    /// The nine-bit raster compare value (`MOS6567-019`).
    raster_compare: u16,
    /// The four interrupt latches (`MOS6567-020`).
    latch: u8,

    /// `VC`, `VCBASE`, `RC` and `VMLI` (`MOS6567-052`).
    vc: u16,
    vcbase: u16,
    rc: u8,
    vmli: u8,
    /// Display state, against idle (`MOS6567-052`).
    display: bool,
    /// The bad line condition of the cycle in flight (`MOS6567-051`).
    bad_line: bool,
    /// `DEN` was set in some cycle of raster `$30` (`MOS6567-051`).
    den_seen: bool,
    /// Consecutive cycles before this one with `BA` low, saturating at three
    /// (`MOS6567-050`).
    ba_run: u8,
    /// The 40 × 12 bit video matrix line (`MOS6567-052`).
    line: [u16; 40],

    /// The matrix word paired with the graphics byte fetched this cycle and
    /// the previous one, for the sequencer's delayed reload (`MOS6567-054`).
    g_this: Option<(u8, u16)>,
    g_prev: Option<(u8, u16)>,
    /// The sequencer's eight-bit register, the word it was loaded with, the
    /// bits left in it and the latched pair (`MOS6567-054`).
    sr: u8,
    sr_c: u16,
    sr_left: u8,
    sr_pair: u8,
    sr_valid: bool,

    sprites: [SpriteLogic; 8],
    /// The eight-bit refresh counter (`MOS6567-059`).
    refresh: u8,

    /// The border unit's two flip-flops (`MOS6567-058`).
    main_border: bool,
    vertical_border: bool,

    /// `LP` as last sampled, and whether this frame's latch has been taken
    /// (`MOS6567-022`, `MOS6567-060`).
    lp_last: bool,
    lp_taken: bool,
    lpx: u8,
    lpy: u8,

    /// The fetches in flight: the first-phase one this cycle asked for, and
    /// the second-phase one, whose answer arrives at the next `phase1`.
    access1: Phase1Access,
    access2: Option<Phase2Access>,
    /// `AEC` is held low through this cycle's second phase.
    aec_low: bool,
}

impl Mos6567 {
    /// The part with every register zero and the raster at line 0, cycle 1.
    #[must_use]
    pub fn new(frame: Frame) -> Self {
        Self {
            regs: [0; 47],
            frame,
            cycle: 1,
            raster: 0,
            started: false,
            raster_compare: 0,
            latch: 0,
            vc: 0,
            vcbase: 0,
            rc: 0,
            vmli: 0,
            display: false,
            bad_line: false,
            den_seen: false,
            ba_run: 0,
            line: [0; 40],
            g_this: None,
            g_prev: None,
            sr: 0,
            sr_c: 0,
            sr_left: 0,
            sr_pair: 0,
            sr_valid: false,
            sprites: [SpriteLogic::default(); 8],
            refresh: 0xff,
            main_border: false,
            vertical_border: false,
            lp_last: false,
            lp_taken: false,
            lpx: 0,
            lpy: 0,
            access1: Phase1Access {
                a: 0x3fff,
                purpose: Phase1Purpose::Idle,
            },
            access2: None,
            aec_low: false,
        }
    }

    /// The raster line the part is on.
    #[must_use]
    pub const fn raster(&self) -> u16 {
        self.raster
    }

    /// The cycle within the line, `1..=65`, of the next `phase1`.
    #[must_use]
    pub const fn line_cycle(&self) -> usize {
        self.cycle as usize
    }

    /// `VC`, the video matrix counter (`MOS6567-052`).
    #[must_use]
    pub const fn vc(&self) -> u16 {
        self.vc
    }

    /// `VCBASE` (`MOS6567-052`).
    #[must_use]
    pub const fn vcbase(&self) -> u16 {
        self.vcbase
    }

    /// `RC`, the row counter (`MOS6567-052`).
    #[must_use]
    pub const fn rc(&self) -> u8 {
        self.rc
    }

    /// Whether the display logic is in display state (`MOS6567-052`).
    #[must_use]
    pub const fn is_in_display_state(&self) -> bool {
        self.display
    }

    /// The bad line condition as last evaluated (`MOS6567-051`).
    #[must_use]
    pub const fn is_bad_line(&self) -> bool {
        self.bad_line
    }

    /// Sprite `k`'s counters and flags (`MOS6567-057`).
    ///
    /// # Panics
    /// If `k` is not `0..=7`.
    #[must_use]
    pub const fn sprite(&self, k: usize) -> Sprite {
        self.sprites[k].state()
    }

    /// The border unit's two flip-flops (`MOS6567-058`).
    #[must_use]
    pub const fn border(&self) -> Border {
        Border {
            main: self.main_border,
            vertical: self.vertical_border,
        }
    }

    /// The X coordinate at the first dot of the next `phase1`'s cycle.
    ///
    /// `$19C` at cycle 1 and eight more per cycle (`MOS6567-048`). The R8's
    /// line is eight dots longer than its nine-bit coordinate, and where the
    /// surplus falls is `MOS6567-OPEN-05`: this crate repeats `$18C`–`$193`
    /// over cycles 63 and 64.
    #[must_use]
    pub const fn x(&self) -> u16 {
        schedule::x_at(self.cycle)
    }

    fn cr1(&self) -> u8 {
        self.regs[usize::from(CR1)]
    }

    fn cr2(&self) -> u8 {
        self.regs[usize::from(CR2)]
    }

    fn mode(&self) -> Mode {
        Mode {
            ecm: self.cr1() & ECM != 0,
            bmm: self.cr1() & BMM != 0,
            mcm: self.cr2() & MCM != 0,
        }
    }

    fn den(&self) -> bool {
        self.cr1() & DEN != 0
    }

    fn yscroll(&self) -> u8 {
        self.cr1() & 0x07
    }

    fn xscroll(&self) -> u8 {
        self.cr2() & 0x07
    }

    fn background(&self) -> [u8; 4] {
        [
            self.regs[0x21] & 0x0f,
            self.regs[0x22] & 0x0f,
            self.regs[0x23] & 0x0f,
            self.regs[0x24] & 0x0f,
        ]
    }

    /// `VM13`–`VM10` as the top four bits of a fourteen-bit address
    /// (`MOS6567-023`).
    fn matrix_base(&self) -> u16 {
        u16::from(self.regs[usize::from(MEMPTR)] & 0xf0) << 6
    }

    /// `CB13`–`CB11` as the top three bits of a fourteen-bit address
    /// (`MOS6567-024`).
    fn character_base(&self) -> u16 {
        u16::from(self.regs[usize::from(MEMPTR)] & 0x0e) << 10
    }

    /// The level at `IRQ`, active Low: [`Level::Low`] while an enabled
    /// interrupt latch is set (`MOS6567-016`, `MOS6567-020`). It is
    /// [`Phase2Outputs::irq_n`] read between cycles.
    #[must_use]
    pub fn irq_n(&self) -> Level {
        Level::from_bool(!self.irq_low())
    }

    fn irq_low(&self) -> bool {
        self.latch & self.regs[usize::from(IER)] & SOURCES != 0
    }

    /// Set an interrupt latch (`MOS6567-020`).
    fn raise(&mut self, source: u8) {
        self.latch |= source;
    }

    /// The first half of a cycle: `AEC` low, the part's own access, and the
    /// decisions that fall at the start of a cycle.
    ///
    /// The part has no `tick`: one `Ø0` cycle is this call and then
    /// [`Mos6567::phase2`], because a board must see this half's `BA`, `AEC`
    /// and fetch before it runs the processor's half and answers the fetch,
    /// and one call taking the whole cycle could not show them in time.
    ///
    /// `inp.access_d` answers the second-phase fetch the previous
    /// [`Mos6567::phase2`] returned, if any.
    #[must_use]
    pub fn phase1(&mut self, inp: Phase1Inputs) -> Phase1Outputs {
        let cycle = self.cycle;

        if let Some(access) = self.access2.take() {
            self.take_phase2_data(access, inp.access_d & 0x0fff);
        }

        if cycle == 1 {
            if self.started {
                self.raster += 1;
                if usize::from(self.raster) >= self.frame.line_count().get() {
                    self.raster = 0;
                }
            }
            self.started = true;
            if self.raster == 0 {
                self.vcbase = 0;
                self.den_seen = false;
                self.refresh = 0xff;
                self.lp_taken = false;
            }
            if self.raster == self.raster_compare {
                self.raise(IRST);
            }
        }

        if self.raster == 0x30 && self.den() {
            self.den_seen = true;
        }
        self.bad_line = (0x30..=0xf7).contains(&self.raster)
            && (self.raster & 0x07) as u8 == self.yscroll()
            && self.den_seen;
        if self.bad_line {
            self.display = true;
        }

        let lp_low = inp.lp_n.is_low();
        if lp_low && !self.lp_last && !self.lp_taken {
            self.lp_taken = true;
            let x_end = schedule::x_at(schedule::after(cycle, 1));
            self.lpx = (x_end >> 1) as u8;
            self.lpy = self.raster as u8;
            self.raise(ILP);
        }
        self.lp_last = lp_low;

        if cycle == schedule::VC_LOAD {
            self.vc = self.vcbase;
            self.vmli = 0;
            if self.bad_line {
                self.rc = 0;
            }
        }
        if cycle == schedule::RC_CHECK {
            if self.rc == 7 {
                self.display = self.bad_line;
                self.vcbase = self.vc;
            }
            if self.display {
                self.rc = (self.rc + 1) & 0x07;
            }
        }

        self.sprite_rules(cycle);

        let ba_low = self.wants_ba(cycle);
        let wants_phase2 = self.wants_phase2(cycle);
        let prior_run = self.ba_run;
        self.ba_run = if ba_low { (self.ba_run + 1).min(3) } else { 0 };
        self.aec_low = wants_phase2 && prior_run >= schedule::BA_LEAD;

        self.access1 = self.phase1_access(cycle);
        Phase1Outputs {
            ba: Level::from_bool(!ba_low),
            aec: Level::from_bool(!self.aec_low),
            access: self.access1,
        }
    }

    /// The second half of a cycle: the cycle's eight dots, the processor's
    /// access, and the part's second-phase access if it holds `AEC` low. With
    /// [`Mos6567::phase1`] before it, one `Ø0` cycle; that item says why the
    /// part has no `tick`.
    ///
    /// `inp.access_d` answers the fetch [`Mos6567::phase1`] returned.
    #[must_use]
    pub fn phase2(&mut self, inp: Phase2Inputs) -> Phase2Outputs {
        let cycle = self.cycle;
        self.take_phase1_data(inp.access_d & 0x0fff);

        let (sync_lum, colour) = self.render(cycle);

        self.access2 = if self.aec_low {
            self.phase2_access(cycle)
        } else {
            None
        };

        let mut d = None;
        if !self.aec_low && inp.ctrl.contains(Ctrl::CS) {
            let a = Select::new(inp.a);
            if inp.ctrl.contains(Ctrl::WRITE) {
                self.write(a, inp.d);
            } else {
                d = Some(self.read(a));
            }
        }

        self.g_prev = self.g_this;
        self.g_this = None;
        self.cycle = schedule::after(cycle, 1);

        Phase2Outputs {
            access: self.access2,
            d,
            irq_n: Level::from_bool(!self.irq_low()),
            sync_lum,
            colour,
        }
    }

    /// Whether `BA` is low this cycle (`MOS6567-050`).
    fn wants_ba(&self, cycle: u8) -> bool {
        let (first, last) = schedule::BAD_LINE_BA_RANGE_CYCLES;
        if self.bad_line && (first..=last).contains(&usize::from(cycle)) {
            return true;
        }
        (0..8u8).any(|k| {
            let first = schedule::before(schedule::pointer_cycle(k), schedule::BA_LEAD);
            self.sprites[usize::from(k)].dma
                && (0..=schedule::BA_LEAD + 1).any(|d| schedule::after(first, d) == cycle)
        })
    }

    /// Whether the part has a second-phase access this cycle, `AEC` allowing.
    fn wants_phase2(&self, cycle: u8) -> bool {
        if self.bad_line && (schedule::MATRIX_FIRST..=schedule::MATRIX_LAST).contains(&cycle) {
            return true;
        }
        (0..8u8).any(|k| {
            let p = schedule::pointer_cycle(k);
            self.sprites[usize::from(k)].dma && (cycle == p || cycle == schedule::after(p, 1))
        })
    }

    /// The first-phase access of `cycle` (`MOS6567-049`).
    fn phase1_access(&mut self, cycle: u8) -> Phase1Access {
        let idle = Phase1Access {
            a: 0x3fff,
            purpose: Phase1Purpose::Idle,
        };
        for k in 0..8u8 {
            let p = schedule::pointer_cycle(k);
            if cycle == p {
                self.sprites[usize::from(k)].begin_line();
                return Phase1Access {
                    a: self.matrix_base() | 0x03f8 | u16::from(k),
                    purpose: Phase1Purpose::SpritePointer(usize::from(k)),
                };
            }
            if cycle == schedule::after(p, 1) {
                let sprite = &mut self.sprites[usize::from(k)];
                if sprite.dma {
                    let access = Phase1Access {
                        a: u16::from(sprite.pointer) << 6 | u16::from(sprite.mc & 0x3f),
                        purpose: Phase1Purpose::SpriteData(usize::from(k)),
                    };
                    sprite.mc = (sprite.mc + 1) & 0x3f;
                    return access;
                }
                return idle;
            }
        }
        if (schedule::REFRESH_FIRST..=schedule::REFRESH_LAST).contains(&cycle) {
            let access = Phase1Access {
                a: 0x3f00 | u16::from(self.refresh),
                purpose: Phase1Purpose::Refresh,
            };
            self.refresh = self.refresh.wrapping_sub(1);
            return access;
        }
        if (schedule::GRAPHICS_FIRST..=schedule::GRAPHICS_LAST).contains(&cycle) {
            let ecm_mask: u16 = if self.cr1() & ECM != 0 { !0x0600 } else { !0 };
            let address = if self.display {
                let c = self.line[usize::from(self.vmli & 0x3f) % 40];
                let address = if self.cr1() & BMM != 0 {
                    (self.character_base() & 0x2000) | (self.vc & 0x03ff) << 3 | u16::from(self.rc)
                } else {
                    self.character_base() | (c & 0xff) << 3 | u16::from(self.rc)
                };
                address & ecm_mask
            } else {
                0x3fff & ecm_mask
            };
            return Phase1Access {
                a: address,
                purpose: Phase1Purpose::Graphics,
            };
        }
        idle
    }

    /// The second-phase access of `cycle` (`MOS6567-049`).
    fn phase2_access(&mut self, cycle: u8) -> Option<Phase2Access> {
        if self.bad_line && (schedule::MATRIX_FIRST..=schedule::MATRIX_LAST).contains(&cycle) {
            return Some(Phase2Access {
                a: self.matrix_base() | (self.vc & 0x03ff),
                purpose: Phase2Purpose::Matrix,
            });
        }
        for k in 0..8u8 {
            let p = schedule::pointer_cycle(k);
            if cycle == p || cycle == schedule::after(p, 1) {
                let sprite = &mut self.sprites[usize::from(k)];
                if sprite.dma {
                    let access = Phase2Access {
                        a: u16::from(sprite.pointer) << 6 | u16::from(sprite.mc & 0x3f),
                        purpose: Phase2Purpose::SpriteData(usize::from(k)),
                    };
                    sprite.mc = (sprite.mc + 1) & 0x3f;
                    return Some(access);
                }
            }
        }
        None
    }

    /// Take the board's answer to this cycle's first-phase fetch.
    fn take_phase1_data(&mut self, data: u16) {
        match self.access1.purpose {
            Phase1Purpose::Graphics => {
                let c = if self.display {
                    let c = self.line[usize::from(self.vmli) % 40];
                    self.vc = (self.vc + 1) & 0x03ff;
                    self.vmli = (self.vmli + 1) % 40;
                    c
                } else {
                    0
                };
                self.g_this = Some((data as u8, c));
            }
            Phase1Purpose::SpritePointer(k) => self.sprites[k].pointer = data as u8,
            Phase1Purpose::SpriteData(k) => self.sprites[k].load(1, data as u8),
            Phase1Purpose::Refresh | Phase1Purpose::Idle => {}
        }
    }

    /// Take the board's answer to the previous cycle's second-phase fetch.
    fn take_phase2_data(&mut self, access: Phase2Access, data: u16) {
        match access.purpose {
            Phase2Purpose::Matrix => self.line[usize::from(self.vmli) % 40] = data,
            Phase2Purpose::SpriteData(k) => {
                let sprite = &mut self.sprites[k];
                let index = if sprite.phase2_loads == 0 { 0 } else { 2 };
                sprite.phase2_loads += 1;
                sprite.load(index, data as u8);
            }
        }
    }

    /// The sprite rules that fall at the start of `cycle` (`MOS6567-057`,
    /// with the R8 cycles of `MOS6567-OPEN-04`).
    fn sprite_rules(&mut self, cycle: u8) {
        let raster_low = self.raster as u8;
        let enable = self.regs[0x15];
        let y_expand = self.regs[0x17];
        for k in 0..8usize {
            let bit = 1u8 << k;
            let y = self.regs[2 * k + 1];
            let sprite = &mut self.sprites[k];
            if y_expand & bit == 0 {
                sprite.expand_flip_flop = true;
            }
            if cycle == schedule::SPRITE_COMPARE_FIRST && y_expand & bit != 0 {
                sprite.expand_flip_flop = !sprite.expand_flip_flop;
            }
            if (cycle == schedule::SPRITE_COMPARE_FIRST || cycle == schedule::SPRITE_COMPARE_SECOND)
                && enable & bit != 0
                && y == raster_low
                && !sprite.dma
            {
                sprite.dma = true;
                sprite.mcbase = 0;
                if y_expand & bit != 0 {
                    sprite.expand_flip_flop = false;
                }
            }
            if cycle == schedule::SPRITE_LOAD {
                sprite.mc = sprite.mcbase;
                if sprite.dma && y == raster_low {
                    sprite.display = true;
                }
            }
            if cycle == schedule::MCBASE_FIRST && sprite.expand_flip_flop {
                sprite.mcbase = (sprite.mcbase + 2) & 0x3f;
            }
            if cycle == schedule::MCBASE_SECOND {
                if sprite.expand_flip_flop {
                    sprite.mcbase = (sprite.mcbase + 1) & 0x3f;
                }
                if sprite.mcbase == 63 {
                    sprite.dma = false;
                    sprite.display_ending = true;
                }
            }
        }
    }

    /// The border unit's vertical checks at the last cycle of a line
    /// (`MOS6567-058`, rules 2 and 3).
    fn border_line_end(&mut self) {
        let (top, bottom) = self.vertical_edges();
        if self.raster == bottom {
            self.vertical_border = true;
        }
        if self.raster == top && self.den() {
            self.vertical_border = false;
        }
    }

    fn vertical_edges(&self) -> (u16, u16) {
        if self.cr1() & RSEL != 0 {
            (51, 251)
        } else {
            (55, 247)
        }
    }

    fn horizontal_edges(&self) -> (u16, u16) {
        if self.cr2() & CSEL != 0 {
            (24, 344)
        } else {
            (31, 335)
        }
    }

    /// The eight dots of `cycle` (`MOS6567-054` to `MOS6567-058`).
    fn render(&mut self, cycle: u8) -> ([SyncLum; 8], [Colour; 8]) {
        let mut dots = [(SyncLum::default(), Colour::default()); 8];
        let x0 = schedule::x_at(cycle);
        let xscroll = self.xscroll();
        let mode = self.mode();
        let background = self.background();
        let exterior = self.regs[0x20] & 0x0f;
        let (left, right) = self.horizontal_edges();
        let (top, bottom) = self.vertical_edges();
        let den = self.den();
        let multicolour = self.regs[0x1c];
        let x_expand = self.regs[0x1d];
        let priority = self.regs[0x1b];
        let mm = [self.regs[0x25] & 0x0f, self.regs[0x26] & 0x0f];
        let mut mmc_hits = 0u8;
        let mut mbc_hits = 0u8;

        if cycle == LAST_CYCLE {
            self.border_line_end();
        }

        for (p, dot) in dots.iter_mut().enumerate() {
            let p = p as u8;
            let x = (x0 + u16::from(p)) & 0x01ff;

            if p == (4 + xscroll) & 0x07 {
                let source = if xscroll < 4 {
                    self.g_this
                } else {
                    self.g_prev
                };
                match source {
                    Some((byte, c)) => {
                        self.sr = byte;
                        self.sr_c = c;
                        self.sr_valid = true;
                    }
                    None => {
                        self.sr = 0;
                        self.sr_c = 0;
                        self.sr_valid = false;
                    }
                }
                self.sr_left = 8;
            }
            let bit = self.sr & 0x80 != 0;
            if self.sr_left.is_multiple_of(2) {
                self.sr_pair = self.sr >> 6;
            }
            let mut pixel = if self.sr_valid {
                sequencer::pixel(mode, self.sr_c, bit, self.sr_pair, background)
            } else {
                Pixel {
                    colour: background[0],
                    foreground: false,
                }
            };
            self.sr <<= 1;
            self.sr_left = self.sr_left.saturating_sub(1);

            if x == right {
                self.main_border = true;
            }
            if x == left {
                if self.raster == bottom {
                    self.vertical_border = true;
                }
                if self.raster == top && den {
                    self.vertical_border = false;
                }
                if !self.vertical_border {
                    self.main_border = false;
                }
            }

            if self.vertical_border {
                pixel = Pixel {
                    colour: background[0],
                    foreground: false,
                };
            }

            let mut first: Option<(usize, u8)> = None;
            let mut hits = 0u8;
            for k in 0..8usize {
                let bit_k = 1u8 << k;
                let sprite_x =
                    u16::from(self.regs[2 * k]) | u16::from(self.regs[0x10] & bit_k != 0) << 8;
                let colours = [self.regs[0x27 + k] & 0x0f, mm[0], mm[1]];
                if let Some(out) = self.sprites[k].dot(
                    x == sprite_x,
                    multicolour & bit_k != 0,
                    x_expand & bit_k != 0,
                    colours,
                ) {
                    hits |= bit_k;
                    if first.is_none() {
                        first = Some((k, out.colour));
                    }
                }
            }
            if hits.count_ones() >= 2 {
                mmc_hits |= hits;
            }
            if hits != 0 && pixel.foreground {
                mbc_hits |= hits;
            }

            let colour = if self.main_border {
                exterior
            } else if let Some((k, sprite_colour)) = first {
                if priority & (1 << k) == 0 || !pixel.foreground {
                    sprite_colour
                } else {
                    pixel.colour
                }
            } else {
                pixel.colour
            };

            *dot = self.mark(x, colour);
        }

        if mmc_hits != 0 {
            if self.regs[0x1e] == 0 {
                self.raise(IMMC);
            }
            self.regs[0x1e] |= mmc_hits;
        }
        if mbc_hits != 0 {
            if self.regs[0x1f] == 0 {
                self.raise(IMBC);
            }
            self.regs[0x1f] |= mbc_hits;
        }
        (
            dots.map(|(sync_lum, _)| sync_lum),
            dots.map(|(_, colour)| colour),
        )
    }

    /// Sync, blanking and burst around a colour (`MOS6567-044`,
    /// `MOS6567-OPEN-08`).
    fn mark(&self, x: u16, colour: u8) -> (SyncLum, Colour) {
        const BLANK: (SyncLum, Colour) = (SyncLum::Blank, Colour::Blank);
        const SYNC: (SyncLum, Colour) = (SyncLum::Sync, Colour::Blank);
        const BURST: (SyncLum, Colour) = (SyncLum::Blank, Colour::Burst);
        let in_vsync = (decode::VSYNC.0..decode::VSYNC.1).contains(&self.raster);
        let in_veq = (decode::VEQ.0..decode::VEQ.1).contains(&self.raster);
        let (first, last) = decode::VBLANK_RANGE_RASTERS;
        let in_vblank = (first..=last).contains(&self.raster);
        let eq = decode::within(x, decode::HEQ1) || decode::within(x, decode::HEQ2);
        if in_vsync {
            return if eq { BLANK } else { SYNC };
        }
        if in_veq {
            return if eq { SYNC } else { BLANK };
        }
        if decode::within(x, decode::HSYNC) {
            return SYNC;
        }
        if in_vblank {
            return BLANK;
        }
        if decode::within(x, decode::BURST) {
            return BURST;
        }
        if decode::within(x, decode::HBLANK) {
            return BLANK;
        }
        (
            SyncLum::Luminance(colour & 0x0f),
            Colour::Chroma(colour & 0x0f),
        )
    }

    /// A read of the register `a` selects, with its side effects: the read
    /// [`Mos6567::phase2`] makes with [`Ctrl::CS`] set and [`Ctrl::WRITE`]
    /// clear, callable without the bus (`MOS6567-017` to `MOS6567-021`,
    /// `MOS6567-062`).
    ///
    /// A read of either collision register clears it (`MOS6567-021`), and may
    /// be made for that effect alone; a read of any other register changes
    /// nothing.
    pub fn read(&mut self, a: Select) -> u8 {
        let rs = a.levels();
        match rs {
            0x11 => (self.cr1() & 0x7f) | ((self.raster >> 1) & 0x80) as u8,
            0x12 => self.raster as u8,
            0x13 => self.lpx,
            0x14 => self.lpy,
            0x16 => self.cr2() | 0xc0,
            0x18 => self.regs[0x18] | 0x01,
            0x19 => (self.latch & SOURCES) | 0x70 | if self.irq_low() { 0x80 } else { 0 },
            0x1a => self.regs[0x1a] | 0xf0,
            0x1e | 0x1f => {
                let value = self.regs[usize::from(rs)];
                self.regs[usize::from(rs)] = 0;
                value
            }
            0x20..=0x2e => self.regs[usize::from(rs)] | 0xf0,
            0x2f..=0x3f => 0xff,
            _ => self.regs[usize::from(rs)],
        }
    }

    /// A write of `d` to the register `a` selects: the write
    /// [`Mos6567::phase2`] makes with [`Ctrl::CS`] and [`Ctrl::WRITE`] set,
    /// callable without the bus (`MOS6567-017` to `MOS6567-020`,
    /// `MOS6567-062`).
    pub fn write(&mut self, a: Select, d: u8) {
        let rs = a.levels();
        let byte = d;
        match rs {
            0x11 => {
                self.regs[0x11] = byte;
                self.raster_compare = (self.raster_compare & 0x00ff) | u16::from(byte & RC8) << 1;
                if self.raster == 0x30 && self.den() {
                    self.den_seen = true;
                }
            }
            0x12 => self.raster_compare = (self.raster_compare & 0x0100) | u16::from(byte),
            0x19 => self.latch &= !(byte & SOURCES),
            0x1e | 0x1f | 0x2f..=0x3f => {}
            _ => self.regs[usize::from(rs)] = byte,
        }
    }
}
