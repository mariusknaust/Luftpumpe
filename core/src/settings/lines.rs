//! The splitting of the data received via the serial interface into lines

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

#[cfg(test)]
mod tests
{
	use super::*;

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
}
