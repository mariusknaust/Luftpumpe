# Luftpumpe monitoring on a Raspberry Pi

The Luftpumpe uses the same monitoring stack as the Ladekaskade (Mosquitto, VictoriaMetrics and a bridge on a diskless Alpine Raspberry Pi). Set it up first as described in the [Ladekaskade monitoring guide](https://github.com/mariusknaust/Ladekaskade/blob/main/docs/raspberry-pi-monitoring.md) (steps 1 to 4, the packages, the data HDD, Mosquitto and VictoriaMetrics), this guide only adds the Luftpumpe to it. Without a Ladekaskade, create the password file in step 3 with the login of the Luftpumpe instead (`mosquitto_passwd -c /etc/mosquitto/passwd luftpumpe`, with the same `chown` and `chmod`), skip section 1 here and include the scripts once before persisting the configuration in section 4 (`lbu include /usr/local/bin`, as in step 7 there).

All commands are run as root on the Pi. Values to adapt are marked with `<...>`.

## 1. Mosquitto

Add a login for the Luftpumpe to the existing password file (you are asked for the password). Without `-c`, which would replace the file and with it the other logins:

```sh
mosquitto_passwd /etc/mosquitto/passwd luftpumpe
rc-service mosquitto restart
```

Mosquitto only reads the password file when starting, the other devices reconnect by themselves after the restart.

## 2. Bridge from MQTT to VictoriaMetrics

The bridge writes every state as one line in the InfluxDB line protocol to VictoriaMetrics, e.g. `{"power":1,...,"faults":[0,0,0,0,0,0,0],...,"return_water":336,...}` becomes `luftpumpe error=0,power=1,...,return_water=336,...,fault_1=0,...`. VictoriaMetrics stores these as the metrics `luftpumpe_power`, `luftpumpe_return_water` etc. (the raw register values as published, see the [README](../README.md#monitoring)). Every state also stores `luftpumpe_error`, 1 if the heat pump couldn't be read and 0 otherwise (the other values then show as a gap).

The script `/usr/local/bin/luftpumpe-bridge`, the same as the Ladekaskade's apart from the conversion:

```sh
#!/bin/sh
# Writes the states published by the Luftpumpe into VictoriaMetrics

# Stops the whole pipeline when the service is stopped (otherwise only this shell would exit and
# the pipeline would keep running next to the restarted one)
trap 'trap - TERM; kill 0' TERM

mosquitto_sub -h 127.0.0.1 -p 1884 -t luftpumpe/state \
	| jq --unbuffered -r '"luftpumpe " + (if .error then "error=1" else
		(["error=0"] + [to_entries[] | select(.value | type == "number") | "\(.key)=\(.value)"]
		+ [.faults | to_entries[] | "fault_\(.key + 1)=\(.value)"]
		+ [.inverter_faults | to_entries[] | "inverter_fault_\(.key + 1)=\(.value)"]
		| join(",")) end)' \
	| while read -r line
	do
		curl -s -m 10 --data-binary "$line" http://127.0.0.1:8428/write
	done &

# Waiting in the background allows the trap to run. If the pipeline exits (Mosquitto unreachable
# when starting, mosquitto_sub reconnects by itself later on, or malformed data), the supervisor
# restarts the bridge.
wait
```

The service `/etc/init.d/luftpumpe-bridge` (restarted by the supervisor if it exits):

```sh
#!/sbin/openrc-run

description="Writes the Luftpumpe states from MQTT into VictoriaMetrics"

command="/usr/local/bin/luftpumpe-bridge"
command_user="nobody:nogroup"
supervisor=supervise-daemon
respawn_delay=10
respawn_max=0

depend()
{
	need mosquitto victoria-metrics
}
```

```sh
chmod +x /usr/local/bin/luftpumpe-bridge /etc/init.d/luftpumpe-bridge
rc-update add luftpumpe-bridge default
rc-service luftpumpe-bridge start
```

## 3. Dashboard

vmui shows dashboards from JSON files in `/etc/victoria-metrics/dashboards` (see the Ladekaskade guide). The dashboard below shows the heating against the target of the heating curve, the hot water, the outputs, and statistics per day and per hour. The temperatures are scaled as the protocol suggests (the return water in 0.1 °C, the others in steps of 0.5 °C, see the [README](../README.md#monitoring)): Compare them once with the status list of the display and adapt the divisors (`/ 10`, `/ 2`) if they don't match. The daily statistics read a whole day for every point, choose a larger step in vmui for ranges of weeks. Create `/etc/victoria-metrics/dashboards/luftpumpe.json`:

```json
{
  "title": "Luftpumpe",
  "rows": [
    {
      "title": "Temperatures",
      "panels": [
        {
          "title": "Heating",
          "description": "The heat pump regulates the heating on the return water: With the heating curve enabled (P22) towards its target, 20 + P24 / 10 * (P23 - outdoor), limited to 20 to 60 °C (shown only while the curve is enabled), otherwise towards P05. The leaving water is the flow to the heating. Gaps are failed readings",
          "unit": "°C",
          "width": 12,
          "expr": [
            "luftpumpe_return_water / 10",
            "luftpumpe_leaving_water / 2",
            "clamp(20 + luftpumpe_p24 / 10 * (luftpumpe_p23 - luftpumpe_ambient / 2), 20, 60) and luftpumpe_heating_curve == 1",
            "luftpumpe_p05 and luftpumpe_heating_curve == 0",
            "luftpumpe_ambient / 2"
          ],
          "alias": ["Return water", "Leaving water", "Target (heating curve)", "Target (P05)", "Outdoor"]
        },
        {
          "title": "Hot water",
          "description": "The tank and its set temperature (P03), it is heated again once it is P02 below",
          "unit": "°C",
          "width": 6,
          "expr": [
            "luftpumpe_tank / 2",
            "luftpumpe_p03"
          ],
          "alias": ["Tank", "Target (P03)"]
        },
        {
          "title": "Compressor",
          "description": "The actual and the target frequency of the compressor (raw values, the protocol gives no unit)",
          "width": 6,
          "expr": [
            "luftpumpe_compressor_frequency",
            "luftpumpe_target_frequency"
          ],
          "alias": ["Actual", "Target"]
        }
      ]
    },
    {
      "title": "Outputs",
      "panels": [
        {
          "title": "Outputs",
          "description": "1 while the output or function is active. The 3-way valve is switched to the hot water tank while the hot water is heated",
          "width": 12,
          "expr": [
            "luftpumpe_compressor",
            "luftpumpe_defrost",
            "luftpumpe_three_way_valve",
            "luftpumpe_space_heater",
            "luftpumpe_tank_heater",
            "luftpumpe_water_pump"
          ],
          "alias": ["Compressor", "Defrost", "3-way valve (hot water)", "Electric heater (heating)", "Electric heater (tank)", "Water pump"]
        }
      ]
    },
    {
      "title": "Statistics",
      "panels": [
        {
          "title": "Running hours per day",
          "description": "Of the time the heat pump was read. Every hour of an electric heater costs about three to four times the electricity of the same heat from the compressor",
          "unit": "h",
          "width": 4,
          "expr": [
            "avg_over_time(luftpumpe_compressor[1d]) * 24",
            "avg_over_time(luftpumpe_space_heater[1d]) * 24",
            "avg_over_time(luftpumpe_tank_heater[1d]) * 24"
          ],
          "alias": ["Compressor", "Electric heater (heating)", "Electric heater (tank)"]
        },
        {
          "title": "Hot water hours per day",
          "description": "Hours with the 3-way valve switched to the hot water tank, the heating waits meanwhile (hot water has priority)",
          "unit": "h",
          "width": 4,
          "expr": ["avg_over_time(luftpumpe_three_way_valve[1d]) * 24"],
          "alias": ["Hot water"]
        },
        {
          "title": "Compressor starts per hour",
          "description": "Frequent starts (more than about 3 per hour) wear the compressor and lower the efficiency, e.g. a too small hysteresis (P01) or flow",
          "width": 4,
          "expr": ["increases_over_time(luftpumpe_compressor[1h])"],
          "alias": ["Starts"]
        },
        {
          "title": "Defrosts per day",
          "description": "Together with the average outdoor temperature of the day, most defrosts happen around 0 to 5 °C with a high humidity",
          "width": 4,
          "expr": [
            "increases_over_time(luftpumpe_defrost[1d])",
            "avg_over_time(luftpumpe_ambient[1d]) / 2"
          ],
          "alias": ["Defrosts", "Outdoor (°C)"]
        },
        {
          "title": "Faults",
          "description": "Minutes per hour with a fault flag of the heat pump set (see the protocol for the flags), and minutes per hour the heat pump couldn't be read",
          "unit": "min",
          "width": 4,
          "expr": [
            "avg_over_time(((luftpumpe_fault_1 + luftpumpe_fault_2 + luftpumpe_fault_3 + luftpumpe_fault_4 + luftpumpe_fault_5 + luftpumpe_fault_6 + luftpumpe_fault_7 + luftpumpe_inverter_fault_1 + luftpumpe_inverter_fault_2) > bool 0)[1h:30s]) * 60",
            "avg_over_time(luftpumpe_error[1h]) * 60"
          ],
          "alias": ["Fault", "Not read"]
        }
      ]
    }
  ]
}
```

vmui reads it whenever the dashboards are opened, reload the page after changes.

## 4. Persist the configuration

The script is in `/usr/local/bin`, which is already included for the Ladekaskade, the rest (also the dashboard) is in `/etc`:

```sh
lbu commit -d
```

The Luftpumpe adds about 70 series, with one state every 30 seconds (about 2 samples per second, less than half of the Ladekaskade's with one state per second, most values change rarely and compress well).

## 5. Configure the Luftpumpe

Connect to the USB console of the Luftpumpe (e.g. `picocom --echo /dev/ttyACM0`) and enter (in addition to the Wi-Fi network, see the [README](../README.md#configuration)):

```text
broker=<ip-of-the-pi>
broker_user=luftpumpe
broker_password=<luftpumpe-password>
```

The Pi needs a fixed address for this (e.g. a DHCP reservation in the router), or enter a host name the router resolves as `broker`.

Then restart the Luftpumpe (press the reset button or reconnect the power). A refused login is logged as `Login refused by the MQTT broker` (see the logs in [Building and flashing](../README.md#building-and-flashing)).

## 6. Check

On the Pi, a state arrives every 30 seconds:

```sh
mosquitto_sub -p 1884 -t luftpumpe/state -v
```

The last value written by the bridge (VictoriaMetrics only returns samples older than 30 seconds by default):

```sh
curl -s 'http://127.0.0.1:8428/api/v1/query?query=luftpumpe_return_water'
```

The dashboard is under **Dashboards** in `http://<ip-of-the-pi>:8428/vmui`. Reboot once (`reboot`) and check that it still shows new states afterwards.
