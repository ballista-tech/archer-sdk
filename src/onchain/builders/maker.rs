use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::onchain::{
    encode_level_ops, ArcherInstruction, ArcherUnit, LevelOp, MakerDepositFundsParams,
    MakerLevel, MakerWithdrawFundsParams, Ticks, UpdateBookData, MAKER_LEVEL_SIZE, MAX_LEVELS,
};
#[allow(unused_imports)]
use crate::onchain::{CROSS_POLICY_ALLOW, CROSS_POLICY_REJECT};

#[derive(Debug)]
pub struct UpdateBookParams {
    /// Mid price in ticks around which the orders are to be placed
    pub mid_price_ticks: u64,

    /// Monotonically increasing sequence number to avoid stale updates
    pub sequence_number: u64,

    /// The bid MakerLevels to define price/size spacing
    pub bid_levels: Vec<MakerLevel>,

    /// The ask MakerLevels to define price/size spacing
    pub ask_levels: Vec<MakerLevel>,
}

#[derive(Debug)]
pub struct UpdateMidPriceParams {
    /// New mid price for the maker book
    ///
    /// All bid and ask levels will shift around this new mid price
    pub new_mid_price_ticks: Ticks,

    /// Monotonically increasing sequence number to avoid stale updates
    pub sequence_number: u64,
}

/// Who a [`MakerBook`](crate::onchain::MakerBook)'s `maker` is, and who signs
/// for it.
///
/// A book's `maker` is either a wallet — which signs directly — or an
/// [`ArcherAccount`](crate::onchain::ArcherAccount) PDA, which has no private
/// key and cannot sign. In the PDA case the account's owner or delegate signs on
/// its behalf, and the program needs one extra account to resolve the
/// authorisation. Which slot that account occupies differs by instruction:
///
/// - **Lifecycle and funds** — `InitializeMakerBook`, `SetBookDelegate`,
///   `CloseMakerBook`, `MakerDepositFunds`, `MakerWithdrawFunds`: the maker slot
///   holds the PDA (**not** marked as a signer) and the signing owner-or-delegate
///   is appended last.
/// - **Quoting** — `UpdateBook`, `UpdateMidPrice`, `ClearBook`,
///   `UpdateExpiryInSlots`: the signer slot holds the owner-or-delegate and the
///   PDA is appended last, read-only, so the program can check it against
///   `maker_book.maker`.
///
/// The builders below place it correctly for each, so the flags are right when
/// the instruction is built rather than patched afterwards.
///
/// Pass the variant matching `maker_book.maker_is_archer_account`. Supplying the
/// trailing account on a wallet-owned book, or omitting it on an
/// ArcherAccount-owned one, is rejected on-chain.
///
/// A plain `Pubkey` converts into [`MakerIdentity::Wallet`], so callers that
/// never touch ArcherAccount keep passing a pubkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MakerIdentity {
    /// The book's `maker` is a wallet, and this key signs.
    ///
    /// On quoting instructions this may instead be the book's delegate, which
    /// signs in the maker's place.
    Wallet(Pubkey),

    /// The book's `maker` is an ArcherAccount PDA; `authority` — its owner or
    /// delegate — signs for it.
    ArcherAccount {
        /// The ArcherAccount PDA. Equals `maker_book.maker`.
        account: Pubkey,
        /// The account's owner or delegate. This is the signer.
        authority: Pubkey,
    },
}

impl From<Pubkey> for MakerIdentity {
    fn from(wallet: Pubkey) -> Self {
        MakerIdentity::Wallet(wallet)
    }
}

impl From<&Pubkey> for MakerIdentity {
    fn from(wallet: &Pubkey) -> Self {
        MakerIdentity::Wallet(*wallet)
    }
}

impl MakerIdentity {
    /// The book's `maker` field, and the third seed of its PDA.
    #[inline]
    pub fn maker(&self) -> Pubkey {
        match self {
            MakerIdentity::Wallet(pk) => *pk,
            MakerIdentity::ArcherAccount { account, .. } => *account,
        }
    }

    /// The key that must sign the transaction.
    #[inline]
    pub fn signer(&self) -> Pubkey {
        match self {
            MakerIdentity::Wallet(pk) => *pk,
            MakerIdentity::ArcherAccount { authority, .. } => *authority,
        }
    }

