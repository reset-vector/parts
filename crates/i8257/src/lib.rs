//! The Intel 8257 programmable DMA controller, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! # Examples
//!
//! ```
//! use i8257::{I8257, Inputs, Level, Select, State};
//!
//! let mut dma = I8257::new();
//! // Channel 0: address 1000h, a read transfer of one byte.
//! for byte in [0x00, 0x10] {
//!     dma.write(Select::new(0x0), byte);
//! }
//! for byte in [0x00, 0x80] {
//!     dma.write(Select::new(0x1), byte);
//! }
//! dma.write(Select::new(0x8), 0x01); // enable channel 0
//! let request = Inputs { drq: 0x01, hlda: Level::High, ..Inputs::idle() };
//! let mut outputs = dma.tick(request);
//! assert_eq!(outputs.hrq, Level::High);
//! while dma.state() != State::S1 {
//!     outputs = dma.tick(request);
//! }
//! assert_eq!((outputs.a, outputs.d), (Some(0x00), Some(0x10)));
//! ```
//!
//! # The shape
//!
//! The part has two faces, and the crate has one call for each:
//!
//! - **Slave**, while the processor owns the bus: [`I8257::write`] is one
//!   `I/OW` pulse and [`I8257::read`] one `I/OR` pulse with `CS` Low, at the
//!   register `A3`–`A0` select (`I8257-004`, `I8257-005`). Neither is tied to
//!   `CLK`.
//! - **Master**, the DMA cycle: [`I8257::tick`] is one period of `CLK`. It
//!   takes the levels at `RESET`, `DRQ0`–`DRQ3`, `HLDA` and `READY` as they stand at
//!   the falling edge that begins the period, which is where the sheet's
//!   set-up times are measured, and returns [`Outputs`], the levels the part
//!   holds its outputs at by the end of the period. One call is one state of
//!   the sheet's state diagram (`I8257-012`).
//!
//! **Time is the count of [`I8257::tick`] calls** and nothing else.
//! Within a period, whether a line moved on the rising or the falling edge
//! is not represented, and a host that needs finer than a period does not get
//! it here.
//!
//! The part does not own memory. A host that sees `MEMR` Low with an address
//! on the pins reads its own memory and puts the byte where its own `I/OW`
//! peripheral latches it, and the same the other way round: the part drives
//! the address and the strobes and never a data byte.
//!
//! # Readings the sheet does not make
//!
//! Each is an open item that names this crate's choice: the illegal transfer
//! type `11` is run as a verify cycle (`I8257-OPEN-01`); `MARK` goes High with
//! `TC` at a count of zero (`I8257-OPEN-02`); requests withdrawn during `S0`
//! drop `HRQ` and return the part to `SI` (`I8257-OPEN-03`); a count of zero
//! decrements to `3FFFh` (`I8257-OPEN-04`); the undefined addresses `9h`–`Fh`
//! are ignored and drive nothing (`I8257-OPEN-05`); a register read returns the
//! working value, not the one programmed (`I8257-OPEN-06`); the update flag
//! rises at the end of the terminal-count cycle (`I8257-OPEN-07`); a burst's
//! next channel is chosen from the requests at the start of `S4`
//! (`I8257-OPEN-08`).

#![no_std]

pub use signal::Level;

/// The seven internal states of the sheet's `DMA OPERATION STATE DIAGRAM`,
/// p. 6-255 (`I8257-012`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum State {
    /// Idle.
    Si,
    /// `HRQ` is out, waiting for `HLDA`.
    S0,
    /// The address goes out and is strobed.
    S1,
    /// The read strobe falls.
    S2,
    /// The write strobe falls, `TC` and `MARK` rise, `READY` is sampled.
    S3,
    /// A wait state.
    Sw,
    /// The write strobe rises and the channel's registers step.
    S4,
}

impl State {
    /// `S1` to `S4` and `SW`: the part is bus master.
    #[must_use]
    pub const fn is_master(self) -> bool {
        matches!(self, Self::S1 | Self::S2 | Self::S3 | Self::Sw | Self::S4)
    }
}

/// The type of DMA cycle bits 15 and 14 of a terminal count register select
/// (`I8257-007`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Transfer {
    /// `00`: no read or write strobe (`I8257-022`).
    Verify,
    /// `01`: peripheral to memory, `I/OR` and `MEMW`.
    Write,
    /// `10`: memory to peripheral, `MEMR` and `I/OW`.
    Read,
    /// `11`: the sheet's "Illegal"; run as [`Transfer::Verify`]
    /// (`I8257-OPEN-01`).
    Illegal,
}

impl Transfer {
    /// The type in the top two bits of a terminal count register.
    const fn of(count: u16) -> Self {
        match count >> 14 {
            0 => Self::Verify,
            1 => Self::Write,
            2 => Self::Read,
            _ => Self::Illegal,
        }
    }
}

