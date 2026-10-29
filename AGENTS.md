# Agent guidelines

Luftpumpe reads the registers of an air-to-water heat pump (Schönknecht & Busch SB-WP-32-8, a rebadged Power World PW030-DKZLRS-A with the main board CC1010B) via the free RS485 port of its main board (Modbus RTU).

## Priorities

1. The Luftpumpe must never put the heat pump at risk: It only reads (function 03H), never writes a register, and never blocks the bus.
2. Simple and robust logic over complex, error prone logic or logic requiring a lot of tuning.
3. Maintainable code over nice to have features.

## Goals

- This aims to be a very idiomatic embedded Rust project: `no_std`, async via embassy, no heap, no `unsafe` unless unavoidable.
- Use the SoC peripherals and DMA wherever possible instead of busy loops or bit banging in software (e.g. async UART), via the async drivers of the HAL.
- Follow the current idioms of the HAL and embassy versions in use (check the crate sources and examples of the exact versions in `Cargo.lock`, the APIs change frequently).
- Follow the protocol of the main board (PW58332-2): 9600 Baud 8N1, unit address 1, at most 120 registers per request, more than 200 ms between requests.
- Writing registers (e.g. a better heating curve later on) needs a separate, deliberate change: The main board saves every written register to its flash (which wears out), so only an allowlist of registers with their ranges may ever be written, only on a change and rarely (never in every cycle). Factory reset (0x03E8), forced defrost (0x7FFE) and the expert parameters (A, B, D, R) must never be written.

## Workflow

- Keep commits focused and building.

## Style

- Formatting is manual: Tabs, braces on the next line, as in the Ladekaskade and the Thermokaskade.
- Comments explain why, doc comments on all items, short and in the existing tone.
- Order items top-down: declarations first (type aliases and interrupt bindings, constants with tuning values before hardware facts, statics), then the public items (the driving structs and functions, in the firmware `main` and the tasks; public methods before private ones), the implementation details last. Readability wins: a helper may stay right after the function it serves, implementation data tables stay with the implementation.
- Log with defmt, user facing errors are `&'static str` messages (`Error::message`).
