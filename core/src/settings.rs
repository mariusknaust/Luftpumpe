//! The settings, their text format (as provided via the serial interface) and their stored format

use core::fmt::Write as _;

/// The settings, which are persisted in the flash
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Settings
{
	/// The Wi-Fi network to connect to, Wi-Fi is disabled without one
	pub wifi_ssid: heapless::String<32>,
	/// The password of the Wi-Fi network, empty for an open network
	pub wifi_password: heapless::String<64>,
}

/// A line shown on the serial interface
pub type Line = heapless::String<96>;

/// The reply to a line handled by `Settings::handle`
#[derive(Debug, PartialEq, Eq)]
pub enum Reply
{
	/// The setting was changed (and has to be saved)
	Changed,
	/// The current value of the setting, as `name=value`
	Shown(Line),
}

/// A setting: Its name, and how it is changed from and shown as text
struct Setting
{
	name: &'static str,
	change: fn(&mut Settings, &str) -> Result<(), &'static str>,
	show: fn(&Settings, &mut Line) -> core::fmt::Result,
}

/// All settings (spaces are significant in the values)
const SETTINGS: [Setting; 2] =
	[
		Setting
		{
			name: "wifi_ssid",
			change: |settings, value| { settings.wifi_ssid = value.try_into().map_err(|_| "SSID too long")?; Ok(()) },
			show: |settings, line| line.write_str(&settings.wifi_ssid),
		},
		Setting
		{
			name: "wifi_password",
			change: |settings, value| { settings.wifi_password = parse_password(value)?; Ok(()) },
			show: |settings, line| line.write_str(&settings.wifi_password),
		},
	];

impl Settings
{
	/// Handles a line of the serial interface: `name=value` changes a setting, `name` shows its
	/// current value
	pub fn handle(&mut self, line: &str) -> Result<Reply, &'static str>
	{
		let (name, value) =
			match line.split_once('=')
			{
				Some((name, value)) => (name, Some(value)),
				None => (line, None),
			};
		let setting = SETTINGS.iter().find(|setting| setting.name == name.trim()).ok_or("Unknown setting")?;

		match value
		{
			Some(value) =>
			{
				(setting.change)(self, value)?;

				Ok(Reply::Changed)
			},
			None =>
			{
				let mut shown = Line::new();

				write!(shown, "{}=", setting.name).and_then(|_| (setting.show)(self, &mut shown))
					.map_err(|_| "Value too long to be shown")?;

				Ok(Reply::Shown(shown))
			},
		}
	}
}

/// Parses a Wi-Fi password, which has to have 8 to 64 characters for WPA (or none for an open
/// network)
fn parse_password(value: &str) -> Result<heapless::String<64>, &'static str>
{
	match value.len()
	{
		0 | 8 ..= 64 => Ok(value.try_into().unwrap_or_default()),
		_ => Err("Wi-Fi password must have 8 to 64 characters"),
	}
}

/// A line received via the serial interface
pub type ReceivedLine = heapless::String<128>;

/// The bytes of a line received via the serial interface
type ReceivedBytes = heapless::Vec<u8, 128>;

/// Splits the data received via the serial interface into lines (terminated by CR, LF or CRLF)
pub struct ReceivedLines
{
	/// The bytes of the incomplete line received so far, or the error while receiving it
	line: Result<ReceivedBytes, &'static str>,
}

impl Default for ReceivedLines
{
	fn default() -> Self
	{
		Self { line: Ok(ReceivedBytes::new()) }
	}
}

impl ReceivedLines
{
	/// Pushes received data, returning the completed lines (or the errors while receiving them),
	/// empty lines are ignored. All lines have to be taken, otherwise the rest of the data is
	/// discarded.
	pub fn push<'a>(&'a mut self, mut data: &'a [u8]) -> impl Iterator<Item = Result<ReceivedLine, &'static str>> + 'a
	{
		core::iter::from_fn(move ||
		{
			// The lines are split as bytes (UTF-8 doesn't contain CR or LF within a character) and
			// decoded when complete, so a character split across two transmissions is decoded as well
			while let Some(end) = data.iter().position(|byte| matches!(byte, b'\r' | b'\n'))
			{
				let line = core::mem::replace(&mut self.line, Ok(ReceivedBytes::new()))
					.and_then(|line| extend_line(line, &data[.. end]))
					.and_then(|line| ReceivedLine::from_utf8(line).map_err(|_| "Invalid data provided"));

				data = &data[end + 1 ..];

				if !line.as_ref().is_ok_and(|line| line.trim().is_empty())
				{
					return Some(line);
				}
			}

			self.line = core::mem::replace(&mut self.line, Ok(ReceivedBytes::new()))
				.and_then(|line| extend_line(line, core::mem::take(&mut data)));

			None
		})
	}
}