/// The levels of the select pins `A3`–`A0`, `A0` in bit 0, as [`I8257::read`]
/// and [`I8257::write`] take them (`I8257-004`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The select at `levels`, masked to `A3`–`A0` (bits 3–0); the other
    /// bits are ignored.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0x0F)
    }

    /// The levels of `A3`–`A0`, in bits 3–0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// The inputs [`I8257::tick`] samples, as they stand at the falling edge
/// that begins the period.
///
/// [`Inputs::default`] is `RESET` Low, no request, no hold acknowledge, and
/// `READY` High: nothing asks the part to do anything or to wait.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `RESET`; High holds the part in its reset state (`I8257-010`).
    pub reset: Level,
    /// `DRQ0`–`DRQ3` at bits 0–3, `1` for a request. Bits 7–4 are ignored.
    pub drq: u8,
    /// `HLDA`.
    pub hlda: Level,
    /// `READY`.
    pub ready: Level,
}

impl Inputs {
    /// `RESET` Low, no request, no hold acknowledge, and `READY` High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            reset: Level::Low,
            drq: 0,
            hlda: Level::Low,
            ready: Level::High,
        }
    }
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

/// The part's outputs by the end of one period of `CLK`. `None` is a line
/// the part is not driving. The active-low lines end `_n`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `HRQ`.
    pub hrq: Level,
    /// `AEN` (`I8257-017`).
    pub aen: Level,
    /// `ADSTB` (`I8257-018`).
    pub adstb: Level,
    /// `DACK0`–`DACK3` at bits 0–3 as levels: a `0` bit is that channel
    /// acknowledged (`I8257-019`).
    pub dack_n: u8,
    /// `TC` (`I8257-026`).
    pub tc: Level,
    /// `MARK` (`I8257-027`).
    pub mark: Level,
    /// `MEMR` as an output.
    pub memr_n: Option<Level>,
    /// `MEMW` as an output.
    pub memw_n: Option<Level>,
    /// `I/OR` as an output.
    pub ior_n: Option<Level>,
    /// `I/OW` as an output.
    pub iow_n: Option<Level>,
    /// `A0`–`A7` as outputs, the low byte of the memory address
    /// (`I8257-018`).
    pub a: Option<u8>,
    /// `D0`–`D7` as outputs, the high byte of the memory address for the
    /// 8212 to latch (`I8257-018`).
    pub d: Option<u8>,
}

impl Outputs {
    /// Nothing on the bus, no request, no acknowledge.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            hrq: Level::Low,
            aen: Level::Low,
            adstb: Level::Low,
            dack_n: 0x0F,
            tc: Level::Low,
            mark: Level::Low,
            memr_n: None,
            memw_n: None,
            ior_n: None,
            iow_n: None,
            a: None,
            d: None,
        }
    }
}

impl Default for Outputs {
    /// [`Outputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

/// One channel's two sixteen-bit registers (`I8257-003`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
struct Channel {
    address: u16,
    count: u16,
}

/// Mode set register bits (`I8257-008`).
const ROTATING: u8 = 0x10;
const EXTENDED_WRITE: u8 = 0x20;
const TC_STOP: u8 = 0x40;
const AUTO_LOAD: u8 = 0x80;

/// The 8257.
///
/// [`I8257::new`] stands at power-up. The data sheet fixes no level there, so
/// this crate starts the part in its reset state, as `RESET` leaves it. A
/// board that needs a known state asserts `RESET` ([`Inputs::reset`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct I8257 {
    channels: [Channel; 4],
    mode: u8,
    /// `TC0`–`TC3` of the status register.
    terminal: u8,
    /// `UP` of the status register.
    update: bool,
    /// Channel 3's parameters are to be copied into channel 2 when channel 2
    /// next begins a cycle (`I8257-030`).
    reload_pending: bool,
    /// The cycle under way is the one that took channel 3's parameters.
    reload_cycle: bool,
    /// The first/last flip-flop: `true` when the next channel access is the
    /// high byte (`I8257-006`).
    last: bool,
    state: State,
    hrq: bool,
    /// The channel priority resolution chose, for the next cycle.
    chosen: Option<usize>,
    /// The channel of the cycle under way, its address and its type.
    serviced: usize,
    cycle_address: u16,
    cycle_transfer: Transfer,
    cycle_tc: bool,
    cycle_mark: bool,
    /// The channel with the highest priority under rotation (`I8257-015`).
    top: usize,
    cycles: u64,
    pins: Outputs,
    /// `RESET` as the last [`I8257::tick`] saw it.
    reset: bool,
}

impl Default for I8257 {
    fn default() -> Self {
        Self::new()
    }
}

