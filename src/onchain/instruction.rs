use num_enum::TryFromPrimitive;

#[repr(u8)]
#[derive(TryFromPrimitive, Debug, Copy, Clone, PartialEq, Eq)]
#[rustfmt::skip]
pub enum ArcherInstruction {
    /// Initialize a new market
    /// 
    /// Creates and initializes a market state account along with its token vaults.
    /// The market will be in Active status after initialization.
    ///
    /// Permissionless. Unless the admin is ARCHER_GLOBAL_AUTHORITY the fees must
    /// be exactly PERMISSIONLESS_MAKER_FEE_PPM / PERMISSIONLESS_TAKER_FEE_PPM.
    /// The admin is the market's revenue seat: it collects the 80% share via
    /// CollectProtocolFee, but holds no control over fees, status or registry.
    ///
    /// Accounts
    /// 0. `[writable]` market_account - Market state PDA account
    /// 1. `[signer]` admin_account - Market admin authority
    /// 2. `[]` base_mint - Base token mint
    /// 3. `[]` quote_mint - Quote token mint
    /// 4. `[writable]` base_vault_account - Base token vault PDA
    /// 5. `[writable]` quote_vault_account - Quote token vault PDA
    /// 6. `[signer, writable]` payer_account - Account paying for rent
    /// 7. `[]` system_program - System program
    /// 8. `[]` base_token_program - Base token program
    /// 9. `[]` quote_token_program - Quote token program
    /// 10. `[]` ata_program - Associated token account program
    /// 11. `[]` rent_account - Rent Sysvar
    InitializeMarket = 0,

    /// Change market status (Active/Paused/Closed)
    /// 
    /// Signed by the ARCHER_GLOBAL_AUTHORITY on any market 
    /// Valid transitions:
    /// - Active --> Paused
    /// - Paused --> Active
    /// - Active/Paused --> Closed (irreversible)
    ///
    /// Accounts
    /// 0. `[writable]` market_account - Market state account
    /// 1. `[signer]` admin_account - ARCHER_GLOBAL_AUTHORITY
    ChangeMarketStatus = 1,

    /// Update maker fee
    ///
    /// ARCHER_GLOBAL_AUTHORITY only, on any market. A market admin sets its fees
    /// once at creation and cannot change them afterwards.
    /// The market must be paused.
    /// Fee must be within the bounds: -500bps to 1000bps (negative = rebate)
    ///
    /// Accounts
    /// 0. `[writable]` market_account - Market state account
    /// 1. `[signer]` admin_account - ARCHER_GLOBAL_AUTHORITY
    UpdateMakerFee = 3,

    /// Update taker fee
    ///
    /// ARCHER_GLOBAL_AUTHORITY only, on any market. A market admin sets its fees
    /// once at creation and cannot change them afterwards.
    /// The market must be paused.
    /// Fee must be within the bounds: -500bps to 1000bps (negative = rebate)
    ///
    /// Accounts
    /// 0. `[writable]` market_account - Market state account
    /// 1. `[signer]` admin_account - ARCHER_GLOBAL_AUTHORITY
    UpdateTakerFee = 4,

    /// Transfer admin authority
    /// 
    /// Transfers the market's revenue seat — the 80% fee share and the right to
    /// call CollectProtocolFee — to a new address. It carries no control over
    /// fees, status or the maker registry; those are the global authority's.
    /// This is irreversible, use with caution.
    /// 
    /// Accounts
    /// 0. `[writable]` market_account - Market state account
    /// 1. `[signer]` current_admin_account - Current market admin
    /// 2. `[]` new_admin_account - New admin pubkey
    TransferAdmin = 5,

