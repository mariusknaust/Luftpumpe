//! Reads the registers of the heat pump's main board via the RS485 transceiver on the UART (Modbus
//! RTU, only reading)

use esp_hal as hal;
use luftpumpe_core::modbus::{read_request, Error, Registers, Response};
use luftpumpe_core::registers::{Readings, BLOCKS};

/// Time after which a response is given up
const RESPONSE_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_millis(luftpumpe_core::modbus::RESPONSE_TIMEOUT);

/// Time after which sending a request is given up
const SEND_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_millis(luftpumpe_core::modbus::SEND_TIMEOUT);

/// Pause between two requests
const REQUEST_PAUSE: embassy_time::Duration = embassy_time::Duration::from_millis(luftpumpe_core::modbus::REQUEST_PAUSE);

/// The UART to the transceiver and the pin enabling its driver
pub(crate) struct Bus
{
	uart: hal::uart::Uart<'static, hal::Async>,
	/// High while sending, low (receiving) otherwise
	driver_enable: hal::gpio::Output<'static>,
}

impl Bus
{
	/// Takes the UART (`modbus::BAUD_RATE`, 8N1) and the driver enable pin (low)
	pub(crate) fn new(uart: hal::uart::Uart<'static, hal::Async>, driver_enable: hal::gpio::Output<'static>) -> Self
	{
		Self { uart, driver_enable }
	}

	/// Reads all blocks of registers
	pub(crate) async fn read(&mut self) -> Result<Readings, Error>
	{
		let mut blocks: [Registers; BLOCKS.len()] = Default::default();

		for (index, block) in BLOCKS.iter().enumerate()
		{
			if index > 0
			{
				embassy_time::Timer::after(REQUEST_PAUSE).await;
			}

			blocks[index] = self.read_block(block.start, block.count).await?;
		}

		Readings::new(blocks).ok_or(Error::Unexpected)
	}

	/// Reads `count` registers starting at the address `start`
	async fn read_block(&mut self, start: u16, count: u16) -> Result<Registers, Error>
	{
		// Bytes received since the last response (e.g. a late reply or noise) would corrupt this one,
		// the attempts are limited in case an error is reported again
		let mut discarded = [0; 32];

		for _ in 0..8
		{
			if self.uart.read_buffered(&mut discarded).is_ok_and(|length| length == 0)
			{
				break;
			}
		}

		self.send(&read_request(start, count)).await?;

		let mut response = Response::new(count);
		let mut received = [0; 64];
		let uart = &mut self.uart;

		embassy_time::with_timeout(RESPONSE_TIMEOUT, async
		{
			loop
			{
				let length =
					match uart.read_async(&mut received).await
					{
						Ok(length) => length,
						// A glitch while the bus turns around, before the response starts (the driver
						// cleared the error and discarded the byte)
						Err(_) if response.is_empty() => continue,
						Err(_) => return Err(Error::Receive),
					};

				if let Some(result) = response.push(&received[.. length])
				{
					return result;
				}
			}
		}).await
			.unwrap_or_else(|_| Err(if response.is_empty() { Error::NoReply } else { Error::Incomplete }))
	}

	/// Sends a request, enabling the driver of the transceiver only until it is sent
	async fn send(&mut self, request: &[u8]) -> Result<(), Error>
	{
		self.driver_enable.set_high();

		let uart = &mut self.uart;
		let sent = embassy_time::with_timeout(SEND_TIMEOUT, async
		{
			let mut remaining = request;

			while !remaining.is_empty()
			{
				let length = uart.write_async(remaining).await.map_err(|_| Error::Send)?;
				remaining = &remaining[length ..];
			}

			// Waits until the last byte left the UART, so the driver isn't disabled too early
			uart.flush_async().await.map_err(|_| Error::Send)
		}).await;

		self.driver_enable.set_low();

		sent.unwrap_or(Err(Error::Send))
	}
}
