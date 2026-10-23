# Test coverage

The generator library is independent of json-skimmer at runtime. Parser-event
validation uses json-skimmer as a development dependency only.

| Behavior | Coverage | Remaining gaps |
| --- | --- | --- |
| Exact output and reproducibility | Byte budgets, small exact fixtures, corpus-v1 golden snapshot, default configuration, seed differences and identical output across read sizes | Broader frozen record snapshots |
| Grammar and candidate lifecycle | Valid, late-invalid, mixed and truncated inputs, candidate counts and errors; whole input, every two-chunk split and one-byte chunks | Independent reference-parser validation |
| Token content and nesting | ASCII, raw UTF-8, JSON escapes, numbers/booleans/nulls, record and giant-string shapes, configurable depth | Broader nested shape corpus |
| Limits and resources | Invalid size/option combinations, numeric overflow, huge declared targets with bounded prefix reads and buffer capacity checks | Actual GiB-scale runs and measured RSS |
| Output I/O | Partial writes, WriteZero, storage failure, broken pipes, exact output prefixes, final flush failure | Compiled CLI/environment tests |

Run lightweight library and CLI unit tests from this project:

```bash
../../scripts/with-rust cargo test --offline --features cli --lib --bins
```

GiB-scale fixture generation and pipelines are manual opt-in checks. See the
[project guide](../README.md) and [FLON performance checklist](../../flon/docs/performance.md).
