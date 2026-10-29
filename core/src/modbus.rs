//! Modbus RTU framing for reading the holding registers (function 03H) of the heat pump's main board,
//! which answers as the server on its free RS485 port (protocol PW58332-2). Nothing is ever written:
//! The main board saves every written register to its flash, which wears out.

/// The values of consecutive registers
pub type Registers = heapless::Vec<u16, MAX_REGISTERS>;

/// Time in ms after which sending a request is given up
pub const SEND_TIMEOUT: u64 = 100;

/// Time in ms after which a response is given up
pub const RESPONSE_TIMEOUT: u64 = 1000;

/// Pause in ms between two requests, the main board requires more than 200 ms between them
pub const REQUEST_PAUSE: u64 = 250;

/// The unit address of the main board (fixed)
pub const UNIT: u8 = 1;

/// The maximal number of registers read with a single request (limited by the main board)
pub const MAX_REGISTERS: usize = 120;

/// The baud rate of the bus (8N1)
pub const BAUD_RATE: u32 = 9600;

/// The function code for reading holding registers
const READ_HOLDING_REGISTERS: u8 = 0x03;

/// The flag of the function code marking an exception response
const EXCEPTION: u8 = 0x80;

/// The checksum of a frame (low byte first)
const CHECKSUM: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_MODBUS);

/// The size of the address, the function code and the checksum around the data of a frame
const FRAME_OVERHEAD: usize = 4;

/// The length of a request: Additionally the start address and the number of registers
const REQUEST_LENGTH: usize = FRAME_OVERHEAD + 4;

/// The longest response: Additionally the byte count before the register values
const MAX_RESPONSE: usize = FRAME_OVERHEAD + 1 + 2 * MAX_REGISTERS;

// The timeouts leave room for the reaction of the main board on top of the transfer (at least as much
// again), the longest response takes about 260 ms
const _: () = core::assert!(REQUEST_PAUSE > 200
	&& 2 * transfer_time(REQUEST_LENGTH) < SEND_TIMEOUT
	&& 2 * transfer_time(MAX_RESPONSE) < RESPONSE_TIMEOUT);

/// An error reading registers
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error
{
	/// The request couldn't be sent
	Send,
	/// The UART reported an error while receiving the response
	Receive,
	/// Nothing was received in reply to the request
	NoReply,
	/// The response ended before it was complete
	Incomplete,
	/// The checksum of the response doesn't match
	Checksum,
	/// The main board refused the request (with the exception code)
	Exception(u8),
	/// The response doesn't match the request
	Unexpected,
}

impl Error
{
	/// The message describing the error
	pub fn message(&self) -> &'static str
	{
		match self
		{
			Error::Send => "Could not send the request",
			Error::Receive => "Error receiving the response",
			Error::NoReply => "No reply from the heat pump",
			Error::Incomplete => "Incomplete response",
			Error::Checksum => "Invalid checksum of the response",
			Error::Exception(_) => "Request refused by the heat pump",
			Error::Unexpected => "Unexpected response",
		}
	}
}

/// Encodes a request reading `count` holding registers starting at the address `start`
pub fn read_request(start: u16, count: u16) -> [u8; REQUEST_LENGTH]
{
	let mut request = [0; REQUEST_LENGTH];
	let [start_high, start_low] = start.to_be_bytes();
	let [count_high, count_low] = count.to_be_bytes();

	request[.. 6].copy_from_slice(&[UNIT, READ_HOLDING_REGISTERS, start_high, start_low, count_high, count_low]);

	let checksum = CHECKSUM.checksum(&request[.. 6]).to_le_bytes();
	request[6 ..].copy_from_slice(&checksum);

	request
}

/// Collects the received bytes of the response to a `read_request`, until it is complete
pub struct Response
{
	/// The number of requested registers
	count: usize,
	/// The bytes of the response received so far
	frame: heapless::Vec<u8, MAX_RESPONSE>,
}

impl Response
{
	/// Starts collecting the response to a request of `count` registers (at most `MAX_REGISTERS`)
	pub fn new(count: u16) -> Self
	{
		Self { count: usize::from(count), frame: heapless::Vec::new() }
	}

	/// Whether nothing was received yet (distinguishing a missing from an incomplete reply)
	pub fn is_empty(&self) -> bool
	{
		self.frame.is_empty()
	}

	/// Pushes received bytes, returning the register values (or the error) once the response is
	/// complete. Bytes beyond the end of the response are an error, as the response then doesn't
	/// belong to the request.
	pub fn push(&mut self, data: &[u8]) -> Option<Result<Registers, Error>>
	{
		if self.frame.extend_from_slice(data).is_err()
		{
			return Some(Err(Error::Unexpected));
		}

		let length = self.expected_length()?;

		match self.frame.len().cmp(&length)
		{
			core::cmp::Ordering::Less => None,
			core::cmp::Ordering::Equal => Some(self.decode()),
			core::cmp::Ordering::Greater => Some(Err(Error::Unexpected)),
		}
	}

