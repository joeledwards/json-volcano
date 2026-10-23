//! Reproducible, exact-size JSON fixtures with bounded memory.
//!
//! Corpus v1. All allocations are bounded independently of the target size.
//! The library uses only the standard library; the CLI is an optional feature.
use std::io::{self, Read, Write};

const BLOCK: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Records,
    String,
    /// Object roots, nested through a `value` member at each level.
    Object,
    /// Array roots containing only arrays and scalar values.
    Array,
    /// Alternating array and object roots, starting with an array.
    Mixed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Valid,
    Invalid,
    Mixed,
    Truncated,
}
impl std::str::FromStr for Shape {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "records" => Ok(Self::Records),
            "string" => Ok(Self::String),
            "object" | "objects" => Ok(Self::Object),
            "array" | "arrays" => Ok(Self::Array),
            "mixed" => Ok(Self::Mixed),
            _ => Err("expected records, string, object, array, or mixed"),
        }
    }
}
impl std::str::FromStr for Scenario {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "valid" => Ok(Self::Valid),
            "invalid" => Ok(Self::Invalid),
            "mixed" => Ok(Self::Mixed),
            "truncated" => Ok(Self::Truncated),
            _ => Err("expected valid, invalid, mixed, or truncated"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bytes: u64,
    pub candidates: u64,
    pub seed: u64,
    pub depth: usize,
    pub escape_percent: u8,
    pub unicode_percent: u8,
    pub shape: Shape,
    pub scenario: Scenario,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            bytes: 1024,
            candidates: 1,
            seed: 42,
            depth: 1,
            escape_percent: 2,
            unicode_percent: 2,
            shape: Shape::Records,
            scenario: Scenario::Valid,
        }
    }
}
impl Config {
    fn kind(&self, index: u64) -> Scenario {
        match self.scenario {
            Scenario::Mixed if index.is_multiple_of(2) => Scenario::Valid,
            Scenario::Mixed => Scenario::Invalid,
            kind => kind,
        }
    }
    fn root_shape(&self, index: u64) -> Shape {
        match self.shape {
            Shape::Mixed if index.is_multiple_of(2) => Shape::Array,
            Shape::Mixed => Shape::Object,
            shape => shape,
        }
    }
    fn prefix(&self, index: u64) -> Vec<u8> {
        if self.root_shape(index) == Shape::Object {
            let mut bytes = b"{\"value\":".repeat(self.depth);
            bytes.extend_from_slice(b"null");
            return bytes;
        }
        let mut bytes = vec![b'['; self.depth];
        bytes.extend_from_slice(if self.shape == Shape::String {
            b"\""
        } else {
            b"null"
        });
        bytes
    }
    fn suffix(&self, index: u64, kind: Scenario) -> Vec<u8> {
        let object = self.root_shape(index) == Shape::Object;
        if kind == Scenario::Truncated {
            return if self.shape == Shape::String {
                Vec::new()
            } else if object {
                b",\"unfinished\":\"".to_vec()
            } else {
                b",\"unfinished".to_vec()
            };
        }
        let mut bytes = Vec::new();
        if self.shape == Shape::String {
            bytes.push(b'"');
        }
        if kind == Scenario::Invalid {
            bytes.extend_from_slice(if object {
                b",\"invalid\":truX"
            } else {
                b",truX"
            });
        }
        bytes.extend(std::iter::repeat_n(
            if object { b'}' } else { b']' },
            self.depth,
        ));
        bytes.push(b'\n');
        bytes
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.candidates == 0 {
            return Err("candidates must be positive".into());
        }
        if !(1..=128).contains(&self.depth) {
            return Err("depth must be between 1 and 128".into());
        }
        if u16::from(self.escape_percent) + u16::from(self.unicode_percent) > 100 {
            return Err("escape-percent plus unicode-percent must not exceed 100".into());
        }
        if self.scenario == Scenario::Truncated && self.candidates != 1 {
            return Err("truncated requires exactly one candidate (EOF occurs only once)".into());
        }
        if self.scenario == Scenario::Mixed && self.candidates < 2 {
            return Err("mixed requires at least two candidates".into());
        }
        // Validate each root kind that will appear, including uneven budgets.
        for index in 0..self.candidates.min(2) {
            let kind = if self.scenario == Scenario::Mixed {
                Scenario::Invalid
            } else {
                self.scenario
            };
            let minimum = (self.prefix(index).len() + self.suffix(index, kind).len() + 1) as u64;
            if self.bytes / self.candidates < minimum {
                return Err(format!(
                    "each candidate needs at least {minimum} bytes for these options"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Start,
    Body,
    End,
}

/// Exact-size source, including JSON syntax, separators and final newline.
/// Keeps at most one 32 KiB block and one small record, even for a giant scalar.
pub struct Generator {
    config: Config,
    rng: u64,
    index: u64,
    remaining: u64,
    phase: Phase,
    pending: Vec<u8>,
    offset: usize,
    row: Vec<u8>,
    flat_records: bool,
    value_sizes: Option<ValueSizes>,
}
impl Generator {
    pub fn new(config: Config) -> Result<Self, String> {
        config.validate()?;
        Ok(Self {
            rng: config.seed,
            config,
            index: 0,
            remaining: 0,
            phase: Phase::Start,
            pending: Vec::with_capacity(BLOCK),
            offset: 0,
            row: Vec::with_capacity(512),
            flat_records: false,
            value_sizes: None,
        })
    }
    fn random(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng
    }
    fn token(&mut self) -> &'static [u8] {
        let n = self.random() >> 32;
        let category = (n % 100) as u16;
        if category < self.config.escape_percent as u16 {
            [b"\\n".as_slice(), b"\\\"", b"\\\\", b"\\u0041"][(n / 100 % 4) as usize]
        } else if category < self.config.escape_percent as u16 + self.config.unicode_percent as u16
        {
            ["é".as_bytes(), "中".as_bytes(), "🙂".as_bytes()][(n / 100 % 3) as usize]
        } else {
            const ASCII: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";
            let i = (n / 100 % ASCII.len() as u64) as usize;
            &ASCII[i..i + 1]
        }
    }
    fn refill(&mut self) {
        self.pending.clear();
        self.offset = 0;
        while self.pending.is_empty() && self.index < self.config.candidates {
            match self.phase {
                Phase::Start => {
                    let size = self.config.bytes / self.config.candidates
                        + u64::from(self.index < self.config.bytes % self.config.candidates);
                    self.pending
                        .extend_from_slice(&self.config.prefix(self.index));
                    self.remaining = size
                        - self.pending.len() as u64
                        - self
                            .config
                            .suffix(self.index, self.config.kind(self.index))
                            .len() as u64;
                    self.phase = Phase::Body;
                }
                Phase::Body => {
                    if self.remaining == 0 {
                        self.phase = Phase::End;
                        continue;
                    }
                    if self.config.shape == Shape::String {
                        let budget = self.remaining.min(BLOCK as u64) as usize;
                        while self.pending.len() < budget {
                            let token = self.token();
                            if token.len() <= budget - self.pending.len() {
                                self.pending.extend_from_slice(token);
                            } else {
                                self.pending.push(b'a');
                            }
                        }
                    } else {
                        let row_bound = self
                            .value_sizes
                            .as_ref()
                            .map(|sizes| 256 + 6 * sizes.max_tokens)
                            .unwrap_or(512);
                        while self.pending.len() + row_bound <= BLOCK
                            && self.remaining > self.pending.len() as u64
                        {
                            self.row.clear();
                            let n = self.random() >> 32;
                            if self.config.root_shape(self.index) == Shape::Object {
                                write!(&mut self.row, ",\"record_{n}\":").unwrap();
                            } else {
                                self.row.push(b',');
                            }
                            let tokens = self
                                .value_sizes
                                .as_ref()
                                .map(|sizes| sizes.tokens(self.rng))
                                .unwrap_or(48);
                            if self.flat_records {
                                self.row.push(b'"');
                                for _ in 0..tokens {
                                    let token = self.token();
                                    self.row.extend_from_slice(token);
                                }
                                self.row.push(b'"');
                            } else {
                                if self.config.root_shape(self.index) == Shape::Array {
                                    write!(
                                        &mut self.row,
                                        "[{n},-{n}.125e-3,{},null,\"",
                                        n.is_multiple_of(2)
                                    )
                                    .unwrap();
                                } else {
                                    write!(&mut self.row, "{{\"id\":{n},\"number\":-{n}.125e-3,\"ok\":{},\"none\":null,\"value\":\"", n.is_multiple_of(2)).unwrap();
                                }
                                for _ in 0..tokens {
                                    let token = self.token();
                                    self.row.extend_from_slice(token);
                                }
                                self.row.extend_from_slice(
                                    if self.config.root_shape(self.index) == Shape::Array {
                                        b"\"]"
                                    } else {
                                        b"\"}"
                                    },
                                );
                            }
                            let available = self.remaining - self.pending.len() as u64;
                            if self.row.len() as u64 <= available {
                                self.pending.extend_from_slice(&self.row);
                            } else {
                                // Legal whitespace fills the exact byte target, never a cut token.
                                self.pending.extend(std::iter::repeat_n(
                                    b' ',
                                    available.min((BLOCK - self.pending.len()) as u64) as usize,
                                ));
                            }
                        }
                    }
                    self.remaining -= self.pending.len() as u64;
                }
                Phase::End => {
                    self.pending.extend_from_slice(
                        &self.config.suffix(self.index, self.config.kind(self.index)),
                    );
                    self.index += 1;
                    self.phase = Phase::Start;
                }
            }
        }
    }
}
impl Read for Generator {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.offset == self.pending.len() {
            self.refill();
        }
        let n = bytes.len().min(self.pending.len() - self.offset);
        bytes[..n].copy_from_slice(&self.pending[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}

/// Copy the fixture to a writer and propagate write and final flush failures.
pub fn generate(config: Config, mut output: impl Write) -> io::Result<u64> {
    let mut source =
        Generator::new(config).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let bytes = io::copy(&mut source, &mut output)?;
    output.flush()?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;

mod fragment;
pub use fragment::{Fragmentation, copy_fragmented};

mod stream;
pub use stream::{Bias, StreamConfig, StreamGenerator, StreamMode, ValueSizes, generate_stream};
