# Releasing json-volcano 1.0.0

The library and optional CLI retain corpus version 1. Version 1.0.0 establishes
the documented public configuration, deterministic byte corpus and streaming
I/O contract. The CLI now has a finite 1 MiB default, optional infinite mode,
additional root shapes, write fragmentation and depth/value-size distributions.
Legacy `records`/`string` configurations retain corpus-v1 bytes; record all
new options when reproducing extended fixtures. Parser validation is owned by json-skimmer consumer tests, so
packaging and publishing require no unpublished sibling crates.

From this repository, run lightweight checks using the workspace wrapper:

```bash
../../scripts/with-rust cargo fmt --check
../../scripts/with-rust cargo test --offline --lib --bins
../../scripts/with-rust cargo test --offline --no-default-features --lib
../../scripts/with-rust cargo package --offline
```

Review the packaged file list and release changes. Git commits, tags and pushes
are developer-managed. Publish from a private developer credential context,
not the workspace-local Cargo home (see the workspace tooling guide):

```bash
cargo publish --dry-run --locked
cargo publish --locked
```

Once crates.io exposes version 1.0.0, regenerate consumer lockfiles and run
lightweight tests from each consumer repository:

```bash
../../scripts/with-rust cargo test --lib --bins
```

This downloads the published dependency if needed. Review Cargo.lock for a
registry source and checksum for json-volcano, rather than a sibling path.
Do not commit temporary local patch configuration or path-resolved lockfiles.
