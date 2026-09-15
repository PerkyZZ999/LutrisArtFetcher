set shell := ["bash", "-euo", "pipefail", "-c"]

# ---------------------------------------------------------
# Fast developer workflow
# ---------------------------------------------------------

fmt:
    cargo fmt --all -- --check


check:
    cargo check --workspace --all-targets --all-features


clippy:
    cargo clippy \
        --workspace \
        --all-targets \
        --all-features \
        -- \
        -D warnings


test:
    cargo nextest run \
        --workspace \
        --all-features

    # NOTE: no `cargo test --doc` step — this is a binary-only crate, so Cargo
    # reports "no library targets found". Doctests cannot exist here; the
    # unit tests above are the full suite.


# Fast developer validation.
quick: fmt check clippy test


# Alias developers/agents are expected to use most often.
doctor-check: quick


# ---------------------------------------------------------
# Dependency verification
# ---------------------------------------------------------

deps:
    cargo deny check

    cargo shear --deny-warnings


# ---------------------------------------------------------
# Cargo feature validation
# ---------------------------------------------------------

features:
    cargo hack check \
        --workspace \
        --each-feature \
        --no-dev-deps


features-deep:
    cargo hack check \
        --workspace \
        --feature-powerset \
        --depth 2 \
        --no-dev-deps


# ---------------------------------------------------------
# Comprehensive LOCAL Rust-Toolchain
# ---------------------------------------------------------
#
# NOTE:
# Plain cargo check is intentionally omitted here because
# Clippy already performs compilation/checking as part of its
# analysis.
#
# `rust-tc check` remains useful during normal development because
# it provides the fastest compiler-only feedback path.
#

doctor: fmt clippy test deps features
    @echo
    @echo "Rust-Toolchain: PASS"


# NOTE: no standalone `doctest` recipe — binary-only crate, see `test` above.


# Optional local coverage. Not part of `rust-tc doctor`.
coverage:
    mkdir -p target/rust-toolchain
    cargo llvm-cov \
        --lcov \
        --output-path target/rust-toolchain/lcov.info \
        nextest \
        --workspace \
        --all-features


# ---------------------------------------------------------
# SemVer validation
# ---------------------------------------------------------
#
# Intended primarily for library/public API crates.
#

semver package baseline="origin/main":
    cargo semver-checks \
        --package "{{package}}" \
        --baseline-rev "{{baseline}}"


# ---------------------------------------------------------
# Deep verification
# ---------------------------------------------------------

mutants:
    cargo mutants


miri:
    cargo +nightly miri test


fuzz target:
    cargo +nightly fuzz run "{{target}}"


deep: features-deep
    @echo
    @echo "Feature powerset validation complete."
    @echo "Run mutation, Miri and fuzz checks selectively:"
    @echo "  rust-tc mutants"
    @echo "  rust-tc miri"
    @echo "  rust-tc fuzz <target>"


# ---------------------------------------------------------
# Convenience
# ---------------------------------------------------------

clean-toolchain:
    rm -rf target/rust-toolchain


toolchain-help:
    @echo "Rust-Toolchain"
    @echo
    @echo "  rust-tc check         Fast compiler check"
    @echo "  rust-tc quick         Fast developer quality gate"
    @echo "  rust-tc doctor        Full local Rust-Toolchain validation"
    @echo "  rust-tc features-deep Deeper Cargo feature combinations"
    @echo "  rust-tc semver PKG    Public API compatibility"
    @echo "  rust-tc mutants       Mutation testing"
    @echo "  rust-tc miri          Undefined-behavior checking"
    @echo "  rust-tc fuzz TARGET   Targeted fuzzing"
    @echo "  rust-tc coverage      Optional local LCOV report"