/// Appends received data to a line
fn extend_line(mut line: ReceivedBytes, data: &[u8]) -> Result<ReceivedBytes, &'static str>
{
	line.extend_from_slice(data).map_err(|_| "Provided line too long")?;

	Ok(line)
}

/// The size of the stored settings (with headroom for further settings, changing it invalidates the
/// checksum of already stored settings)
pub const STORED_SIZE: usize = 512;

/// The size of the checksum at the end of the stored settings
const CHECKSUM_SIZE: usize = 4;

/// The checksum of the stored settings, so corrupted settings aren't used
const CHECKSUM: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);

/// Serializes settings for storing them, zero padded (so settings added in the future are
/// deserialized as empty) and followed by the checksum of the padded settings
pub fn to_stored(settings: &impl serde::Serialize) -> Result<[u8; STORED_SIZE], &'static str>
{
	let mut stored = [0; STORED_SIZE];
	let (serialized, checksum) = stored.split_at_mut(STORED_SIZE - CHECKSUM_SIZE);

	postcard::to_slice(settings, serialized).map_err(|_| "Could not serialize settings")?;
	checksum.copy_from_slice(&CHECKSUM.checksum(serialized).to_le_bytes());

	Ok(stored)
}

/// Deserializes stored settings, if their checksum matches
pub fn from_stored<T: serde::de::DeserializeOwned>(stored: &[u8; STORED_SIZE]) -> Result<T, &'static str>
{
	let (serialized, checksum) = stored.split_at(STORED_SIZE - CHECKSUM_SIZE);

	if CHECKSUM.checksum(serialized).to_le_bytes() != checksum
	{
		return Err("Invalid checksum of the stored settings");
	}

	postcard::from_bytes(serialized).map_err(|_| "Could not deserialize settings")
}

#[cfg(test)]
mod tests
{
	use super::*;

	#[test]
	fn change_wifi_settings()
	{
		let mut settings = Settings::default();

		assert_eq!(settings.handle("wifi_ssid=My Network "), Ok(Reply::Changed));
		assert_eq!(settings.handle("wifi_password=a=b, c d"), Ok(Reply::Changed));
		assert_eq!((settings.wifi_ssid.as_str(), settings.wifi_password.as_str()), ("My Network ", "a=b, c d"));

		assert_eq!(settings.handle(&format!("wifi_ssid={}", "x".repeat(33))), Err("SSID too long"));
		assert_eq!(settings.handle(&format!("wifi_password={}", "x".repeat(65))), Err("Wi-Fi password must have 8 to 64 characters"));
		assert_eq!(settings.handle("wifi_password=1234567"), Err("Wi-Fi password must have 8 to 64 characters"));
		assert_eq!(settings.handle("wifi_password="), Ok(Reply::Changed));
		assert!(settings.wifi_password.is_empty());
		assert_eq!(settings.handle("wifi_ssid="), Ok(Reply::Changed));
		assert!(settings.wifi_ssid.is_empty());
	}

	#[test]
	fn show_settings()
	{
		let mut settings = Settings::default();

		assert_eq!(settings.handle("wifi_ssid"), Ok(Reply::Shown("wifi_ssid=".try_into().unwrap())));
		assert_eq!(settings.handle("wifi_password"), Ok(Reply::Shown("wifi_password=".try_into().unwrap())));

		settings.handle("wifi_ssid= My Network").unwrap();
		settings.handle("wifi_password=secret123").unwrap();

		assert_eq!(settings.handle("wifi_ssid"), Ok(Reply::Shown("wifi_ssid= My Network".try_into().unwrap())));
		assert_eq!(settings.handle("wifi_password"), Ok(Reply::Shown("wifi_password=secret123".try_into().unwrap())));
		assert_eq!(settings.handle("192.168.1.10"), Err("Unknown setting"));
	}

