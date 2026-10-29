//! The stored format of the settings: zero padded and followed by a checksum

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
