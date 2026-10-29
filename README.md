# Luftpumpe

Pumps the data out of an air-to-water heat pump (German: Luft-Wasser-Wärmepumpe).

## About

The display of the heat pump only shows the current values and offers a single timer. The Luftpumpe reads the values of the heat pump (temperatures, compressor, heaters, valves, faults and the settings) via the free RS485 port of its main board, to record and analyze them (e.g. how often the electric heater runs). Steering the heat pump (e.g. a better heating curve or separate schedules for heating and hot water) is meant to follow later, built on top of it.

It was created for the Schönknecht & Busch SB-WP-32-8, a rebadged Power World PW030-DKZLRS-A (Easylife series, main board CC1010B), but should work with other Power World heat pumps using the same protocol (PW58332-2, or PW58321 with the same addresses) and probably with other heat pumps on the same main board.

## Building

[rustup](https://rustup.rs) installs the toolchain pinned in `rust-toolchain.toml`. The chip independent logic in `core` is tested on the host:

```bash
cargo test
```