    /// The ArcherAccount PDA, when the book is ArcherAccount-owned.
    #[inline]
    pub fn archer_account(&self) -> Option<Pubkey> {
        match self {
            MakerIdentity::Wallet(_) => None,
            MakerIdentity::ArcherAccount { account, .. } => Some(*account),
        }
    }

    /// Maker slot for the lifecycle/funds instructions.
    ///
    /// A wallet signs from this slot; a PDA cannot, so its `is_signer` stays
    /// false and [`Self::trailing_authority`] supplies the signature instead.
    #[inline]
    fn maker_slot(&self, writable: bool) -> AccountMeta {
        AccountMeta {
            pubkey: self.maker(),
            is_signer: matches!(self, MakerIdentity::Wallet(_)),
            is_writable: writable,
        }
    }

    /// Trailing signer for the lifecycle/funds instructions.
    #[inline]
    fn trailing_authority(&self) -> Option<AccountMeta> {
        match self {
            MakerIdentity::Wallet(_) => None,
            MakerIdentity::ArcherAccount { authority, .. } => {
                Some(AccountMeta::new_readonly(*authority, true))
            }
        }
    }

    /// Trailing read-only ArcherAccount for the quoting instructions.
    #[inline]
    fn trailing_archer_account(&self) -> Option<AccountMeta> {
        self.archer_account()
            .map(|pk| AccountMeta::new_readonly(pk, false))
    }
}

pub fn create_initialize_maker_book_instruction(
    maker: impl Into<MakerIdentity>,
    market: Pubkey,
    kind: u8,
) -> Instruction {
    let maker = maker.into();
    let (maker_book_key, _) = crate::pda::derive_maker_book(&market, &maker.maker());

    let mut accounts = vec![
        // A wallet signs here and pays the rent; an ArcherAccount pays from its
        // own lamports without signing.
        maker.maker_slot(true),
        AccountMeta::new(maker_book_key, false),
        AccountMeta::new_readonly(market, false),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    accounts.extend(maker.trailing_authority());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data: [ArcherInstruction::InitializeMakerBook.to_vec(), vec![kind]].concat(),
    }
}

/// Raw 536-byte `UpdateBookData` for `params`. Byte 0 (the discriminator slot
/// inside the struct) is left zero for the caller to set.
pub fn encode_update_book_data(params: &UpdateBookParams) -> Vec<u8> {
    let mut data = vec![0u8; UpdateBookData::LEN];
    data[1..9].copy_from_slice(&params.sequence_number.to_le_bytes());
    data[9..17].copy_from_slice(&params.mid_price_ticks.to_le_bytes());
    data[17] = params.bid_levels.len().min(MAX_LEVELS) as u8;
    data[18] = params.ask_levels.len().min(MAX_LEVELS) as u8;

    let bids_offset = UpdateBookData::BIDS_OFFSET;
    for (i, level) in params.bid_levels.iter().take(MAX_LEVELS).enumerate() {
        let offset = bids_offset + i * MAKER_LEVEL_SIZE;
        data[offset..offset + 8].copy_from_slice(&level.size_in_base_lots.as_u64().to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&level.price_offset_ticks.to_le_bytes());
    }

    let asks_offset = UpdateBookData::ASKS_OFFSET;
    for (i, level) in params.ask_levels.iter().take(MAX_LEVELS).enumerate() {
        let offset = asks_offset + i * MAKER_LEVEL_SIZE;
        data[offset..offset + 8].copy_from_slice(&level.size_in_base_lots.as_u64().to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&level.price_offset_ticks.to_le_bytes());
    }
    data
}

/// `UpdateBookRescale` (37): `UpdateBook`'s payload and accounts, with the
/// book's sequence number and last-updated slot left untouched. The sequence
/// in `params` is ignored by the program.
pub fn create_update_book_rescale_instruction(
    maker: impl Into<MakerIdentity>,
    market: Pubkey,
    maker_book: Pubkey,
    params: UpdateBookParams,
) -> Instruction {
    let mut ix = create_update_book_instruction(maker, market, maker_book, params);
    ix.data[0] = ArcherInstruction::UpdateBookRescale as u8;
    ix
}

