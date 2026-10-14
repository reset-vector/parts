//! Williams' Special Chip 1, `A-5410-09911`, the block-transfer pair of
//! Robotron, Joust, Sinistar and Bubbles, at its pins.
//!
//! What a blit does to memory and how many cycles it holds the processor off
//! are as Sean Riddle's *Blitter Information* gives them. Where this crate
//! chooses, each choice names its open item.
//!
//! # Examples
//!
//! ```
//! use williams_sc1::{Inputs, Level, Memory, Outputs, Select, WilliamsSc1};
//!
//! struct Ram([u8; 0x1_0000]);
//! impl Memory for Ram {
//!     fn read(&mut self, address: u16) -> u8 {
//!         self.0[usize::from(address)]
//!     }
//!     fn write(&mut self, address: u16, data: u8, pins: Outputs) {
//!         // A nibble whose `/WINH` is Low keeps its old bits.
//!         let keep = if pins.winhu_n.is_low() { 0xf0 } else { 0 } | if pins.winhl_n.is_low() { 0x0f } else { 0 };
//!         let cell = &mut self.0[usize::from(address)];
//!         *cell = (*cell & keep) | (data & !keep);
//!     }
//! }
//!
//! let mut ram = Ram([0; 0x1_0000]);
//! ram.0[0x8000..0x8004].copy_from_slice(&[1, 2, 3, 4]);
//! let mut chips = WilliamsSc1::new();
//! for (offset, value) in [(2, 0x80), (3, 0x00), (4, 0x10), (5, 0x00), (6, 4 ^ 4), (7, 1 ^ 4)] {
//!     chips.write(Select::new(offset), value);
//! }
//! chips.write(Select::new(0), 0x00); // a fast copy; /HALT falls
//! assert_eq!(chips.outputs().halt_n, Level::Low);
//! let mut cycles = 0;
//! while chips.outputs().halt_n.is_low() {
//!     // `/BABS` Low: the bus is the pair's.
//!     let _ = chips.tick(Inputs { babs_n: Level::Low, ..Inputs::default() }, &mut ram);
//!     cycles += 1;
//! }
//! assert_eq!(&ram.0[0x1000..0x1004], &[1, 2, 3, 4]);
//! assert_eq!(cycles, 5, "WILLIAMS-SC1-024: one idle cycle, then a byte a cycle");
//! ```
//!
//! # The shape
//!
//! **The unit is the pair** — the package strapped upper (pin 14 High) and
//! the one strapped lower — because that is what a board fits
//! (`WILLIAMS-SC1-001`). [`Outputs`] carries the two packages' own
//! `/WINH` outputs; everything else the two drive is driven alike.
//!
//! - [`WilliamsSc1::write`] is a processor write cycle with
//!   `/EVIE` Low: `A2`–`A0` pick the register (`WILLIAMS-SC1-009`). Writing
//!   offset 0 starts a blit and pulls `/HALT` Low within that same cycle
//!   (`WILLIAMS-SC1-022`).
//! - [`WilliamsSc1::tick`] is one `E` cycle. The caller gives the levels at
//!   `/BABS`, Low when the processor shows `BA` and `BS` both High in it
//!   (`WILLIAMS-SC1-023`), and at `/RESET`, as [`Inputs`], and lends a
//!   [`Memory`]; in a cycle the pair owns, it reads and writes through it. A
//!   write carries the byte and the [`Outputs`] levels, and the board applies
//!   them (`WILLIAMS-SC1-021`); the tick returns the same levels.
//! - [`WilliamsSc1::outputs`] carries `/HALT` during the cycle about to be ticked:
//!   Low from the control write's own cycle through the cycle of the last
//!   write, High from the next (`WILLIAMS-SC1-022`, `WILLIAMS-SC1-026`). A host that
//!   presents each cycle's levels to the processor reads it after the cycle's
//!   register write and before [`WilliamsSc1::tick`].
//!
//! # The part, not the machine
//!
//! No decode, no memory, no video: which addresses select the pair
//! (`ca00`–`caff` on Robotron's ROM board), what a read at an address returns
//! and whether the video circuit is using RAM in a given half-cycle are the
//! board's. A write the pair makes into its own window is the board's to
//! route back, and the crate does not (`WILLIAMS-SC1-029`,
//! `WILLIAMS-SC1-OPEN-04`).
//!
//! # What this crate chooses
//!
//! The registers at power-up (`WILLIAMS-SC1-OPEN-02`; [`WilliamsSc1::new`]
//! clears them), which half of a fast cycle the read takes
//! (`WILLIAMS-SC1-OPEN-01`), the carries of the step-256 arithmetic
//! (`WILLIAMS-SC1-OPEN-03`), `/BABS` going High mid-blit
//! (`WILLIAMS-SC1-OPEN-10`), `/RESET` mid-blit (`WILLIAMS-SC1-OPEN-07`), a
//! fast blit reading RAM, which reads its source address and takes whatever
//! the bus answers (`WILLIAMS-SC1-OPEN-06`), and the window, where only
//! `A2`–`A0` are decoded and the rest mirror the eight registers
//! (`WILLIAMS-SC1-OPEN-11`).

