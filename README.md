# Luftpumpe

Pumps the data out of an air-to-water heat pump (German: Luft-Wasser-Wärmepumpe).

## About

The display of the heat pump only shows the current values and offers a single timer. The Luftpumpe reads the values of the heat pump (temperatures, compressor, heaters, valves, faults and the settings) via the free RS485 port of its main board, to record and analyze them (e.g. how often the electric heater runs). Steering the heat pump (e.g. a better heating curve or separate schedules for heating and hot water) is meant to follow later, built on top of it.

It was created for the Schönknecht & Busch SB-WP-32-8, a rebadged Power World PW030-DKZLRS-A (Easylife series, main board CC1010B), but should work with other Power World heat pumps using the same protocol (PW58332-2, or PW58321 with the same addresses) and probably with other heat pumps on the same main board.

## How it works

- Every 30 seconds, the registers of the main board are read with 3 requests (Modbus RTU, function 03H): The control flags, the mode and the first defrost parameters (8 registers), the system parameters P01 to F1 (116 registers) and the status (43 registers). The requests are 250 ms apart, the main board requires more than 200 ms.
- The Luftpumpe only reads, it never writes a register: The main board saves every written register to its flash, which wears out. The display keeps working as before, it is connected to another port of the main board.
- The transceiver only drives the bus while sending a request: GPIO5 is driven low right after the start, until then (during a reset, while flashing and in the download mode) the pin isn't driven, so the pull-down keeps the driver off.
- The system restarts on panics and, via a watchdog, if reading the heat pump gets stuck. As the watchdog keeps running while debugging, the system restarts after halting it for about 40 seconds.

The LED is lit while the heat pump replies.

## Requirements

### Components

| Price | Component | Note |
| - | - | - |
| ~3€ | ESP32-C3 development board | e.g. ESP32-C3 SuperMini with 4 MB flash (RISC-V, built-in USB Serial/JTAG). On Espressif DevKits GPIO8 drives an RGB LED, so the status LED does not work as described |
| ~2€ | RS485 transceiver module for 3.3 V | e.g. with a MAX3485 or SP3485 (with the DE and RE pins). Modules with an automatic direction control work as well (GPIO5 and the pull-down are then not connected). Modules with a MAX485 run on 5 V, their output would damage the ESP32-C3 (not 5 V tolerant) |
| | 10 kΩ resistor | Pull-down from DE/RE to GND, keeps the driver off until the firmware drives GPIO5 (also against the weak internal pull-up of the pin), unless the module already has one |
| | Twisted pair cable with a third wire | From the main board to the Luftpumpe, e.g. a network cable (one pair for A and B, a wire of another pair for GND) |
| ~3€ | USB power supply | The Luftpumpe is powered separately, the RS485 port of the main board has no supply pin |

### Wiring

| Peripheral | Pin | Connected to |
| - | - | - |
| UART1 TX | GPIO4 | Transceiver DI pin |
| UART1 RX | GPIO3 | Transceiver RO pin |
| Driver enable | GPIO5 | Transceiver DE and RE pins (connected to each other, with the 10 kΩ pull-down to GND) |
| Status LED | GPIO8 | Onboard LED (active low) |
| USB Serial/JTAG | USB-C | Host PC (configuration, flashing and logging) |

The transceiver is powered from 3V3 and GND. Its A, B and GND are connected to the 3-pole port `CN485` (`GND`, `A`, `B`) of the main board in the outdoor unit, not to the 4-pin port of the display (`CN6`) or the free 4-pin port next to it (`CN8`), both with 12 V. Check the labels on the board, on a similar unit the wires were black (GND), blue (A) and yellow (B). If the heat pump doesn't reply, swap A and B.

Switch off the heat pump at the circuit breaker before opening the outdoor unit and only work on the low voltage headers, have it done by an electrician if in doubt. The Luftpumpe is best placed indoors (e.g. next to the display), the RS485 cable can be long.

## Building and flashing

On Linux, probe-rs needs access to the USB device (see the [probe-rs setup](https://probe.rs/docs/getting-started/probe-setup/)), and the USB serial interface access to `/dev/ttyACM0` (e.g. via the `dialout` group, ModemManager may have to be told to ignore the device). [rustup](https://rustup.rs) installs the toolchain and target pinned in `rust-toolchain.toml`.

The project consists of the chip independent logic in `core`, which is tested on the host, and the `firmware`:

```bash
# Run the tests of the chip independent logic on the host
cargo test

# Build and flash the firmware with probe-rs (`cargo install probe-rs-tools --locked`), showing the logs
cd firmware
cargo run --release
```

Alternatively the firmware can be flashed with [`cargo-espflash`](https://github.com/esp-rs/espflash) (installed with `cargo install cargo-espflash`, run `cargo espflash flash --release --partition-table partitions.csv` in `firmware`), but the logs are only available via probe-rs, as they are transferred via RTT. The custom partition table reserves a `settings` partition for the configuration, so it is required to flash with it. The logs of a running Luftpumpe are shown without flashing by `probe-rs attach --chip esp32c3 ../target/riscv32imc-unknown-none-elf/release/luftpumpe` (in `firmware`).

## Troubleshooting

| Log message | Cause |
| - | - |
| `Could not read the heat pump: No reply from the heat pump` | Check the wiring (A and B swapped, GND, the transceiver's supply, `CN485` and not the display port) and that the heat pump is powered |
| `Could not read the heat pump: Invalid checksum of the response` (or `Incomplete response`, `Unexpected response`, `Error receiving the response`) | The transmission is disturbed: Check the GND wire and use a twisted pair for A and B. Another device sending on the same port causes this as well |
