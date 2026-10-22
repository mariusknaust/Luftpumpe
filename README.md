# Luftpumpe

Pumps the data out of an air-to-water heat pump (German: Luft-Wasser-Wärmepumpe).

## About

The display of the heat pump only shows the current values and offers a single timer. The Luftpumpe reads the values of the heat pump (temperatures, compressor, heaters, valves, faults and the settings) via the free RS485 port of its main board and publishes them via MQTT, to record and analyze them (e.g. how often the electric heater runs). Steering the heat pump (e.g. a better heating curve or separate schedules for heating and hot water) is meant to follow later, built on top of it.

It was created for the Schönknecht & Busch SB-WP-32-8, a rebadged Power World PW030-DKZLRS-A (Easylife series, main board CC1010B), but should work with other Power World heat pumps using the same protocol (PW58321) and probably with other heat pumps on the same main board.

## How it works

- Every 30 seconds, the registers of the main board are read with 3 requests (Modbus RTU, function 03H): The control flags, the mode and the first defrost parameters (8 registers), the system parameters P01 to F1 (116 registers) and the status (43 registers). The requests are 250 ms apart, the main board requires more than 200 ms.
- The Luftpumpe only reads, it never writes a register: The main board saves every written register to its flash, which wears out. The display keeps working as before, it is connected to another port of the main board.
- The transceiver only drives the bus while sending a request: GPIO5 is driven low right after the start, until then (during a reset, while flashing and in the download mode) the pin isn't driven, so the pull-down keeps the driver off.
- After every cycle, the read values (or the error) are published (see [Monitoring](#monitoring)).
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

The transceiver is powered from 3V3 and GND. Its A, B and GND are connected to the 3-pin header `CN485` (`A`, `B`, `GND`) of the main board in the outdoor unit, not to the 4-pin port of the display (`CN6`, with 12 V). Check the labels on the board, on a similar unit the wires were black (GND), blue (A) and yellow (B). If the heat pump doesn't reply, swap A and B.

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

## Configuration

Connect to the USB serial interface (e.g. `picocom --echo /dev/ttyACM0`) and enter a setting as `name=value`, terminated by Enter. The settings are persisted to the flash and take effect after a restart (press the reset button or reconnect the power, the console has to be reconnected afterwards). Entering only the name of a setting (e.g. `broker`) shows its saved value as `name=value` (also the passwords, in plain text).

| Setting | Value |
| - | - |
| `wifi_ssid` | The Wi-Fi network to connect to (2.4 GHz, WPA/WPA2 or open), Wi-Fi is disabled without one |
| `wifi_password` | The password of the Wi-Fi network (8 to 64 characters), empty for an open network |
| `broker` | The IPv4 address or host name (resolved via the DNS server of the network, not via mDNS like `raspberrypi.local`) of the MQTT broker the state is published to (see [Monitoring](#monitoring)), nothing is published without one |
| `broker_user`, `broker_password` | The login for the MQTT broker, without a user name no login is used |

```text
wifi_ssid=My Network
wifi_password=my secret
broker=192.168.1.10
broker_user=luftpumpe
broker_password=secret
```

Spaces are part of the `wifi_ssid`, `wifi_password`, `broker_user` and `broker_password` values (also after the `=`). The passwords are stored unencrypted in the flash, the MQTT login is sent unencrypted. The Luftpumpe gets its address via DHCP and reconnects after the network was lost.

The console has no prompt, every non-empty line is answered (with a confirmation or an error). It doesn't support line editing (e.g. backspace), enter a mistyped line again instead.

## Monitoring

With a Wi-Fi network and an MQTT broker configured, the state is published after every cycle to the topic `luftpumpe/state` of the broker (MQTT 3.1.1, port 1883, optionally with a login), as a JSON object of the raw register values (shortened):

```json
{"power":1,"powerful":0,"silent":0,"heating_curve":0,"vacation":0,"mode":3,"p01":2,"p02":5,"p03":50,...,"p26":0,"vacation_setpoint":25,"p27":30,...,"defrost":0,"compressor":1,...,"space_heater":0,"three_way_valve":0,"tank_heater":0,"water_pump":1,...,"faults":[0,0,0,0,0,0,0],"inverter_faults":[0,0],"return_water":336,"tank":96,"ambient":14,...}
{"error":"No reply from the heat pump"}
```

| Fields | Meaning |
| - | - |
| `power`, `powerful`, `silent`, `heating_curve` (P22), `vacation` | The control flags (0 or 1) |
| `mode` | 0: Hot water, 1: Heating, 2: Cooling, 3: Hot water and heating, 4: Hot water and cooling |
| `p01` to `p05`, `p08` to `p21`, `p23`, `p24`, `p26`, `p27`, `vacation_setpoint`, `f01` | The system parameters as shown on the display (°C, Hz, minutes, days; `p18` the hour, `p24` in steps of 0.1, `f01` the model selection 1 to 4). P06, P07 and P25 aren't published, the protocol doesn't list their registers |
| `defrost`, `compressor`, `fan`, `four_way_valve`, `chassis_heater`, `space_heater`, `three_way_valve`, `tank_heater`, `water_pump`, `crankcase_heater`, `antifreeze_primary`, `antifreeze_secondary` | Whether the output or function is active (0 or 1), `space_heater` is the electric heater of the heating and `tank_heater` the one of the hot water tank |
| `faults`, `inverter_faults` | The fault flags 1 to 7 and the inverter faults (bit fields, see the protocol) |
| `return_water`, `tank`, `ambient`, `leaving_water`, `suction`, `outdoor_coil`, `cooling_coil`, `exhaust`, `low_pressure_temperature` | Temperatures, as raw signed register values |
| `expansion_valve_main`, `expansion_valve_auxiliary`, `compressor_frequency`, `target_frequency`, `compressor_current`, `dc_bus_voltage`, `fan_speed`, `pump_speed` | Expansion valves, compressor frequencies and current, DC bus voltage, fan and pump speeds, as raw register values (the protocol gives no units or scaling) |

The protocol is ambiguous about the scaling of the temperatures ("Accuracy 0.1" for the return water, "Accuracy 0.5" for the others, i.e. either a value in 0.1 °C or in steps of 0.5 °C, none for `low_pressure_temperature`) and gives no units for the other status values, so they are published as raw values and scaled by the receiver: Compare them once with the status list of the display (SETTING, STATUS).

Publishing never delays reading the heat pump, states are skipped while the broker can't be reached and the connection is established again.

[docs/raspberry-pi-monitoring.md](docs/raspberry-pi-monitoring.md) adds the Luftpumpe to the monitoring setup of the [Ladekaskade](https://github.com/mariusknaust/Ladekaskade/blob/main/docs/raspberry-pi-monitoring.md) on a Raspberry Pi (Mosquitto, VictoriaMetrics and a small bridge between both).

## Commissioning

Different firmware versions of the main board use different register maps. Before relying on the values, compare them once with the display: The `mode`, `p03` and `p05` with the mode and the parameters (SETTING, SYSTEM PARAMETERS), the temperatures and the other status values with the status list (SETTING, STATUS). If they don't match, the register map of `core/src/registers.rs` has to be adapted to the protocol of the installed firmware.

## Troubleshooting

| Log message | Cause |
| - | - |
| `Could not read the heat pump: No reply from the heat pump` | Check the wiring (A and B swapped, GND, the transceiver's supply, `CN485` and not the display port) and that the heat pump is powered |
| `Could not read the heat pump: Invalid checksum of the response` (or `Incomplete response`, `Unexpected response`, `Error receiving the response`) | The transmission is disturbed: Check the GND wire and use a twisted pair for A and B. Another device sending on the same port causes this as well |
| `Could not read the heat pump: Request refused by the heat pump` | The main board doesn't know the requested registers: Its firmware uses another register map (see [Commissioning](#commissioning)) |
| `Could not connect to the Wi-Fi network` | Check the `wifi_ssid` and `wifi_password` settings (see [Configuration](#configuration)) and the signal strength (antenna) |
| `Not publishing to the MQTT broker` | Check the `broker` setting and whether the broker accepts connections from the network, on `Login refused by the MQTT broker` check `broker_user` and `broker_password` |
| `Error loading settings: Invalid checksum of the stored settings` | Expected on the first start (nothing was saved yet), otherwise the stored settings are corrupted: Enter them again |
| `No valid settings partition found` | The firmware was flashed without the custom partition table (`partitions.csv`) |
