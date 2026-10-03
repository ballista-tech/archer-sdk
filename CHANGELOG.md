# Changelog

Every release of `archer-sdk` is recorded here, newest first.

Versions follow [Semantic Versioning](https://semver.org). While the crate is
below 1.0, a **minor** bump (`0.1` → `0.2`) may contain breaking changes and a
**patch** bump (`0.1.0` → `0.1.1`) never does, so `archer-sdk = "0.1"` in your
`Cargo.toml` only ever picks up compatible releases.

Each release has up to three sections, and an empty one is left out:

- **New** — added instructions, builders, helpers, types or features.
- **Breaking** — anything that can stop existing code compiling or change what
  it does: removed or renamed items, changed signatures, changed defaults, and
  major-version bumps of dependencies whose types appear in the public API
  (the Solana and SPL crates). Each entry says how to migrate.
- **Fixes** — corrected behaviour, with no API change.

Each release also states the Solana crate versions it is built against, since
`Pubkey` and `Instruction` in the public API come from them and must match the
ones in your own dependency tree.

## [Unreleased]

Changes merged to `main` that are not in a release yet.

## [0.1.0] - 2026-10-03

The first versioned release, and the first published to crates.io. It is the
SDK as it stands today, on the Solana 2.x crates. Pin to `0.1` to stay on this
line.

**Built against:** `solana-program` 2.2, `solana-sdk` / `solana-client` 2.2
(feature `client`), `spl-token` 8, `spl-token-2022` 8,
`spl-associated-token-account` 6.

### New

- **On-chain surface** (`onchain`, re-exported at the crate root): account
  layouts (`MarketStateHeader`, `MakerBook`, `MakerRegistry`, `ArcherAccount`),
  the instruction enum and its parameters, error codes, events and their
  discriminators, and protocol constants.
- **Instruction builders** (`ix_builder`, `onchain::builders`): swaps, maker
  operations, market creation and admin, taking human-readable amounts.
- **Quoting math** (`math`): tick, lot and fee conversions, book construction
  from a spread, and `math::engine`, which reproduces the program's exact
  integer arithmetic.
- **Limit orders** (`limit_order`): place, modify, cancel, cancel-all and
  replace-all over `UpdateBookLimit` with compare-and-set level ops, post-only
  handling, collateral computation, and ladder discovery across makers.
- **Delegated accounts** (`identity`, `archer_account`): `Identity` for a
  wallet or a platform acting through an ArcherAccount, plus create, fund,
  delegate, revoke, deposit and withdraw instructions.
- **Account decoding** (`accounts`): typed parsing with discriminator checks,
  and balance, level and spread helpers.
- **Address derivation** (`pda`): every program address, with verification
  helpers for addresses supplied from outside.
- **RPC client** (`client`, behind the `client` feature): `ArcherClient`, an
  async client with per-market config caching.

### Breaking

Nothing for anyone already building against the current `main`.

The `0.0.1` git tag points at a much older commit and was never published. If
you pinned to that tag, this release differs from it:

- Limit-order writes go through `UpdateBookLimit` with compare-and-set ops.
- Instruction builders return a single `Instruction` rather than a `Vec`.
- `append_authority` is removed.
- `UpdateBookRescale` and `MarketStatus::Frozen` are added.
- The Archer fee treasury address is updated.

[Unreleased]: https://github.com/ballista-tech/archer-sdk/compare/0.1.0...HEAD
[0.1.0]: https://github.com/ballista-tech/archer-sdk/releases/tag/0.1.0
