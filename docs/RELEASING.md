# Releasing FusionGraph to crates.io

**Status:** publish-ready as of 2026-07-03. All four crate names are
unregistered on crates.io (verified via the API). The leaf crates pass
`cargo publish --dry-run` including build verification; the dependent crates
can only be fully validated at publish time because they resolve
`fusiongraph-core`/`-ontology` against the live index (standard workspace
chicken-and-egg).

## Prerequisites

- crates.io account with a verified email, and `cargo login` completed
- Clean working tree on `main` with CI green
- `cargo --version` >= the workspace MSRV toolchain (rust-version = 1.85,
  bounded by iceberg 0.5.1)

## Pre-flight checklist

```bash
git status --short              # must be empty
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --target x86_64-apple-darwin --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Publish order (dependency order matters)

```bash
# 1. Leaf crates (no internal dependencies)
cargo publish -p fusiongraph-core
cargo publish -p fusiongraph-ontology

# 2. Wait for the index to pick both up (usually < 1 minute), then:
cargo publish -p fusiongraph-datafusion

# 3. FFI last (depends on core)
cargo publish -p fusiongraph-ffi
```

If step 2 or 3 fails with `no matching package named 'fusiongraph-core'`,
the index has not propagated yet — wait a minute and retry.

## Versioning

`version` is inherited from `[workspace.package]` in the root `Cargo.toml`;
the internal `[workspace.dependencies]` entries carry a matching `version`
requirement (required for publishing — the `path` component is stripped on
upload). **Bump both together**: the single `version` field plus the four
internal `version = "..."` requirements.

Tag releases: `git tag v0.1.0 && git push origin v0.1.0`.

## After publishing

- Verify docs.rs built all four crates (it builds with default features, so
  the `iceberg` module is included)
- Uncomment/verify the crates.io + docs.rs badges in `README.md`
- Open the `datafusion-contrib` proposal (ROADMAP M5): the pitch is the
  README benchmark table plus the `graph_traverse` SQL surface

## Notes

- Each crate packages its own `LICENSE` copy plus the workspace `README.md`
- `fusiongraph-datafusion`'s benches/examples include the test-support
  memory catalog via `#[path]`; those files ship in the package (they are
  under `tests/`) and compile only as dev targets
