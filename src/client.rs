//! High-level async client for interacting with the Archer protocol.
//!
//! Provides convenient methods for fetching accounts, building and sending
//! transactions, and managing market maker operations.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::onchain::{MakerBook as MakerBookProgram, Side, MAKER_BOOK_DISCRIMINATOR};
use solana_client::client_error::ClientErrorKind;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_client::rpc_request::RpcError;
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;
use solana_sdk::commitment_config::CommitmentConfig;

use crate::accounts::{self, MakerBalances};
use crate::config::MarketConfig;
use crate::error::{ArcherSDKError, SdkResult};
use crate::identity::Identity;
use crate::ix_builder;
use crate::limit_order::{
    actions::{
        build_cancel, build_cancel_all, build_close_book, build_modify, build_place,
        build_replace_all, CollateralArgs, LimitOrderActionResult,
    },
    discovery::{build_ladder, build_ladder_for_side, filter_active},
    LimitOrderBookView, LimitOrderId, LimitOrderRung, NewLimitOrder,
};
use crate::pda;
use crate::types::{MakerBook, MakerRegistry};
use crate::ARCHER_V1_PROGRAM_ID;

/// High-level client for Archer protocol interactions.
///
/// Wraps an RPC connection with convenience methods for common operations.
/// Internally caches `MarketConfig` per market so callers don't need to
/// thread it through every call — the first method that touches a market
/// fetches and caches; subsequent calls reuse.
///
/// Use [`ArcherClient::invalidate_market`] to drop a cached config after a
/// fee update or other admin action that changes market parameters.
///
/// # Example
///
/// ```rust,ignore
/// let client = ArcherClient::new("https://api.mainnet-beta.solana.com");
/// let book = client.get_maker_book(&market_pubkey, &maker_pubkey).await?;
/// // MarketConfig is fetched lazily on the first call that needs it.
/// ```
pub struct ArcherClient {
    rpc: RpcClient,
    market_configs: RwLock<HashMap<Pubkey, MarketConfig>>,
}