    /// Initializes a micro orderbook for a market maker
    ///
    /// PDA: MakerBook
    ///
    /// Instruction data (optional): 1 byte `kind` — 0 = MM (default if omitted),
    /// 1 = LO (limit-order book; mid pinned to 0). Init-only and immutable.
    ///
    /// Accounts
    /// 0. `[signer]` maker_account - Market maker pubkey
    /// 1. `[writable]` maker_book_account - MakerBook PDA for market maker and corresponding market
    /// 2. `[]` market_account - Market state account
    /// 3. `[]` system_program - System program
    InitializeMakerBook = 6,

    /// Updates the order book levels for a market maker
    /// 
    /// Accounts
    /// 0. `[signer]` maker_account - Market maker pubkey (or the delegate pubkey)
    /// 1. `[writable]` maker_book_account - The MakerBook PDA
    /// 2. `[]` market_account - Market state account
    UpdateBook = 7,

    /// Update the reference price automatically moving all orders
    ///
    /// This is more efficient than updating the entire book when
    /// only the reference price needs to change
    ///
    /// Accounts
    /// 0. `[signer]` maker_account - Market maker pubkey (or delegate pubkey)
    /// 1. `[writable]` maker_book_account - The MakerBook PDA
    /// 2. `[]` clock_sysvar - Optional Clock sysvar; when supplied the slot is read
    ///    from it directly, otherwise the program falls back to the Clock::get() syscall
    UpdateMidPrice = 8,

    /// Clear all pending orders
    /// 
    /// Accounts
    /// 0. `[signer]` maker_account - Market maker pubkey (or delegate pubkey)
    /// 1. `[writable]` maker_book_account - The MakerBook PDA
    ClearBook = 9,

    /// Set a delegate for market making
    /// 
    /// Accounts
    /// 0. `[signer]` maker_account - Market maker pubkey
    /// 1. `[writable]` maker_book_account - The MakerBook PDA
    /// 2. `[]` delegate_account - The delegate pubkey (optional, if None clears delegate)
    SetBookDelegate = 10,

    /// Deposit funds for a maker
    ///
    /// Accounts
    /// 0. `[]` market_account - Market state account
    /// 1. `[writable]` maker_book_account - Maker book account
    /// 2. `[signer, writable]` maker_account - Market maker
    /// 3. `[]` base_mint - Base token mint
    /// 4. `[]` quote_mint - Quote token mint
    /// 5. `[]` base_vault_account - The market's base token vault
    /// 6. `[]` quote_vault_account - The market's quote token vault
    /// 7. `[writable]` maker_base_token_account - The maker's token account for base token
    /// 8. `[writable]` maker_quote_token_account - The maker's token account for quote token
    /// 9. `[]` base_token_program - Base token program
    /// 10. `[]` quote_token_program - Quote token program
    MakerDepositFunds = 11,

    /// Withdraw funds for a maker
    ///
    /// Accounts
    /// 0. `[]` market_account - Market state account
    /// 1. `[writable]` maker_book_account - Maker book account
    /// 2. `[signer, writable]` maker_account - Market maker
    /// 3. `[]` base_mint - Base token mint
    /// 4. `[]` quote_mint - Quote token mint
    /// 5. `[]` base_vault_account - The market's base token vault
    /// 6. `[]` quote_vault_account - The market's quote token vault
    /// 7. `[writable]` maker_base_token_account - The maker's token account for base token
    /// 8. `[writable]` maker_quote_token_account - The maker's token account for quote token
    /// 9. `[]` base_token_program - Base token program
    /// 10. `[]` quote_token_program - Quote token program
    MakerWithdrawFunds = 12,

    /// Toggle suspension of maker book
    /// 
    /// Only the market admin can suspend or activate back a particular maker book
    /// 
    /// Accounts
    /// 0. `[writable]` market_account - Market state account
    /// 1. `[signer]` admin_account - Market admin authority (must match market.admin)
    /// 2. `[writable]` maker_book_account - The MakerBook PDA
    ToggleBookSuspension = 13,

