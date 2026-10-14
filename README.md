# parts

This is an AI-generated collection of emulated circuits implemented in Rust. The
goal is to provide a stable foundation for agentic emulator projects.

## Transient warning
**Warning: loud transients.** Some audio parts here reproduce the power-on pop
of the original hardware, and a level read at a pin carries its DC offset, so
either can come out as a full-scale step. Route output through `rvec-dc-block`
(`use dc_block::DcBlock;`), which removes the DC and fades in from silence, or
keep your monitoring level low on first playback; a pop at full scale can damage
speakers, headphones or hearing.

## Usage
```toml
[dependencies]
rvec-z80 = { git = "https://github.com/reset-vector/parts", tag = "z80-v1.0.0" }
```

The package is `rvec-<part>` and the library keeps the part's name, so the code
says `use z80::Z80;`. A crate's `CHANGELOG.md` lists its releases.

A part that takes or returns a quantity does so in `rvec-units`'s types
(`use units::Microvolts;`). Cargo builds a git dependency once per commit, so
take every crate, `rvec-units` included, at one tag: the newest holds every
crate's latest release, and parts taken at two tags carry two `units` whose
types do not mix.

The crates make no claim of accuracy against any real part.

MIT licensed; see `LICENSE`.
