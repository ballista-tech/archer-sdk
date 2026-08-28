use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

use crate::onchain::{swap_types::SwapParams, ArcherInstruction};

pub fn create_swap_instrucion(
    program_id: &Pubkey,
    taker: &Pubkey,
    market: &Pubkey,
    builder_fee_wallet: &Pubkey,
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
    base_vault: &Pubkey,
    quote_vault: &Pubkey,
    taker_base_token_account: &Pubkey,
    taker_quote_token_account: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    maker_books: &[Pubkey],
    params: SwapParams,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new_readonly(*taker, true),
        AccountMeta::new(*market, false),
        AccountMeta::new(*builder_fee_wallet, false),
        AccountMeta::new_readonly(*base_mint, false),
        AccountMeta::new_readonly(*quote_mint, false),
        AccountMeta::new(*base_vault, false),
        AccountMeta::new(*quote_vault, false),
        AccountMeta::new(*taker_base_token_account, false),
        AccountMeta::new(*taker_quote_token_account, false),
        AccountMeta::new_readonly(*base_token_program, false),
        AccountMeta::new_readonly(*quote_token_program, false),
    ];

    for maker_book in maker_books {
        accounts.push(AccountMeta::new(*maker_book, false));
    }

    Instruction {
        program_id: *program_id,
        accounts,
        data: [
            ArcherInstruction::Swap.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_swap_from_archer_account_instruction(
    program_id: &Pubkey,
    authority: &Pubkey,
    archer_account: &Pubkey,
    market: &Pubkey,
    builder_fee_wallet: &Pubkey,
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
    base_vault: &Pubkey,
    quote_vault: &Pubkey,
    taker_base_token_account: &Pubkey,
    taker_quote_token_account: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    maker_books: &[Pubkey],
    params: SwapParams,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new_readonly(*authority, true),
        AccountMeta::new(*archer_account, false),
        AccountMeta::new(*market, false),
        AccountMeta::new(*builder_fee_wallet, false),
        AccountMeta::new_readonly(*base_mint, false),
        AccountMeta::new_readonly(*quote_mint, false),
        AccountMeta::new(*base_vault, false),
        AccountMeta::new(*quote_vault, false),
        AccountMeta::new(*taker_base_token_account, false),
        AccountMeta::new(*taker_quote_token_account, false),
        AccountMeta::new_readonly(*base_token_program, false),
        AccountMeta::new_readonly(*quote_token_program, false),
    ];

    for maker_book in maker_books {
        accounts.push(AccountMeta::new(*maker_book, false));
    }

    Instruction {
        program_id: *program_id,
        accounts,
        data: [
            ArcherInstruction::SwapFromArcherAccount.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}
