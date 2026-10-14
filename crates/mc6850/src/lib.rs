//! The Motorola MC6850 asynchronous communications interface adapter, at its
//! pins.
//!
//! Every item below names the claim it implements.
//!
//! # Examples
//!
//! ```
//! use mc6850::{Level, Mc6850, control, status};
//!
//! let mut acia = Mc6850::new();
//! // Master reset, then divide by one and eight data bits, one stop bit
//! // (`MC6850-012`, `MC6850-014`).
//! acia.write(Level::Low, control::MASTER_RESET);
//! acia.write(Level::Low, 0b101 << 2);
//! assert_ne!(acia.status() & status::TDRE, 0);
//!
//! acia.write(Level::High, 0x41);
//! assert_eq!(acia.status() & status::TDRE, 0, "the byte waits in the register");
//! let mut line = [true; 12];
//! for level in &mut line {
//!     acia.tick_tx();
//!     *level = acia.tx_data().is_high();
//! }
//! assert!(line.contains(&false), "a start bit went out");
//! assert_ne!(acia.status() & status::TDRE, 0, "and the register is empty again");
//! ```
//!
//! # The shape
//!
//! This part has **three clock domains and no common one** (`MC6850-005`,
//! `MC6850-010`): the bus moves on `E`, the transmitter on `Tx CLK` and the
//! receiver on `Rx CLK`, and the part divides the two serial clocks by a ratio
//! its own control register holds (`MC6850-012`). So there is no single `tick`
//! that could mean anything here, and the three domains are three calls:
//!
//! - [`Mc6850::read`] and [`Mc6850::write`] are one `E` transaction each,
//!   with the part selected: `CS0` and `CS1` High, `CS2` Low, which the board
//!   decodes (`MC6850-006`).
//! - [`Mc6850::tick_tx`] is one period of `Tx CLK`, taken at its negative
//!   transition.
//! - [`Mc6850::tick_rx`] is one period of `Rx CLK`, taken at its positive
//!   transition, and samples the level [`Mc6850::set_rx_data`] last set at
//!   `Rx Data`.
//!
//! **A host that can only carry writes cannot drive this part.** Both of the
//! bus calls take `&mut self`, and that is the part and not an implementation
//! choice: a read of the receive data register clears `RDRF` (`MC6850-019`) and
//! a read of the status register arms the two-read sequence that releases the
//! latched `DCD` bit (`MC6850-023`). [`Mc6850::status`] is the one
//! side-effect-free view, for a host that wants to show the byte without being
//! the program that read it.
//!
//! `IRQ`, `RTS`, `CTS` and `DCD` are active Low and carry `_n`.
//!
//! # What this crate chooses
//!
//! Six behaviours here are the crate's choice, each under an open item, and
//! each is marked where it is written: the receiver starts a frame on the
//! leading edge of the start bit (`MC6850-OPEN-01`); a break code overrides
//! `Tx Data` and leaves the shifter running underneath (`MC6850-OPEN-02`);
//! master reset leaves the transmitter empty, `TDRE` hidden only while the
//! reset condition stands (`MC6850-OPEN-04`); a new divide ratio resets
//! neither counter (`MC6850-OPEN-05`); a write to a full transmit data
//! register replaces the waiting byte (`MC6850-OPEN-06`); and `RTS` follows
//! the control register with no delay (`MC6850-OPEN-08`).

#![no_std]
#![allow(clippy::struct_excessive_bools)]

use core::num::NonZeroUsize;

/// A pin's level, as a getter named for the pin returns it (`irq_n()`).
pub use signal::Level;

