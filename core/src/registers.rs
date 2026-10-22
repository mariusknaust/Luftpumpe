//! The registers of the heat pump's main board (Power World protocol PW58321 for the Easylife series,
//! main board CC1010B) and the values published from them

use crate::modbus::{Registers, MAX_REGISTERS, REQUEST_PAUSE, RESPONSE_TIMEOUT, SEND_TIMEOUT};

/// A block of consecutive registers, read with a single request
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block
{
	/// The address of the first register
	pub start: u16,
	/// The number of registers
	pub count: u16,
}

impl Block
{
	/// Whether the block contains the register at the address
	const fn contains(&self, address: u16) -> bool
	{
		address >= self.start && address - self.start < self.count
	}
}

/// The blocks read in every cycle: The control flags, the mode and the first defrost parameters (P09,
/// P10, P12), the system parameters (P01 to F1) and the status (temperatures, outputs and faults). Reading the parameters every cycle shows
/// changes made on the display or in the app, reading doesn't wear the flash of the main board.
pub const BLOCKS: [Block; 3] =
	[
		Block { start: 0x0000, count: 8 },
		Block { start: 0x008d, count: 116 },
		Block { start: 0x8000, count: 43 },
	];

// The main board answers at most `MAX_REGISTERS` per request
const _: () =
{
	let mut index = 0;

	while index < BLOCKS.len()
	{
		core::assert!(BLOCKS[index].count > 0 && BLOCKS[index].count as usize <= MAX_REGISTERS);
		index += 1;
	}
};

/// The longest time in ms reading all blocks takes, if every request runs into its timeouts
pub const MAX_READ_DURATION: u64 = BLOCKS.len() as u64 * (SEND_TIMEOUT + RESPONSE_TIMEOUT + REQUEST_PAUSE);

// Reading all blocks fits into the poll interval, also on errors, so the cycles keep their cadence
const _: () = core::assert!(MAX_READ_DURATION < crate::POLL_INTERVAL);

/// The register values of all blocks of a cycle
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Readings
{
	/// The values of each block (in the order of `BLOCKS`)
	blocks: [Registers; BLOCKS.len()],
}

impl Readings
{
	/// Takes the values read from the blocks (in the order of `BLOCKS`), none if a block has the wrong
	/// number of values
	pub fn new(blocks: [Registers; BLOCKS.len()]) -> Option<Self>
	{
		blocks.iter().zip(&BLOCKS).all(|(values, block)| values.len() == usize::from(block.count))
			.then_some(Self { blocks })
	}

	/// The value of the register at the address, none if it isn't read
	pub fn value(&self, address: u16) -> Option<u16>
	{
		BLOCKS.iter().zip(&self.blocks)
			.find(|(block, _)| block.contains(address))
			.and_then(|(block, values)| values.get(usize::from(address - block.start)).copied())
	}
}

/// How a published value is taken from the registers
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind
{
	/// The register as an unsigned number
	Unsigned,
	/// The register as a signed number (two's complement), e.g. temperatures
	Signed,
	/// A single bit of the register (0 or 1)
	Bit(u8),
	/// The given number of consecutive registers as a list of unsigned numbers, e.g. fault flags
	List(u16),
}

/// A published value: Its name in the state and where it is taken from
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field
{
	/// The name in the published state
	pub name: &'static str,
	/// The address of the (first) register
	pub address: u16,
	/// How the value is taken from the register
	pub kind: Kind,
}

