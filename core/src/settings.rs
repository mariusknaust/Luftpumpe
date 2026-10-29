//! The settings, their text format (as provided via the serial interface) and their stored format

use core::fmt::Write as _;

pub mod lines;
pub mod stored;

/// A line shown on the serial interface
pub type Line = heapless::String<96>;

/// The settings, which are persisted in the flash
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Settings
{
	/// The Wi-Fi network to connect to, Wi-Fi is disabled without one
	pub wifi_ssid: heapless::String<32>,
	/// The password of the Wi-Fi network, empty for an open network
	pub wifi_password: heapless::String<64>,
	/// The host name or IPv4 address of the MQTT broker the state is published to, nothing is
	/// published without one
	pub mqtt_broker: heapless::String<64>,
	/// The user name for the MQTT broker, no authentication without a user name
	pub mqtt_user: heapless::String<32>,
	/// The password for the MQTT broker (only used with a user name)
	pub mqtt_password: heapless::String<64>,
}

/// The reply to a line handled by `Settings::handle`
#[derive(Debug, PartialEq, Eq)]
pub enum Reply
{
	/// The setting was changed (and has to be saved)
	Changed,
	/// The current value of the setting, as `name=value`
	Shown(Line),
}

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

	/// The login for the MQTT broker (user name and password), none without a user name
	pub fn mqtt_login(&self) -> Option<(&str, &str)>
	{
		(!self.mqtt_user.is_empty()).then_some((self.mqtt_user.as_str(), self.mqtt_password.as_str()))
	}
}

/// A setting: Its name, and how it is changed from and shown as text
struct Setting
{
	name: &'static str,
	change: fn(&mut Settings, &str) -> Result<(), &'static str>,
	show: fn(&Settings, &mut Line) -> core::fmt::Result,
}

/// All settings (spaces are significant in the values, apart from the broker)
const SETTINGS: [Setting; 5] =
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
		Setting
		{
			name: "broker",
			change: |settings, value| { settings.mqtt_broker = value.trim().try_into().map_err(|_| "Broker too long")?; Ok(()) },
			show: |settings, line| line.write_str(&settings.mqtt_broker),
		},
		Setting
		{
			name: "broker_user",
			change: |settings, value| { settings.mqtt_user = value.try_into().map_err(|_| "Broker user name too long")?; Ok(()) },
			show: |settings, line| line.write_str(&settings.mqtt_user),
		},
		Setting
		{
			name: "broker_password",
			change: |settings, value| { settings.mqtt_password = value.try_into().map_err(|_| "Broker password too long")?; Ok(()) },
			show: |settings, line| line.write_str(&settings.mqtt_password),
		},
	];

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

#[cfg(test)]
mod tests
{
	use super::*;
	use crate::settings::stored::{from_stored, to_stored, STORED_SIZE};

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
	fn change_broker()
	{
		let mut settings = Settings::default();

		assert_eq!(settings.handle("broker= 192.168.1.10 "), Ok(Reply::Changed));
		assert_eq!(settings.mqtt_broker, "192.168.1.10");
		assert_eq!(settings.handle(&format!("broker={}", "x".repeat(65))), Err("Broker too long"));
	}

	#[test]
	fn change_broker_login()
	{
		let mut settings = Settings::default();

		assert_eq!(settings.handle("broker_user=luftpumpe"), Ok(Reply::Changed));
		assert_eq!(settings.handle("broker_password=a b=c"), Ok(Reply::Changed));
		assert_eq!((settings.mqtt_user.as_str(), settings.mqtt_password.as_str()), ("luftpumpe", "a b=c"));

		assert_eq!(settings.mqtt_login(), Some(("luftpumpe", "a b=c")));

		assert_eq!(settings.handle(&format!("broker_user={}", "x".repeat(33))), Err("Broker user name too long"));
		assert_eq!(settings.handle(&format!("broker_password={}", "x".repeat(65))), Err("Broker password too long"));

		assert_eq!(settings.handle("broker_user= a "), Ok(Reply::Changed));
		assert_eq!(settings.mqtt_user, " a ");
		assert_eq!(settings.handle("broker_user="), Ok(Reply::Changed));
		assert_eq!(settings.mqtt_login(), None);
	}

	#[test]
	fn show_settings()
	{
		let mut settings = Settings::default();

		assert_eq!(settings.handle("broker"), Ok(Reply::Shown("broker=".try_into().unwrap())));
		assert_eq!(settings.handle("wifi_password"), Ok(Reply::Shown("wifi_password=".try_into().unwrap())));

		settings.handle("broker=192.168.1.10").unwrap();
		settings.handle("wifi_ssid= My Network").unwrap();
		settings.handle("wifi_password=secret123").unwrap();

		assert_eq!(settings.handle(" broker "), Ok(Reply::Shown("broker=192.168.1.10".try_into().unwrap())));
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
		settings.handle(&format!("broker={}", "b".repeat(64))).unwrap();
		settings.handle(&format!("broker_user={}", "u".repeat(32))).unwrap();
		settings.handle(&format!("wifi_password={}", "p".repeat(64))).unwrap();
		settings.handle(&format!("broker_password={}", "p".repeat(64))).unwrap();

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

		// The other settings are deserialized as empty, so settings added at the end keep the stored
		// ones
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
			mqtt_broker: "b".repeat(64).as_str().try_into().unwrap(),
			mqtt_user: "u".repeat(32).as_str().try_into().unwrap(),
			mqtt_password: "p".repeat(64).as_str().try_into().unwrap(),
		};

		assert_eq!(from_stored::<Settings>(&to_stored(&settings).unwrap()), Ok(settings));
	}
}