/// The status register's bits (`MC6850-018`), as [`Mc6850::status`] and a
/// status read present them.
pub mod status {
    /// `RDRF`, status bit 0 — the receive data register holds a character
    /// (`MC6850-018`, `MC6850-019`).
    pub const RDRF: u8 = 0x01;
    /// `TDRE`, status bit 1 — the transmit data register may be written
    /// (`MC6850-018`, `MC6850-021`).
    pub const TDRE: u8 = 0x02;
    /// `DCD`, status bit 2 (`MC6850-018`, `MC6850-023`).
    pub const DCD: u8 = 0x04;
    /// `CTS`, status bit 3 (`MC6850-018`, `MC6850-024`).
    pub const CTS: u8 = 0x08;
    /// `FE`, status bit 4 — framing error (`MC6850-018`, `MC6850-025`).
    pub const FE: u8 = 0x10;
    /// `OVRN`, status bit 5 — receiver overrun (`MC6850-018`, `MC6850-026`).
    pub const OVRN: u8 = 0x20;
    /// `PE`, status bit 6 — parity error (`MC6850-018`, `MC6850-028`).
    pub const PE: u8 = 0x40;
    /// `IRQ`, status bit 7 — the `IRQ` output is Low (`MC6850-018`,
    /// `MC6850-029`).
    pub const IRQ: u8 = 0x80;
}

/// The control register's fields (`MC6850-011`), as a write with `RS` Low
/// stores them.
pub mod control {
    /// `CR1`·`CR0`, the counter divide select (`MC6850-012`).
    pub const DIVIDE_MASK: u8 = 0x03;
    /// `CR1`·`CR0` = `11`, the master reset code (`MC6850-047`).
    pub const MASTER_RESET: u8 = 0x03;
    /// `CR4`·`CR3`·`CR2`, the word select (`MC6850-014`).
    pub const WORD_SELECT_MASK: u8 = 0x1c;
    /// `CR6`·`CR5`, the transmitter control (`MC6850-016`).
    pub const TRANSMIT_CONTROL_MASK: u8 = 0x60;
    /// `CR7`, the receive interrupt enable (`MC6850-017`).
    pub const RECEIVE_INTERRUPT_ENABLE: u8 = 0x80;
}

/// `n`, which the caller has checked is not zero.
const fn nonzero(n: usize) -> NonZeroUsize {
    match NonZeroUsize::new(n) {
        Some(n) => n,
        None => panic!("a ratio or a bit count is never zero"),
    }
}

/// The ratio the part divides both serial clocks by, from `CR1`·`CR0`.
///
/// `MC6850-012` is the encoding and `MC6850-013` is that one field serves the
/// transmitter and the receiver together.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Divide {
    /// `00` — the clock pin is the bit rate, and the board synchronises it to
    /// the data itself (`MC6850-040`).
    By1,
    /// `01`.
    By16,
    /// `10`.
    By64,
    /// `11` — not a ratio at all, but the master reset command
    /// (`MC6850-047`).
    MasterReset,
}

impl Divide {
    /// Clock cycles per bit time, or `None` in the master reset code.
    #[must_use]
    pub const fn cycles_per_bit(self) -> Option<NonZeroUsize> {
        match self {
            Self::By1 => Some(nonzero(1)),
            Self::By16 => Some(nonzero(16)),
            Self::By64 => Some(nonzero(64)),
            Self::MasterReset => None,
        }
    }
}

/// The sense of the parity bit, where one is selected.
///
/// `MC6850-028` defines odd parity as an odd total number of ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Parity {
    /// Even.
    Even,
    /// Odd.
    Odd,
}

/// One of the eight word formats `CR4`·`CR3`·`CR2` selects (`MC6850-014`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct WordFormat {
    /// Seven or eight.
    pub data_bit_count: NonZeroUsize,
    /// The parity bit, or none.
    pub parity: Option<Parity>,
    /// One or two.
    pub stop_bit_count: NonZeroUsize,
}

impl WordFormat {
    /// The bits between the start bit and the first stop bit.
    ///
    /// Eight or nine, never fewer (`MC6850-014`).
    #[must_use]
    pub const fn payload_bit_count(self) -> NonZeroUsize {
        self.data_bit_count
            .saturating_add(if self.parity.is_some() { 1 } else { 0 })
    }
}