    /// Collect protocol fee
    /// 
    /// Only the market admin can collect the protocol fee into their token ATA
    /// 
    /// Accounts
    /// 0. `[signer]` admin_account - Market admin authority (must match market.admin)
    /// 1. `[writable]` market_account - Market state account
    /// 2. `[]` quote_mint - The quote token mint
    /// 3. `[]` quote_vault_account - The market's quote token vault
    /// 4. `[writable]` admin_quote_token_account - The admin's token account for quote token
    /// 5. `[]` archer_treasury - Archer Exchange's treasury account. Must be: ELGWUVJD6NBNLyJ5Xv98PzoSg9Wh2Y8Bwep9JZgm9nuo
    /// 6.  `[writable]` treasury_quote_token_account - The treasury's token account for quote token
    /// 7. `[]` token_program - SPL token program
    CollectProtocolFee = 14,

    /// Execute a synchronous Fill-or-Kill swap
    ///
    /// Immediately executes a swap against aggregated maker book liquidity.
    /// Atomic execution with immediate token settlement.
    /// An optional builder fee (`builder_fee_ppm` in the params, capped at
    /// MAX_BUILDER_FEE_PPM) is charged on the quote notional traded, paid by the
    /// taker on top of the protocol fee and forwarded to `builder_fee_wallet`.
    ///
    /// Accounts
    /// 0. `[signer]` taker - Taker authority
    /// 1. `[writable]` market_account - Market state account
    /// 2. `[writable]` builder_fee_wallet - Quote token account receiving the builder fee
    /// 3. `[]` base_mint - Base token mint
    /// 4. `[]` quote_mint - Quote token mint
    /// 5. `[]` base_vault - Market's base token vault PDA
    /// 6. `[]` quote_vault - Market's quote token vault PDA
    /// 7. `[writable]` taker_base_token_account - Taker's base token account
    /// 8. `[writable]` taker_quote_token_account - Taker's quote token account
    /// 9. `[]` base_token_program - Base token program
    /// 10. `[]` quote_token_program - Quote token program
    /// 11..N. `[writable]` maker_book_accounts - MakerBook PDAs to match against
    ///
    /// Backward-compat: legacy callers may still pass an instructions sysvar
    /// account between `quote_token_program` and the first maker book. The
    /// program detects and skips it automatically. New callers should omit it.
    Swap = 15,

    /// Initialize a maker registry for a market
    ///
    /// Creates a MakerRegistry PDA that tracks all registered maker books.
    /// ARCHER_GLOBAL_AUTHORITY only, on any market; it also pays the rent.
    ///
    /// Accounts
    /// 0. `[signer, writable]` admin_account - ARCHER_GLOBAL_AUTHORITY (pays rent)
    /// 1. `[]` market_account - Market state account
    /// 2. `[writable]` registry_account - MakerRegistry PDA
    /// 3. `[]` system_program - System program
    InitializeMakerRegistry = 27,

    /// Register a maker book in the registry
    ///
    /// Adds a maker book to the market's registry. ARCHER_GLOBAL_AUTHORITY only.
    ///
    /// Accounts
    /// 0. `[signer]` admin_account - ARCHER_GLOBAL_AUTHORITY
    /// 1. `[]` market_account - Market state account
    /// 2. `[]` maker_book_account - MakerBook PDA to register
    /// 3. `[writable]` registry_account - MakerRegistry PDA
    RegisterMaker = 28,

    /// Deregister a maker book from the registry
    ///
    /// Removes a maker book from the market's registry. ARCHER_GLOBAL_AUTHORITY only.
    ///
    /// Accounts
    /// 0. `[signer]` admin_account - ARCHER_GLOBAL_AUTHORITY
    /// 1. `[]` market_account - Market state account
    /// 2. `[]` maker_book_account - MakerBook PDA to deregister
    /// 3. `[writable]` registry_account - MakerRegistry PDA
    DeregisterMaker = 29,

