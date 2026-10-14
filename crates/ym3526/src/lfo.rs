//! The two low-frequency oscillators (`YM3526-025`, `YM3526-026`).
//!
//! **AM** is a triangle of 27 levels, 0 to 26 in steps of 0.1875 dB, moving
//! one step every [`AM_SAMPLES_PER_STEP`] samples through a cycle of
//! [`AM_STEPS`]: level 0 is held for seven steps, level 26 for three and
//! every other level for four on each slope. The shallow depth is the level
//! shifted down by two. **Vibrato** moves through eight steps, one every
//! [`VIB_SAMPLES_PER_STEP`] samples, adding `0, x/2, x, x/2, 0, −x/2, −x,
//! −x/2` to the F-number, `x` its top three bits at the deep depth and half
//! them at the shallow.
//!
//! Where in either cycle initial clear leaves them is `YM3526-OPEN-08`: both
//! start at step 0 of the tables below.

/// Samples between two AM steps.
pub(crate) const AM_SAMPLES_PER_STEP: u32 = 64;
/// Steps in one AM cycle.
pub(crate) const AM_STEPS: u8 = 210;
/// The deepest AM level, 26 steps of 0.1875 dB.
pub(crate) const AM_MAX: u8 = 26;
/// Samples between two vibrato steps.
pub(crate) const VIB_SAMPLES_PER_STEP: u32 = 1024;
/// Steps in one vibrato cycle.
pub(crate) const VIB_STEPS: u8 = 8;

/// The AM level at a step of its cycle: 0 for steps 0–6, rising one level
/// every four steps to 25, 26 for three steps, and falling back to 1.
pub(crate) const fn am_level(step: u8) -> u8 {
    let s = step % AM_STEPS;
    if s < 7 {
        0
    } else if s < 107 {
        1 + (s - 7) / 4
    } else if s < 110 {
        AM_MAX
    } else {
        25 - (s - 110) / 4
    }
}

/// The AM attenuation at a step in the envelope's units of 0.09375 dB:
/// two a level, the level shifted down by two at the shallow depth
/// (`YM3526-024`).
pub(crate) const fn am_attenuation(step: u8, deep: bool) -> u16 {
    let level = am_level(step);
    let level = if deep { level } else { level >> 2 };
    level as u16 * 2
}

/// Vibrato's F-number offset at a step of its cycle for an F-number and a
/// depth.
pub(crate) const fn vib_offset(step: u8, fnum: u16, deep: bool) -> i32 {
    let top = ((fnum >> 7) & 7) as i32;
    let x = if deep { top } else { top >> 1 };
    let h = x >> 1;
    match step % VIB_STEPS {
        0 | 4 => 0,
        1 | 3 => h,
        2 => x,
        5 | 7 => -h,
        _ => -x,
    }
}
