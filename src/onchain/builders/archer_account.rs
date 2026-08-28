use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::onchain::{
    ArcherAccountWithdrawParams, ArcherInstruction, DelegatedPlatform,
    SetArcherAccountDelegateParams,
};

pub fn create_initialize_archer_account_instruction(
    owner: Pubkey,
    payer: Pubkey,
    platform: DelegatedPlatform,
) -> Instruction {
    let (archer_account, _) = crate::pda::derive_archer_account(&owner, platform);

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(payer, true),
            AccountMeta::new(archer_account, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: [
            ArcherInstruction::InitializeArcherAccount.to_vec(),
            vec![platform.as_u8()],
        ]
        .concat(),
    }
}

pub fn create_set_archer_account_delegate_instruction(
    owner: Pubkey,
    platform: DelegatedPlatform,
    delegate: Option<Pubkey>,
    max_builder_fee_ppm: u32,
) -> Instruction {
    let (archer_account, _) = crate::pda::derive_archer_account(&owner, platform);

    let params = SetArcherAccountDelegateParams {
        max_builder_fee_ppm,
    };

    let delegate_unwrapped = delegate.unwrap_or(Pubkey::default());

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(archer_account, false),
            AccountMeta::new_readonly(delegate_unwrapped, false),
        ],
        data: [
            ArcherInstruction::SetArcherAccountDelegate.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}

pub fn create_revoke_archer_account_delegate_instruction(
    owner: Pubkey,
    platform: DelegatedPlatform,
) -> Instruction {
    create_set_archer_account_delegate_instruction(owner, platform, None, 0)
}

#[allow(clippy::too_many_arguments)]
pub fn create_archer_account_withdraw_instruction(
    owner: Pubkey,
    platform: DelegatedPlatform,
    token_amount: u64,
    lamports: u64,
    token_leg: Option<ArcherAccountWithdrawTokenLeg>,
) -> Instruction {
    let (archer_account, _) = crate::pda::derive_archer_account(&owner, platform);

    let params = ArcherAccountWithdrawParams {
        token_amount,
        lamports,
    };

    let mut accounts = vec![
        AccountMeta::new(owner, true),
        AccountMeta::new(archer_account, false),
    ];

    if let Some(leg) = token_leg {
        accounts.push(AccountMeta::new_readonly(leg.token_program, false));
        accounts.push(AccountMeta::new_readonly(leg.mint, false));
        accounts.push(AccountMeta::new(leg.source_token_account, false));
        accounts.push(AccountMeta::new(leg.destination_token_account, false));
    }

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data: [
            ArcherInstruction::ArcherAccountWithdraw.to_vec(),
            borsh::to_vec(&params).unwrap(),
        ]
        .concat(),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArcherAccountWithdrawTokenLeg {
    pub token_program: Pubkey,
    pub mint: Pubkey,
    /// Must be owned by the ArcherAccount.
    pub source_token_account: Pubkey,
    /// Any destination the owner chooses.
    pub destination_token_account: Pubkey,
}
