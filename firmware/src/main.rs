#![no_std]
#![no_main]

use esp_hal as hal;
use defmt::*;
use defmt_rtt as _;
use esp_backtrace as _;

mod modbus;
mod mqtt;
mod network;
mod settings;

esp_bootloader_esp_idf::esp_app_desc!();

/// Timeout of the watchdog, which resets the system if reading the heat pump gets stuck (fed once per
/// poll interval, the margin covers a cycle in which every request runs into its timeouts)
const WATCHDOG_TIMEOUT: hal::time::Duration =
	hal::time::Duration::from_millis(luftpumpe_core::POLL_INTERVAL + luftpumpe_core::registers::MAX_READ_DURATION + 5_000);

/// Main task: Handles the initialization of the peripherals and reads the heat pump in every cycle
#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner)
{
	let config = hal::Config::default();
	let peripherals = hal::init(config);

	// The transceiver only drives the bus while sending a request, so it never blocks the bus (before
	// this, e.g. during a reset or while flashing, the external pull-down keeps the driver off)
	let driver_enable = hal::gpio::Output::new(
		peripherals.GPIO5,
		hal::gpio::Level::Low,
		hal::gpio::OutputConfig::default());

	let timg0 = hal::timer::timg::TimerGroup::new(peripherals.TIMG0);

	esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

	let settings = settings::initialize(spawner, peripherals.USB_DEVICE, peripherals.FLASH);
	let network = network::initialize(spawner, peripherals.WIFI, &settings);
	mqtt::initialize(spawner, network, &settings);

	// The LED indicates that the heat pump replies
	let mut led = hal::gpio::Output::new(
		peripherals.GPIO8,
		hal::gpio::Level::High,
		hal::gpio::OutputConfig::default());

	let uart_config = hal::uart::Config::default()
		.with_baudrate(luftpumpe_core::modbus::BAUD_RATE);
	let uart =
		hal::uart::Uart::new(peripherals.UART1, uart_config)
			.expect("Could not create UART")
			.with_rx(peripherals.GPIO3)
			.with_tx(peripherals.GPIO4)
			.into_async();
	let mut bus = modbus::Bus::new(uart, driver_enable);

	// The watchdog is fed in every cycle and resets the system if it gets stuck
	let mut watchdog = hal::rtc_cntl::Rtc::new(peripherals.RTC_TIMER).rwdt;
	watchdog.set_timeout(hal::rtc_cntl::RwdtStage::Stage0, WATCHDOG_TIMEOUT);
	watchdog.enable();

	let mut ticker = embassy_time::Ticker::every(embassy_time::Duration::from_millis(luftpumpe_core::POLL_INTERVAL));

	loop
	{
		watchdog.feed();

		let readings = bus.read().await;

		match &readings
		{
			Ok(_) =>
			{
				info!("Read the heat pump");
				led.set_low();
			},
			Err(error) =>
			{
				warn!("Could not read the heat pump: {}", error.message());
				led.set_high();
			},
		}

		if let Ok(state) = luftpumpe_core::mqtt::state(readings.as_ref().map_err(luftpumpe_core::modbus::Error::message))
		{
			mqtt::STATE.signal(state);
		}

		ticker.next().await;
	}
}

/// Restart instead of halting on a panic, so the heat pump is monitored again
#[unsafe(no_mangle)]
fn custom_halt() -> !
{
	hal::system::software_reset()
}