#![no_std]

/// The control register's bits, offset 0 (`WILLIAMS-SC1-010`).
pub mod control {
    /// Control bit 0: the source steps 256 within a row (`WILLIAMS-SC1-015`).
    pub const SRC_STRIDE_256: u8 = 0x01;
    /// Control bit 1: the destination steps 256 within a row (`WILLIAMS-SC1-015`).
    pub const DST_STRIDE_256: u8 = 0x02;
    /// Control bit 2: a read cycle and a write cycle per byte (`WILLIAMS-SC1-025`).
    pub const SLOW: u8 = 0x04;
    /// Control bit 3: a zero pixel is not written (`WILLIAMS-SC1-019`).
    pub const FOREGROUND_ONLY: u8 = 0x08;
    /// Control bit 4: the solid register is written (`WILLIAMS-SC1-017`).
    pub const SOLID: u8 = 0x10;
    /// Control bit 5: the image moves one pixel right (`WILLIAMS-SC1-018`).
    pub const SHIFT: u8 = 0x20;
    /// Control bit 6: inverts the lower nibble's inhibit (`WILLIAMS-SC1-020`).
    pub const INVERT_LOWER: u8 = 0x40;
    /// Control bit 7: inverts the upper nibble's inhibit (`WILLIAMS-SC1-020`).
    pub const INVERT_UPPER: u8 = 0x80;
}

/// The levels at `A2`–`A0`: which register a [`WilliamsSc1::write`] reaches
/// (`WILLIAMS-SC1-009`), so a select and a data byte cannot be swapped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The levels `levels` at `A2`–`A0`, bit 0 `A0`; the bits above are no
    /// pin of the pair's and are dropped.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0x07)
    }

    /// The levels at `A2`–`A0`, bit 0 `A0`.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

use control::{
    DST_STRIDE_256, FOREGROUND_ONLY, INVERT_LOWER, INVERT_UPPER, SHIFT, SLOW, SOLID, SRC_STRIDE_256,
};

/// A pin's level, as [`Inputs`] and [`Outputs`] hold it.
pub use signal::Level;

/// The input pins [`WilliamsSc1::tick`] takes in one `E` cycle.
///
/// [`Inputs::default`] is the pair idle: `/BABS` High, the processor owning
/// the bus, and `/RESET` High.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `/BABS`: Low when the processor's `BA` and `BS` are
    /// both High in this cycle (`WILLIAMS-SC1-023`).
    pub babs_n: Level,
    /// `/RESET`: Low stops a blit and releases `/HALT`; the
    /// registers keep their values (`WILLIAMS-SC1-011`, `WILLIAMS-SC1-028`,
    /// `WILLIAMS-SC1-OPEN-07`).
    pub reset_n: Level,
}