	#[test]
	fn shown_settings_can_be_entered_again()
	{
		let mut settings = Settings::default();

		// The default settings and the longest ones
		assert_shown_settings_can_be_entered_again(&mut settings);

		settings.handle(&format!("wifi_ssid={}", "s".repeat(32))).unwrap();
		settings.handle(&format!("wifi_password={}", "p".repeat(64))).unwrap();

		assert_shown_settings_can_be_entered_again(&mut settings);
	}

	/// Every shown value gives the same setting when entered again
	fn assert_shown_settings_can_be_entered_again(settings: &mut Settings)
	{
		for setting in &SETTINGS
		{
			let Ok(Reply::Shown(shown)) = settings.handle(setting.name)
			else
			{
				panic!("{} not shown", setting.name)
			};
			let mut entered = Settings::default();

			assert_eq!(entered.handle(&shown), Ok(Reply::Changed));
			assert_eq!(entered.handle(setting.name), Ok(Reply::Shown(shown)));
		}
	}

	/// Pushes the chunks of data, returning the received lines
	fn receive(lines: &mut ReceivedLines, chunks: &[&[u8]]) -> Vec<Result<String, &'static str>>
	{
		chunks.iter()
			.flat_map(|chunk| lines.push(chunk).map(|line| line.map(|line| line.as_str().into())).collect::<Vec<_>>())
			.collect()
	}

	#[test]
	fn received_lines()
	{
		let mut lines = ReceivedLines::default();

		assert_eq!(receive(&mut lines, &[b"a=1\rb=2\n", b"c=", b"3\r\n\r\n", b"d"]),
			[Ok("a=1".into()), Ok("b=2".into()), Ok("c=3".into())]);
		assert_eq!(receive(&mut lines, &[b"=4\n"]), [Ok("d=4".into())]);
		assert_eq!(receive(&mut lines, &[b"e=5\r", b"\nf=6\n"]), [Ok("e=5".into()), Ok("f=6".into())]);
	}

	#[test]
	fn invalid_received_lines()
	{
		let mut lines = ReceivedLines::default();

		assert_eq!(receive(&mut lines, &[&[b'a'; 200], b"\nb=2\n"]), [Err("Provided line too long"), Ok("b=2".into())]);
		assert_eq!(receive(&mut lines, &[b"a=", &[0xff], b"1\nb=2\n"]), [Err("Invalid data provided"), Ok("b=2".into())]);
		assert_eq!(receive(&mut lines, &[b"a=\xff\nb=2\n"]), [Err("Invalid data provided"), Ok("b=2".into())]);
	}

	#[test]
	fn received_character_split_across_transmissions()
	{
		let mut lines = ReceivedLines::default();

		assert_eq!(receive(&mut lines, &[b"a=\xc3", b"\xa4\n"]), [Ok("a=\u{e4}".into())]);
	}

	#[test]
	fn stored_settings()
	{
		let broker = heapless::String::<64>::try_from("192.168.1.10").unwrap();

		assert_eq!(from_stored::<heapless::String<64>>(&to_stored(&broker).unwrap()), Ok(broker));
	}

	#[test]
	fn corrupted_stored_settings()
	{
		let mut stored = to_stored(&heapless::String::<64>::try_from("192.168.1.10").unwrap()).unwrap();
		stored[1] ^= 0x01;

		assert_eq!(from_stored::<heapless::String<64>>(&stored), Err("Invalid checksum of the stored settings"));

		// Erased flash, e.g. on the first start
		assert_eq!(from_stored::<Settings>(&[0xff; STORED_SIZE]), Err("Invalid checksum of the stored settings"));
	}

	#[test]
	fn stored_wifi_ssid_is_kept()
	{
		let wifi_ssid = heapless::String::<32>::try_from("My Network").unwrap();
		let stored = to_stored(&wifi_ssid).unwrap();
		let settings = from_stored::<Settings>(&stored).unwrap();

		// The other settings are deserialized as empty, so settings added at the end keep the stored ones
		assert_eq!(settings.wifi_ssid, wifi_ssid);
		assert_eq!(to_stored(&settings).unwrap(), stored);
	}

	#[test]
	fn longest_settings_can_be_stored()
	{
		let settings = Settings
		{
			wifi_ssid: "s".repeat(32).as_str().try_into().unwrap(),
			wifi_password: "p".repeat(64).as_str().try_into().unwrap(),
		};

		assert_eq!(from_stored::<Settings>(&to_stored(&settings).unwrap()), Ok(settings));
	}
}