impl ArcherClient {
    /// Create a new client connected to the given RPC endpoint.
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc: RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed()),
            market_configs: RwLock::new(HashMap::new()),
        }
    }

    /// Create a client with a custom commitment level.
    pub fn with_commitment(rpc_url: &str, commitment: CommitmentConfig) -> Self {
        Self {
            rpc: RpcClient::new_with_commitment(rpc_url.to_string(), commitment),
            market_configs: RwLock::new(HashMap::new()),
        }
    }

    /// Get a reference to the underlying RPC client.
    pub fn rpc(&self) -> &RpcClient {
        &self.rpc
    }

    /// Fetch a `MarketConfig`, caching on miss.
    ///
    /// The first call for a given market triggers three RPC fetches (market
    /// header + base mint + quote mint). Subsequent calls return the cached
    /// clone.
    pub async fn get_market_config(&self, market: &Pubkey) -> SdkResult<MarketConfig> {
        if let Some(cached) = self
            .market_configs
            .read()
            .expect("market_configs poisoned")
            .get(market)
            .cloned()
        {
            return Ok(cached);
        }

        let config = self.fetch_market_config(market).await?;
        self.market_configs
            .write()
            .expect("market_configs poisoned")
            .insert(*market, config.clone());
        Ok(config)
    }

    /// Force a re-fetch on the next access. Call after a market admin updates
    /// fees, fee multiplier, or any other parameter the cached config reflects.
    pub fn invalidate_market(&self, market: &Pubkey) {
        self.market_configs
            .write()
            .expect("market_configs poisoned")
            .remove(market);
    }

    /// Pre-populate the cache for a market. Optional — every method below
    /// triggers the same fetch lazily on first use.
    pub async fn preload_market(&self, market: &Pubkey) -> SdkResult<()> {
        self.get_market_config(market).await.map(|_| ())
    }

    async fn fetch_market_config(&self, market: &Pubkey) -> SdkResult<MarketConfig> {
        let account = self
            .rpc
            .get_account(market)
            .await
            .map_err(ArcherSDKError::RpcError)?;

        let header = accounts::parse_market_state(&account.data)?;

        let base_mint_account = self
            .rpc
            .get_account(&header.base_mint)
            .await
            .map_err(ArcherSDKError::RpcError)?;

        let quote_mint_account = self
            .rpc
            .get_account(&header.quote_mint)
            .await
            .map_err(ArcherSDKError::RpcError)?;

        let base_decimals = base_mint_account.data[44];
        let quote_decimals = quote_mint_account.data[44];

        Ok(MarketConfig::from_header(
            *market,
            header,
            base_decimals,
            quote_decimals,
            base_mint_account.owner,
            quote_mint_account.owner,
        ))
    }

    /// Fetch and deserialize a maker book.
    pub async fn get_maker_book(&self, market: &Pubkey, maker: &Pubkey) -> SdkResult<MakerBook> {
        let (pda, _) = pda::derive_maker_book(market, maker);

        let account = self
            .rpc
            .get_account(&pda)
            .await
            .map_err(ArcherSDKError::RpcError)?;

        let book = accounts::parse_maker_book(&account.data)?;
        Ok(*book)
    }

    /// Fetch a maker book if it exists, returning `Ok(None)` if not.
    ///
    /// Distinguishes "account doesn't exist yet" (fresh user) from genuine RPC
    /// errors. Limit-order action builders rely on this to know whether to
    /// prepend `InitializeMakerBook`.
    pub async fn get_maker_book_optional(
        &self,
        market: &Pubkey,
        maker: &Pubkey,
    ) -> SdkResult<Option<MakerBook>> {
        let (pda, _) = pda::derive_maker_book(market, maker);
        match self.rpc.get_account(&pda).await {
            Ok(account) => {
                let book = accounts::parse_maker_book(&account.data)?;
                Ok(Some(*book))
            }
            Err(err) => {
                if is_account_not_found(&err) {
                    Ok(None)
                } else {
                    Err(ArcherSDKError::RpcError(err))
                }
            }
        }
    }

    /// Fetch human-readable balances for a maker.
    pub async fn get_maker_balances(
        &self,
        market: &Pubkey,
        maker: &Pubkey,
    ) -> SdkResult<MakerBalances> {
        let config = self.get_market_config(market).await?;
        let book = self.get_maker_book(market, maker).await?;
        Ok(accounts::maker_balances(&book, &config))
    }

    /// Fetch the maker registry for a market.
    ///
    /// The registry lists all registered maker book pubkeys for the market.
    /// Use this for efficient discovery of maker books instead of `get_all_maker_books`.
    pub async fn get_maker_registry(&self, market: &Pubkey) -> SdkResult<MakerRegistry> {
        let (pda, _) = pda::derive_maker_registry(market);

        let account = self
            .rpc
            .get_account(&pda)
            .await
            .map_err(ArcherSDKError::RpcError)?;

        let registry = accounts::parse_maker_registry(&account.data)?;
        Ok(*registry)
    }

    /// Fetch all registered maker books for a market using the registry.
    ///
    /// First reads the registry to discover maker book pubkeys, then fetches
    /// each one. More efficient than `get_all_maker_books` for markets with
    /// a registry since it avoids `getProgramAccounts`.
    pub async fn get_registered_maker_books(
        &self,
        market: &Pubkey,
    ) -> SdkResult<Vec<(Pubkey, MakerBook)>> {
        let registry = self.get_maker_registry(market).await?;
        let maker_keys = &registry.makers[..registry.num_makers as usize];

        let mut books = Vec::with_capacity(maker_keys.len());
        for key in maker_keys {
            let account = self
                .rpc
                .get_account(key)
                .await
                .map_err(ArcherSDKError::RpcError)?;
            if let Ok(book) = accounts::parse_maker_book(&account.data) {
                books.push((*key, *book));
            }
        }

        Ok(books)
    }

    /// Fetch all active maker books for a market.
    ///
    /// Uses `getProgramAccounts` with discriminator + market filter.
    /// Can be slow on mainnet — use sparingly.
    /// Consider using `get_registered_maker_books` instead if the market has a registry.
    pub async fn get_all_maker_books(
        &self,
        market: &Pubkey,
    ) -> SdkResult<Vec<(Pubkey, MakerBook)>> {
        let filters = vec![
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, MAKER_BOOK_DISCRIMINATOR.to_vec())),
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(40, market.to_bytes().to_vec())),
        ];

        let config = RpcProgramAccountsConfig {
            filters: Some(filters),
            ..Default::default()
        };

        let accounts = self
            .rpc
            .get_program_accounts_with_config(&ARCHER_V1_PROGRAM_ID, config)
            .await
            .map_err(ArcherSDKError::RpcError)?;

        let mut books = Vec::with_capacity(accounts.len());
        for (pubkey, account) in accounts {
            if let Ok(book) = accounts::parse_maker_book(&account.data) {
                books.push((pubkey, *book));
            }
        }

        Ok(books)
    }

    /// Get the current slot.
    pub async fn get_slot(&self) -> SdkResult<u64> {
        self.rpc.get_slot().await.map_err(ArcherSDKError::RpcError)
    }


    // ───── Limit-order convenience methods ─────
    //
    // All methods below auto-fetch (cache-backed) the `MarketConfig` they need.
    // Callers who want explicit config control can drop down to the
    // `limit_order::actions::build_*` family and pass their own.

    /// Fetch a user's MakerBook (if any) and project it into a
    /// [`LimitOrderBookView`].
    pub async fn get_limit_order_book(
        &self,
        market: &Pubkey,
        owner: &Pubkey,
    ) -> SdkResult<Option<LimitOrderBookView>> {
        let config = self.get_market_config(market).await?;
        let (pda, _) = pda::derive_maker_book(market, owner);
        let Some(book) = self.get_maker_book_optional(market, owner).await? else {
            return Ok(None);
        };
        Ok(Some(LimitOrderBookView::from_maker_book(
            pda, &book, &config,
        )))
    }

    /// Place one or more limit orders. Bootstraps the MakerBook and bundles
    /// a deposit if needed. See [`build_place`] for the underlying logic.
    pub async fn place_limit_orders(
        &self,
        owner: &Pubkey,
        market: &Pubkey,
        orders: &[NewLimitOrder],
        deposit: Option<CollateralArgs>,
    ) -> SdkResult<LimitOrderActionResult> {
        let config = self.get_market_config(market).await?;
        let book = self.get_maker_book_optional(market, owner).await?;
        build_place(owner, market, book.as_ref(), orders, deposit, &config)
    }

    /// Modify a single existing limit order (price and/or size).
    pub async fn modify_limit_order(
        &self,
        owner: &Pubkey,
        market: &Pubkey,
        id: LimitOrderId,
        new_price: f64,
        new_size: f64,
    ) -> SdkResult<LimitOrderActionResult> {
        let config = self.get_market_config(market).await?;
        let book = self
            .get_maker_book_optional(market, owner)
            .await?
            .ok_or(ArcherSDKError::NoMakerBook)?;
        build_modify(owner, market, &book, id, new_price, new_size, &config)
    }

    /// Cancel one or more limit orders atomically.
    pub async fn cancel_limit_orders(
        &self,
        owner: &Pubkey,
        market: &Pubkey,
        ids: &[LimitOrderId],
        withdraw: Option<CollateralArgs>,
    ) -> SdkResult<LimitOrderActionResult> {
        let config = self.get_market_config(market).await?;
        let book = self
            .get_maker_book_optional(market, owner)
            .await?
            .ok_or(ArcherSDKError::NoMakerBook)?;
        build_cancel(owner, market, &book, ids, withdraw, &config)
    }

    /// Cancel every active limit order via `ClearBook`.
    pub async fn cancel_all_limit_orders(
        &self,
        owner: &Pubkey,
        market: &Pubkey,
        withdraw: Option<CollateralArgs>,
    ) -> SdkResult<LimitOrderActionResult> {
        let config = self.get_market_config(market).await?;
        let book = self
            .get_maker_book_optional(market, owner)
            .await?
            .ok_or(ArcherSDKError::NoMakerBook)?;
        build_cancel_all(owner, market, &book, withdraw, &config)
    }

    /// Replace the user's entire active order set with the supplied list.
    pub async fn replace_all_limit_orders(
        &self,
        owner: &Pubkey,
        market: &Pubkey,
        orders: &[NewLimitOrder],
        deposit: Option<CollateralArgs>,
    ) -> SdkResult<LimitOrderActionResult> {
        let config = self.get_market_config(market).await?;
        let book = self.get_maker_book_optional(market, owner).await?;
        build_replace_all(owner, market, book.as_ref(), orders, deposit, &config)
    }

    /// Tear down an LO book: ClearBook → optional Withdraw → CloseMakerBook.
    pub async fn close_limit_order_book(
        &self,
        owner: &Pubkey,
        market: &Pubkey,
        withdraw: Option<CollateralArgs>,
    ) -> SdkResult<LimitOrderActionResult> {
        let config = self.get_market_config(market).await?;
        let book = self
            .get_maker_book_optional(market, owner)
            .await?
            .ok_or(ArcherSDKError::NoMakerBook)?;
        build_close_book(owner, market, &book, withdraw, &config)
    }

    /// Build a price-sorted ladder of limit orders across every MakerBook on
    /// the market. Uses the registry if present, falling back to
    /// `getProgramAccounts`. Skips suspended books.
    pub async fn get_lo_ladder(&self, market: &Pubkey) -> SdkResult<Vec<LimitOrderRung>> {
        let config = self.get_market_config(market).await?;
        let raw = self.fetch_market_maker_books(market).await?;
        let books = filter_active(raw);
        Ok(build_ladder(&books, &config))
    }

    /// Same as [`get_lo_ladder`] but filtered to one side.
    pub async fn get_lo_ladder_for_side(
        &self,
        market: &Pubkey,
        side: Side,
    ) -> SdkResult<Vec<LimitOrderRung>> {
        let config = self.get_market_config(market).await?;
        let raw = self.fetch_market_maker_books(market).await?;
        let books = filter_active(raw);
        Ok(build_ladder_for_side(&books, side, &config))
    }

    // ───── Maker funds-management convenience methods ─────

    /// Build a deposit instruction for the user's maker book.
    pub async fn build_deposit(
        &self,
        maker: impl Into<Identity>,
        market: &Pubkey,
        base_amount: f64,
        quote_amount: f64,
        maker_base_ata: &Pubkey,
        maker_quote_ata: &Pubkey,
    ) -> SdkResult<Instruction> {
        let config = self.get_market_config(market).await?;
        ix_builder::maker::build_deposit_ix(
            maker,
            market,
            base_amount,
            quote_amount,
            maker_base_ata,
            maker_quote_ata,
            &config.base_token_program,
            &config.quote_token_program,
            &config,
        )
    }

    /// Build a withdraw instruction. `f64::MAX` on either amount drains all
    /// free balance on that side.
    pub async fn build_withdraw(
        &self,
        maker: impl Into<Identity>,
        market: &Pubkey,
        base_amount: f64,
        quote_amount: f64,
        maker_base_ata: &Pubkey,
        maker_quote_ata: &Pubkey,
    ) -> SdkResult<Instruction> {
        let config = self.get_market_config(market).await?;
        ix_builder::maker::build_withdraw_ix(
            maker,
            market,
            base_amount,
            quote_amount,
            maker_base_ata,
            maker_quote_ata,
            &config.base_token_program,
            &config.quote_token_program,
            &config,
        )
    }

    // ───── Taker (swap) convenience methods ─────

    /// "Buy base with up to `quote_amount` quote, require at least `min_base_out` base out."
    pub async fn build_buy_max_amount_in(
        &self,
        taker: impl Into<Identity>,
        market: &Pubkey,
        builder_fee_wallet: &Pubkey,
        quote_amount: f64,
        min_base_out: f64,
        taker_base_ata: &Pubkey,
        taker_quote_ata: &Pubkey,
        maker_books: &[Pubkey],
        builder_fee_ppm: u32,
    ) -> SdkResult<Instruction> {
        let config = self.get_market_config(market).await?;
        ix_builder::swap::build_buy_max_amount_in(
            taker,
            market,
            builder_fee_wallet,
            quote_amount,
            min_base_out,
            taker_base_ata,
            taker_quote_ata,
            &config.base_token_program,
            &config.quote_token_program,
            maker_books,
            &config,
            builder_fee_ppm,
        )
    }

    /// "Buy at least `base_amount` base, pay at most `max_quote_in` quote."
    pub async fn build_buy_min_amount_out(
        &self,
        taker: impl Into<Identity>,
        market: &Pubkey,
        builder_fee_wallet: &Pubkey,
        base_amount: f64,
        max_quote_in: f64,
        taker_base_ata: &Pubkey,
        taker_quote_ata: &Pubkey,
        maker_books: &[Pubkey],
        builder_fee_ppm: u32,
    ) -> SdkResult<Instruction> {
        let config = self.get_market_config(market).await?;
        ix_builder::swap::build_buy_min_amount_out(
            taker,
            market,
            builder_fee_wallet,
            base_amount,
            max_quote_in,
            taker_base_ata,
            taker_quote_ata,
            &config.base_token_program,
            &config.quote_token_program,
            maker_books,
            &config,
            builder_fee_ppm,
        )
    }

    /// "Sell up to `base_amount` base, require at least `min_quote_out` quote out."
    pub async fn build_sell_max_amount_in(
        &self,
        taker: impl Into<Identity>,
        market: &Pubkey,
        builder_fee_wallet: &Pubkey,
        base_amount: f64,
        min_quote_out: f64,
        taker_base_ata: &Pubkey,
        taker_quote_ata: &Pubkey,
        maker_books: &[Pubkey],
        builder_fee_ppm: u32,
    ) -> SdkResult<Instruction> {
        let config = self.get_market_config(market).await?;
        ix_builder::swap::build_sell_max_amount_in(
            taker,
            market,
            builder_fee_wallet,
            base_amount,
            min_quote_out,
            taker_base_ata,
            taker_quote_ata,
            &config.base_token_program,
            &config.quote_token_program,
            maker_books,
            &config,
            builder_fee_ppm,
        )
    }

    /// "Sell to get at least `quote_amount` quote, deliver at most `max_base_in` base."
    pub async fn build_sell_min_amount_out(
        &self,
        taker: impl Into<Identity>,
        market: &Pubkey,
        builder_fee_wallet: &Pubkey,
        quote_amount: f64,
        max_base_in: f64,
        taker_base_ata: &Pubkey,
        taker_quote_ata: &Pubkey,
        maker_books: &[Pubkey],
        builder_fee_ppm: u32,
    ) -> SdkResult<Instruction> {
        let config = self.get_market_config(market).await?;
        ix_builder::swap::build_sell_min_amount_out(
            taker,
            market,
            builder_fee_wallet,
            quote_amount,
            max_base_in,
            taker_base_ata,
            taker_quote_ata,
            &config.base_token_program,
            &config.quote_token_program,
            maker_books,
            &config,
            builder_fee_ppm,
        )
    }

    /// Full `getProgramAccounts` scan of every MakerBook on the market.
    async fn fetch_market_maker_books(
        &self,
        market: &Pubkey,
    ) -> SdkResult<Vec<(Pubkey, MakerBookProgram)>> {
        let filters = vec![
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, MAKER_BOOK_DISCRIMINATOR.to_vec())),
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(40, market.to_bytes().to_vec())),
        ];

        let cfg = RpcProgramAccountsConfig {
            filters: Some(filters),
            ..Default::default()
        };

        let accounts = self
            .rpc
            .get_program_accounts_with_config(&ARCHER_V1_PROGRAM_ID, cfg)
            .await
            .map_err(ArcherSDKError::RpcError)?;

        let mut out = Vec::with_capacity(accounts.len());
        for (pk, acc) in accounts {
            if let Ok(book) = accounts::parse_maker_book(&acc.data) {
                out.push((pk, *book));
            }
        }
        Ok(out)
    }
}

/// Inspect a `ClientError` to decide whether it indicates a missing account.
/// Used by `get_maker_book_optional` to distinguish "not initialized" from
/// real RPC failures.
fn is_account_not_found(err: &solana_client::client_error::ClientError) -> bool {
    matches!(
        err.kind(),
        ClientErrorKind::RpcError(RpcError::ForUser(msg)) if msg.contains("AccountNotFound") || msg.contains("could not find account")
    )
}
