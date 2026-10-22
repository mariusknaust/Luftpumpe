//! The published state and the MQTT packets (version 3.1.1) required for publishing it

use core::fmt::Write as _;

use crate::registers::{Kind, Readings, FIELDS};

/// A packet sent to the MQTT broker
pub type Packet = heapless::Vec<u8, 1600>;

/// The published state, formatted as JSON
pub type State = heapless::String<1536>;

/// The keep alive of the connection in seconds
const KEEP_ALIVE_SECONDS: u16 = 60;

// A state is published in every cycle (also on errors), which has to keep the connection alive
const _: () = core::assert!(crate::POLL_INTERVAL < KEEP_ALIVE_SECONDS as u64 * 1000);

/// Formats the state: The published values of the registers (see `registers::FIELDS`), or the error
/// reading them (which must not require escaping)
pub fn state(readings: Result<&Readings, &str>) -> Result<State, core::fmt::Error>
{
	let mut state = State::new();

	let readings =
		match readings
		{
			Ok(readings) => readings,
			Err(error) =>
			{
				write!(state, r#"{{"error":"{error}"}}"#)?;

				return Ok(state);
			},
		};

	for (index, field) in FIELDS.iter().enumerate()
	{
		write!(state, r#"{}"{}":"#, if index == 0 { "{" } else { "," }, field.name)?;

		// Every field is read (see the tests of the registers), a missing one is a mistake and fails the
		// formatting
		let value = |address| readings.value(address).ok_or(core::fmt::Error);

		match field.kind
		{
			Kind::Unsigned => write!(state, "{}", value(field.address)?)?,
			Kind::Signed => write!(state, "{}", value(field.address)?.cast_signed())?,
			Kind::Bit(bit) => write!(state, "{}", value(field.address)? >> bit & 1)?,
			Kind::List(count) =>
			{
				for offset in 0 .. count
				{
					write!(state, "{}{}", if offset == 0 { "[" } else { "," }, value(field.address + offset)?)?;
				}

				write!(state, "]")?;
			},
		}
	}

	write!(state, "}}")?;

	Ok(state)
}

/// Encodes a CONNECT packet with a clean session and a keep alive of 60 seconds, so the broker
/// closes dead connections (the state is published in every cycle once the network is up, so no
/// pings are required), optionally with a login (user name and password)
pub fn connect(client_id: &str, login: Option<(&str, &str)>) -> Result<Packet, &'static str>
{
	const CLEAN_SESSION: u8 = 0x02;
	const USER_NAME_AND_PASSWORD: u8 = 0xc0;

	let flags = CLEAN_SESSION | if login.is_some() { USER_NAME_AND_PASSWORD } else { 0 };
	let (user_name, password) = login.unwrap_or_default();
	let parts: [&[u8]; 9] =
		[
			b"\x00\x04MQTT\x04", &[flags], &KEEP_ALIVE_SECONDS.to_be_bytes(),
			&length(client_id)?, client_id.as_bytes(),
			&length(user_name)?, user_name.as_bytes(), &length(password)?, password.as_bytes(),
		];

	// Without a login, the user name and password are omitted
	packet(0x10, &parts[.. if login.is_some() { 9 } else { 5 }])
}

/// Checks the CONNACK packet received in reply to the CONNECT packet
pub fn check_connack(packet: &[u8; 4]) -> Result<(), &'static str>
{
	match packet
	{
		[0x20, 0x02, _, 0x00] => Ok(()),
		// Bad user name or password, not authorized
		[0x20, 0x02, _, 0x04 | 0x05] => Err("Login refused by the MQTT broker"),
		[0x20, 0x02, _, _] => Err("Connection refused by the MQTT broker"),
		_ => Err("Unexpected reply of the MQTT broker"),
	}
}

/// Encodes a PUBLISH packet (QoS 0, not retained)
pub fn publish(topic: &str, payload: &[u8]) -> Result<Packet, &'static str>
{
	packet(0x30, &[&length(topic)?, topic.as_bytes(), payload])
}

/// The length prefix of a string
fn length(string: &str) -> Result<[u8; 2], &'static str>
{
	u16::try_from(string.len()).map(u16::to_be_bytes).map_err(|_| "String too long")
}

/// Encodes a packet from its type and the parts of its content
fn packet(packet_type: u8, parts: &[&[u8]]) -> Result<Packet, &'static str>
{
	let mut packet = Packet::new();
	let mut remaining_length = parts.iter().map(|part| part.len()).sum::<usize>();

	packet.push(packet_type).map_err(|_| "Packet too long")?;

	// The remaining length is encoded with 7 bits per byte, the highest bit marks a following byte
	loop
	{
		let byte = (remaining_length % 128) as u8;
		remaining_length /= 128;

		packet.push(if remaining_length > 0 { byte | 0x80 } else { byte }).map_err(|_| "Packet too long")?;

		if remaining_length == 0
		{
			break;
		}
	}

	parts.iter()
		.try_for_each(|part| packet.extend_from_slice(part))
		.map_err(|_| "Packet too long")?;

	Ok(packet)
}

#[cfg(test)]
mod tests
{
	use super::*;

	use crate::modbus::Registers;
	use crate::registers::BLOCKS;

	/// Readings with the given registers set and all others 0
	fn readings(values: &[(u16, u16)]) -> Readings
	{
		let mut blocks = BLOCKS.map(|block| Registers::from_iter(core::iter::repeat_n(0, usize::from(block.count))));

		for &(address, value) in values
		{
			let (index, block) = BLOCKS.iter().enumerate()
				.find(|(_, block)| (block.start .. block.start + block.count).contains(&address))
				.unwrap();

			blocks[index][usize::from(address - block.start)] = value;
		}

		Readings::new(blocks).unwrap()
	}

	#[test]
	fn states()
	{
		let state = state(Ok(&readings(&[(0x0000, 0x0001), (0x0001, 0x0040), (0x0004, 3), (0x00ed, 0xfffb),
			(0x8005, 0x00a0), (0x8007, 0x0021), (0x800d, 0x0040), (0x800e, 335), (0x8011, 0xffec)]))).unwrap();

		assert!(state.starts_with(r#"{"power":1,"powerful":0,"silent":0,"heating_curve":1,"vacation":0,"mode":3,"p01":0,"#), "{state}");
		assert!(state.contains(r#","p26":-5,"#), "{state}");
		assert!(state.contains(r#","chassis_heater":0,"space_heater":1,"three_way_valve":0,"tank_heater":1,"#), "{state}");
		assert!(state.contains(r#","faults":[33,0,0,0,0,0,64],"inverter_faults":[0,0],"#), "{state}");
		assert!(state.contains(r#","return_water":335,"tank":0,"ambient":-20,"#), "{state}");
		assert!(state.ends_with(r#","pump_speed":0}"#), "{state}");
	}

	#[test]
	fn error_state()
	{
		assert_eq!(state(Err("No reply from the heat pump")).unwrap(), r#"{"error":"No reply from the heat pump"}"#);
	}

	#[test]
	fn longest_state_fits()
	{
		// 0x8000 has the most digits as a signed (-32768) and as an unsigned (32768) value, a bit
		// always has one
		let blocks = BLOCKS.map(|block| Registers::from_iter(core::iter::repeat_n(0x8000, usize::from(block.count))));

		assert!(state(Ok(&Readings::new(blocks).unwrap())).is_ok());
		assert!(state(Err(&"x".repeat(80))).is_ok());

		// Also as the payload of a packet (State holds up to 1536 bytes)
		assert!(publish("luftpumpe/state", &[0; 1536]).is_ok());
	}

	#[test]
	fn connect_packet()
	{
		assert_eq!(connect("luftpumpe", None).unwrap(),
			b"\x10\x15\x00\x04MQTT\x04\x02\x00\x3c\x00\x09luftpumpe");
		assert_eq!(connect("luftpumpe", Some(("u", "pw"))).unwrap(),
			b"\x10\x1c\x00\x04MQTT\x04\xc2\x00\x3c\x00\x09luftpumpe\x00\x01u\x00\x02pw");
	}

	#[test]
	fn longest_connect_fits()
	{
		assert!(connect("luftpumpe", Some((&"x".repeat(32), &"x".repeat(64)))).is_ok());
	}

	#[test]
	fn connack()
	{
		assert_eq!(check_connack(&[0x20, 0x02, 0x00, 0x00]), Ok(()));
		assert_eq!(check_connack(&[0x20, 0x02, 0x00, 0x04]), Err("Login refused by the MQTT broker"));
		assert_eq!(check_connack(&[0x20, 0x02, 0x00, 0x05]), Err("Login refused by the MQTT broker"));
		assert_eq!(check_connack(&[0x20, 0x02, 0x00, 0x02]), Err("Connection refused by the MQTT broker"));
		assert_eq!(check_connack(&[0x30, 0x02, 0x00, 0x00]), Err("Unexpected reply of the MQTT broker"));
	}

	#[test]
	fn publish_packet()
	{
		assert_eq!(publish("a/b", b"{}").unwrap(), b"\x30\x07\x00\x03a/b{}");

		// Remaining lengths above 127 require two bytes
		let packet = publish("t", &[0; 200]).unwrap();
		assert_eq!(packet[.. 3], [0x30, 0xcb, 0x01]);
		assert_eq!(packet.len(), 3 + 203);

		assert!(publish("t", &[0; 1600]).is_err());
	}
}
