//! Explicit application-level write boundaries for streaming consumers.
use std::io::{self, Read, Write};

/// Repeating logical write sizes. Empty means normal 32 KiB blocks.
/// Sizes must be in 1..=65536. OS pipes may merge or split writes; these are
/// application write boundaries, not a guarantee about a consumer's reads.
#[derive(Clone, Debug, Default)]
pub struct Fragmentation {
    pub sizes: Vec<usize>,
    /// Flush after each logical fragment, including the final partial fragment.
    pub flush_each: bool,
}
impl Fragmentation {
    pub fn validate(&self) -> Result<(), String> {
        if self.sizes.len() > 1024 || self.sizes.iter().any(|&n| n == 0 || n > 65536) {
            return Err("fragment pattern needs at most 1024 sizes, each in 1..=65536".into());
        }
        Ok(())
    }
}

/// Copy using a repeating write pattern, propagating read, write and flush
/// failures. A read error discards the unfinished buffered fragment and does
/// not flush; EOF writes the final partial fragment and checks flush. Fragmentation may split UTF-8 and JSON escapes. It never changes
/// the bytes. Interrupted reads are retried; a final partial fragment is kept.
/// The returned byte counter saturates at u64::MAX for unbounded streams.
pub fn copy_fragmented(
    mut source: impl Read,
    mut output: impl Write,
    fragmentation: &Fragmentation,
) -> io::Result<u64> {
    fragmentation
        .validate()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let capacity = fragmentation.sizes.iter().copied().max().unwrap_or(32768);
    let mut buffer = vec![0; capacity];
    let mut index = 0;
    let mut total = 0_u64;
    loop {
        let target = fragmentation.sizes.get(index).copied().unwrap_or(capacity);
        let mut filled = 0;
        while filled < target {
            match source.read(&mut buffer[filled..target]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        if filled == 0 {
            break;
        }
        output.write_all(&buffer[..filled])?;
        total = total.saturating_add(filled as u64);
        if fragmentation.flush_each {
            output.flush()?;
        }
        if !fragmentation.sizes.is_empty() {
            index = (index + 1) % fragmentation.sizes.len();
        }
        if filled < target {
            break;
        }
    }
    output.flush()?;
    Ok(total)
}
