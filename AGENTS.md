# Agent guidelines

Luftpumpe reads the registers of an air-to-water heat pump (Schönknecht & Busch SB-WP-32-8, a rebadged Power World PW030-DKZLRS-A with the main board CC1010B) via the free RS485 port of its main board (Modbus RTU) and publishes them via MQTT. See README.md for the behavior and wiring.

## Priorities

1. The Luftpumpe must never put the heat pump at risk: It only reads (function 03H), never writes a register, and never blocks the bus.
2. Simple and robust logic over complex, error prone logic or logic requiring a lot of tuning.
3. Maintainable code over nice to have features.

## Goals

- This aims to be a very idiomatic embedded Rust project: `no_std`, async via embassy, no heap (apart from the one required by the Wi-Fi driver), no `unsafe` unless unavoidable.
- Use the SoC peripherals and DMA wherever possible instead of busy loops or bit banging in software (e.g. async UART, USB Serial/JTAG), via the async drivers of the HAL.
- Follow the current idioms of the HAL and embassy versions in use (check the crate sources and examples of the exact versions in `Cargo.lock`, the APIs change frequently).
- Follow the protocol of the main board (PW58332-2): 9600 Baud 8N1, unit address 1, at most 120 registers per request, more than 200 ms between requests. The transceiver's driver is only enabled while sending: It is held low by an external pull-down and driven low right after `hal::init`, saving the settings (erasing the flash stalls the executor) never overlaps a read, every await is bounded and the watchdog resets a stuck firmware.
- Writing registers (e.g. a better heating curve later on) needs a separate, deliberate change: The main board saves every written register to its flash (which wears out), so only an allowlist of registers with their ranges may ever be written, only on a change and rarely (never in every cycle). Factory reset (0x03E8), forced defrost (0x7FFE) and the expert parameters (A, B, D, R) must never be written.

## Structure

- `core/`: Chip independent logic (`no_std`, host-testable), everything that can be tested on the host belongs here (the protocol and the data, later on the decisions, e.g. which registers may be written):
  - `modbus.rs`: Modbus RTU framing: The read request, collecting and checking the response, the errors and the timing of the bus (checked against the limits of the main board at compile time).
  - `registers.rs`: The register map: The blocks read in every cycle and the published fields (raw values, the scaling of the temperatures is applied by the receiver).
  - `mqtt.rs`: The published state and the MQTT packets.
  - `settings.rs`: The settings and their parsing.
  - `settings/lines.rs`: The splitting of the serial input into lines.
  - `settings/stored.rs`: The stored format of the settings.
- `firmware/`: Thin ESP32-C3 binary (esp-hal, esp-rtos, embassy), only peripheral setup, tasks and glue. Has its own `.cargo/config.toml` (target, probe-rs runner) and `partitions.csv` (settings partition).
  - `src/main.rs`: Peripheral setup and the cycle: Reading the heat pump, the LED and handing the state to the MQTT task.
  - `src/modbus.rs`: The RS485 bus on the UART: Sending the requests (driver enable) and receiving the responses.
  - `src/settings.rs`: USB Serial/JTAG console and storing the settings in the flash.
  - `src/network.rs`: Wi-Fi connection and network stack (esp-radio, embassy-net).
  - `src/mqtt.rs`: Connection to the MQTT broker and publishing of the state (never blocking the cycle).

## Workflow

- `cargo test` and `cargo clippy --all-targets -- -D warnings` in the root: Host tests of `core`.
- `cd firmware && cargo build --release` (the target is configured there), `cargo clippy --release -- -D warnings` lints it, `cargo run --release` flashes via probe-rs.
- The crates shared with the Ladekaskade and the Thermokaskade are locked to the same versions (`Cargo.lock`), upgrade the projects together.
- Add tests in `core` for every behavior change, the firmware can't be tested on the host.
- Keep commits focused and building.

## Style

- Formatting is manual (`rustfmt.toml` disables rustfmt): Tabs, braces on the next line, see the existing code.
- Comments explain why, doc comments on all items, short and in the existing tone.
- Order items top-down: declarations first (type aliases and interrupt bindings, constants with tuning values before hardware facts, statics), then the public items (the driving structs and functions, in the firmware `main` and the tasks; public methods before private ones), the implementation details last. Readability wins: a helper may stay right after the function it serves, implementation data tables stay with the implementation.
- Log with defmt, user facing errors are `&'static str` messages (`Error::message`).
