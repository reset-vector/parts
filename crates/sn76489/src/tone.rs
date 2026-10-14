use crate::Level;

/// One tone generator: a ten-stage counter and the flip-flop its borrow toggles
/// (`SN76489-016`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ToneState {
    /// The counter, counting down to zero at the divided clock rate.
    pub counter: u16,
    /// The frequency flip-flop, which is what leaves the generator.
    pub level: Level,
}

impl Default for ToneState {
    /// The counter at zero and the flip-flop set, its level High. **The sheet fixes no level at
    /// power-up** for the flip-flop (`SN76489-016`) and the part has no reset pin;
    /// this crate starts it in the state a period under two holds it in
    /// (`SN76489-018`), so a channel whose period register is still zero
    /// already holds that state.
    fn default() -> Self {
        Self {
            counter: 0,
            level: Level::High,
        }
    }
}

impl ToneState {
    /// One divided clock edge, with the channel's live period register.
    ///
    /// A period under two holds the output instead of oscillating
    /// (`SN76489-018`); otherwise the counter decrements, and on reaching zero
    /// reloads and toggles, which is a half-period of `period` divided edges
    /// and so a full period of twice it.
    pub(crate) fn tick(&mut self, period: u16) {
        if period < 2 {
            self.counter = period;
            self.level = Level::High;
            return;
        }
        if self.counter > 0 {
            self.counter -= 1;
        }
        if self.counter == 0 {
            self.counter = period;
            self.level = !self.level;
        }
    }
}
