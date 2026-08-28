use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::onchain::{
    ArcherInstruction, ArcherUnit, MakerDepositFundsParams, MakerLevel,
    MakerWithdrawFundsParams, Ticks, UpdateBookData, MAKER_LEVEL_SIZE, MAX_LEVELS,
};

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

pub fn create_initialize_maker_book_instruction(
    maker: Pubkey,
    market: Pubkey,
    kind: u8,
) -> Instruction {
    let (maker_book_key, _) = crate::pda::derive_maker_book(&market, &maker);

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(maker, true),
            AccountMeta::new(maker_book_key, false),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: [ArcherInstruction::InitializeMakerBook.to_vec(), vec![kind]].concat(),
    }
}

pub fn create_update_book_instruction(
    maker: Pubkey,
    market: Pubkey,
    maker_book: Pubkey,
    params: UpdateBookParams,
) -> Instruction {
    let mut data = vec![0u8; UpdateBookData::LEN];

    data[0] = ArcherInstruction::UpdateBook as u8;
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

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new(maker_book, false),
            AccountMeta::new_readonly(market, false),
        ],
        data,
    }
}

pub fn create_update_mid_price_instruction(
    maker: Pubkey,
    maker_book: Pubkey,
    params: UpdateMidPriceParams,
) -> Instruction {
    let mut data = ArcherInstruction::UpdateMidPrice.to_vec();

    data.extend_from_slice(&params.sequence_number.to_le_bytes());
    data.extend_from_slice(&params.new_mid_price_ticks.as_u64().to_le_bytes());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new(maker_book, false),
            AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        ],
        data,
    }
}

pub fn create_update_expiry_in_slots_instruction(
    maker: Pubkey,
    maker_book: Pubkey,
    expiry_in_slots: u64,
) -> Instruction {
    let params = crate::onchain::UpdateExpiryInSlotsParams { expiry_in_slots };

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new(maker_book, false),
        ],
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

pub fn create_close_maker_book_instruction(maker: Pubkey, market: Pubkey) -> Instruction {
    let (maker_book_key, _) = crate::pda::derive_maker_book(&market, &maker);

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(maker, true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(maker_book_key, false),
        ],
        data: ArcherInstruction::CloseMakerBook.to_vec(),
    }
}

pub fn create_clear_book_instruction(
    maker: Pubkey,
    maker_book: Pubkey,
    sequence_number: u64,
) -> Instruction {
    let mut data = ArcherInstruction::ClearBook.to_vec();
    data.extend_from_slice(&sequence_number.to_le_bytes());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new(maker_book, false),
        ],
        data,
    }
}

pub fn create_set_maker_book_delegate_instruction(
    maker: Pubkey,
    maker_book: Pubkey,
    delegate: Pubkey,
) -> Instruction {
    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new(maker_book, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        data: [ArcherInstruction::SetBookDelegate.to_vec()].concat(),
    }
}

pub fn create_maker_deposit_funds_instruction(
    params: MakerDepositFundsParams,
    maker: Pubkey,
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
    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(maker_book, false),
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new_readonly(base_mint, false),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new(base_vault, false),
            AccountMeta::new(quote_vault, false),
            AccountMeta::new(maker_base_token_account, false),
            AccountMeta::new(maker_quote_token_account, false),
            AccountMeta::new_readonly(base_token_program, false),
            AccountMeta::new_readonly(quote_token_program, false),
        ],
        data: [
            ArcherInstruction::MakerDepositFunds.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}

pub fn create_maker_withdraw_funds_instruction(
    params: MakerWithdrawFundsParams,
    maker: Pubkey,
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
    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(maker_book, false),
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new_readonly(base_mint, false),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new(base_vault, false),
            AccountMeta::new(quote_vault, false),
            AccountMeta::new(maker_base_token_account, false),
            AccountMeta::new(maker_quote_token_account, false),
            AccountMeta::new_readonly(base_token_program, false),
            AccountMeta::new_readonly(quote_token_program, false),
        ],
        data: [
            ArcherInstruction::MakerWithdrawFunds.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}