pub fn create_update_book_instruction(
    maker: impl Into<MakerIdentity>,
    market: Pubkey,
    maker_book: Pubkey,
    params: UpdateBookParams,
) -> Instruction {
    let maker = maker.into();
    let mut data = encode_update_book_data(&params);
    data[0] = ArcherInstruction::UpdateBook as u8;

    let mut accounts = vec![
        AccountMeta::new_readonly(maker.signer(), true),
        AccountMeta::new(maker_book, false),
        AccountMeta::new_readonly(market, false),
    ];
    accounts.extend(maker.trailing_archer_account());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data,
    }
}

/// `UpdateBookLimit` (36): place, resize or cancel individual levels on an
/// LO book with on-chain post-only enforcement against the market's
/// registered makers.
///
/// Data is `[36, cross_policy, sequence u64 LE, num_ops u8, ops…]` with one
/// 25-byte [`LevelOp`] per op, `1..=MAX_LEVEL_OPS` of them (the function
/// panics outside that range; chunk first). After the fixed accounts (and the
/// ArcherAccount, if the identity has one) come exactly the registry's maker
/// books, each once, in any order — the program rejects a wrong count, a
/// duplicate or an unregistered book with `RegistryBooksMismatch` (525).
/// `cross_policy` is [`CROSS_POLICY_REJECT`] to fail a crossing place/grow
/// (`PostOnlyWouldCross`, 524) or [`CROSS_POLICY_ALLOW`] to place it anyway.
#[allow(clippy::too_many_arguments)]
pub fn create_update_book_limit_instruction(
    maker: impl Into<MakerIdentity>,
    market: Pubkey,
    maker_book: Pubkey,
    maker_registry: Pubkey,
    registry_books: &[Pubkey],
    cross_policy: u8,
    sequence_number: u64,
    ops: &[LevelOp],
) -> Instruction {
    let maker = maker.into();
    let data = encode_level_ops(cross_policy, sequence_number, ops);

    let mut accounts = vec![
        AccountMeta::new_readonly(maker.signer(), true),
        AccountMeta::new(maker_book, false),
        AccountMeta::new_readonly(market, false),
        AccountMeta::new_readonly(maker_registry, false),
    ];
    accounts.extend(maker.trailing_archer_account());
    accounts.extend(
        registry_books
            .iter()
            .map(|book| AccountMeta::new_readonly(*book, false)),
    );

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data,
    }
}

/// The Clock sysvar sits at index 2, letting the program read the slot directly
/// instead of paying for the `Clock::get()` syscall. For a wallet-owned book
/// that is also the exact 3-account shape the entrypoint's raw fast path
/// matches; an ArcherAccount book appends the PDA at index 3 and falls through
/// to the normal handler.
pub fn create_update_mid_price_instruction(
    maker: impl Into<MakerIdentity>,
    maker_book: Pubkey,
    params: UpdateMidPriceParams,
) -> Instruction {
    let maker = maker.into();
    let mut data = ArcherInstruction::UpdateMidPrice.to_vec();

    data.extend_from_slice(&params.sequence_number.to_le_bytes());
    data.extend_from_slice(&params.new_mid_price_ticks.as_u64().to_le_bytes());

    let mut accounts = vec![
        AccountMeta::new_readonly(maker.signer(), true),
        AccountMeta::new(maker_book, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];
    accounts.extend(maker.trailing_archer_account());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data,
    }
}

/// Callable by the book's maker or its delegate.
pub fn create_update_expiry_in_slots_instruction(
    maker: impl Into<MakerIdentity>,
    maker_book: Pubkey,
    expiry_in_slots: u64,
) -> Instruction {
    let maker = maker.into();
    let params = crate::onchain::UpdateExpiryInSlotsParams { expiry_in_slots };

    let mut accounts = vec![
        AccountMeta::new_readonly(maker.signer(), true),
        AccountMeta::new(maker_book, false),
    ];
    accounts.extend(maker.trailing_archer_account());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data: [
            ArcherInstruction::UpdateExpiryInSlots.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}

pub fn create_toggle_maker_book_suspension_instruction(
    market: Pubkey,
    admin: Pubkey,
    maker_book: Pubkey,
) -> Instruction {
    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(market, false),
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new(maker_book, false),
        ],
        data: [ArcherInstruction::ToggleBookSuspension.to_vec()].concat(),
    }
}

