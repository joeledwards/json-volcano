//! Standalone bounded-memory fixture generator. Corpus format version: 1.
use clap::Parser;
use json_volcano::{Config, Scenario, Shape};
use std::io::{self, Write};

#[derive(Debug, Parser)]
#[command(
    about = "Generate exact-size, seeded JSON test fixtures with bounded memory",
    version
)]
struct Cli {
    /// Total output bytes including syntax/newlines. Accepts B, KiB, MiB, GiB, TiB.
    #[arg(long, value_parser = parse_size)]
    bytes: u64,
    /// Split total bytes evenly among this many root candidates (first receive remainder).
    #[arg(long, default_value_t = 1)]
    candidates: u64,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Array nesting; record objects add one extra container level.
    #[arg(long, default_value_t = 1)]
    depth: usize,
    #[arg(long, default_value = "records")]
    shape: Shape,
    #[arg(long, default_value = "valid")]
    scenario: Scenario,
    /// Probability per generated string token (not byte); range 0..100.
    #[arg(long, default_value_t = 2)]
    escape_percent: u8,
    /// Probability per generated string token; sum with escape-percent <= 100.
    #[arg(long, default_value_t = 2)]
    unicode_percent: u8,
    /// Report corpus version, seed, bytes, elapsed seconds and MiB/s on stderr.
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
fn execute(cli: Cli) -> io::Result<()> {
    let config = Config {
        bytes: cli.bytes,
        candidates: cli.candidates,
        seed: cli.seed,
        depth: cli.depth,
        escape_percent: cli.escape_percent,
        unicode_percent: cli.unicode_percent,
        shape: cli.shape,
        scenario: cli.scenario,
    };
    // Validate before locking stdout or starting measurement.
    config
        .validate()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let start = std::time::Instant::now();
    let bytes = json_volcano::generate(
        config,
        io::BufWriter::with_capacity(64 * 1024, io::stdout().lock()),
    )?;
    if cli.stats {
        let seconds = start.elapsed().as_secs_f64();
        writeln!(
            io::stderr().lock(),
            "corpus=1 seed={} bytes={bytes} candidates={} depth={} shape={:?} scenario={:?} escape_percent={} unicode_percent={} elapsed_s={seconds:.6} mib_s={:.3}",
            cli.seed,
            cli.candidates,
            cli.depth,
            cli.shape,
            cli.scenario,
            cli.escape_percent,
            cli.unicode_percent,
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
        assert_eq!(cli.bytes, 1 << 30);
        assert_eq!(cli.shape, Shape::String);
        assert_eq!(cli.scenario, Scenario::Mixed);
        assert!(Cli::try_parse_from(["json-volcano"]).is_err());
        assert!(
            Cli::try_parse_from(["json-volcano", "--bytes", "1MiB", "--scenario", "oops"]).is_err()
        );
    }
}