impl Inputs {
    /// The pair idle: `/BABS` and `/RESET` High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            babs_n: Level::High,
            reset_n: Level::High,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The pair's outputs.
///
/// The two packages' `/WINH` pins (`WILLIAMS-SC1-021`): in a write cycle a
/// pin Low holds its nibble of the destination byte back, and the board
/// applies it. Outside a write cycle this crate reports both High, holding
/// nothing back.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `/WINHU`, pin 40 of the package strapped upper: `D7`–`D4`.
    pub winhu_n: Level,
    /// `/WINHL`, pin 40 of the package strapped lower: `D3`–`D0`.
    pub winhl_n: Level,
    /// `/HALT`, Low while the pair holds the processor off, from a write to
    /// offset 0 to the end of the blit's last cycle (`WILLIAMS-SC1-022`).
    pub halt_n: Level,
}

/// Both `/WINH` pins High, nothing held back, and `/HALT` released.
const RELEASED: Outputs = Outputs {
    winhu_n: Level::High,
    winhl_n: Level::High,
    halt_n: Level::High,
};

/// What the board answers in a cycle the pair owns.
pub trait Memory {
    /// The byte at `address`.
    #[must_use]
    fn read(&mut self, address: u16) -> u8;
    /// `data` written to `address`, with the nibbles whose `/WINH` pin in
    /// `pins` is Low held back.
    fn write(&mut self, address: u16, data: u8, pins: Outputs);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Phase {
    /// `/HALT` High.
    Idle,
    /// `/HALT` Low, waiting for `/BABS` (`WILLIAMS-SC1-023`).
    Requested,
    /// Bit 2 clear: a byte a cycle (`WILLIAMS-SC1-024`).
    Fast,
    /// Bit 2 set, the next cycle reads (`WILLIAMS-SC1-025`).
    SlowRead,
    /// Bit 2 set, the next cycle writes `byte`.
    SlowWrite,
}

/// The pair.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WilliamsSc1 {
    reg: [u8; 8],
    phase: Phase,
    /// The control byte the running blit started with.
    ctrl: u8,
    width: u16,
    height: u16,
    x: u16,
    y: u16,
    src: u16,
    dst: u16,
    src_row: u16,
    dst_row: u16,
    /// The low nibble of the last source byte (`WILLIAMS-SC1-018`).
    carry: u8,
    /// The byte a slow read fetched for the next cycle's write.
    byte: u8,
    /// The `/WINH` levels of the last cycle ticked.
    winh: Outputs,
}

impl Default for WilliamsSc1 {
    fn default() -> Self {
        Self::new()
    }
}

