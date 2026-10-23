use super::*;
use json_skimmer::{Event, Mode, Parser};

fn config(shape: Shape, scenario: Scenario) -> Config {
    Config {
        bytes: 512,
        candidates: 1,
        seed: 42,
        depth: 2,
        escape_percent: 15,
        unicode_percent: 15,
        shape,
        scenario,
    }
}
fn bytes(config: Config, chunk: usize) -> Vec<u8> {
    let mut source = Generator::new(config).unwrap();
    let mut all = Vec::new();
    let mut buffer = vec![0; chunk];
    loop {
        // Empty reads must neither consume input nor indicate a persistent EOF.
        assert_eq!(source.read(&mut []).unwrap(), 0);
        let n = source.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        all.extend_from_slice(&buffer[..n]);
    }
    assert_eq!(source.read(&mut buffer).unwrap(), 0);
    all
}
#[derive(Debug, Default, PartialEq, Eq)]
struct Observation {
    events: Vec<String>,
    tokens: Vec<Vec<u8>>,
    completed: usize,
    errors: Vec<json_skimmer::ParseError>,
}
fn observe(input: &[u8], cuts: &[usize], mode: Mode) -> Observation {
    let mut parser = Parser::new(mode);
    let mut seen = Observation::default();
    let mut record = |event: Event<'_>| {
        match &event {
            Event::StringStart { .. } | Event::NumberStart { .. } => seen.tokens.push(Vec::new()),
            Event::StringFragment { bytes, .. } | Event::NumberFragment { bytes, .. } => {
                seen.tokens.last_mut().unwrap().extend_from_slice(bytes);
                return;
            }
            Event::CandidateEnd { .. } => seen.completed += 1,
            Event::Invalid(error) => seen.errors.push(error.clone()),
            _ => (),
        }
        seen.events.push(format!("{event:?}"));
    };
    let mut previous = 0;
    for end in cuts.iter().copied().chain(std::iter::once(input.len())) {
        parser.push(&input[previous..end], &mut record);
        previous = end;
    }
    parser.finish(&mut record);
    seen
}

#[test]
fn exact_sizes_reproducibility_and_candidate_lifecycle_across_chunks() {
    for shape in [Shape::Records, Shape::String] {
        for scenario in [
            Scenario::Valid,
            Scenario::Invalid,
            Scenario::Mixed,
            Scenario::Truncated,
        ] {
            let mut cfg = config(shape, scenario);
            cfg.candidates = if scenario == Scenario::Truncated {
                1
            } else {
                3
            };
            cfg.bytes = 611; // Uneven candidate sizes, some too small for complete records.
            let expected = bytes(cfg.clone(), 8192);
            assert_eq!(expected.len(), 611);
            assert_eq!(expected, bytes(cfg.clone(), 1));
            assert_eq!(expected, bytes(cfg.clone(), 17));
            let baseline = observe(&expected, &[], Mode::Scan);
            let (completed, invalid) = match scenario {
                Scenario::Valid => (3, 0),
                Scenario::Invalid => (0, 3),
                Scenario::Mixed => (2, 1),
                Scenario::Truncated => (0, 1),
            };
            assert_eq!(
                (baseline.completed, baseline.errors.len()),
                (completed, invalid),
                "{shape:?} {scenario:?}"
            );
            if scenario == Scenario::Truncated {
                assert_eq!(
                    baseline.errors[0].kind,
                    json_skimmer::ErrorKind::UnexpectedEnd
                );
            }
            for split in 0..=expected.len() {
                assert_eq!(
                    observe(&expected, &[split], Mode::Scan),
                    baseline,
                    "{shape:?} {scenario:?} split={split}"
                );
            }
            assert_eq!(
                observe(
                    &expected,
                    &(0..expected.len()).collect::<Vec<_>>(),
                    Mode::Scan
                ),
                baseline
            );
        }
    }
}

#[test]
fn scalar_tokens_unicode_escapes_numbers_depth_and_seed() {
    for shape in [Shape::Records, Shape::String] {
        for (escape, unicode) in [(0, 0), (100, 0), (0, 100), (50, 50)] {
            let mut cfg = config(shape, Scenario::Valid);
            cfg.bytes = 8193;
            cfg.depth = 4;
            cfg.escape_percent = escape;
            cfg.unicode_percent = unicode;
            let output = bytes(cfg.clone(), 31);
            let baseline = observe(&output, &[], Mode::Single);
            assert_eq!(baseline.completed, 1);
            assert!(baseline.errors.is_empty());
            assert!(std::str::from_utf8(&output).is_ok());
            assert_eq!(
                observe(
                    &output,
                    &(0..output.len()).collect::<Vec<_>>(),
                    Mode::Single
                ),
                baseline
            );
            if unicode > 0 {
                assert!(output.iter().any(|b| *b >= 128));
            }
            if escape > 0 {
                assert!(output.contains(&b'\\'));
            }
            if shape == Shape::Records {
                assert!(output.windows(7).any(|w| w == b"\"none\":"));
                assert!(baseline.events.iter().any(|e| e.starts_with("NumberStart")));
            }
            cfg.seed += 1;
            assert_ne!(output, bytes(cfg, 31));
        }
    }
}

