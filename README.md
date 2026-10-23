# json-volcano

`json-volcano` produces exact-size, reproducible fixtures on stdout. It uses a
seeded arithmetic PRNG, not `/dev/urandom`. The CLI is enabled by default and uses Clap. Library-only consumers can set
`default-features = false` to avoid CLI dependencies.
Generation holds a 32 KiB block and one bounded record (up to 32 KiB with
size sampling), plus a copy buffer of up to 64 KiB and the CLI's optional 64 KiB
output buffer; memory does not grow with the target or candidate size. The
format/algorithm is identified as corpus version 1. Keep the seed, all options,
corpus version, and executable revision with performance results.

## Defaults and stream controls

From a source checkout, `cargo run -- ...` works with the default features:

```bash
cargo run -- --multi --shape mixed --include-invalid --max-depth 8 \
  --depth-bias smaller --size-bias larger --max-value-tokens 1024
```

Running `json-volcano` with no arguments emits exactly **1 MiB**, as one valid
root array containing randomized records, then exits. `--bytes` changes the
finite cap; syntax, padding and separators all count. The seed defaults to 42.
`--single` makes the default explicit; `--records N` (alias `--candidates N`)
shares the total budget among N root values. `--multi` selects 1024 roots.
Multiple roots form a JSON stream, rather than one JSON document.

```bash
json-volcano
json-volcano --single --shape object --bytes 4KiB
json-volcano --records 12 --shape mixed --include-invalid --bytes 1MiB
json-volcano --multi --shape arrays --max-depth 8 --depth-bias smaller
json-volcano --multi --shape objects --size-bias larger --max-value-tokens 1024
json-volcano --records 4 --fragment-bytes 1,2,7 --flush-fragments
json-volcano --scenario truncated --bytes 4KiB --fragment-bytes 1
json-volcano --infinite --shape mixed --include-invalid --record-bytes 4KiB
```

Shape and validity are independent:

| Shape | Structure |
| --- | --- |
| `records` (default) | Root arrays containing null and record objects with numeric, boolean, null and string fields; preserves the original corpus |
| `string` | Nested arrays around one giant string per root; preserves the original corpus |
| `array` / `arrays` | Only array containers and scalar values; no object containers |
| `object` / `objects` | Only object containers and scalar values; no array containers; nested roots use `value` members and generated records use seeded names |
| `mixed` | Alternate array-only and object-only roots, starting with an array |

`--scenario valid|invalid|mixed|truncated` controls candidate validity.
`mixed` alternates valid and late-invalid values, starting valid.
`--include-invalid` is a shortcut for that scenario; with no explicit record
count it selects two finite roots. `--single --include-invalid` is rejected.
`truncated` emits one incomplete finite value and omits closing syntax/newline.
Tiny budgets that cannot hold the selected grammar are rejected before output.

**Depth and size distributions.** `--depth N` preserves fixed root nesting.
`--max-depth N` instead samples root nesting from 1 to N and caps the actual
JSON container depth, including generated record objects/arrays. At the cap,
records become scalar strings so they cannot add another container.
`--depth-bias smaller|uniform|larger` requires `--max-depth`; max depth is 1–128.
For each sample, uniform uses one pseudorandom draw, smaller uses the minimum
of two draws, and larger uses their maximum. Seed and options reproduce the
same distribution independently of I/O chunk sizes.

`--size-bias smaller|uniform|larger` separately samples string payload lengths
inside records. `--min-value-tokens` and `--max-value-tokens` bound those lengths
(default sampled range: 1–256; allowed: 0 <= min <= max <= 4096). A token is
ASCII, a complete JSON escape, or one Unicode character, so token counts are
not byte counts. Without these options record strings retain 48 tokens.
These controls do not change number magnitude, record byte budgets, or giant
`string` fixtures. Complete records that do not fit are replaced by padding;
a tiny fixture may contain only the initial null. The total byte cap stays exact.

**Fragmentation.** `--fragment-bytes N` or a repeating comma-separated pattern
controls logical application writes (each size 1–65536, at most 1024 sizes).
Fragments may split UTF-8 or escapes without changing the concatenated bytes.
`--flush-fragments` flushes each fragment. Pipes may merge/split writes, so this
does not guarantee consumer read boundaries. For incomplete JSON, select
`--scenario truncated` as well. There is no timing/pacing directive.

