use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

use crate::onchain::{
    ArcherInstruction, CollectProtocolFeeParams, InitializeMarketParams, MarketStateHeader,
};

pub fn create_initialize_market_instruction(
    params: InitializeMarketParams,
    admin: Pubkey,
    payer: Pubkey,
) -> Instruction {
    let (market_pda, _bump) = crate::pda::derive_market(&params.market_id);

    let base_vault = MarketStateHeader::get_vault_ata_address(
        &market_pda,
        &params.base_mint,
        &params.base_token_program,
    );

    let quote_vault = MarketStateHeader::get_vault_ata_address(
        &market_pda,
        &params.quote_mint,
        &params.quote_token_program,
    );

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(market_pda, false),
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new_readonly(params.base_mint, false),
            AccountMeta::new_readonly(params.quote_mint, false),
            AccountMeta::new(base_vault, false),
            AccountMeta::new(quote_vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
            AccountMeta::new_readonly(params.base_token_program, false),
            AccountMeta::new_readonly(params.quote_token_program, false),
            AccountMeta::new_readonly(spl_associated_token_account::ID, false),
            AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
        ],
        data: [
            ArcherInstruction::InitializeMarket.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}

pub fn create_transfer_admin_instruction(
    market: Pubkey,
    admin: Pubkey,
    new_admin: Pubkey,
) -> Vec<Instruction> {
    vec![Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(market, false),
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new_readonly(new_admin, false),
        ],
        data: [ArcherInstruction::TransferAdmin.to_vec()].concat(),
    }]
}

pub fn create_collect_protocol_fee_instruction(
    params: CollectProtocolFeeParams,
    market: Pubkey,
    admin: Pubkey,
    quote_mint: Pubkey,
    quote_vault: Pubkey,
    quote_admin_token_account: Pubkey,
    archer_treasury: Pubkey,
    treasury_quote_token_account: Pubkey,
    token_program: Pubkey,
) -> Instruction {
    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new(market, false),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new(quote_vault, false),
            AccountMeta::new(quote_admin_token_account, false),
            AccountMeta::new_readonly(archer_treasury, false),
            AccountMeta::new(treasury_quote_token_account, false),
            AccountMeta::new_readonly(token_program, false),
        ],
        data: [
            ArcherInstruction::CollectProtocolFee.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}