/// Shortens the table of the fields
const fn field(name: &'static str, address: u16, kind: Kind) -> Field
{
	Field { name, address, kind }
}

/// The published values, as raw register values: The protocol is ambiguous about the scaling of
/// the temperatures ("Accuracy 0.1" for the return water, "Accuracy 0.5" for the others, none for
/// `low_pressure_temperature`) and gives no units for the other status values, the scaling is
/// applied by the receiver once compared with the display.
pub const FIELDS: [Field; 62] =
	[
		// Control flags and mode, as set on the display
		field("power", 0x0000, Kind::Bit(0)),
		field("powerful", 0x0001, Kind::Bit(4)),
		field("silent", 0x0001, Kind::Bit(5)),
		// P22: The weather compensated heating curve (P23, P24)
		field("heating_curve", 0x0001, Kind::Bit(6)),
		field("vacation", 0x0002, Kind::Bit(1)),
		// 0: DHW, 1: Heating, 2: Cooling, 3: DHW + Heating, 4: DHW + Cooling
		field("mode", 0x0004, Kind::Unsigned),
		// System parameters, as on the display
		field("p01", 0x008d, Kind::Unsigned),
		field("p02", 0x008e, Kind::Unsigned),
		field("p03", 0x008f, Kind::Unsigned),
		field("p04", 0x0090, Kind::Unsigned),
		field("p05", 0x0091, Kind::Unsigned),
		field("p08", 0x0092, Kind::Signed),
		field("p09", 0x0005, Kind::Unsigned),
		field("p10", 0x0006, Kind::Unsigned),
		field("p11", 0x0093, Kind::Signed),
		field("p12", 0x0007, Kind::Unsigned),
		field("p13", 0x0094, Kind::Unsigned),
		field("p14", 0x0095, Kind::Unsigned),
		field("p15", 0x0096, Kind::Unsigned),
		field("p16", 0x0097, Kind::Unsigned),
		field("p17", 0x00e6, Kind::Unsigned),
		field("p18", 0x00e7, Kind::Unsigned),
		field("p19", 0x00e8, Kind::Unsigned),
		field("p20", 0x00e9, Kind::Unsigned),
		field("p21", 0x00ea, Kind::Unsigned),
		field("p23", 0x00eb, Kind::Unsigned),
		field("p24", 0x00ec, Kind::Unsigned),
		field("p26", 0x00ed, Kind::Signed),
		field("vacation_setpoint", 0x00f2, Kind::Unsigned),
		field("p27", 0x00fe, Kind::Unsigned),
		field("f01", 0x0100, Kind::Unsigned),
		// Status: Defrosting and the outputs
		field("defrost", 0x8003, Kind::Bit(7)),
		field("compressor", 0x8004, Kind::Bit(0)),
		field("fan", 0x8004, Kind::Bit(5)),
		field("four_way_valve", 0x8004, Kind::Bit(6)),
		field("chassis_heater", 0x8005, Kind::Bit(0)),
		field("space_heater", 0x8005, Kind::Bit(5)),
		field("three_way_valve", 0x8005, Kind::Bit(6)),
		field("tank_heater", 0x8005, Kind::Bit(7)),
		field("water_pump", 0x8006, Kind::Bit(0)),
		field("crankcase_heater", 0x8006, Kind::Bit(1)),
		field("antifreeze_secondary", 0x800c, Kind::Bit(4)),
		field("antifreeze_primary", 0x800c, Kind::Bit(5)),
		// The fault flags 1 to 7 and the fault of the inverter (low and high byte)
		field("faults", 0x8007, Kind::List(7)),
		field("inverter_faults", 0x801f, Kind::List(2)),
		// Temperatures (raw, see above)
		field("return_water", 0x800e, Kind::Signed),
		field("tank", 0x800f, Kind::Signed),
		field("ambient", 0x8011, Kind::Signed),
		field("leaving_water", 0x8012, Kind::Signed),
		field("suction", 0x8015, Kind::Signed),
		field("outdoor_coil", 0x8016, Kind::Signed),
		field("cooling_coil", 0x801a, Kind::Signed),
		field("exhaust", 0x801b, Kind::Signed),
		field("low_pressure_temperature", 0x8028, Kind::Signed),
		// Expansion valves, compressor frequencies and current, DC bus voltage, fan and pump speeds (raw,
		// see above)
		field("expansion_valve_main", 0x801c, Kind::Unsigned),
		field("expansion_valve_auxiliary", 0x801d, Kind::Unsigned),
		field("compressor_frequency", 0x801e, Kind::Unsigned),
		field("target_frequency", 0x8024, Kind::Unsigned),
		field("compressor_current", 0x8025, Kind::Unsigned),
		field("dc_bus_voltage", 0x8021, Kind::Unsigned),
		field("fan_speed", 0x8026, Kind::Unsigned),
		field("pump_speed", 0x802a, Kind::Unsigned),
	];

#[cfg(test)]
mod tests
{
	use super::*;

	/// Readings with every register holding the given value
	fn readings(value: u16) -> Readings
	{
		Readings::new(BLOCKS.map(|block| Registers::from_iter(core::iter::repeat_n(value, usize::from(block.count))))).unwrap()
	}

	#[test]
	fn fields_are_read()
	{
		let readings = readings(0);

		for field in &FIELDS
		{
			let count = match field.kind { Kind::List(count) => count, _ => 1 };

			assert!((field.address .. field.address + count).all(|address| readings.value(address).is_some()), "{} not read", field.name);
		}
	}

	#[test]
	fn field_names_are_unique()
	{
		for (index, field) in FIELDS.iter().enumerate()
		{
			assert!(FIELDS[.. index].iter().all(|other| other.name != field.name), "{} not unique", field.name);
		}
	}

	#[test]
	fn values()
	{
		let mut blocks = BLOCKS.map(|block| Registers::from_iter(core::iter::repeat_n(0, usize::from(block.count))));
		blocks[1][0x00ed - 0x008d] = 0xfffb;
		blocks[2][0x800e - 0x8000] = 335;

		let readings = Readings::new(blocks).unwrap();

		assert_eq!(readings.value(0x00ed), Some(0xfffb));
		assert_eq!(readings.value(0x800e), Some(335));
		assert_eq!(readings.value(0x0008), None);
		assert_eq!(readings.value(0x802b), None);
	}

	#[test]
	fn wrong_number_of_values()
	{
		let mut blocks = readings(0).blocks;
		blocks[2].pop();

		assert_eq!(Readings::new(blocks), None);
	}
}