/// The book must be fully empty and NOT suspended — a suspended book cannot be
/// closed (`MakerBookSuspended`), so it cannot be recreated to shed the
/// suspension.
pub fn create_close_maker_book_instruction(
    maker: impl Into<MakerIdentity>,
    market: Pubkey,
) -> Instruction {
    let maker = maker.into();
    let (maker_book_key, _) = crate::pda::derive_maker_book(&market, &maker.maker());

    let mut accounts = vec![
        maker.maker_slot(true),
        AccountMeta::new_readonly(market, false),
        AccountMeta::new(maker_book_key, false),
    ];
    accounts.extend(maker.trailing_authority());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data: ArcherInstruction::CloseMakerBook.to_vec(),
    }
}

pub fn create_clear_book_instruction(
    maker: impl Into<MakerIdentity>,
    maker_book: Pubkey,
    sequence_number: u64,
) -> Instruction {
    let maker = maker.into();
    let mut data = ArcherInstruction::ClearBook.to_vec();
    data.extend_from_slice(&sequence_number.to_le_bytes());

    let mut accounts = vec![
        AccountMeta::new_readonly(maker.signer(), true),
        AccountMeta::new(maker_book, false),
    ];
    accounts.extend(maker.trailing_archer_account());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data,
    }
}

/// Pass `Pubkey::default()` as `delegate` to clear the existing one. Only the
/// book's maker may set it — the current delegate cannot rotate itself.
pub fn create_set_maker_book_delegate_instruction(
    maker: impl Into<MakerIdentity>,
    maker_book: Pubkey,
    delegate: Pubkey,
) -> Instruction {
    let maker = maker.into();

    let mut accounts = vec![
        maker.maker_slot(false),
        AccountMeta::new(maker_book, false),
        AccountMeta::new_readonly(delegate, false),
    ];
    accounts.extend(maker.trailing_authority());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data: [ArcherInstruction::SetBookDelegate.to_vec()].concat(),
    }
}

pub fn create_maker_deposit_funds_instruction(
    params: MakerDepositFundsParams,
    maker: impl Into<MakerIdentity>,
    maker_book: Pubkey,
    market: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    maker_base_token_account: Pubkey,
    maker_quote_token_account: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
    base_token_program: Pubkey,
    quote_token_program: Pubkey,
) -> Instruction {
    let maker = maker.into();

    let mut accounts = vec![
        AccountMeta::new_readonly(market, false),
        AccountMeta::new(maker_book, false),
        // Transfer authority only; never written.
        maker.maker_slot(false),
        AccountMeta::new_readonly(base_mint, false),
        AccountMeta::new_readonly(quote_mint, false),
        AccountMeta::new(base_vault, false),
        AccountMeta::new(quote_vault, false),
        AccountMeta::new(maker_base_token_account, false),
        AccountMeta::new(maker_quote_token_account, false),
        AccountMeta::new_readonly(base_token_program, false),
        AccountMeta::new_readonly(quote_token_program, false),
    ];
    accounts.extend(maker.trailing_authority());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data: [
            ArcherInstruction::MakerDepositFunds.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}

pub fn create_maker_withdraw_funds_instruction(
    params: MakerWithdrawFundsParams,
    maker: impl Into<MakerIdentity>,
    maker_book: Pubkey,
    market: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    maker_base_token_account: Pubkey,
    maker_quote_token_account: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
    base_token_program: Pubkey,
    quote_token_program: Pubkey,
) -> Instruction {
    let maker = maker.into();

    let mut accounts = vec![
        AccountMeta::new_readonly(market, false),
        AccountMeta::new(maker_book, false),
        // Transfer authority only; never written.
        maker.maker_slot(false),
        AccountMeta::new_readonly(base_mint, false),
        AccountMeta::new_readonly(quote_mint, false),
        AccountMeta::new(base_vault, false),
        AccountMeta::new(quote_vault, false),
        AccountMeta::new(maker_base_token_account, false),
        AccountMeta::new(maker_quote_token_account, false),
        AccountMeta::new_readonly(base_token_program, false),
        AccountMeta::new_readonly(quote_token_program, false),
    ];
    accounts.extend(maker.trailing_authority());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data: [
            ArcherInstruction::MakerWithdrawFunds.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}
