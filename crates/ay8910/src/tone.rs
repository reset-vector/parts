use crate::Level;

/// One tone generator: a 12-bit up-counter and its raw square-wave level.
///
/// On each divided clock edge the counter increments; reaching the live
/// period (`max(1, period)`) toggles the level and clears the counter. A
/// period lowered to or below the current count therefore transitions on the
/// next divided edge; a raised one extends the current half-cycle.
///
/// The reset state, `level` Low, is the die just after its first divided
/// edge and not the High before it (`AY8910-052`): the crate's tick *k* is
/// the part's divided edge *k*.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ToneState {
    /// The 12-bit up-counter, counting divided edges since the last toggle.
    pub counter: u16,
    /// The raw square-wave level.
    pub level: Level,
}

impl Default for ToneState {
    /// The counter at 0 and the level Low.
    fn default() -> Self {
        Self {
            counter: 0,
            level: Level::Low,
        }
    }
}

impl ToneState {
    pub(crate) fn tick(&mut self, period: u16) {
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.level = !self.level;
        }
    }
}
