# json-volcano

`json-volcano` produces exact-size, reproducible fixtures on stdout. It uses a
seeded arithmetic PRNG, not `/dev/urandom`. The library has no runtime dependencies; the optional CLI uses Clap.
Generation holds a 32 KiB block and one small record, plus the CLI's 64 KiB
output buffer; memory does not grow with the target or candidate size. The
format/algorithm is identified as corpus version 1. Keep the seed, all options,
corpus version, and executable revision with performance results.

Build from the json-volcano project (commands here are manual, not automatic tests):

```bash
../../scripts/with-rust cargo build --offline --release --features cli
```

Generate one exact 1 GiB candidate, or a 10 GiB stream of 1,024 candidates:

```bash
./target/release/json-volcano --bytes 1GiB --seed 42 --stats > one-gib.json
./target/release/json-volcano --bytes 10GiB --candidates 1024 --seed 42 --stats > ten-gib.json
```

Every byte counts toward `--bytes`: syntax, payload, whitespace, and newlines.
Candidates divide that budget evenly; the first candidates receive remainder
bytes. Tiny sizes that cannot hold the selected syntax are rejected. A valid
single-candidate fixture is one JSON document; multiple candidates form a JSON
stream, not one document. Valid/invalid candidates have newline separators.
Truncated input ends without a newline or closing syntax.

| Option | Default | Contract |
| --- | --- | --- |
| `--bytes SIZE` | required | Positive total fitting `u64`; integer bytes or case-sensitive B/KiB/MiB/GiB/TiB suffix |
| `--candidates N` | 1 | Positive root candidate count; total bytes must accommodate every candidate |
| `--seed N` | 42 | Any `u64`; same options and corpus version produce identical bytes |
| `--shape records` | records | Nested root arrays starting with `null`, followed by randomized objects with number, boolean, null and string fields; remaining tail bytes are whitespace |
| `--shape string` | | Nested root arrays containing one giant string per candidate |
| `--depth N` | 1 | 1–128 array levels; records add one object level |
| `--scenario valid` | valid | All candidates complete successfully |
| `--scenario invalid` | | Every candidate gets `truX` near its end |
| `--scenario mixed` | | Alternating valid/invalid candidates, starting valid; requires at least two candidates |
| `--scenario truncated` | | Ends inside a string at EOF; requires exactly one candidate |
| `--escape-percent N` | 2 | String-token probability of an escape: newline, quote, backslash, or Unicode escape |
| `--unicode-percent N` | 2 | String-token probability of raw UTF-8: accented Latin, CJK, or emoji |
| `--stats` | disabled | Report corpus version, parameters, bytes, elapsed time and MiB/s on stderr |

Probabilities are per generated token, not exact percentages of output bytes.
Their sum must not exceed 100. Complete escape/UTF-8 tokens are never cut to
meet the byte budget. At block or payload boundaries, ASCII fills a short
remaining string span. String values in record mode contain 48 generated
tokens. Very small records-mode fixtures may contain only `null` and padding.
Changing read chunk sizes does not change output.

Examples for resource and recovery checks:

```bash
# A giant scalar within a giant candidate; no escapes or non-ASCII.
./target/release/json-volcano --bytes 1GiB --shape string \
  --escape-percent 0 --unicode-percent 0 > giant-string.json

# Late invalid candidates interspersed with valid candidates.
./target/release/json-volcano --bytes 1GiB --candidates 128 \
  --scenario mixed > mixed.json

# Force a late rollback or EOF discard after substantial spill.
./target/release/json-volcano --bytes 1GiB --scenario invalid > invalid.json
./target/release/json-volcano --bytes 1GiB --scenario truncated > truncated.json
```

Output write errors and final flush errors stop generation with a nonzero exit.
A closed stdout pipe exits successfully, as a consumer may deliberately stop
early; statistics are only emitted after complete generation and flush. Shell
redirection creates/truncates files before the command runs; partial fixtures
can remain after an error. Generation does not fsync output.

## Measuring FLON separately

The following commands are run from the sibling FLON repository after generating
the fixture there (or supplying its absolute path). Build FLON separately.

Generate fixtures before starting FLON timing. For example:

```bash
./target/release/flon --color never --summary one-gib.json > /dev/null
./target/release/flon --profile infinite --color never --confirmed-memory 64KiB \
  one-gib.json > /dev/null
```

The first measures live output; the second forces confirmed spill with no
candidate output limit. For a 10 GiB input candidate, generated FLON can exceed
10 GiB, so use unlimited confirmed output when measuring throughput. Choose
an explicit spool directory when comparing disks. Capture FLON summaries and
version/build metadata separately for each run; generator statistics measure
generation plus output writes, not FLON speed. File reads may hit the OS cache.
A pipeline is useful for end-to-end testing but its throughput can be capped
by the generator. See [FLON performance guidance](../flon/docs/performance.md) for measurement
limits and manual disk-exhaustion checks.

Unit tests (small bounded datasets only):

```bash
../../scripts/with-rust cargo test --offline --features cli --lib --bins
```

Tests cover exact bytes and a fixed corpus snapshot, deterministic seeds and
read sizes, valid/invalid/mixed/truncated lifecycle, whole/every two-chunk
split/one-byte parsing, Unicode and escapes, numeric fields, option/size limits,
partial writes, WriteZero, storage failure, broken pipes, and final flush errors.
A small prefix of a `u64::MAX` target checks fixed buffer bounds without creating
a huge fixture. No GiB-scale generation or compiled CLI integration runs are
part of these unit checks.

## Library use

This is an independent project beside json-skimmer and FLON, not a member of
either Cargo workspace. Both can use it as a development dependency without
adding generator code to their runtime dependencies:

```toml
[dev-dependencies]
json-volcano = { path = "../json-volcano", default-features = false }
```

For an installed command, run `cargo install --path . --features cli` from
this project; building alone leaves the binary under its own `target/`.


From a consumer, depend on `json-volcano` without the `cli` feature. The
library does not depend on Clap or json-skimmer. json-skimmer is used only as
a development dependency for parser-event validation; independent reference
parser validation remains a coverage gap.

```rust
use std::io::Read;
use json_volcano::{Config, Generator};

let mut source = Generator::new(Config {
    bytes: 1024,
    ..Config::default()
}).unwrap();
let mut fixture = Vec::new();
source.read_to_end(&mut fixture).unwrap();
assert_eq!(fixture.len(), 1024);
```

`Generator` implements `Read`; `generate(config, writer)` copies to a writer
and checks its final flush. Neither retains the full fixture. `Config::default()`
selects a 1 KiB valid records fixture, one candidate, depth 1, seed 42, and
2 percent each escape/Unicode probabilities. Shape and scenario names parse
case-sensitively through `FromStr`.

The original binary was developed under FLON as `flon-json-gen`. Moving it
to this independent project and naming it `json-volcano` preserves standalone corpus-v1 bytes and
options. Historical performance logs retain the original executable paths;
record the new generator build identity for future measurements. Crate-name
availability has not been checked; publishing is a separate release action.

See the [coverage matrix](docs/test-coverage.md) for tested contracts and remaining gaps.
