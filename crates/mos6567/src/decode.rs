//! Sync, blanking and burst as the sheet's decode tables place them
//! (`MOS6567-045`, `MOS6567-046`), in this crate's sprite-coordinate frame.
//!
//! The sheet's counts are read as four dots ahead of the coordinate at which
//! their effect appears, `MOS6567-OPEN-03`, so each horizontal window below
//! is the sheet's set and clear counts less [`DECODE_LEAD`]. A window is
//! `(set, clear)`; a horizontal one may wrap past `$1FF`.

/// The dots between a decode count and its effect (`MOS6567-OPEN-03`).
pub(crate) const DECODE_LEAD: u16 = 4;
/// `HSYNC` 416–452.
pub(crate) const HSYNC: (u16, u16) = (416 - DECODE_LEAD, 452 - DECODE_LEAD);
/// `HEQ1` 178–196.
pub(crate) const HEQ1: (u16, u16) = (178 - DECODE_LEAD, 196 - DECODE_LEAD);
/// `HEQ2` 434–452.
pub(crate) const HEQ2: (u16, u16) = (434 - DECODE_LEAD, 452 - DECODE_LEAD);
/// `HBLANK` 396–496.
pub(crate) const HBLANK: (u16, u16) = (396 - DECODE_LEAD, 496 - DECODE_LEAD);
/// `BURST` 456–492.
pub(crate) const BURST: (u16, u16) = (456 - DECODE_LEAD, 492 - DECODE_LEAD);
/// The rasters `VBLANK` is set on, as `(min, max)`, both included: set at
/// raster 13 and cleared at raster 24 (`MOS6567-045`).
pub const VBLANK_RANGE_RASTERS: (u16, u16) = (13, 23);
/// `VSYNC` 17–20, in rasters.
pub(crate) const VSYNC: (u16, u16) = (17, 20);
/// `VEQ` 14–23, in rasters.
pub(crate) const VEQ: (u16, u16) = (14, 23);

/// Whether `x` lies in a window that may wrap past `$1FF`.
#[must_use]
pub(crate) const fn within(x: u16, window: (u16, u16)) -> bool {
    let (set, clear) = window;
    if set <= clear {
        x >= set && x < clear
    } else {
        x >= set || x < clear
    }
}
