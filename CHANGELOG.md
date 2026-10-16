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

## [0.4.0] - 2026-10-16

Fixes the taker swap builders

### Breaking

- **`identity::archer_account_token_address` (and
  `archer_account::token_address`) takes the token program:**
  `(archer_account, mint, token_program)`. The two-argument form derived the
  classic SPL Token address for every mint, which is wrong for a Token-2022
  mint. To migrate, pass `&spl_token::ID` for a classic mint, or the market's
  `base_token_program` / `quote_token_program` from `MarketConfig`.
- **`archer_account::deposit_tokens` takes the token program and the mint's
  decimals, and returns `Instruction`:**
  `(from, from_authority, archer_account, mint, token_program, decimals,
  amount)`. It builds a `TransferChecked` for either token program instead of
  a classic `Transfer`, and can no longer fail, so drop the `?`.

### New

- **`ix_builder::maker::build_withdraw_all_ix`** drains both sides of a book
  you have read; **`build_withdraw_lots_ix`** takes lots directly;
  **`resolve_withdraw_lots`** is the human-amount conversion the builders use.
  **`accounts::withdrawable_lots`** is the free balance the program caps a
  withdrawal at.
- **`ArcherSDKError::WithdrawAllNeedsBook`**, **`RegistryFullSplitRequired`**
  and **`InvalidBps`**.

### Fixes

- **Swap builders send lots, not atoms.** `ix_builder::swap::*` and the
  `ArcherClient::build_*` taker methods converted human amounts with the atoms
  helpers, but the program reads `SwapParams.amount` and `threshold` in lots.
- **Swap builders no longer append the event authority.**
- **`builder_fee_ppm` is checked client-side.** A value above
  `MAX_BUILDER_FEE_PPM` (10 000) returns `BuilderFeeTooHigh` instead of
  reaching the program.
- **`build_ladder` includes limit-order books.** It skipped every book at mid
  0, which is where the program pins every limit-order book, so
  `get_lo_ladder` never returned a limit order. Only market-maker books
  without a mid are skipped now, along with suspended books and levels whose
  absolute price is not positive.
- **Withdraw drain sentinel.** `build_withdraw_ix` turned `f64::MAX` into
  `u64::MAX`, which the program rejects with `InsufficientBalance` (402): it
  has no drain sentinel. The stateless builder now returns
  `WithdrawAllNeedsBook` for `f64::MAX`; `ArcherClient::build_withdraw` reads
  the book and sends its exact free lots.
- **Token-2022 token accounts.** `archer_account::withdraw_tokens` derived
  both token accounts with the classic SPL Token program, ignoring its
  `token_program` argument, so a Token-2022 withdrawal named accounts that do
  not exist. It derives with the program given now, as do the address and
  deposit helpers under Breaking above.
- **`get_registered_maker_books` skips closed seats** instead of failing on
  the first registry entry whose book account is gone, and reads the seat
  count through `get_makers` rather than slicing with it.
- **Level readers scan every slot.** `accounts::active_bid_levels`,
  `active_ask_levels`, `best_bid_price` and `best_ask_price` stopped at the
  first empty slot or looked at slot 0 only. A fully filled level leaves an
  empty slot ahead of deeper ones on a market-maker book, which the program
  reads past, and so do these now. A best price is `None` rather than `0.0`
  when the level's absolute price is not positive.
- **Multi-chunk limit-order placements seat correctly.** When a batch of more
  than 32 ops seats the book in its first chunk, later placing chunks now pass
  the registry as it will be then, with the book in it, instead of the
  pre-seat set (`RegistryBooksMismatch`, 525). On a full registry the seat
  comes from an eviction the client cannot predict, so that case returns
  `RegistryFullSplitRequired`.
- **`encode_update_book_data` panics on more than 16 levels per side**
  instead of silently encoding the first 16. `build_book_update` still returns
  `TooManyLevels` first.
- **`LevelOp::decode` rejects a side byte other than 0 or 1.**
- **`bps_to_ticks` rejects a non-finite price or bps** (`InvalidPrice`,
  `InvalidBps`) instead of returning a one-tick offset for `NaN`.
- **`ArcherClient` mint reads check the account length** before reading the
  decimals byte, and the account-not-found match is case-insensitive.
- **README:** a closed registered book is a vacant seat for `UpdateBookLimit`
  but fails a `Swap` that lists it; `Swap` never reads the registry.

## [0.3.0]

Introduce the self-maintaining maker registry: limit-order books
seat themselves in the registry when they place, the global authority replaces
the whole set with `UpdateRegistry`, and a registry seats at most 48 books.

### New

- **`UpdateRegistry` (38)** — `onchain::builders::create_update_registry_instruction`
  and `ix_builder::market::build_update_registry_ix`. Replaces the registry's
  whole maker set, global authority only
- **`MakerRegistry::version()`** reads the new change counter, and
  **`MakerRegistry::get_makers()`** returns the seated books in registry
  order. **`MAX_SEATED_MAKERS`** (48) is the seat cap, also in `constants`.
- **`RegistrySeatEvent`** and `REGISTRY_SEAT_DISC`, emitted when a placing
  `UpdateBookLimit` seats its book: market, seated book, evicted book (the
  default pubkey when a free slot was used), registry version after.
- **`LevelOp::may_place_or_grow`** and **`level_ops_may_seat`** say whether an
  op, or an instruction's ops, can place or grow a level — the test the
  program applies to decide whether a call reads the registry and may seat.
- Errors **530** `RegistryVersionMismatch` and **531**
  `LimitOrderBookRequiresUpdateBookLimit` (an LO book written with
  `UpdateBook` or `UpdateBookRescale`).
- `MAX_LEVEL_OPS` is in `constants`.

### Breaking

- **`MakerRegistry` layout.** `reserved_padding_1` shrinks from `[u8; 7]` to
  `[u8; 3]` and `version: [u8; 4]` follows it. The account size (2128) and
  every other offset are unchanged, so existing on-chain registries read
  correctly; code that named `reserved_padding_1` by type must change.
- **`MAX_MAKER_BOOKS_PER_AUCTION` is 48, not 64.** A swap may carry at most
  48 maker books. No registry seats more, so a swap built from a registry is
  unaffected.
- **`create_update_book_limit_instruction` passes the registry writable**
  when any op may place or grow a level (`level_ops_may_seat`). The builder
  still attaches exactly the `registry_books` it is
  given: pass `&[]` on a cancel-only call to leave the registered books off
  the transaction, or the full set to keep today's shape.
- **`limit_order::actions` cancel-only instructions carry no registered
  books** — `build_cancel`, and `build_modify` when it only shrinks or
  cancels — with the registry read-only.

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