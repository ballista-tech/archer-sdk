# archer-sdk

Rust SDK for [Archer Protocol](https://archer.exchange) — a fully on-chain
trading venue on Solana that aggregates sovereign market-maker orderbooks into a
single atomic execution layer.

Everything you need to read Archer's state, build its instructions, quote
against its liquidity, and run a market-making book.

```toml
[dependencies]
archer-sdk = "0.0.1"

# with an async RPC client:
archer-sdk = { version = "0.0.1", features = ["client"] }
```

> **Note — this release targets an unreleased program version.**
>
> Reading state, address derivation, the quoting math, and the maker and
> limit-order instructions all work against the program deployed today.
> **Swaps, market creation and the delegated-account instructions require the
> upcoming program upgrade** and will be rejected until it ships

---

## Two ways in

**`ArcherClient`** is the high-level path: async, fetches and caches market
config, and hands you ready-to-sign instructions in human units.

```rust
use archer_sdk::prelude::*;

let client = ArcherClient::new("https://api.mainnet-beta.solana.com");
let market: Pubkey = "u8tnfCb1JSSghuNFquQ2beStYgAN1kmd1f1Lhxbaec4".parse()?;

// Spend at most 100 USDC, require at least 0.6 SOL back.
let ix = client.build_buy_max_amount_in(
    taker, &market, &builder_fee_wallet,
    100.0, 0.6,
    &taker_base_ata, &taker_quote_ata,
    &spl_token::ID, &spl_token::ID,
    &maker_books, 0,
).await?;
```

**The builders and math directly**, if you manage your own RPC and caching.
Everything the client does is available as pure functions over data you supply.

```rust
use archer_sdk::{ix_builder, config::MarketConfig, accounts};

let header = accounts::parse_market_state(&account.data)?;
let config = MarketConfig::from_header(&market, header)?;
let ix = ix_builder::swap::build_buy_max_amount_in(/* … */)?;
```

---

## Human units, exact math

The hardest part of integrating an on-chain orderbook is unit conversion. Archer
prices in ticks and sizes in lots, scaled by `base_atoms_per_base_unit`; getting
this subtly wrong produces orders that are off by orders of magnitude.

So the SDK takes `f64` amounts at its edges and does the conversion with the
program's own integer arithmetic underneath.

```rust
use archer_sdk::math::{lots, ticks};

let price_ticks = ticks::price_to_ticks(148.50, &config)?;
let size_lots   = lots::base_amount_to_lots(1.5, &config)?;
let back        = ticks::ticks_to_price(price_ticks, &config);
```

`math::fees` estimates what a fill will cost and what collateral a quote
requires, before you commit to it:

```rust
use archer_sdk::math::fees;

let cost      = fees::estimate_taker_fees(1_000.0, builder_fee_ppm, &config);
let effective = fees::effective_taker_price(148.50, &config);
let margin    = fees::estimate_required_quote_margin(&quotes, &config)?;
```

---

## Reading state

`accounts` decodes raw account data into typed structs, verifying the
discriminator so a wrong account fails loudly instead of deserializing into
nonsense.

```rust
use archer_sdk::accounts;

let header   = accounts::parse_market_state(&data)?;
let book     = accounts::parse_maker_book(&data)?;
let registry = accounts::parse_maker_registry(&data)?;

let balances = accounts::maker_balances(book, &config);
let bids     = accounts::active_bid_levels(book);
let spread   = accounts::spread_bps(book, &config);
```

`pda` derives every address the program uses, plus verification helpers for
addresses that arrive from outside:

```rust
use archer_sdk::pda;

let (market, _)     = pda::derive_market(&market_id);
let (book, _)       = pda::derive_maker_book(&market, &maker);
let (registry, _)   = pda::derive_maker_registry(&market);
let (account, _)    = pda::derive_archer_account(&owner, platform);

assert!(pda::verify_maker_book_pda(&supplied, &market, &maker).is_some());
```

With the `client` feature, `ArcherClient` wraps all of this with caching:

```rust
let config   = client.get_market_config(&market).await?;   // cached
let book     = client.get_maker_book(&market, &maker).await?;
let balances = client.get_maker_balances(&market, &maker).await?;
let books    = client.get_registered_maker_books(&market).await?;
```

---

## Limit orders

Archer's maker books are parametric — levels are offsets from a moving reference
price, and repricing is one write. `limit_order` presents that as an order book
with stable, price-keyed order IDs.

```rust
use archer_sdk::prelude::*;

// Place two bids and an ask in a single instruction.
let result = client.place_limit_orders(&identity, &market, &[
    NewLimitOrder { side: Side::Bid, price: 148.00, size: 2.0 },
    NewLimitOrder { side: Side::Bid, price: 147.50, size: 3.0 },
    NewLimitOrder { side: Side::Ask, price: 149.00, size: 2.5 },
], collateral).await?;

for id in result.placed_ids {
    println!("resting at {} on {:?}", id.price_ticks, id.side);
}
```

Because every update rewrites the whole book, the SDK computes the resulting
state locally and submits it as one instruction — placing, modifying and
cancelling are all the same operation underneath:

```rust
client.modify_limit_order(&identity, &market, id, 147.75, 2.5).await?;
client.cancel_limit_orders(&identity, &market, &[id]).await?;
client.cancel_all_limit_orders(&identity, &market).await?;
client.replace_all_limit_orders(&identity, &market, &new_orders, collateral).await?;
```

`compute_required_collateral` tells you what a set of orders will lock — using
the program's exact ceiling arithmetic, so your estimate matches what the chain
will demand:

```rust
let needed = compute_required_collateral(&orders, CollateralArgs { /* … */ })?;
```

For reading the aggregate book across every maker, `discovery` builds the ladder
a taker would actually hit:

```rust
let ladder = client.get_lo_ladder(&market).await?;
for rung in ladder {
    println!("{:?} {} @ {}", rung.side, rung.size, rung.price);
}
```

---

## Trading on behalf of others

`Identity` distinguishes a wallet acting for itself from a platform acting for a
user through an ArcherAccount, and threads the right signer and account layout
through every builder.

```rust
use archer_sdk::prelude::*;

let me = Identity::from(my_wallet);

let on_behalf = Identity::ArcherAccount {
    account: archer_account_pda,
    authority: platform_signer,
};

// Same call, correct accounts for each.
client.place_limit_orders(&on_behalf, &market, &orders, collateral).await?;
```

Delegated accounts are created and managed through `archer_account`:

```rust
use archer_sdk::archer_account;

let create   = archer_account::create(&owner, &payer, platform);
let fund     = archer_account::deposit_sol(&payer, &account, 10_000_000);
let delegate = archer_account::set_delegate(&owner, platform, &new_delegate);
let revoke   = archer_account::revoke_delegate(&owner, platform);
```

It also handles the account's own token custody — creating ATAs it owns, and
moving funds in and out:

```rust
let ata      = archer_account::create_token_account(&payer, &account, &mint, &token_program);
let deposit  = archer_account::deposit_tokens(/* … */);
let withdraw = archer_account::withdraw_tokens(/* … */);
```

---

## Creating markets

Market creation is permissionless. `build_initialize_market_ix` applies every
rule the program applies — parameter bounds, lot and tick relationships, header
invariants, and the fixed fee config — so an invalid market is refused locally
instead of costing a failed transaction.

```rust
use archer_sdk::ix_builder::market;

let mut params = InitializeMarketParams { /* mints, decimals, lot and tick sizes */ };
market::permissionless_fee_config(&mut params);   // the protocol's fixed rate

let ix = market::build_initialize_market_ix(params, &admin, &payer)?;
```

Read the fee config from `constants` rather than hardcoding it, so a reprice
doesn't leave a stale literal in your code:

```rust
use archer_sdk::constants::{PERMISSIONLESS_MAKER_FEE_PPM, PERMISSIONLESS_TAKER_FEE_PPM};
```

Creating a market makes you its admin, which entitles you to 80% of the fees it
collects:

```rust
let collect  = market::build_collect_protocol_fee_ix(/* … */);
let transfer = market::build_transfer_admin_ix(&market, &admin, &new_admin);
```

---

## Events

Every Archer event is `discriminator || borsh(event)`. Match the leading eight
bytes of a `Program data:` payload and deserialize the rest:

```rust
use archer_sdk::onchain::events::{MakerFillEvent, MAKER_FILL_DISC};

if payload.starts_with(&MAKER_FILL_DISC) {
    let fill = MakerFillEvent::try_from_slice(&payload[8..])?;
}
```

Passing the event authority as the final account of a swap upgrades
`SyncFillEvent` from a log line to a self-CPI event, which lands in the
transaction's inner instructions and so escapes the per-transaction log budget.
The SDK's swap builders attach it for you.

---

## Module map

| Module | What it gives you |
|---|---|
| `onchain` | Account layouts, instruction enum and parameters, error codes, events, constants. Re-exported at the crate root. |
| `accounts` | Typed decoding with discriminator verification, plus balance and level helpers. |
| `ix_builder` | Instruction builders for swaps, maker operations and market creation. |
| `math` | Tick, lot and fee conversions; book construction from a spread. |
| `limit_order` | Order-book abstraction over parametric maker books. |
| `pda` | Address derivation and verification. |
| `identity` | Wallet vs. delegated-account signing. |
| `config` | Cached per-market scaling factors. |
| `client` | Async RPC client (feature `client`). |
| `constants` | Protocol constants: fee config, bounds, limits, addresses. |