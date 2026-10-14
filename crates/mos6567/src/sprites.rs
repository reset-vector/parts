//! One sprite — the sheet's movable image block — as a data counter, a base, a
//! 24-bit shift register and an expansion flip-flop.
//!
//! `MOS6567-057` is Bauer's account of how the part keeps one, and the cycle
//! numbers this crate runs the rules at are in `schedule.rs`.

/// One sprite's counters and flags as [`crate::Mos6567::sprite`] shows
/// them. Not pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[non_exhaustive]
pub struct Sprite {
    /// `MC`, the six-bit data counter (`MOS6567-026`, `MOS6567-057`).
    pub mc: u8,
    /// `MCBASE` (`MOS6567-057`).
    pub mcbase: u8,
    /// The expansion flip-flop (`MOS6567-057`).
    pub expand_flip_flop: bool,
    /// Whether the three data fetches are on this line.
    pub dma: bool,
    /// Whether the sprite is displayed.
    pub display: bool,
    /// The eight-bit pointer fetched this line (`MOS6567-026`).
    pub pointer: u8,
}

/// One sprite's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub(crate) struct SpriteLogic {
    /// `MC`, the six-bit data counter (`MOS6567-026`, `MOS6567-057`).
    pub mc: u8,
    /// `MCBASE` (`MOS6567-057`).
    pub mcbase: u8,
    /// The expansion flip-flop (`MOS6567-057`).
    pub expand_flip_flop: bool,
    /// Whether the three data fetches are on this line.
    pub dma: bool,
    /// Whether the sprite is displayed.
    pub display: bool,
    /// The display ends at the next pointer cycle: `MCBASE` reached 63 in
    /// this line, after this line's data was already fetched.
    pub display_ending: bool,
    /// The eight-bit pointer fetched this line (`MOS6567-026`).
    pub pointer: u8,
    /// The 24-bit shift register (`MOS6567-057`), with the three bytes in its
    /// high, middle and low positions.
    pub shift: u32,
    /// The register is shifting out this line: the X coordinate has matched.
    pub shifting: bool,
    /// Dots shifted out so far this line, counting the doubled ones.
    pub shifted: u8,
    /// The pair latched for a multi-colour dot.
    pub pair: u8,
    /// Second-phase data bytes taken this line, to tell the first from the
    /// third.
    pub phase2_loads: u8,
}

/// The colour of one sprite dot, or `None` where it is transparent
/// (`MOS6567-038`, `MOS6567-039`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) struct SpriteDot {
    /// The colour code.
    pub colour: u8,
}

impl SpriteLogic {
    /// The counters and flags a debugger is shown.
    pub const fn state(&self) -> Sprite {
        Sprite {
            mc: self.mc,
            mcbase: self.mcbase,
            expand_flip_flop: self.expand_flip_flop,
            dma: self.dma,
            display: self.display,
            pointer: self.pointer,
        }
    }

    /// Load a fetched data byte into the register: `index` 0, 1 or 2 for the
    /// high, middle and low byte (`MOS6567-057`).
    pub fn load(&mut self, index: u8, byte: u8) {
        let shift = 16 - 8 * u32::from(index);
        self.shift = (self.shift & !(0xff << shift)) | (u32::from(byte) << shift);
    }

    /// Start the line's display: the register will shift from the dot the X
    /// coordinate matches.
    pub fn begin_line(&mut self) {
        self.shifting = false;
        self.shifted = 0;
        self.phase2_loads = 0;
        if self.display_ending {
            self.display = false;
            self.display_ending = false;
        }
    }

    /// One dot of the sprite's output, given whether the beam is at its X
    /// coordinate, and its multi-colour and X-expand bits, and the three
    /// colours a dot may take: the sprite's own, multi-colour 0 and
    /// multi-colour 1.
    ///
    /// Bauer's rule 6 (`MOS6567-057`): from the matching dot the register
    /// shifts one bit per dot, every second dot when expanded, and in pairs
    /// when multi-colour. Twenty-four bits later it is empty for the line.
    pub fn dot(
        &mut self,
        at_x: bool,
        multicolour: bool,
        x_expand: bool,
        colours: [u8; 3],
    ) -> Option<SpriteDot> {
        if !self.display {
            return None;
        }
        if !self.shifting && at_x && self.shifted == 0 {
            self.shifting = true;
        }
        if !self.shifting {
            return None;
        }
        let span: u8 = if x_expand { 48 } else { 24 };
        if self.shifted >= span {
            self.shifting = false;
            return None;
        }
        let bit_index = if x_expand {
            self.shifted / 2
        } else {
            self.shifted
        };
        let out = if multicolour {
            if bit_index.is_multiple_of(2) {
                self.pair = ((self.shift >> (22 - bit_index)) & 0x03) as u8;
            }
            match self.pair {
                0 => None,
                1 => Some(colours[1]),
                2 => Some(colours[0]),
                _ => Some(colours[2]),
            }
        } else if (self.shift >> (23 - bit_index)) & 1 != 0 {
            Some(colours[0])
        } else {
            None
        };
        self.shifted += 1;
        out.map(|colour| SpriteDot { colour })
    }
}
