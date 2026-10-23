# Test coverage

The generator library is independent of json-skimmer at runtime. Parser-event
validation lives in json-skimmer’s `src/volcano_tests.rs`, consuming the published
generator as a development dependency. The generator has no parser dependency.

| Behavior | Coverage | Remaining gaps |
| --- | --- | --- |
| Exact output and reproducibility | Byte budgets, small exact fixtures, corpus-v1 golden snapshot, default configuration, seed differences and identical output across read sizes | Broader frozen record snapshots |
| Grammar and candidate lifecycle | Consumer tests in json-skimmer: valid, late-invalid, mixed and truncated inputs, candidate counts and errors; whole input, every two-chunk split and one-byte chunks | Independent reference-parser validation |
| Token content and nesting | Consumer tests in json-skimmer: ASCII, raw UTF-8, JSON escapes, numbers/booleans/nulls, record and giant-string shapes, configurable depth | Broader nested shape corpus |
| Root types | Object-only, array-only and alternating roots; parser checks valid/invalid/truncated lifecycle and exact output sizes, whole/every split/one-byte | More frozen corpus snapshots |
| Cargo feature defaults | `cargo test --offline --bin json-volcano` exercises default binary availability; library-only unit tests use `--no-default-features --lib` | Installed-package checks remain manual |
| Finite default and infinite source | Bare CLI configuration, single/multi/count aliases and conflicts; exact reported mixed/invalid/depth/size option combination completes at 1 MiB/1024 roots; exact 1 MiB default; deterministic bounded infinite prefixes across read sizes, changing values, mixed shapes/scenarios, BrokenPipe termination | Actual process signals and installed CLI checks (manual only) |
| Depth cap and bias | Actual parser-enforced caps 1/4/128, both root types and giant strings, 32-root bounded infinite prefixes, smaller/uniform/larger depth ordering, whole/every split/one-byte parsing; fixed depth remains available | Broader sampled tree topology (current topology is nested roots plus records) |
| Value-size bias | Public parser observes bounded string sizes and ordered smaller/uniform/larger means; independent controls, range validation, fixed 0/4096-token bounds with huge-target bounded prefixes, finite size/lifecycle across chunk boundaries | Number-magnitude and per-record byte-size distributions are not configurable |
| Write fragmentation | Exact repeating write patterns, 65536-byte boundary and empty input, final partial fragments, unchanged UTF-8/escape bytes, per-fragment/final flush, partial writes, WriteZero, BrokenPipe, read errors/Interrupted retry, flush failures | Consumer OS-read boundaries cannot be guaranteed; timing/pacing is not implemented |
| Limits and resources | Invalid size/option combinations, numeric overflow, huge declared targets with bounded prefix reads and buffer capacity checks | Actual GiB-scale runs and measured RSS |
| Output I/O | Partial writes, WriteZero, storage failure, broken pipes, exact output prefixes, final flush failure | Compiled CLI/environment tests |

Run lightweight library and CLI unit tests from this project:

```bash
../../scripts/with-rust cargo test --offline --lib --bins
../../scripts/with-rust cargo test --offline --bin json-volcano
../../scripts/with-rust cargo test --offline --no-default-features --lib
```

GiB-scale fixture generation and pipelines are manual opt-in checks. See the
[project guide](../README.md) and [FLON performance checklist](../../flon/docs/performance.md).