	/// The length of the complete response, once its function code was received (an exception
	/// response only holds the exception code)
	fn expected_length(&self) -> Option<usize>
	{
		let function = *self.frame.get(1)?;

		Some(FRAME_OVERHEAD + if function & EXCEPTION != 0 { 1 } else { 1 + 2 * self.count })
	}

	/// Decodes the complete response
	fn decode(&self) -> Result<Registers, Error>
	{
		let (content, checksum) = self.frame.split_at(self.frame.len() - 2);

		if CHECKSUM.checksum(content).to_le_bytes() != checksum
		{
			return Err(Error::Checksum);
		}

		match content
		{
			[UNIT, function, code] if *function == READ_HOLDING_REGISTERS | EXCEPTION => Err(Error::Exception(*code)),
			[UNIT, READ_HOLDING_REGISTERS, byte_count, values @ ..] if usize::from(*byte_count) == values.len() =>
				Ok(values.as_chunks().0.iter().copied().map(u16::from_be_bytes).collect()),
			_ => Err(Error::Unexpected),
		}
	}
}

/// The time in ms a frame of the given length takes on the bus (10 bits per byte, with the start and
/// the stop bit)
const fn transfer_time(length: usize) -> u64
{
	(length as u64 * 10 * 1000).div_ceil(BAUD_RATE as u64)
}

#[cfg(test)]
mod tests
{
	use super::*;

	/// Appends the checksum to a frame
	fn with_checksum(content: &[u8]) -> Vec<u8>
	{
		[content, &CHECKSUM.checksum(content).to_le_bytes()].concat()
	}

	#[test]
	fn checksum()
	{
		// The check value of CRC-16/MODBUS and a well known request (reading 1 register at 0 of
		// unit 1)
		assert_eq!(CHECKSUM.checksum(b"123456789"), 0x4b37);
		assert_eq!(read_request(0x0000, 1), [0x01, 0x03, 0x00, 0x00, 0x00, 0x01, 0x84, 0x0a]);
	}

	#[test]
	fn requests()
	{
		assert_eq!(read_request(0x0000, 5), with_checksum(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x05])[..]);
		assert_eq!(read_request(0x8000, 43), with_checksum(&[0x01, 0x03, 0x80, 0x00, 0x00, 0x2b])[..]);
	}

	#[test]
	fn responses()
	{
		let mut response = Response::new(2);

		assert!(response.is_empty());
		assert_eq!(response.push(&[0x01]), None);
		assert!(!response.is_empty());

		// Split across several receptions, also within the checksum
		let frame = with_checksum(&[0x01, 0x03, 0x04, 0x01, 0x4f, 0xff, 0xec]);
		assert_eq!(response.push(&frame[1 .. 5]), None);
		assert_eq!(response.push(&frame[5 .. 8]), None);
		assert_eq!(response.push(&frame[8 ..]), Some(Ok(Registers::from_slice(&[335, 0xffec]).unwrap())));
	}

	#[test]
	fn longest_response()
	{
		let values = [0x12; 2 * MAX_REGISTERS];
		let frame = with_checksum(&[&[0x01, 0x03, 2 * MAX_REGISTERS as u8], &values[..]].concat());

		assert_eq!(Response::new(MAX_REGISTERS as u16).push(&frame),
			Some(Ok(Registers::from_slice(&[0x1212; MAX_REGISTERS]).unwrap())));
	}

	#[test]
	fn exception_response()
	{
		assert_eq!(Response::new(5).push(&with_checksum(&[0x01, 0x83, 0x02])), Some(Err(Error::Exception(2))));
	}

	#[test]
	fn invalid_responses()
	{
		let mut frame = with_checksum(&[0x01, 0x03, 0x02, 0x00, 0x01]);

		// Too many bytes
		assert_eq!(Response::new(1).push(&[&frame[..], &[0x00]].concat()), Some(Err(Error::Unexpected)));
		assert_eq!(Response::new(1).push(&[0x01; MAX_RESPONSE + 1]), Some(Err(Error::Unexpected)));

		// Another unit, function or byte count (with a valid checksum)
		assert_eq!(Response::new(1).push(&with_checksum(&[0x02, 0x03, 0x02, 0x00, 0x01])), Some(Err(Error::Unexpected)));
		assert_eq!(Response::new(1).push(&with_checksum(&[0x01, 0x04, 0x02, 0x00, 0x01])), Some(Err(Error::Unexpected)));
		assert_eq!(Response::new(1).push(&with_checksum(&[0x01, 0x03, 0x01, 0x00, 0x01])), Some(Err(Error::Unexpected)));
		assert_eq!(Response::new(1).push(&with_checksum(&[0x01, 0x84, 0x02])), Some(Err(Error::Unexpected)));

		frame[3] ^= 0x01;
		assert_eq!(Response::new(1).push(&frame), Some(Err(Error::Checksum)));
	}
}
