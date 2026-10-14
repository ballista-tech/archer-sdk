use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use solana_system_interface::program as system_program;

use crate::onchain::ArcherInstruction;

pub fn create_initialize_maker_registry_instruction(admin: Pubkey, market: Pubkey) -> Instruction {
    let (registry_pda, _) = crate::pda::derive_maker_registry(&market);

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(registry_pda, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: ArcherInstruction::InitializeMakerRegistry.to_vec(),
    }
}

pub fn create_register_maker_instruction(
    admin: Pubkey,
    market: Pubkey,
    maker_book: Pubkey,
) -> Instruction {
    let (registry_pda, _) = crate::pda::derive_maker_registry(&market);

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new_readonly(maker_book, false),
            AccountMeta::new(registry_pda, false),
        ],
        data: ArcherInstruction::RegisterMaker.to_vec(),
    }
}

pub fn create_deregister_maker_instruction(
    admin: Pubkey,
    market: Pubkey,
    maker_book: Pubkey,
) -> Instruction {
    let (registry_pda, _) = crate::pda::derive_maker_registry(&market);

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new_readonly(maker_book, false),
            AccountMeta::new(registry_pda, false),
        ],
        data: ArcherInstruction::DeregisterMaker.to_vec(),
    }
}

pub fn create_update_registry_instruction(
    admin: Pubkey,
    market: Pubkey,
    books: &[Pubkey],
    expected_version: u32,
) -> Instruction {
    let (registry_pda, _) = crate::pda::derive_maker_registry(&market);
    let mut data = ArcherInstruction::UpdateRegistry.to_vec();
    data.extend_from_slice(&expected_version.to_le_bytes());

    let mut accounts = Vec::with_capacity(3 + books.len());
    accounts.push(AccountMeta::new_readonly(admin, true));
    accounts.push(AccountMeta::new_readonly(market, false));
    accounts.push(AccountMeta::new(registry_pda, false));
    accounts.extend(books.iter().map(|k| AccountMeta::new_readonly(*k, false)));

    Instruction {
        program_id: crate::ARCHER_V1_PROGRAM_ID,
        accounts,
        data,
    }
}
