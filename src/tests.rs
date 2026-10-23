use super::*;

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

#[test]
fn root_shapes_exact_small_outputs_and_truncated_tails() {
    let mut cfg = Config {
        bytes: 20,
        shape: Shape::Object,
        ..Config::default()
    };
    assert_eq!(bytes(cfg.clone(), 1), b"{\"value\":null     }\n");
    cfg.bytes = 80;
    cfg.scenario = Scenario::Invalid;
    assert!(bytes(cfg.clone(), 7).ends_with(b",\"invalid\":truX}\n"));
    cfg.scenario = Scenario::Truncated;
    assert!(bytes(cfg, 1).ends_with(b",\"unfinished\":\""));
    for name in ["object", "objects", "array", "arrays", "mixed"] {
        assert!(name.parse::<Shape>().is_ok());
    }
    let cfg = Config {
        bytes: 64,
        candidates: 2,
        shape: Shape::Mixed,
        ..Config::default()
    };
    let output = bytes(cfg, 7);
    assert_eq!(output.len(), 64);
    assert_eq!(output[0], b'[');
    assert_eq!(output[32], b'{');
}

fn stream_bytes(config: StreamConfig, count: u64, read_size: usize) -> Vec<u8> {
    let mut source = StreamGenerator::new(config).unwrap().take(count);
    let mut output = Vec::new();
    let mut buffer = vec![0; read_size];
    loop {
        assert_eq!(source.read(&mut []).unwrap(), 0);
        let n = source.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..n]);
    }
    output
}

#[test]
fn infinite_prefixes_are_repeatable_vary_and_ignore_read_boundaries() {
    for shape in [
        Shape::Records,
        Shape::String,
        Shape::Object,
        Shape::Array,
        Shape::Mixed,
    ] {
        for scenario in [Scenario::Valid, Scenario::Invalid, Scenario::Mixed] {
            let config = StreamConfig {
                fixture: Config {
                    bytes: 1024,
                    shape,
                    scenario,
                    ..Config::default()
                },
                mode: StreamMode::Infinite,
                ..StreamConfig::default()
            };
            let expected = stream_bytes(config.clone(), 3 * 1024 + 19, 8192);
            assert_eq!(expected.len(), 3 * 1024 + 19);
            assert_ne!(&expected[..1024], &expected[1024..2048]);
            for chunk in [1, 7, 1024] {
                assert_eq!(
                    stream_bytes(config.clone(), expected.len() as u64, chunk),
                    expected
                );
            }
        }
    }
    let default = StreamConfig::default();
    assert_eq!(stream_bytes(default, u64::MAX, 8192).len(), 1 << 20);
}

#[derive(Default)]
struct RecordingWriter {
    bytes: Vec<u8>,
    writes: Vec<usize>,
    flushes: usize,
}
impl Write for RecordingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes.push(bytes.len());
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}
#[test]
fn fragmentation_controls_writes_without_changing_utf8_or_escape_bytes() {
    let input = "a雪\\u0041🙂z".as_bytes();
    let mut out = RecordingWriter::default();
    copy_fragmented(
        input,
        &mut out,
        &Fragmentation {
            sizes: vec![1, 2, 7],
            flush_each: true,
        },
    )
    .unwrap();
    assert_eq!(out.bytes, input);
    assert_eq!(out.writes, [1, 2, 7, 1, 2, 2]);
    assert_eq!(out.flushes, 7);
    let config = StreamConfig {
        fixture: Config {
            bytes: 611,
            candidates: 3,
            shape: Shape::Mixed,
            scenario: Scenario::Mixed,
            ..Config::default()
        },
        fragmentation: Fragmentation {
            sizes: vec![1, 3, 17],
            flush_each: false,
        },
        ..StreamConfig::default()
    };
    let mut out = RecordingWriter::default();
    assert_eq!(generate_stream(config.clone(), &mut out).unwrap(), 611);
    assert_eq!(out.bytes, bytes(config.fixture, 31));
    assert_eq!(&out.writes[..6], &[1, 3, 17, 1, 3, 17]);
    assert_eq!(out.flushes, 1);
}