impl I8257 {
    /// Power-up, which the sheet does not fix: the part's reset state, as
    /// just after `RESET`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            channels: [Channel {
                address: 0,
                count: 0,
            }; 4],
            mode: 0,
            terminal: 0,
            update: false,
            reload_pending: false,
            reload_cycle: false,
            last: false,
            state: State::Si,
            hrq: false,
            chosen: None,
            serviced: 0,
            cycle_address: 0,
            cycle_transfer: Transfer::Verify,
            cycle_tc: false,
            cycle_mark: false,
            top: 0,
            cycles: 0,
            pins: Outputs::idle(),
            reset: false,
        }
    }

    /// The state the last [`I8257::tick`] left the part in.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// The outputs as the last [`I8257::tick`] left them.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        self.pins
    }

    /// Periods of `CLK` since power-up (construction), a running total that
    /// `RESET` does not clear.
    #[must_use]
    pub const fn cycles(&self) -> u64 {
        self.cycles
    }

    /// One `I/OW` pulse with `CS` Low at the select `a`.
    /// Ignored while the part is bus master, when `CS` is disabled
    /// (`I8257-005`), and while the last [`I8257::tick`] saw `RESET` High
    /// (`I8257-010`).
    pub fn write(&mut self, a: Select, d: u8) {
        if self.state.is_master() || self.reset {
            return;
        }
        let data = d;
        let address = a.levels();
        if address & 0x08 != 0 {
            if address == 0x08 {
                self.mode = data;
                self.last = false;
                if data & AUTO_LOAD == 0 {
                    self.update = false;
                    self.reload_pending = false;
                }
            }
            return;
        }
        let channel = usize::from(address >> 1);
        let count = address & 1 != 0;
        self.store(channel, count, data);
        if channel == 2 && self.mode & AUTO_LOAD != 0 {
            self.store(3, count, data);
        }
        self.last = !self.last;
    }

    /// One `I/OR` pulse with `CS` Low at the select `a`.
    /// `None` is the data bus not driven: while the part is bus master
    /// (`I8257-005`), or at an undefined address (`I8257-OPEN-05`). While the
    /// last [`I8257::tick`] saw `RESET` High the part stays in its reset
    /// state: the read presents the cleared register and changes nothing
    /// (`I8257-010`).
    ///
    /// Otherwise a read may be made for its effect alone: a status read
    /// clears the terminal count flags (`I8257-009`), and a channel register
    /// read toggles the first/last flip-flop (`I8257-006`).
    pub fn read(&mut self, a: Select) -> Option<u8> {
        if self.state.is_master() {
            return None;
        }
        let address = a.levels();
        if self.reset {
            return (address <= 0x08).then_some(0);
        }
        if address & 0x08 != 0 {
            if address != 0x08 {
                return None;
            }
            let status = self.terminal | if self.update { 0x10 } else { 0 };
            self.terminal = 0;
            return Some(status);
        }
        let channel = &self.channels[usize::from(address >> 1)];
        let word = if address & 1 != 0 {
            channel.count
        } else {
            channel.address
        };
        let [low, high] = word.to_le_bytes();
        let byte = if self.last { high } else { low };
        self.last = !self.last;
        Some(byte)
    }

    fn store(&mut self, channel: usize, count: bool, data: u8) {
        let channel = &mut self.channels[channel];
        let word = if count {
            &mut channel.count
        } else {
            &mut channel.address
        };
        let [low, high] = word.to_le_bytes();
        *word = if self.last {
            u16::from_le_bytes([low, data])
        } else {
            u16::from_le_bytes([data, high])
        };
    }

    /// The requesting channel priority gives the bus to, among the enabled
    /// ones (`I8257-011`, `I8257-014`, `I8257-015`).
    fn resolve(&self, drq: u8) -> Option<usize> {
        let requests = drq & self.mode & 0x0F;
        let top = if self.mode & ROTATING != 0 {
            self.top
        } else {
            0
        };
        (0..4)
            .map(|n| (top + n) % 4)
            .find(|&channel| requests & (1 << channel) != 0)
    }

    /// One period of `CLK`, from one falling edge to the next: `inputs` are
    /// the levels set up to the falling edge that begins the period, and the
    /// returned [`Outputs`] are the outputs as they stand by the end of it. Lines
    /// the sheet moves on the rising edge within a state are not separated.
    ///
    /// With `RESET` High the period clears every register and leaves the part
    /// idle and off the bus (`I8257-010`); the count of [`I8257::cycles`] is
    /// the host's and survives.
    pub fn tick(&mut self, inputs: Inputs) -> Outputs {
        self.cycles += 1;
        self.reset = inputs.reset.is_high();
        if self.reset {
            let cycles = self.cycles;
            *self = Self::new();
            self.cycles = cycles;
            self.reset = true;
            return self.pins;
        }
        let verify = matches!(self.cycle_transfer, Transfer::Verify | Transfer::Illegal);
        self.state = match self.state {
            State::Si if self.hrq => State::S0,
            State::Si => State::Si,
            State::S0 if !self.hrq => State::Si,
            State::S0 if inputs.hlda.is_high() => self.begin(),
            State::S0 => State::S0,
            State::S1 => State::S2,
            State::S2 => State::S3,
            State::S3 | State::Sw if verify || inputs.ready.is_high() => State::S4,
            State::S3 | State::Sw => State::Sw,
            State::S4 if self.hrq && inputs.hlda.is_high() => self.begin(),
            State::S4 => State::Si,
        };
        match self.state {
            State::Si => self.hrq = self.resolve(inputs.drq).is_some(),
            State::S0 => {
                self.chosen = self.resolve(inputs.drq);
                if self.chosen.is_none() {
                    self.hrq = false;
                }
            }
            State::S3 => {
                if self.cycle_tc {
                    self.terminal |= 1 << self.serviced;
                }
            }
            State::S4 => self.end(inputs.drq),
            State::S1 | State::S2 | State::Sw => {}
        }
        self.pins = self.levels();
        self.pins
    }

    /// Enter `S1` for the channel priority chose (`I8257-012`).
    fn begin(&mut self) -> State {
        let Some(channel) = self.chosen else {
            self.hrq = false;
            return State::Si;
        };
        if channel == 2 && self.reload_pending {
            self.channels[2] = self.channels[3];
            self.reload_pending = false;
            self.reload_cycle = true;
        }
        let registers = self.channels[channel];
        let count = registers.count & 0x3FFF;
        self.serviced = channel;
        self.cycle_address = registers.address;
        self.cycle_transfer = Transfer::of(registers.count);
        self.cycle_tc = count == 0;
        self.cycle_mark = count.is_multiple_of(128);
        State::S1
    }

    /// The end of a cycle, in `S4` (`I8257-013`, `I8257-025`, `I8257-028`,
    /// `I8257-030`).
    fn end(&mut self, drq: u8) {
        let channel = self.serviced;
        let registers = &mut self.channels[channel];
        registers.address = registers.address.wrapping_add(1);
        registers.count = (registers.count & 0xC000) | (registers.count.wrapping_sub(1) & 0x3FFF);
        if self.reload_cycle {
            self.reload_cycle = false;
            self.update = false;
        }
        if self.cycle_tc {
            if channel == 2 && self.mode & AUTO_LOAD != 0 {
                self.update = true;
                self.reload_pending = true;
            } else if self.mode & TC_STOP != 0 {
                self.mode &= !(1 << channel);
            }
        }
        if self.mode & ROTATING != 0 {
            self.top = (channel + 1) % 4;
        }
        self.chosen = self.resolve(drq);
        self.hrq = self.chosen.is_some();
    }

    /// The outputs for the state just entered.
    fn levels(&self) -> Outputs {
        let mut pins = Outputs {
            hrq: Level::from_bool(self.hrq),
            ..Outputs::idle()
        };
        if !self.state.is_master() {
            return pins;
        }
        let [low, high] = self.cycle_address.to_le_bytes();
        pins.aen = Level::High;
        pins.a = Some(low);
        pins.memr_n = Some(Level::High);
        pins.memw_n = Some(Level::High);
        pins.ior_n = Some(Level::High);
        pins.iow_n = Some(Level::High);
        if self.state == State::S1 {
            pins.adstb = Level::High;
            pins.d = Some(high);
            return pins;
        }
        pins.dack_n = 0x0F & !(1 << self.serviced);
        let write_low = match self.state {
            State::S2 => self.mode & EXTENDED_WRITE != 0,
            State::S3 | State::Sw => true,
            State::Si | State::S0 | State::S1 | State::S4 => false,
        };
        if matches!(self.state, State::S3 | State::Sw) {
            pins.tc = Level::from_bool(self.cycle_tc);
            pins.mark = Level::from_bool(self.cycle_mark);
        }
        let (read, write) = match self.cycle_transfer {
            Transfer::Read => (&mut pins.memr_n, &mut pins.iow_n),
            Transfer::Write => (&mut pins.ior_n, &mut pins.memw_n),
            Transfer::Verify | Transfer::Illegal => return pins,
        };
        *read = Some(Level::Low);
        *write = Some(Level::from_bool(!write_low));
        pins
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_select_keeps_only_a3_to_a0() {
        assert_eq!(Select::new(0xFF).levels(), 0x0F);
        assert_eq!(Select::new(0x38).levels(), 0x08);
    }
}
