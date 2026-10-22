//! Connects to the configured Wi-Fi network and runs the network stack (configured via DHCP)

use esp_hal as hal;
use defmt::*;
use esp_radio::wifi;
use luftpumpe_core::settings::Settings;

/// Delay before connecting again after a failed connection or a disconnect
const RECONNECT_DELAY: embassy_time::Duration = embassy_time::Duration::from_secs(10);

/// Time after which a connection attempt is given up, in case the driver never reports its result
const CONNECT_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(30);

/// Interval in which the connection state is checked again, in case the driver's disconnect event
/// got lost
const CHECK_INTERVAL: embassy_time::Duration = embassy_time::Duration::from_secs(30);

/// Starts connecting to the configured Wi-Fi network, returning the network stack (none without a
/// configured Wi-Fi network or if the Wi-Fi can't be initialized)
pub(crate) fn initialize
(
	spawner: embassy_executor::Spawner,
	wifi_peripheral: hal::peripherals::WIFI<'static>,
	settings: &Settings,
) -> Option<embassy_net::Stack<'static>>
{
	if settings.wifi_ssid.is_empty()
	{
		info!("No Wi-Fi network configured");

		return None;
	}

	// The Wi-Fi driver requires a heap
	esp_alloc::heap_allocator!(#[hal::ram(reclaimed)] size: 64 * 1024);
	esp_alloc::heap_allocator!(size: 36 * 1024);

	// The lengths of the SSID and password are already limited by the settings
	let authentication =
		match settings.wifi_password.is_empty()
		{
			true => wifi::AuthenticationMethodConfig::Open,
			false => wifi::AuthenticationMethodConfig::WpaWpa2Personal(settings.wifi_password.as_str().try_into().ok()?),
		};
	let config = wifi::Config::Station(
		wifi::sta::StationConfig::default()
			.with_ssid(settings.wifi_ssid.as_str().try_into().ok()?)
			.with_authentication(authentication));
	let controller =
		wifi::WifiController::new(wifi_peripheral, wifi::ControllerConfig::default().with_initial_config(config))
			.inspect_err(|error| error!("Could not initialize the Wi-Fi: {}", error))
			.ok()?;

	// One socket each for DHCP, DNS and the MQTT connection
	static RESOURCES: static_cell::StaticCell<embassy_net::StackResources<3>> = static_cell::StaticCell::new();

	let rng = hal::rng::Rng::new();
	let seed = u64::from(rng.random()) << 32 | u64::from(rng.random());
	let (stack, runner) =
		embassy_net::new(
			wifi::Interface::station(),
			embassy_net::Config::dhcpv4(Default::default()),
			RESOURCES.init(embassy_net::StackResources::new()),
			seed);

	spawner.spawn(connection_task(controller).expect("Could not spawn Wi-Fi connection task"));
	spawner.spawn(network_task(runner).expect("Could not spawn network task"));

	Some(stack)
}

/// Connection task: Keeps the Wi-Fi connection up
#[embassy_executor::task]
async fn connection_task(mut controller: wifi::WifiController<'static>)
{
	loop
	{
		match embassy_time::with_timeout(CONNECT_TIMEOUT, controller.connect_async()).await
		{
			Ok(Ok(_)) =>
			{
				info!("Connected to the Wi-Fi network");

				// Only a timeout continues waiting, a disconnect (or a lost disconnect event, as the
				// connection state is checked first) reconnects
				while embassy_time::with_timeout(CHECK_INTERVAL, controller.wait_for_disconnect_async()).await.is_err() {}

				warn!("Disconnected from the Wi-Fi network");
			},
			Ok(Err(error)) => warn!("Could not connect to the Wi-Fi network: {}", error),
			Err(_) =>
			{
				warn!("Could not connect to the Wi-Fi network: Timeout");

				// Cancels the connection attempt
				let _ = embassy_time::with_timeout(CONNECT_TIMEOUT, controller.disconnect_async()).await;
			},
		}

		embassy_time::Timer::after(RECONNECT_DELAY).await;
	}
}

/// Network task: Runs the network stack
#[embassy_executor::task]
async fn network_task(mut runner: embassy_net::Runner<'static, wifi::Interface>)
{
	runner.run().await
}