impl WilliamsSc1 {
    /// A pair at rest with every register 0 and no blit running, this
    /// crate's choice under `WILLIAMS-SC1-OPEN-02`; `/RESET` stops a blit and
    /// keeps the registers, so a board that needs known registers writes them.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reg: [0; 8],
            phase: Phase::Idle,
            ctrl: 0,
            width: 1,
            height: 1,
            x: 0,
            y: 0,
            src: 0,
            dst: 0,
            src_row: 0,
            dst_row: 0,
            carry: 0,
            byte: 0,
            winh: RELEASED,
        }
    }

    /// A processor write with `/EVIE` Low; `a` carries `A2`–`A0`
    /// (`WILLIAMS-SC1-009`). Offset 0 starts a blit
    /// with `d` as its control byte and asserts `/HALT` (`WILLIAMS-SC1-010`,
    /// `WILLIAMS-SC1-022`). A write to offset 0 while a blit runs restarts it
    /// from the registers; offsets 2 to 7 are read only at that start, so a
    /// write to one leaves the running blit alone, while offset 1, the solid
    /// colour, is read at each byte a solid blit writes and takes effect at
    /// once.
    pub fn write(&mut self, a: Select, d: u8) {
        let offset = usize::from(a.levels());
        self.reg[offset] = d;
        if offset == 0 {
            self.start(d);
        }
    }

    /// The outputs as they stand: the `/WINH` levels of the last cycle
    /// ticked, and `/HALT` as the pair drives it into the cycle about to be
    /// ticked, so Low at once after a write to offset 0.
    #[must_use]
    pub fn outputs(&self) -> Outputs {
        Outputs {
            halt_n: Level::from_bool(self.phase == Phase::Idle),
            ..self.winh
        }
    }

    /// One `E` cycle at the levels `inputs` gives, returning the `/WINH`
    /// levels of the cycle and `/HALT` as it stands at its end.
    ///
    /// `/RESET` Low stops a blit and releases `/HALT`, the registers kept
    /// (`WILLIAMS-SC1-011`, `WILLIAMS-SC1-028`, `WILLIAMS-SC1-OPEN-07`). With
    /// `/BABS` High the pair leaves the bus alone (`WILLIAMS-SC1-023`).
    pub fn tick<M: Memory + ?Sized>(&mut self, inputs: Inputs, memory: &mut M) -> Outputs {
        self.winh = self.cycle(inputs, memory);
        self.outputs()
    }

    fn cycle<M: Memory + ?Sized>(&mut self, inputs: Inputs, memory: &mut M) -> Outputs {
        if inputs.reset_n.is_low() {
            self.phase = Phase::Idle;
            return RELEASED;
        }
        if inputs.babs_n.is_high() {
            return RELEASED;
        }
        match self.phase {
            Phase::Idle => RELEASED,
            Phase::Requested => {
                if self.ctrl & SLOW == 0 {
                    self.phase = Phase::Fast;
                } else {
                    self.byte = memory.read(self.src);
                    self.phase = Phase::SlowWrite;
                }
                RELEASED
            }
            Phase::Fast => {
                let byte = memory.read(self.src);
                self.put(byte, memory)
            }
            Phase::SlowRead => {
                self.byte = memory.read(self.src);
                self.phase = Phase::SlowWrite;
                RELEASED
            }
            Phase::SlowWrite => {
                self.phase = Phase::SlowRead;
                self.put(self.byte, memory)
            }
        }
    }

    fn start(&mut self, ctrl: u8) {
        self.ctrl = ctrl;
        self.width = u16::from(self.reg[6] ^ 4).max(1);
        self.height = u16::from(self.reg[7] ^ 4).max(1);
        self.src = u16::from_be_bytes([self.reg[2], self.reg[3]]);
        self.dst = u16::from_be_bytes([self.reg[4], self.reg[5]]);
        self.src_row = self.src;
        self.dst_row = self.dst;
        self.x = 0;
        self.y = 0;
        self.carry = 0;
        self.phase = Phase::Requested;
    }

    /// Write one byte read from the source, then step (`WILLIAMS-SC1-014`).
    fn put<M: Memory + ?Sized>(&mut self, byte: u8, memory: &mut M) -> Outputs {
        let ctrl = self.ctrl;
        let pixels = if ctrl & SHIFT == 0 {
            byte
        } else {
            (self.carry << 4) | (byte >> 4)
        };
        self.carry = byte & 0x0f;
        let fore = ctrl & FOREGROUND_ONLY != 0;
        let pins = Outputs {
            winhu_n: Level::from_bool(!((fore && pixels & 0xf0 == 0) ^ (ctrl & INVERT_UPPER != 0))),
            winhl_n: Level::from_bool(!((fore && pixels & 0x0f == 0) ^ (ctrl & INVERT_LOWER != 0))),
            halt_n: Level::Low,
        };
        let data = if ctrl & SOLID == 0 {
            pixels
        } else {
            self.reg[1]
        };
        memory.write(self.dst, data, pins);
        self.step();
        pins
    }

    /// `WILLIAMS-SC1-015`, `WILLIAMS-SC1-016`.
    fn step(&mut self) {
        let ctrl = self.ctrl;
        let stride = |bit: u8| if ctrl & bit == 0 { 1 } else { 0x100 };
        self.src = self.src.wrapping_add(stride(SRC_STRIDE_256));
        self.dst = self.dst.wrapping_add(stride(DST_STRIDE_256));
        self.x += 1;
        if self.x < self.width {
            return;
        }
        self.x = 0;
        self.y += 1;
        if self.y == self.height {
            self.phase = Phase::Idle;
            return;
        }
        self.src_row = if ctrl & SRC_STRIDE_256 == 0 {
            self.src
        } else {
            self.src_row.wrapping_add(1)
        };
        self.dst_row = if ctrl & DST_STRIDE_256 == 0 {
            self.dst
        } else {
            self.dst_row.wrapping_add(1)
        };
        self.src = self.src_row;
        self.dst = self.dst_row;
    }
}