/// What `CR6`·`CR5` selects, all three of it at once (`MC6850-016`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TransmitControl {
    /// `CR6`·`CR5` = `00` — `RTS` asserted (Low), the transmitter interrupt
    /// disabled.
    Rts,
    /// `01` — `RTS` asserted (Low), the transmitter interrupt enabled.
    RtsInterruptEnabled,
    /// `10` — `RTS` not asserted (High), the transmitter interrupt disabled.
    NoRts,
    /// `11` — `RTS` asserted (Low), a break level on `Tx Data`, the interrupt
    /// disabled.
    RtsBreak,
}

/// Where the transmitter is within a character.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Tx {
    /// Nothing to send; `Tx Data` idles at mark.
    Idle,
    /// A frame in the shift register, least significant bit first, with the
    /// stop bits already appended as ones.
    Sending { shift: u16, remaining: usize },
}

/// Where the receiver is within a character.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Rx {
    /// Watching `Rx Data` for the leading edge of a start bit.
    Idle,
    /// Counting low samples towards the half bit `MC6850-042` requires.
    Start { low_samples: usize },
    /// Counting clocks to the centre of the next bit.
    Bits {
        counter: usize,
        index: usize,
        shift: u16,
    },
}

/// The whole part.
///
/// [`Mc6850::new`] is the part as the supply comes up: `MC6850-048`'s internal
/// power-on reset holds it there, and it stays there until a master reset is
/// written to the control register.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mc6850 {
    /// The control register, write-only at the bus (`MC6850-011`).
    control: u8,
    /// Held reset by the power-on logic, until the first master reset command
    /// releases it (`MC6850-048`).
    power_on_held: bool,
    /// How many master reset commands have been written, saturating. Only the
    /// first is distinguished (`MC6850-049`).
    master_resets: u8,

    /// The receive data register.
    rdr: u8,
    /// `RDRF` before `DCD` hides it (`MC6850-019`, `MC6850-020`).
    rdrf: bool,
    /// A character arrived with `RDRF` still set; the overrun has begun but has
    /// not reached the status register (`MC6850-026`, `MC6850-027`).
    overrun_pending: bool,
    /// `OVRN` as the status register shows it.
    overrun: bool,
    /// `FE`, belonging to the character in `rdr` (`MC6850-025`).
    framing_error: bool,
    /// `PE`, belonging to the character in `rdr` (`MC6850-028`).
    parity_error: bool,

    /// The transmit data register, `None` when empty.
    tdr: Option<u8>,
    /// `TDRE` before `CTS` and the reset condition inhibit it (`MC6850-021`,
    /// `MC6850-022`).
    tdre: bool,

    /// The level at the `CTS` pin.
    cts_pin: bool,
    /// The level at the `DCD` pin.
    dcd_pin: bool,
    /// The `DCD` status bit is latched High and has not been released
    /// (`MC6850-023`).
    dcd_latched: bool,
    /// A low-to-high `DCD` transition is still asking for an interrupt
    /// (`MC6850-044`).
    dcd_interrupt: bool,
    /// The status register has been read; the second half of the two-read
    /// sequence will release the latch (`MC6850-023`, `MC6850-045`).
    status_read: bool,

    tx: Tx,
    tx_counter: usize,
    /// The level the transmitter holds `Tx Data` at, before a break overrides
    /// it.
    tx_line: bool,

    rx: Rx,
    /// The level at the `Rx Data` pin.
    rx_data_pin: bool,
}

impl Default for Mc6850 {
    fn default() -> Self {
        Self::new()
    }
}