#[test]
fn fragmented_io_propagates_partial_write_read_and_flush_failures() {
    struct Partial {
        bytes: Vec<u8>,
        budget: usize,
        kind: Option<io::ErrorKind>,
        fail_flush: bool,
    }
    impl Write for Partial {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.len() == self.budget {
                return self
                    .kind
                    .map_or(Ok(0), |kind| Err(io::Error::new(kind, "write failure")));
            }
            let n = bytes.len().min(2).min(self.budget - self.bytes.len());
            self.bytes.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                Err(io::Error::other("flush failure"))
            } else {
                Ok(())
            }
        }
    }
    let pattern = Fragmentation {
        sizes: vec![3, 7],
        flush_each: false,
    };
    for failure in [
        None,
        Some(io::ErrorKind::BrokenPipe),
        Some(io::ErrorKind::PermissionDenied),
    ] {
        for budget in [0, 1, 9] {
            let mut out = Partial {
                bytes: Vec::new(),
                budget,
                kind: failure,
                fail_flush: false,
            };
            assert_eq!(
                copy_fragmented(b"abcdefghijk".as_slice(), &mut out, &pattern)
                    .unwrap_err()
                    .kind(),
                failure.unwrap_or(io::ErrorKind::WriteZero)
            );
            assert_eq!(out.bytes, &b"abcdefghijk"[..budget]);
        }
    }
    let mut out = Partial {
        bytes: Vec::new(),
        budget: usize::MAX,
        kind: None,
        fail_flush: true,
    };
    assert_eq!(
        copy_fragmented(b"abc".as_slice(), &mut out, &pattern)
            .unwrap_err()
            .to_string(),
        "flush failure"
    );
    assert_eq!(out.bytes, b"abc");
    struct ReadFailure {
        interrupted: bool,
        remaining: usize,
    }
    impl Read for ReadFailure {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.remaining == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            buffer[0] = b'x';
            self.remaining -= 1;
            Ok(1)
        }
    }
    let mut out = RecordingWriter::default();
    assert_eq!(
        copy_fragmented(
            ReadFailure {
                interrupted: false,
                remaining: 4
            },
            &mut out,
            &pattern
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert_eq!(out.bytes, b"xxx"); // Unfinished fragment is buffered when read fails.
    let config = StreamConfig {
        fixture: Config {
            bytes: 1024,
            ..Config::default()
        },
        mode: StreamMode::Infinite,
        fragmentation: pattern,
        ..StreamConfig::default()
    };
    let mut out = Partial {
        bytes: Vec::new(),
        budget: 57,
        kind: Some(io::ErrorKind::BrokenPipe),
        fail_flush: false,
    };
    assert_eq!(
        generate_stream(config.clone(), &mut out)
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(out.bytes, stream_bytes(config, 57, 1));
}

#[test]
fn new_option_limits_reject_before_writing() {
    let mut bad = Vec::new();
    for sizes in [vec![0], vec![65537], vec![1; 1025]] {
        bad.push(StreamConfig {
            fragmentation: Fragmentation {
                sizes,
                flush_each: false,
            },
            ..StreamConfig::default()
        });
    }
    for maximum in [0, 129, usize::MAX] {
        bad.push(StreamConfig {
            max_depth: Some(maximum),
            ..StreamConfig::default()
        });
    }
    bad.push(StreamConfig {
        depth_bias: Bias::Smaller,
        ..StreamConfig::default()
    });
    for (min_tokens, max_tokens) in [(2, 1), (0, 4097), (usize::MAX, usize::MAX)] {
        bad.push(StreamConfig {
            value_sizes: Some(ValueSizes {
                min_tokens,
                max_tokens,
                bias: Bias::Uniform,
            }),
            ..StreamConfig::default()
        });
    }
    for scenario in [Scenario::Truncated, Scenario::Valid] {
        bad.push(StreamConfig {
            fixture: Config {
                scenario,
                candidates: if scenario == Scenario::Valid { 2 } else { 1 },
                ..Config::default()
            },
            mode: StreamMode::Infinite,
            ..StreamConfig::default()
        });
    }
    for config in bad {
        let mut out = RecordingWriter::default();
        assert_eq!(
            generate_stream(config, &mut out).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(out.bytes.is_empty());
        assert_eq!(out.flushes, 0);
    }
}

#[test]
fn maximum_fragment_and_value_sizes_remain_bounded_and_reproducible() {
    for length in [0, 65535, 65536, 65537] {
        let input = vec![b'x'; length];
        let mut output = RecordingWriter::default();
        copy_fragmented(
            input.as_slice(),
            &mut output,
            &Fragmentation {
                sizes: vec![65536],
                flush_each: false,
            },
        )
        .unwrap();
        assert_eq!(output.bytes, input);
        assert!(output.writes.iter().all(|&n| n <= 65536));
        assert_eq!(output.flushes, 1);
    }
    for shape in [Shape::Array, Shape::Object] {
        for tokens in [0, 4096] {
            let config = StreamConfig {
                fixture: Config {
                    bytes: u64::MAX,
                    shape,
                    escape_percent: 100,
                    unicode_percent: 0,
                    ..Config::default()
                },
                max_depth: Some(1),
                value_sizes: Some(ValueSizes {
                    min_tokens: tokens,
                    max_tokens: tokens,
                    bias: Bias::Uniform,
                }),
                ..StreamConfig::default()
            };
            let prefix = stream_bytes(config.clone(), 65537, 8192);
            assert_eq!(prefix.len(), 65537);
            assert_eq!(stream_bytes(config, 65537, 1), prefix);
        }
    }
}
