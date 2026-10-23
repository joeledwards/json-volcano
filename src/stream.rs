//! Finite fixtures and opt-in unbounded streams of complete root values.
use crate::{Config, Fragmentation, Generator, Scenario, copy_fragmented};
use std::io::{self, Read, Write};

/// Deterministic sampling preference. Smaller/larger select the minimum/maximum
/// of two uniform samples, respectively; uniform selects one sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bias {
    Smaller,
    #[default]
    Uniform,
    Larger,
}
impl std::str::FromStr for Bias {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "smaller" | "small" => Ok(Self::Smaller),
            "uniform" => Ok(Self::Uniform),
            "larger" | "large" => Ok(Self::Larger),
            _ => Err("expected smaller, uniform, or larger"),
        }
    }
}
/// Distribution of string payload lengths inside generated records. A token
/// is ASCII, a complete JSON escape, or one UTF-8 character, not an exact byte.
#[derive(Clone, Debug)]
pub struct ValueSizes {
    pub min_tokens: usize,
    pub max_tokens: usize,
    pub bias: Bias,
}
impl Default for ValueSizes {
    fn default() -> Self {
        Self {
            min_tokens: 1,
            max_tokens: 256,
            bias: Bias::Uniform,
        }
    }
}
impl ValueSizes {
    pub fn validate(&self) -> Result<(), String> {
        if self.min_tokens > self.max_tokens || self.max_tokens > 4096 {
            return Err("value token bounds must satisfy 0 <= min <= max <= 4096".into());
        }
        Ok(())
    }
    pub(crate) fn tokens(&self, seed: u64) -> usize {
        self.min_tokens + sample(seed, 0, self.max_tokens - self.min_tokens + 1, self.bias) - 1
    }
}

fn sample(seed: u64, index: u64, maximum: usize, bias: Bias) -> usize {
    fn mix(mut n: u64) -> u64 {
        n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
        n ^ (n >> 31)
    }
    let n = seed.wrapping_add(index.wrapping_mul(0x9e3779b97f4a7c15));
    let first = 1 + (mix(n) % maximum as u64) as usize;
    let second = 1 + (mix(n ^ 0xd1b54a32d192ed03) % maximum as u64) as usize;
    match bias {
        Bias::Smaller => first.min(second),
        Bias::Uniform => first,
        Bias::Larger => first.max(second),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StreamMode {
    #[default]
    Finite,
    /// Repeat root values indefinitely. `fixture.bytes` is bytes per value;
    /// `fixture.candidates` must be 1. Mixed shapes/scenarios alternate globally.
    /// Dropping the reader stops generation; no signal handlers are installed.
    Infinite,
}

#[derive(Clone, Debug)]
pub struct StreamConfig {
    pub fixture: Config,
    pub mode: StreamMode,
    pub fragmentation: Fragmentation,
    /// Optional actual JSON container-depth cap (1..=128). When set, root
    /// nesting varies from 1 to this cap; records flatten at the cap.
    pub max_depth: Option<usize>,
    pub depth_bias: Bias,
    /// Optional sampled string payload lengths; None preserves 48-token records.
    pub value_sizes: Option<ValueSizes>,
}
impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            fixture: Config {
                bytes: 1 << 20,
                ..Config::default()
            },
            mode: StreamMode::Finite,
            fragmentation: Fragmentation::default(),
            max_depth: None,
            depth_bias: Bias::Uniform,
            value_sizes: None,
        }
    }
}
impl StreamConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.fragmentation.validate()?;
        if let Some(sizes) = &self.value_sizes {
            sizes.validate()?;
            if self.fixture.shape == crate::Shape::String {
                return Err(
                    "value size sampling applies to record strings, not the giant-string shape"
                        .into(),
                );
            }
        }
        if let Some(maximum) = self.max_depth {
            if !(1..=128).contains(&maximum) {
                return Err("max-depth must be in 1..=128".into());
            }
        } else if self.depth_bias != Bias::Uniform {
            return Err("depth bias requires max-depth".into());
        }
        if self.mode == StreamMode::Infinite {
            if self.fixture.candidates != 1 {
                return Err("infinite mode has no candidate count; use candidates=1".into());
            }
            if self.fixture.scenario == Scenario::Truncated {
                return Err("truncated requires finite output and EOF".into());
            }
        }
        if self.max_depth.is_none() && self.mode == StreamMode::Finite {
            return self.fixture.validate();
        }
        // Validate the worst allowed nesting and both alternating root types.
        let mut worst = self.fixture.clone();
        if let Some(maximum) = self.max_depth {
            worst.depth = maximum;
        }
        if self.mode == StreamMode::Infinite {
            for index in 0..2 {
                worst.shape = self.fixture.root_shape(index);
                worst.scenario = self.fixture.kind(index);
                worst.validate()?;
            }
        } else {
            worst.validate()?;
        }
        Ok(())
    }
    fn candidate(&self, index: u64, seed: u64) -> Config {
        let bytes = if self.mode == StreamMode::Finite {
            self.fixture.bytes / self.fixture.candidates
                + u64::from(index < self.fixture.bytes % self.fixture.candidates)
        } else {
            self.fixture.bytes
        };
        Config {
            bytes,
            candidates: 1,
            shape: self.fixture.root_shape(index),
            scenario: self.fixture.kind(index),
            seed,
            depth: self
                .max_depth
                .map(|maximum| sample(self.fixture.seed, index, maximum, self.depth_bias))
                .unwrap_or(self.fixture.depth),
            ..self.fixture.clone()
        }
    }
    fn source(&self, index: u64, seed: u64) -> Result<Generator, String> {
        let mut source = Generator::new(self.candidate(index, seed))?;
        source.value_sizes = self.value_sizes.clone();
        source.flat_records = self.max_depth == Some(source.config.depth);
        Ok(source)
    }
}

/// Bounded-memory Read source. Read chunk sizes do not change the byte stream.
/// Finite mode preserves the legacy generator's exact-size corpus; infinite
/// mode carries the PRNG state across independently sized root values.
pub struct StreamGenerator {
    config: StreamConfig,
    source: Generator,
    index: u64,
}
impl StreamGenerator {
    pub fn new(config: StreamConfig) -> Result<Self, String> {
        config.validate()?;
        let source = if config.mode == StreamMode::Finite
            && config.max_depth.is_none()
            && config.value_sizes.is_none()
        {
            Generator::new(config.fixture.clone())?
        } else {
            config.source(0, config.fixture.seed)?
        };
        Ok(Self {
            source,
            config,
            index: 0,
        })
    }
}
impl Read for StreamGenerator {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let read = self.source.read(buffer)?;
        let legacy_finite = self.config.mode == StreamMode::Finite
            && self.config.max_depth.is_none()
            && self.config.value_sizes.is_none();
        let last = self.config.mode == StreamMode::Finite
            && self.index + 1 >= self.config.fixture.candidates;
        if read != 0 || legacy_finite || last {
            return Ok(read);
        }
        self.index = self.index.wrapping_add(1);
        self.source = self
            .config
            .source(self.index, self.source.rng)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        self.source.read(buffer)
    }
}

/// Generate finite or infinite output with optional write fragmentation.
/// Stops on writer failure (including BrokenPipe) and checks final flush at EOF.
/// Infinite mode has no EOF; process signals use the platform's normal behavior
/// and may leave a partial final value. Use finite mode for a complete fixture.
pub fn generate_stream(config: StreamConfig, output: impl Write) -> io::Result<u64> {
    let fragmentation = config.fragmentation.clone();
    let source =
        StreamGenerator::new(config).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    copy_fragmented(source, output, &fragmentation)
}