#[test]
fn exact_minimum_fixtures_and_rejected_configurations() {
    let mut cfg = config(Shape::Records, Scenario::Valid);
    cfg.depth = 1;
    cfg.bytes = 10;
    assert_eq!(bytes(cfg.clone(), 1), b"[null   ]\n");
    cfg.scenario = Scenario::Invalid;
    cfg.bytes = 13;
    assert_eq!(bytes(cfg.clone(), 2), b"[null ,truX]\n");
    cfg.bytes = 12;
    assert!(Generator::new(cfg.clone()).is_err());
    cfg.bytes = 512;
    for candidates in [0, u64::MAX] {
        let mut bad = cfg.clone();
        bad.candidates = candidates;
        assert!(Generator::new(bad).is_err());
    }
    for depth in [0, 129, usize::MAX] {
        let mut bad = cfg.clone();
        bad.depth = depth;
        assert!(Generator::new(bad).is_err());
    }
    for (escape, unicode) in [(101, 0), (0, 101), (50, 51), (255, 255)] {
        let mut bad = cfg.clone();
        bad.escape_percent = escape;
        bad.unicode_percent = unicode;
        assert!(Generator::new(bad).is_err());
    }
    for (scenario, candidates) in [(Scenario::Truncated, 2), (Scenario::Mixed, 1)] {
        let mut bad = cfg.clone();
        bad.scenario = scenario;
        bad.candidates = candidates;
        assert!(Generator::new(bad).is_err());
    }
    cfg.bytes = 0;
    let mut destination = Vec::new();
    assert_eq!(
        generate(cfg, &mut destination).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(destination.is_empty());
}

#[test]
fn enormous_targets_have_bounded_buffering() {
    for shape in [Shape::Records, Shape::String] {
        let mut cfg = config(shape, Scenario::Valid);
        cfg.bytes = u64::MAX;
        cfg.candidates = 1;
        let mut source = Generator::new(cfg).unwrap();
        let mut prefix = [0; 8192];
        for _ in 0..16 {
            source.read_exact(&mut prefix).unwrap();
            assert!(source.pending.capacity() <= BLOCK);
            assert!(source.row.capacity() <= 512);
        }
    }
}

#[test]
fn partial_writes_failures_broken_pipe_and_final_flush() {
    struct Writer {
        bytes: Vec<u8>,
        budget: usize,
        failure: Option<io::ErrorKind>,
        flush_error: bool,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.len() == self.budget {
                return match self.failure {
                    Some(kind) => Err(io::Error::new(kind, "injected write")),
                    None => Ok(0),
                };
            }
            let n = bytes.len().min(3).min(self.budget - self.bytes.len());
            self.bytes.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.flush_error {
                Err(io::Error::other("injected flush"))
            } else {
                Ok(())
            }
        }
    }
    let cfg = config(Shape::Records, Scenario::Valid);
    let expected = bytes(cfg.clone(), 17);
    for failure in [
        None,
        Some(io::ErrorKind::BrokenPipe),
        Some(io::ErrorKind::StorageFull),
    ] {
        for budget in [0, 7, 511] {
            let mut writer = Writer {
                bytes: Vec::new(),
                budget,
                failure,
                flush_error: false,
            };
            let error = generate(cfg.clone(), &mut writer).unwrap_err();
            assert_eq!(error.kind(), failure.unwrap_or(io::ErrorKind::WriteZero));
            assert_eq!(writer.bytes, expected[..budget]);
        }
    }
    let mut writer = Writer {
        bytes: Vec::new(),
        budget: usize::MAX,
        failure: None,
        flush_error: false,
    };
    assert_eq!(generate(cfg.clone(), &mut writer).unwrap(), 512);
    assert_eq!(writer.bytes, expected);
    writer.bytes.clear();
    writer.flush_error = true;
    assert_eq!(
        generate(cfg, &mut writer).unwrap_err().to_string(),
        "injected flush"
    );
    assert_eq!(writer.bytes, expected);
}

#[test]
fn corpus_v1_golden_ascii_string() {
    let mut cfg = config(Shape::String, Scenario::Valid);
    cfg.bytes = 32;
    cfg.depth = 1;
    cfg.escape_percent = 0;
    cfg.unicode_percent = 0;
    assert_eq!(bytes(cfg, 1), b"[\"ZNUgHuo0DSEllk0V5oNUr8I0a1x\"]\n");
}

#[test]
fn public_defaults_and_option_names() {
    let config = Config::default();
    assert_eq!(bytes(config, 7).len(), 1024);
    for (name, shape) in [("records", Shape::Records), ("string", Shape::String)] {
        assert_eq!(name.parse::<Shape>().unwrap(), shape);
    }
    for (name, kind) in [
        ("valid", Scenario::Valid),
        ("invalid", Scenario::Invalid),
        ("mixed", Scenario::Mixed),
        ("truncated", Scenario::Truncated),
    ] {
        assert_eq!(name.parse::<Scenario>().unwrap(), kind);
    }
    for invalid in ["", "Records", "unknown"] {
        assert!(invalid.parse::<Shape>().is_err());
        assert!(invalid.parse::<Scenario>().is_err());
    }
}
