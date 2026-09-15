# Agent instructions — Rust-Toolchain

Follow the project's Rust-Toolchain quality ownership rules.

The public command is `rust-tc` (a wrapper around `just`). Do not tell developers to run `just` directly.

## After meaningful code changes

```bash
rust-tc quick
```

Fix failures before continuing.

## Before completing substantial work

```bash
rust-tc doctor
```

## Hard rules

- Do not add `cargo-audit` to the default pipeline (`cargo-deny` owns advisories).
- Do not blanket-suppress Clippy/`allow` warnings to force green.
- Do not invent fuzz targets just to satisfy the toolchain.
- Do not run `rust-tc mutants`, `rust-tc miri`, `rust-tc fuzz`, or `rust-tc features-deep` after every edit.

## Project-specific notes (LutrisArtFetcher)

- Binary-only crate, no Cargo features: the `test` recipe runs Nextest only
  (no `cargo test --doc` step — Cargo errors with "no library targets found").
  `rust-tc semver` does not apply (no public API contract).
- `deny.toml` allows `MPL-2.0` (option-ext via dirs), reviewed against the
  real dependency tree.
- `RUSTSEC-2024-0436` (`paste`, unmaintained, no CVE) is ignored: build-time
  proc-macro pinned by ratatui 0.29. Revisit on ratatui upgrade.
