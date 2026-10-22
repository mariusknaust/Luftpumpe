//! Publishes the state of the heat pump to the configured MQTT broker

use defmt::*;
use embedded_io_async::{Read as _, Write as _};
use luftpumpe_core::{mqtt, settings::Settings};

/// The port of the MQTT broker
const PORT: u16 = 1883;

/// The client id the state is published with
const CLIENT_ID: &str = "luftpumpe";

/// The topic the state is published to
const TOPIC: &str = "luftpumpe/state";

/// Time after which an unresponsive connection is closed
const TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(10);

/// Interval of the TCP keep alive probes, which keep the connection from running into the timeout
/// while no state is sent (between the cycles), several probes are sent within the timeout, so a
/// single lost one doesn't close the connection
const KEEP_ALIVE_INTERVAL: embassy_time::Duration = embassy_time::Duration::from_secs(3);

/// Delay before connecting again after the connection failed
const RECONNECT_DELAY: embassy_time::Duration = embassy_time::Duration::from_secs(10);

/// The latest state to publish (a signal and not a channel, so reading the heat pump is never blocked
/// and outdated states are overwritten)
pub(crate) static STATE: embassy_sync::signal::Signal<
	embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, mqtt::State>
		= embassy_sync::signal::Signal::new();

/// Spawns the publishing of the state, if a network and an MQTT broker is configured
pub(crate) fn initialize
(
	spawner: embassy_executor::Spawner,
	network: Option<embassy_net::Stack<'static>>,
	settings: &Settings,
)
{
	let Some(network) = network else { return };

	if settings.mqtt_broker.is_empty()
	{
		info!("No MQTT broker configured");

		return;
	}

	spawner.spawn(mqtt_task(network, settings.clone()).expect("Could not spawn MQTT task"));
}

/// MQTT task: Keeps the connection to the MQTT broker up and publishes the state
#[embassy_executor::task]
async fn mqtt_task(network: embassy_net::Stack<'static>, settings: Settings)
{
	let mut rx_buffer = [0; 64];
	let mut tx_buffer = [0; 2048];

	loop
	{
		let Err(error) = connect_and_publish(network, &settings, &mut rx_buffer, &mut tx_buffer).await;

		warn!("Not publishing to the MQTT broker: {}", error);

		embassy_time::Timer::after(RECONNECT_DELAY).await;
	}
}

/// Connects to the MQTT broker (once the network is up) and publishes the state until an error
/// occurs
async fn connect_and_publish
(
	network: embassy_net::Stack<'static>, settings: &Settings,
	rx_buffer: &mut [u8], tx_buffer: &mut [u8],
) -> Result<core::convert::Infallible, &'static str>
{
	network.wait_config_up().await;

	let address = network.dns_query(&settings.mqtt_broker, embassy_net::dns::DnsQueryType::A).await
		.ok()
		.and_then(|addresses| addresses.first().copied())
		.ok_or("Could not resolve the MQTT broker")?;

	let mut socket = embassy_net::tcp::TcpSocket::new(network, rx_buffer, tx_buffer);
	socket.set_timeout(Some(TIMEOUT));
	socket.set_keep_alive(Some(KEEP_ALIVE_INTERVAL));

	socket.connect((address, PORT)).await.map_err(|_| "Could not connect to the MQTT broker")?;
	socket.write_all(&mqtt::connect(CLIENT_ID, settings.mqtt_login())?).await.map_err(|_| "Could not send to the MQTT broker")?;

	// The socket timeout doesn't apply while the broker acknowledges the keep alive probes, but never
	// replies
	let mut connack = [0; 4];
	embassy_time::with_timeout(TIMEOUT, socket.read_exact(&mut connack)).await
		.map_err(|_| "No reply from the MQTT broker")?
		.map_err(|_| "Could not receive from the MQTT broker")?;
	mqtt::check_connack(&connack)?;

	info!("Connected to the MQTT broker at {}", address);

	// A state signalled while not connected is outdated
	STATE.reset();

	loop
	{
		let state = STATE.wait().await;

		// Also limited, as the socket timeout doesn't apply here either (e.g. a hung broker on a
		// running host)
		embassy_time::with_timeout(TIMEOUT, socket.write_all(&mqtt::publish(TOPIC, state.as_bytes())?)).await
			.map_err(|_| "Timeout sending to the MQTT broker")?
			.map_err(|_| "Could not send to the MQTT broker")?;
	}
}