    /// Update the `expiry_in_slots` on a MakerBook.
    ///
    /// Callable by the book's maker OR its delegate.
    /// `0` disables the aggregator's expiry-skip check for this book.
    ///
    /// Accounts
    /// 0. `[signer]` authority_account - Maker or delegate
    /// 1. `[writable]` maker_book_account - MakerBook PDA
    UpdateExpiryInSlots = 30,

    /// Close a MakerBook PDA and refund rent to the maker.
    ///
    /// Permissioned by the book's `maker` field — the delegate cannot close.
    /// The book must be fully empty:
    ///   - `base_locked == 0` and `quote_locked == 0`
    ///   - `base_free == 0` and `quote_free == 0`
    ///
    /// Accounts
    /// 0. `[signer, writable]` maker_account - Book owner; receives the rent refund
    /// 1. `[]` market_account - Market state account
    /// 2. `[writable]` maker_book_account - MakerBook PDA to close
    CloseMakerBook = 31,

    /// Create an ArcherAccount — a user's delegated trading identity.
    ///
    /// PDA seeds are `["archer-account", owner, platform]`, so one wallet may
    /// hold one account per `DelegatedPlatform`, each independently delegated.
    ///
    /// Accounts
    /// 0. `[signer]` owner_account - Wallet the account belongs to
    /// 1. `[signer, writable]` payer_account - Pays rent; may be the owner or a platform
    /// 2. `[writable]` archer_account - ArcherAccount PDA
    /// 3. `[]` system_program - System program
    InitializeArcherAccount = 32,

    /// Set or revoke an ArcherAccount's delegate, and its builder-fee ceiling.
    ///
    /// Accounts
    /// 0. `[signer]` owner_account - The ArcherAccount's owner
    /// 1. `[writable]` archer_account - ArcherAccount PDA
    /// 2. `[]` delegate_account - New delegate; default pubkey to revoke
    SetArcherAccountDelegate = 33,

    /// Move value out of an ArcherAccount.
    ///
    /// Accounts
    /// 0. `[signer, writable]` owner_account - The owner; receives withdrawn SOL
    /// 1. `[writable]` archer_account - ArcherAccount PDA
    /// 2. `[]` token_program - Token or Token-2022 (token leg only)
    /// 3. `[]` mint - Mint being withdrawn (token leg only)
    /// 4. `[writable]` source_token_account - Token account owned by the ArcherAccount
    /// 5. `[writable]` destination_token_account - Any destination the owner chooses
    ArcherAccountWithdraw = 34,

    /// A taker swap funded from an ArcherAccount's own token accounts.
    ///
    /// The non-custodial market order. The taker is the account PDA, its owner
    /// or delegate signs, and the program signs the token movement on the
    /// account's behalf — so the delegate never gains authority over the user's
    /// wallet. Proceeds land back in the account's token accounts.
    ///
    /// No MakerBook is required on the market being traded.
    ///
    /// Accounts
    /// 0. `[signer]` authority_account - The ArcherAccount's owner or delegate
    /// 1. `[writable]` archer_account - ArcherAccount PDA; the taker
    /// 2. `[writable]` market_account - Market state account
    /// 3. `[writable]` builder_fee_wallet - Quote token account receiving the builder fee
    /// 4. `[]` base_mint - Base token mint
    /// 5. `[]` quote_mint - Quote token mint
    /// 6. `[writable]` base_vault_account - Market base vault
    /// 7. `[writable]` quote_vault_account - Market quote vault
    /// 8. `[writable]` taker_base_token_account - Base token account owned by the ArcherAccount
    /// 9. `[writable]` taker_quote_token_account - Quote token account owned by the ArcherAccount
    /// 10. `[]` base_token_program - Base token program
    /// 11. `[]` quote_token_program - Quote token program
    /// 12+. `[writable]` maker_book_accounts - MakerBooks to match against
    SwapFromArcherAccount = 35,

}

impl ArcherInstruction {
    pub fn to_vec(&self) -> Vec<u8> {
        vec![*self as u8]
    }
}