**Infinite mode.** `--infinite` emits indefinitely, using 1 KiB per complete
root by default; `--record-bytes` changes that size. Root shapes and validity
alternate globally, and the PRNG state continues across roots. This mode
conflicts with finite byte/count/single/multi options and rejects truncation.
Ctrl-C/SIGINT or SIGTERM uses normal platform process termination; there is
no signal handler or final flush guarantee, and the last root can be partial.
Broken pipes exit successfully. `--stats` reports only after finite completion,
so infinite runs do not print a final report. Memory stays bounded.

Build from the json-volcano project (commands here are manual, not automatic tests):

```bash
../../scripts/with-rust cargo build --offline --release
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
| `--bytes SIZE` | 1MiB | Positive total fitting `u64`; integer bytes or case-sensitive B/KiB/MiB/GiB/TiB suffix |
| `--candidates N` | 1 | Positive root candidate count; total bytes must accommodate every candidate |
| `--seed N` | 42 | Any `u64`; same options and corpus version produce identical bytes |
| `--shape records` | records | Nested root arrays starting with `null`, followed by randomized objects with number, boolean, null and string fields; remaining tail bytes are whitespace |
| `--shape string` | | Nested root arrays containing one giant string per candidate |
| `--depth N` | 1 | Fixed 1–128 root nesting levels; records add one container level; conflicts with `--max-depth` |
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
../../scripts/with-rust cargo test --offline --lib --bins
```

Generator and consumer tests together cover exact bytes and a fixed corpus snapshot, deterministic seeds and
read sizes, valid/invalid/mixed/truncated lifecycle, whole/every two-chunk
split/one-byte parsing, Unicode and escapes, numeric fields, option/size limits,
partial writes, WriteZero, storage failure, broken pipes, and final flush errors.
A small prefix of a `u64::MAX` target checks fixed buffer bounds without creating
a huge fixture. No GiB-scale generation or compiled CLI integration runs are
part of these unit checks.

## Library use

`Config` / `Generator` / `generate` retain the original exact-size interface.
`StreamConfig` / `StreamGenerator` / `generate_stream` add finite/infinite mode,
fragmentation, maximum depth and sampled value sizes. `Bias`, `ValueSizes` and
`Fragmentation` expose the same controls as the CLI. `StreamConfig::default()`
is a finite 1 MiB single root; its `fixture` is a `Config`. Infinite mode uses
`fixture.bytes` per value and requires `fixture.candidates = 1`.
`copy_fragmented` also accepts any `Read` source. Source read errors propagate
without flushing and discard an unfinished buffered fragment; EOF writes the
last partial fragment and checks flush.

```rust
use json_volcano::{Bias, StreamConfig, StreamMode, ValueSizes};
let mut config = StreamConfig::default();
config.max_depth = Some(8);
config.depth_bias = Bias::Smaller;
config.value_sizes = Some(ValueSizes { min_tokens: 1, max_tokens: 256, bias: Bias::Larger });
// For an unbounded source, explicitly opt in:
config.mode = StreamMode::Infinite;
config.fixture.bytes = 4096; // per complete root
```

This is an independent project beside json-skimmer and FLON, not a member of
either Cargo workspace. Both can use it as a development dependency without
adding generator code to their runtime dependencies:

```toml
[dev-dependencies]
json-volcano = { version = "1.0.0", default-features = false }
```

Install the published command with `cargo install json-volcano --version 1.0.0 --locked`.
Building from source leaves the binary under its own `target/`.


From a consumer, depend on `json-volcano` without the `cli` feature. The
library does not depend on Clap or json-skimmer. Parser-event validation lives in json-skimmer
as consumer unit tests; json-volcano has no dependency on that parser.
Independent reference-parser validation remains a coverage gap.

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
record the new generator build identity for future measurements. Release preparation and publishing instructions are in [the release guide](docs/releasing.md).

See the [coverage matrix](docs/test-coverage.md) for tested contracts and remaining gaps.
