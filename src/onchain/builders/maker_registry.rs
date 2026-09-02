use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

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
