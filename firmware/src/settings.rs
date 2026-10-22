//! Changes the settings via the USB Serial/JTAG console and stores them in the flash

use esp_hal as hal;
use defmt::*;
use luftpumpe_core::settings::{from_stored, to_stored, ReceivedLines, Reply, Settings, STORED_SIZE};

/// Label of the data partition holding the settings (see partitions.csv)
const SETTINGS_PARTITION_LABEL: &str = "settings";

/// Time after which a reply the host doesn't read (e.g. after closing the port) is given up, so the
/// console keeps handling the received lines
const REPLY_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(1);

/// Loads the settings from the flash and spawns the serial interface for changing them
pub(crate) fn initialize
(
	spawner: embassy_executor::Spawner,
	usb_device: hal::peripherals::USB_DEVICE<'static>,
	flash_peripheral: hal::peripherals::FLASH<'static>,
) -> Settings
{
	let mut flash = esp_storage::FlashStorage::new(flash_peripheral);
	let offset = find_settings_partition(&mut flash);

	let settings = offset.ok_or("No valid settings partition found")
		.and_then(|offset|
		{
			let mut stored = [0; STORED_SIZE];

			flash.read(offset, &mut stored).map_err(|_| "Could not read from flash")?;

			from_stored::<Settings>(&stored)
		})
		.inspect_err(|error| warn!("Error loading settings: {}", error))
		.unwrap_or_default();

	spawner.spawn(usb_serial_task(usb_device, flash, offset, settings.clone())
		.expect("Could not spawn usb serial task"));

	settings
}

/// Looks up the offset of the settings partition in the partition table
fn find_settings_partition(flash: &mut esp_storage::FlashStorage<'_>) -> Option<u32>
{
	let mut buffer = [0u8; esp_bootloader_esp_idf::partitions::PARTITION_TABLE_MAX_LEN];
	let partition = esp_bootloader_esp_idf::partitions::read_partition_table(flash, &mut buffer)
		.inspect_err(|error| error!("Could not read partition table: {}", error))
		.ok()?
		.iter()
		.find(|partition| partition.label_as_str() == SETTINGS_PARTITION_LABEL)?;

	Some(partition.offset())
}

/// Saves the settings to the settings partition
fn save(flash: &mut esp_storage::FlashStorage<'static>, offset: Option<u32>, settings: &Settings)
	-> Result<(), &'static str>
{
	let offset = offset.ok_or("No valid settings partition found")?;

	// Erases the affected flash sector before writing
	flash.write(offset, &to_stored(settings)?).map_err(|_| "Could not write to flash")
}

/// USB serial task: Changes and shows the settings, line by line
#[embassy_executor::task]
async fn usb_serial_task
(
	usb_device: hal::peripherals::USB_DEVICE<'static>,
	mut flash: esp_storage::FlashStorage<'static>,
	offset: Option<u32>,
	mut settings: Settings,
)
{
	use embedded_io_async::{Read as _, Write as _};

	let mut usb_serial = hal::usb::usb_serial_jtag::UsbSerialJtag::new(usb_device).into_async();
	let mut packet_buffer = [0u8; 64];
	let mut lines = ReceivedLines::default();

	loop
	{
		let Ok(length) = usb_serial.read(&mut packet_buffer).await;

		for line in lines.push(&packet_buffer[..length])
		{
			// Only kept once saved, so a failed change is neither shown nor saved with the next one
			let mut changed_settings = settings.clone();
			let shown;
			let reply =
				match line.and_then(|line| changed_settings.handle(&line))
				{
					Ok(Reply::Changed) =>
					{
						// Erasing the flash stalls the executor, not while reading the heat pump
						let _lock = crate::modbus::LOCK.lock().await;

						save(&mut flash, offset, &changed_settings)
							.map(|_| settings = changed_settings)
							.map_or_else(|error| error, |_| "Settings saved, changes take effect after a restart")
					},
					Ok(Reply::Shown(value)) =>
					{
						shown = value;

						shown.as_str()
					},
					Err(error) => error,
				};

			let _ = embassy_time::with_timeout(REPLY_TIMEOUT, async
			{
				usb_serial.write_all(reply.as_bytes()).await?;
				usb_serial.write_all(b"\r\n").await?;
				usb_serial.flush().await
			}).await;
		}
	}
}