impl Mc6850 {
    /// The part with the supply just up, in the reset condition
    /// ([`Mc6850::is_in_reset`]).
    ///
    /// `MC6850-048`: the internal power-on reset holds it here, and only a
    /// bus-programmed master reset releases it. The part has no reset pin: a
    /// board reaches the reset condition by writing the master reset code,
    /// `CR1`·`CR0` = `11`, to the control register (`MC6850-047`), and leaves
    /// it by writing any other. Neither edition of the data sheet fixes the
    /// control register's contents at power-up, so this crate starts it at 0,
    /// where a master reset would store `11`; a board that needs a known
    /// word writes one after the master reset. `CTS` and `DCD` start Low,
    /// asserted, until [`Mc6850::set_cts_n`] and [`Mc6850::set_dcd_n`] say
    /// otherwise.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            control: 0,
            power_on_held: true,
            master_resets: 0,
            rdr: 0,
            rdrf: false,
            overrun_pending: false,
            overrun: false,
            framing_error: false,
            parity_error: false,
            tdr: None,
            tdre: true,
            cts_pin: false,
            dcd_pin: false,
            dcd_latched: false,
            dcd_interrupt: false,
            status_read: false,
            tx: Tx::Idle,
            tx_counter: 0,
            tx_line: true,
            rx: Rx::Idle,
            rx_data_pin: true,
        }
    }

    /// `CR1`·`CR0` (`MC6850-012`).
    #[must_use]
    pub const fn divide(&self) -> Divide {
        match self.control & control::DIVIDE_MASK {
            0 => Divide::By1,
            1 => Divide::By16,
            2 => Divide::By64,
            _ => Divide::MasterReset,
        }
    }

    /// `CR4`·`CR3`·`CR2` (`MC6850-014`).
    #[must_use]
    pub const fn word_format(&self) -> WordFormat {
        match (self.control & control::WORD_SELECT_MASK) >> 2 {
            0 => WordFormat {
                data_bit_count: nonzero(7),
                parity: Some(Parity::Even),
                stop_bit_count: nonzero(2),
            },
            1 => WordFormat {
                data_bit_count: nonzero(7),
                parity: Some(Parity::Odd),
                stop_bit_count: nonzero(2),
            },
            2 => WordFormat {
                data_bit_count: nonzero(7),
                parity: Some(Parity::Even),
                stop_bit_count: nonzero(1),
            },
            3 => WordFormat {
                data_bit_count: nonzero(7),
                parity: Some(Parity::Odd),
                stop_bit_count: nonzero(1),
            },
            4 => WordFormat {
                data_bit_count: nonzero(8),
                parity: None,
                stop_bit_count: nonzero(2),
            },
            5 => WordFormat {
                data_bit_count: nonzero(8),
                parity: None,
                stop_bit_count: nonzero(1),
            },
            6 => WordFormat {
                data_bit_count: nonzero(8),
                parity: Some(Parity::Even),
                stop_bit_count: nonzero(1),
            },
            _ => WordFormat {
                data_bit_count: nonzero(8),
                parity: Some(Parity::Odd),
                stop_bit_count: nonzero(1),
            },
        }
    }

    /// `CR6`·`CR5` (`MC6850-016`).
    #[must_use]
    pub const fn transmit_control(&self) -> TransmitControl {
        match (self.control & control::TRANSMIT_CONTROL_MASK) >> 5 {
            0 => TransmitControl::Rts,
            1 => TransmitControl::RtsInterruptEnabled,
            2 => TransmitControl::NoRts,
            _ => TransmitControl::RtsBreak,
        }
    }

    /// `CR7` (`MC6850-017`).
    #[must_use]
    pub const fn is_receive_interrupt_enabled(&self) -> bool {
        self.control & control::RECEIVE_INTERRUPT_ENABLE != 0
    }

    /// Whether the part is being held in the reset condition — either by the
    /// power-on logic or by the master reset command standing in `CR1`·`CR0`
    /// (`MC6850-047`, `MC6850-048`).
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.power_on_held || matches!(self.divide(), Divide::MasterReset)
    }

    /// One read: `RS` Low reaches the status register, High the receive data
    /// register (`MC6850-008`, `MC6850-009`).
    ///
    /// Both have side effects, which is why this takes `&mut self`, and a read
    /// may be made for them alone: the status read arms the sequence of
    /// `MC6850-023`, and the data read clears `RDRF` or the overrun.
    pub fn read(&mut self, rs: Level) -> u8 {
        if rs.is_high() {
            self.read_data()
        } else {
            self.read_status()
        }
    }

    /// One write: `RS` Low reaches the control register, High the transmit data
    /// register (`MC6850-008`, `MC6850-009`).
    ///
    /// The call *is* the negative transition of `E` that `MC6850-010` names.
    ///
    /// A data write is taken while the part is held in master reset as at any
    /// other time: it loads the transmit data register and clears `TDRE`, and
    /// the byte is still there when the reset is released. That is this
    /// crate's choice; no claim decides it.
    pub fn write(&mut self, rs: Level, d: u8) {
        if rs.is_high() {
            self.tdr = Some(d);
            self.tdre = false;
        } else {
            self.write_control(d);
        }
    }

    /// The status register: the byte a status read would return, without
    /// performing the read.
    ///
    /// Every bit is `MC6850-018`'s position, named in [`status`]. `RDRF` is hidden by a High at
    /// `DCD` (`MC6850-020`) and `TDRE` by a High at `CTS` or by the reset
    /// condition (`MC6850-022`).
    #[must_use]
    pub const fn status(&self) -> u8 {
        let mut status = 0;
        if self.rdrf && !self.dcd_pin {
            status |= status::RDRF;
        }
        if self.tdre && !self.cts_pin && !self.is_in_reset() {
            status |= status::TDRE;
        }
        if self.dcd_latched || self.dcd_pin {
            status |= status::DCD;
        }
        if self.cts_pin {
            status |= status::CTS;
        }
        if self.framing_error {
            status |= status::FE;
        }
        if self.overrun {
            status |= status::OVRN;
        }
        if self.parity_error {
            status |= status::PE;
        }
        if self.irq_asserted() {
            status |= status::IRQ;
        }
        status
    }

    fn read_status(&mut self) -> u8 {
        let status = self.status();
        self.status_read = true;
        status
    }

    fn read_data(&mut self) -> u8 {
        if self.overrun_pending {
            self.overrun_pending = false;
            self.overrun = true;
        } else {
            self.rdrf = false;
            self.overrun = false;
        }
        if self.status_read {
            self.dcd_latched = false;
            self.dcd_interrupt = false;
            self.status_read = false;
        }
        self.rdr
    }

    fn write_control(&mut self, value: u8) {
        let was_reset = matches!(self.divide(), Divide::MasterReset);
        self.control = value;
        if matches!(self.divide(), Divide::MasterReset) {
            if !was_reset {
                self.master_resets = self.master_resets.saturating_add(1);
            }
            self.master_reset();
        }
    }

    /// `MC6850-047`: the status register is cleared but for the external
    /// conditions on `CTS` and `DCD`, and both halves of the part are
    /// initialised. The other control bits are untouched because this is
    /// reached from a write that has already stored them.
    fn master_reset(&mut self) {
        self.power_on_held = false;
        self.rdrf = false;
        self.overrun = false;
        self.overrun_pending = false;
        self.framing_error = false;
        self.parity_error = false;
        self.status_read = false;
        self.dcd_latched = false;
        self.dcd_interrupt = false;
        self.tdr = None;
        self.tdre = true;
        self.tx = Tx::Idle;
        self.tx_counter = 0;
        self.tx_line = true;
        self.rx = Rx::Idle;
    }

    /// The `IRQ` output's level, [`Level::Low`] while the part asks for an
    /// interrupt. The pin is open drain (`MC6850-004`): [`Level::High`] is
    /// released, and the board's pull-up makes it High.
    ///
    /// `MC6850-043` is the transmitter's condition, `MC6850-044` the receiver's
    /// three, and `MC6850-046` is that this is a level and not a pulse.
    /// `MC6850-049` holds the output High through any master reset.
    #[must_use]
    pub const fn irq_n(&self) -> Level {
        Level::from_bool(!self.irq_asserted())
    }

    const fn irq_asserted(&self) -> bool {
        if self.is_in_reset() {
            return false;
        }
        let transmitter = matches!(
            self.transmit_control(),
            TransmitControl::RtsInterruptEnabled
        ) && self.tdre
            && !self.cts_pin;
        let receiver = self.is_receive_interrupt_enabled()
            && ((self.rdrf && !self.dcd_pin) || self.overrun || self.dcd_interrupt);
        transmitter || receiver
    }

    /// The `RTS` output's level.
    ///
    /// `MC6850-051` is the level as a function of `CR5` and `CR6`;
    /// `MC6850-049` is why the first master reset overrides it. The power-on
    /// state is read the same way, because `MC6850-048` has the power-on logic
    /// holding the part reset to prevent erroneous output transitions and the
    /// sheet gives the pin no other level to be at.
    #[must_use]
    pub const fn rts_n(&self) -> Level {
        let high = (self.power_on_held || (self.master_resets <= 1 && self.is_in_reset()))
            || matches!(self.transmit_control(), TransmitControl::NoRts);
        Level::from_bool(high)
    }

    /// The `Tx Data` output's level, [`Level::High`] for a mark.
    ///
    /// `MC6850-016`'s break code forces the line to a space. How that interacts
    /// with a character already in the shift register is `MC6850-OPEN-02`; this
    /// crate overrides the level and leaves the shifter running underneath.
    #[must_use]
    pub const fn tx_data(&self) -> Level {
        Level::from_bool(
            self.tx_line && !matches!(self.transmit_control(), TransmitControl::RtsBreak),
        )
    }

    /// Drive the `CTS` input (`MC6850-024`, `MC6850-050`).
    pub const fn set_cts_n(&mut self, level: Level) {
        self.cts_pin = level.is_high();
    }

    /// Drive the `DCD` input.
    ///
    /// A low-to-high transition latches the status bit and raises the interrupt
    /// (`MC6850-023`, `MC6850-052`).
    pub const fn set_dcd_n(&mut self, level: Level) {
        let level = level.is_high();
        if level && !self.dcd_pin {
            self.dcd_latched = true;
            self.dcd_interrupt = true;
        }
        self.dcd_pin = level;
    }

    /// One period of `Tx CLK`, taken at its negative transition
    /// (`MC6850-034`).
    ///
    /// The part divides this pin by `MC6850-012`'s ratio, so a bit time is one,
    /// sixteen or sixty-four of these calls.
    pub fn tick_tx(&mut self) {
        let Some(divisor) = self.divide().cycles_per_bit().map(NonZeroUsize::get) else {
            return;
        };
        if self.power_on_held {
            return;
        }
        self.tx_counter += 1;
        if self.tx_counter < divisor {
            return;
        }
        self.tx_counter = 0;
        self.tx_bit_time();
    }

    fn tx_bit_time(&mut self) {
        if matches!(self.tx, Tx::Idle) {
            self.tx_line = true;
            self.load_transmit_shift_register();
        }
        if let Tx::Sending { shift, remaining } = self.tx {
            self.tx_line = shift & 1 != 0;
            let remaining = remaining - 1;
            self.tx = if remaining == 0 {
                Tx::Idle
            } else {
                Tx::Sending {
                    shift: shift >> 1,
                    remaining,
                }
            };
        }
    }

    /// `MC6850-031`: the pending character moves into the shift register on its
    /// own, and that transfer is what raises `TDRE` (`MC6850-030`).
    fn load_transmit_shift_register(&mut self) {
        let Some(byte) = self.tdr.take() else {
            return;
        };
        self.tdre = true;
        let format = self.word_format();
        let data = byte & mask(format.data_bit_count.get());
        let mut shift = u16::from(data) << 1;
        let mut length = 1 + format.data_bit_count.get();
        if let Some(parity) = format.parity {
            if parity_bit(data, parity) {
                shift |= 1u16 << length;
            }
            length += 1;
        }
        shift |= mask16(format.stop_bit_count.get()) << length;
        length += format.stop_bit_count.get();
        self.tx = Tx::Sending {
            shift,
            remaining: length,
        };
    }

    /// The level at the `Rx Data` input, High for a mark, held until set
    /// again; High from [`Mc6850::new`].
    pub const fn set_rx_data(&mut self, level: Level) {
        self.rx_data_pin = level.is_high();
    }

    /// One period of `Rx CLK`, taken at its positive transition, sampling the
    /// level at `Rx Data` (`MC6850-040`) as [`Mc6850::set_rx_data`] last set it.
    ///
    /// A High at `DCD` holds the receiver initialised (`MC6850-052`), and so
    /// does the reset condition.
    pub fn tick_rx(&mut self) {
        let rx_data = self.rx_data_pin;
        let Some(divisor) = self.divide().cycles_per_bit().map(NonZeroUsize::get) else {
            self.rx = Rx::Idle;
            return;
        };
        if self.power_on_held || self.dcd_pin {
            self.rx = Rx::Idle;
            return;
        }
        match self.rx {
            Rx::Idle => {
                if !rx_data {
                    self.rx = if divisor == 1 {
                        Rx::Bits {
                            counter: 0,
                            index: 0,
                            shift: 0,
                        }
                    } else {
                        Rx::Start { low_samples: 1 }
                    };
                }
            }
            Rx::Start { low_samples } => {
                if rx_data {
                    self.rx = Rx::Idle;
                } else {
                    let low_samples = low_samples + 1;
                    self.rx = if low_samples >= divisor / 2 {
                        Rx::Bits {
                            counter: 0,
                            index: 0,
                            shift: 0,
                        }
                    } else {
                        Rx::Start { low_samples }
                    };
                }
            }
            Rx::Bits {
                counter,
                index,
                shift,
            } => {
                let counter = counter + 1;
                if counter < divisor {
                    self.rx = Rx::Bits {
                        counter,
                        index,
                        shift,
                    };
                    return;
                }
                let shift = shift | (u16::from(rx_data) << index);
                let index = index + 1;
                let format = self.word_format();
                if index < format.payload_bit_count().get() + 1 {
                    self.rx = Rx::Bits {
                        counter: 0,
                        index,
                        shift,
                    };
                } else {
                    self.rx = Rx::Idle;
                    self.deliver(shift, format);
                }
            }
        }
    }

    /// A whole character has been sampled; put it where `MC6850-036` says.
    fn deliver(&mut self, sampled: u16, format: WordFormat) {
        if self.rdrf {
            self.overrun_pending = true;
            return;
        }
        let data = (sampled & 0x00ff) as u8 & mask(format.data_bit_count.get());
        self.rdr = data;
        self.parity_error = match format.parity {
            None => false,
            Some(parity) => {
                let received = (sampled >> format.data_bit_count.get()) & 1 != 0;
                received != parity_bit(data, parity)
            }
        };
        let first_stop = (sampled >> format.payload_bit_count().get()) & 1 != 0;
        self.framing_error = !first_stop;
        self.rdrf = true;
    }
}

/// The parity bit that makes the total number of ones agree with `parity`
/// (`MC6850-028`).
const fn parity_bit(data: u8, parity: Parity) -> bool {
    let odd = data.count_ones() % 2 == 1;
    match parity {
        Parity::Even => odd,
        Parity::Odd => !odd,
    }
}

const fn mask(bits: usize) -> u8 {
    ((1u16 << bits) - 1) as u8
}

const fn mask16(bits: usize) -> u16 {
    (1u16 << bits) - 1
}
