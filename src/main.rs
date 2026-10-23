//! Standalone bounded-memory fixture generator. Corpus format version: 1.
use clap::Parser;
use json_volcano::{
    Bias, Config, Fragmentation, Scenario, Shape, StreamConfig, StreamMode, ValueSizes,
};
use std::io::{self, Write};

#[derive(Debug, Parser)]
#[command(
    about = "Generate seeded JSON fixtures; defaults to one 1MiB value",
    version
)]
struct Cli {
    /// Exact finite output bytes including syntax/newlines (default: 1MiB).
    #[arg(long, value_parser = parse_size, conflicts_with = "infinite")]
    bytes: Option<u64>,
    /// Top-level values sharing the finite byte budget (default: 1).
    #[arg(long, alias = "records", conflicts_with_all = ["single", "multi", "infinite"])]
    candidates: Option<u64>,
    /// Emit exactly one root value (the default).
    #[arg(long, conflicts_with_all = ["multi", "infinite"])]
    single: bool,
    /// Emit 1024 root values; use --records N for an explicit count.
    #[arg(long, conflicts_with = "infinite")]
    multi: bool,
    /// Emit root values indefinitely, until interrupted or a writer error.
    #[arg(long)]
    infinite: bool,
    /// Bytes per root value in infinite mode. Accepts B/KiB/MiB/GiB/TiB.
    #[arg(long, value_parser = parse_size, requires = "infinite")]
    record_bytes: Option<u64>,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Root nesting depth; record objects add one extra container level.
    #[arg(long, default_value_t = 1, conflicts_with = "max_depth")]
    depth: usize,
    /// Vary nesting with an actual container-depth cap, including record objects.
    #[arg(long)]
    max_depth: Option<usize>,
    /// Prefer smaller, uniform, or larger nesting depths (requires --max-depth).
    #[arg(long, default_value = "uniform", requires = "max_depth")]
    depth_bias: Bias,
    /// String payload token minimum when sampling value sizes (default: 1).
    #[arg(long)]
    min_value_tokens: Option<usize>,
    /// String payload token maximum when sampling value sizes (default: 256, cap: 4096).
    #[arg(long)]
    max_value_tokens: Option<usize>,
    /// Prefer smaller, uniform, or larger record string values.
    #[arg(long)]
    size_bias: Option<Bias>,
    /// Root structure: records, string, array, object, or alternating mixed.
    #[arg(long, default_value = "records")]
    shape: Shape,
    /// Validity: valid, invalid, mixed (alternating), or truncated (finite only).
    #[arg(long, default_value = "valid")]
    scenario: Scenario,
    /// Alternate valid and invalid roots (shortcut for --scenario mixed).
    #[arg(long, conflicts_with = "scenario")]
    include_invalid: bool,
    /// Repeating application write sizes, e.g. 1 or 1,2,7 (each <=65536).
    #[arg(long, value_delimiter = ',', value_parser = parse_fragment)]
    fragment_bytes: Vec<usize>,
    /// Flush each logical fragment; OS pipes can still merge/split writes.
    #[arg(long, requires = "fragment_bytes")]
    flush_fragments: bool,
    /// Probability per string token (not byte); range 0..100.
    #[arg(long, default_value_t = 2)]
    escape_percent: u8,
    /// Probability per string token; sum with escape-percent <= 100.
    #[arg(long, default_value_t = 2)]
    unicode_percent: u8,
    /// Report statistics on stderr after finite output completes and flushes.
    #[arg(long)]
    stats: bool,
}
fn parse_size(s: &str) -> Result<u64, String> {
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let number: u64 = s[..split]
        .parse()
        .map_err(|_| "expected an unsigned integer size")?;
    let factor = match &s[split..] {
        "" | "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1_u64 << 40,
        _ => return Err("expected B, KiB, MiB, GiB, or TiB (case sensitive)".into()),
    };
    number
        .checked_mul(factor)
        .ok_or_else(|| "byte size exceeds u64".into())
}
fn parse_fragment(value: &str) -> Result<usize, String> {
    let n: usize = value
        .parse()
        .map_err(|_| "expected fragment size in 1..=65536")?;
    if !(1..=65536).contains(&n) {
        return Err("expected fragment size in 1..=65536".into());
    }
    Ok(n)
}
impl Cli {
    fn config(&self) -> Result<StreamConfig, String> {
        let scenario = if self.include_invalid {
            Scenario::Mixed
        } else {
            self.scenario
        };
        let config = StreamConfig {
            fixture: Config {
                bytes: if self.infinite {
                    self.record_bytes.unwrap_or(1024)
                } else {
                    self.bytes.unwrap_or(1 << 20)
                },
                candidates: self.candidates.unwrap_or(if self.multi {
                    1024
                } else if self.include_invalid && !self.single && !self.infinite {
                    2
                } else {
                    1
                }),
                seed: self.seed,
                depth: self.depth,
                escape_percent: self.escape_percent,
                unicode_percent: self.unicode_percent,
                shape: self.shape,
                scenario,
            },
            max_depth: self.max_depth,
            depth_bias: self.depth_bias,
            value_sizes: if self.min_value_tokens.is_some()
                || self.max_value_tokens.is_some()
                || self.size_bias.is_some()
            {
                Some(ValueSizes {
                    min_tokens: self.min_value_tokens.unwrap_or(1),
                    max_tokens: self.max_value_tokens.unwrap_or(256),
                    bias: self.size_bias.unwrap_or_default(),
                })
            } else {
                None
            },
            mode: if self.infinite {
                StreamMode::Infinite
            } else {
                StreamMode::Finite
            },
            fragmentation: Fragmentation {
                sizes: self.fragment_bytes.clone(),
                flush_each: self.flush_fragments,
            },
        };
        config.validate()?;
        Ok(config)
    }
}
fn execute(cli: Cli) -> io::Result<()> {
    let config = cli
        .config()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let fixture = config.fixture.clone();
    let controls = if cli.stats {
        format!(
            "max_depth={:?} depth_bias={:?} value_sizes={:?} fragment_bytes={:?} flush_fragments={}",
            config.max_depth,
            config.depth_bias,
            config.value_sizes,
            config.fragmentation.sizes,
            config.fragmentation.flush_each
        )
    } else {
        String::new()
    };
    let start = std::time::Instant::now();
    // Avoid hiding requested write boundaries behind the usual output buffer.
    let bytes = if cli.fragment_bytes.is_empty() {
        json_volcano::generate_stream(
            config,
            io::BufWriter::with_capacity(64 * 1024, io::stdout().lock()),
        )?
    } else {
        json_volcano::generate_stream(config, io::stdout().lock())?
    };
    if cli.stats {
        let seconds = start.elapsed().as_secs_f64();
        writeln!(
            io::stderr().lock(),
            "corpus=1 seed={} bytes={bytes} candidates={} depth={} shape={:?} scenario={:?} escape_percent={} unicode_percent={} {controls} elapsed_s={seconds:.6} mib_s={:.3}",
            fixture.seed,
            fixture.candidates,
            fixture.depth,
            fixture.shape,
            fixture.scenario,
            fixture.escape_percent,
            fixture.unicode_percent,
            bytes as f64 / 1048576.0 / seconds
        )?;
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match execute(Cli::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("json-volcano: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    #[test]
    fn sizes_and_options() {
        for (s, n) in [
            ("17", 17),
            ("1B", 1),
            ("1KiB", 1024),
            ("1MiB", 1 << 20),
            ("10GiB", 10_u64 << 30),
            ("1TiB", 1_u64 << 40),
        ] {
            assert_eq!(parse_size(s).unwrap(), n);
        }
        for s in [
            "",
            "-1",
            "1.5GiB",
            "1GB",
            "18446744073709551616",
            "18446744073709551615TiB",
        ] {
            assert!(parse_size(s).is_err(), "{s}");
        }
        let cli = Cli::try_parse_from([
            "json-volcano",
            "--bytes",
            "1GiB",
            "--shape",
            "string",
            "--scenario",
            "mixed",
            "--candidates",
            "2",
        ])
        .unwrap();
        assert_eq!(cli.bytes, Some(1 << 30));
        assert_eq!(cli.shape, Shape::String);
        assert_eq!(cli.scenario, Scenario::Mixed);
        assert!(
            Cli::try_parse_from(["json-volcano", "--bytes", "1MiB", "--scenario", "oops"]).is_err()
        );
    }
    #[test]
    fn default_single_multi_infinite_and_fragment_options() {
        let config = Cli::try_parse_from(["json-volcano"])
            .unwrap()
            .config()
            .unwrap();
        assert_eq!(config.fixture.bytes, 1 << 20);
        assert_eq!(config.fixture.candidates, 1);
        assert_eq!(config.mode, StreamMode::Finite);
        for (args, count, mode) in [
            (vec!["--single"], 1, StreamMode::Finite),
            (vec!["--multi"], 1024, StreamMode::Finite),
            (vec!["--records", "7"], 7, StreamMode::Finite),
            (vec!["--include-invalid"], 2, StreamMode::Finite),
            (
                vec!["--infinite", "--include-invalid"],
                1,
                StreamMode::Infinite,
            ),
        ] {
            let config = Cli::try_parse_from(std::iter::once("json-volcano").chain(args))
                .unwrap()
                .config()
                .unwrap();
            assert_eq!(config.fixture.candidates, count);
            assert_eq!(config.mode, mode);
        }
        let config = Cli::try_parse_from([
            "json-volcano",
            "--infinite",
            "--record-bytes",
            "2KiB",
            "--shape",
            "mixed",
            "--fragment-bytes",
            "1,2,7",
            "--flush-fragments",
        ])
        .unwrap()
        .config()
        .unwrap();
        assert_eq!(config.fixture.bytes, 2048);
        assert_eq!(config.fragmentation.sizes, [1, 2, 7]);
        assert!(config.fragmentation.flush_each);
        for args in [
            vec!["--infinite", "--bytes", "1KiB"],
            vec!["--single", "--multi"],
            vec!["--single", "--records", "2"],
            vec!["--infinite", "--candidates", "2"],
            vec!["--record-bytes", "1KiB"],
            vec!["--flush-fragments"],
            vec!["--fragment-bytes", "0"],
            vec!["--fragment-bytes", "65537"],
            vec!["--include-invalid", "--scenario", "valid"],
        ] {
            assert!(
                Cli::try_parse_from(std::iter::once("json-volcano").chain(args.clone())).is_err(),
                "{args:?}"
            );
        }
        for args in [
            vec!["--bytes", "0"],
            vec!["--records", "0"],
            vec!["--single", "--include-invalid"],
            vec!["--infinite", "--scenario", "truncated"],
        ] {
            assert!(
                Cli::try_parse_from(std::iter::once("json-volcano").chain(args))
                    .unwrap()
                    .config()
                    .is_err()
            );
        }
    }
    #[test]
    fn depth_and_value_distributions_have_independent_validated_controls() {
        let config = Cli::try_parse_from([
            "json-volcano",
            "--multi",
            "--shape",
            "mixed",
            "--max-depth",
            "8",
            "--depth-bias",
            "smaller",
            "--size-bias",
            "larger",
            "--min-value-tokens",
            "0",
            "--max-value-tokens",
            "1024",
        ])
        .unwrap()
        .config()
        .unwrap();
        assert_eq!(config.max_depth, Some(8));
        assert_eq!(config.depth_bias, Bias::Smaller);
        let sizes = config.value_sizes.unwrap();
        assert_eq!(sizes.min_tokens, 0);
        assert_eq!(sizes.max_tokens, 1024);
        assert_eq!(sizes.bias, Bias::Larger);
        for args in [
            vec!["--depth", "2", "--max-depth", "4"],
            vec!["--depth-bias", "smaller"],
        ] {
            assert!(Cli::try_parse_from(std::iter::once("json-volcano").chain(args)).is_err());
        }
        for args in [
            vec!["--max-depth", "0"],
            vec!["--max-depth", "129"],
            vec!["--min-value-tokens", "3", "--max-value-tokens", "2"],
            vec!["--max-value-tokens", "4097"],
            vec!["--shape", "string", "--size-bias", "smaller"],
        ] {
            assert!(
                Cli::try_parse_from(std::iter::once("json-volcano").chain(args))
                    .unwrap()
                    .config()
                    .is_err()
            );
        }
    }
    #[test]
    fn reported_multi_mixed_pipeline_configuration_finishes_at_the_cap() {
        let config = Cli::try_parse_from([
            "json-volcano",
            "--multi",
            "--shape",
            "mixed",
            "--include-invalid",
            "--max-depth",
            "8",
            "--depth-bias",
            "smaller",
            "--size-bias",
            "larger",
            "--max-value-tokens",
            "1024",
        ])
        .unwrap()
        .config()
        .unwrap();
        let mut output = Vec::new();
        assert_eq!(
            json_volcano::generate_stream(config, &mut output).unwrap(),
            1 << 20
        );
        assert_eq!(output.len(), 1 << 20);
        assert_eq!(output.iter().filter(|&&byte| byte == b'\n').count(), 1024);
        assert_eq!(output[0], b'[');
        assert_eq!(output[1024], b'{');
    }
}
