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

## [0.2.0] - 2026-10-03

Moves the SDK to the current Solana crates. The SDK's own API, the account
layouts and every instruction it builds are unchanged; what changes is the
version of the Solana types in its public API.

**Built against:** `solana-program` 5, `solana-system-interface` 3,
`spl-token` 9, `spl-associated-token-account` 8, and with the `client`
feature `solana-client` 4 (Agave 4.3) and `solana-commitment-config` 3.

### New

- **Offline tests for the `client` feature** (`tests/client.rs`). They run
  `ArcherClient` against a local JSON-RPC stub, so the RPC request and
  response shapes are checked without a network. Run them with
  `cargo test --features client`.

### Breaking

- **Solana crates moved from 2.x to the current majors.** `Pubkey` and
  `Instruction` in the SDK's API are now the types from `solana-program` 5
  (`solana-pubkey` 4, `solana-instruction` 4). A project still on the Solana
  2.x crates gets type-mismatch errors wherever SDK values meet its own Solana
  code. To migrate, move your own `solana-*` and `spl-*` dependencies to the
  versions listed above, then change `archer-sdk` to `"0.2"`.
- **Minimum Rust version raised.** The default build needs Rust 1.89. The
  `client` feature needs Rust 1.97.1, which `solana-client` 4.3 requires.
- **`ArcherClient::with_commitment` takes `CommitmentConfig` from
  `solana-commitment-config`.** `solana-sdk` 5 no longer exports it. Import
  it as `solana_commitment_config::CommitmentConfig`.
- **`spl-token-2022` and `solana-sdk` are no longer dependencies.** The SDK
  never used the first, and used the second only for `CommitmentConfig`. If
  you relied on either arriving transitively, add it to your own `Cargo.toml`.

### Fixes

- **Maker-book scans request base64 explicitly.** `get_all_maker_books` and
  the limit-order ladder scan now ask `getProgramAccounts` for base64 account
  data and decode it themselves, as `solana-client` 4 no longer does this.

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