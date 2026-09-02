use archer_sdk::{
    identity::Identity,
    ix_builder::{maker, swap},
    math::BookUpdate,
    pda,
};
use archer_sdk::onchain::{
    state::DelegatedPlatform, ArcherInstruction, ArcherUnit, BaseLots, MakerLevel,
    MarketStateHeader,
};
use bytemuck::Zeroable;
use solana_program::pubkey::Pubkey;

fn book_update(mid_changed: bool) -> BookUpdate {
    BookUpdate {
        new_mid_price_ticks: 100_000,
        mid_price_changed: mid_changed,
        bid_levels: vec![MakerLevel::new(BaseLots::new(10), -100)],
        ask_levels: vec![MakerLevel::new(BaseLots::new(10), 100)],
        estimated_base_lots_locked: 10,
        estimated_quote_lots_locked: 10,
    }
}

fn disc(ix: &solana_program::instruction::Instruction) -> u8 {
    ix.data[0]
}

#[test]
fn wallet_maker_passes_a_bare_pubkey() {
    let me = Pubkey::new_unique();
    let market = Pubkey::new_unique();

    // Note: no `Identity`, no `ArcherAccount`, no import beyond the builder.
    let ixs = maker::build_update_instructions(&book_update(false), &market, me, 1).unwrap();

    assert_eq!(ixs.len(), 1);
    assert_eq!(disc(&ixs[0]), ArcherInstruction::UpdateBook as u8);

    // The wallet is the signer, and nothing was appended.
    assert_eq!(ixs[0].accounts[0].pubkey, me);
    assert!(ixs[0].accounts[0].is_signer);
    assert_eq!(ixs[0].accounts.len(), 3, "wallet books take no extra account");
}

#[test]
fn wallet_update_mid_price_stays_at_three_accounts() {
    let me = Pubkey::new_unique();
    let market = Pubkey::new_unique();

    let ixs = maker::build_update_instructions(&book_update(true), &market, me, 1).unwrap();

    assert_eq!(ixs.len(), 2);
    assert_eq!(disc(&ixs[0]), ArcherInstruction::UpdateMidPrice as u8);
    assert_eq!(
        ixs[0].accounts.len(),
        3,
        "a 4th account would miss matches_fast_path and triple the CU cost"
    );
}

#[test]
fn wallet_taker_builds_a_plain_swap() {
    let me = Pubkey::new_unique();
    let cfg = test_config();

    let ix = swap::build_buy_max_amount_in(
        me,
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        1.0,
        0.0,
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        &spl_token::ID,
        &spl_token::ID,
        &[Pubkey::new_unique()],
        &cfg,
        0,
    )
    .unwrap();

    assert_eq!(disc(&ix), ArcherInstruction::Swap as u8);
    assert_eq!(ix.accounts[0].pubkey, me);
    assert!(ix.accounts[0].is_signer);
}

#[test]
fn platform_quotes_with_the_same_builder() {
    let user = Pubkey::new_unique();
    let delegate = Pubkey::new_unique();
    let market = Pubkey::new_unique();

    let account = pda::derive_archer_account(&user, DelegatedPlatform::TreadFi).0;

    // Identical call to the wallet case above — only the identity differs.
    let ixs = maker::build_update_instructions(
        &book_update(false),
        &market,
        Identity::archer_account(&user, DelegatedPlatform::TreadFi, delegate),
        1,
    )
    .unwrap();

    assert_eq!(disc(&ixs[0]), ArcherInstruction::UpdateBook as u8);

    // The delegate signs; the account is appended so the program can read
    // authority from it live.
    assert_eq!(ixs[0].accounts[0].pubkey, delegate);
    assert!(ixs[0].accounts[0].is_signer);
    assert_eq!(*ixs[0].accounts.last().unwrap(), solana_program::instruction::AccountMeta::new_readonly(account, false));
    assert_eq!(ixs[0].accounts.len(), 4);
}

#[test]
fn platform_taker_builds_swap_from_account() {
    let user = Pubkey::new_unique();
    let delegate = Pubkey::new_unique();
    let cfg = test_config();

    let ix = swap::build_buy_max_amount_in(
        Identity::archer_account(&user, DelegatedPlatform::TreadFi, delegate),
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        1.0,
        0.0,
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        &spl_token::ID,
        &spl_token::ID,
        &[Pubkey::new_unique()],
        &cfg,
        0,
    )
    .unwrap();

    // The identity picked the instruction; the caller never chose.
    assert_eq!(disc(&ix), ArcherInstruction::SwapFromArcherAccount as u8);
    assert_eq!(ix.accounts[0].pubkey, delegate, "authority at index 0");
    assert!(ix.accounts[0].is_signer);
    assert_eq!(
        ix.accounts[1].pubkey,
        pda::derive_archer_account(&user, DelegatedPlatform::TreadFi).0
    );
}

#[test]
fn identity_accessors_separate_maker_from_signer() {
    let account = Pubkey::new_unique();
    let delegate = Pubkey::new_unique();
    let wallet = Pubkey::new_unique();

    let w: Identity = wallet.into();
    assert_eq!(w.maker(), wallet);
    assert_eq!(w.authority(), wallet);
    assert!(!w.is_archer_account());

    let a = Identity::archer_account_at(account, delegate);
    assert_eq!(a.maker(), account, "the account owns the collateral");
    assert_eq!(a.authority(), delegate, "the delegate signs");
    assert_eq!(a.archer_account_key(), Some(account));
}

fn test_config() -> archer_sdk::config::MarketConfig {
    let mut header = MarketStateHeader::zeroed();
    header.base_mint = Pubkey::new_unique();
    header.quote_mint = Pubkey::new_unique();
    header.base_vault = Pubkey::new_unique();
    header.quote_vault = Pubkey::new_unique();
    header.base_atoms_per_base_lot = archer_sdk::onchain::BaseAtomsPerLot::new(1_000_000);
    header.quote_atoms_per_quote_lot = archer_sdk::onchain::QuoteAtomsPerLot::new(1);
    header.tick_size_in_quote_atoms_per_base_unit =
        archer_sdk::onchain::QuoteAtomsPerBaseUnitPerTick::new(1_000);
    header.raw_base_units_per_base_unit = 1;
    header.base_decimals = 9;
    header.quote_decimals = 6;

    archer_sdk::config::MarketConfig::from_header(
        Pubkey::new_unique(),
        &header,
        9,
        6,
        spl_token::ID,
        spl_token::ID,
    )
}
