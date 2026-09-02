use archer_sdk::{
    identity::Identity,
    limit_order::{
        actions::{build_cancel_all, build_place, CollateralArgs},
        NewLimitOrder,
    },
    pda,
};
use archer_sdk::onchain::{
    state::DelegatedPlatform, ArcherInstruction, ArcherUnit, MakerBook, MarketStateHeader, Side,
};
use bytemuck::Zeroable;
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;

fn disc(ix: &Instruction) -> u8 {
    ix.data[0]
}

fn collateral() -> CollateralArgs {
    CollateralArgs {
        base_lots: 100,
        quote_lots: 100,
        maker_base_ata: Pubkey::new_unique(),
        maker_quote_ata: Pubkey::new_unique(),
    }
}

fn orders() -> Vec<NewLimitOrder> {
    vec![NewLimitOrder {
        side: Side::Bid,
        price: 99.0,
        size: 1.0,
    }]
}

/// An existing LO book owned by `maker`, flagged or not.
fn lo_book(market: Pubkey, maker: Pubkey, is_archer_account: bool) -> MakerBook {
    let mut b = MakerBook::zeroed();
    b.discriminator = *archer_sdk::onchain::MAKER_BOOK_DISCRIMINATOR;
    b.maker = maker;
    b.market = market;
    b.kind = archer_sdk::onchain::MAKER_KIND_LO;
    b.maker_is_archer_account = u8::from(is_archer_account);
    b.status = 1;
    b.last_updated_sequence_number = 7;
    b.quote_free = archer_sdk::onchain::QuoteLots::new(1_000_000);
    b.base_free = archer_sdk::onchain::BaseLots::new(1_000_000);
    b
}

fn assert_account_is_referenced(ix: &Instruction, account: &Pubkey, authority: &Pubkey, label: &str) {
    let keys: Vec<_> = ix.accounts.iter().map(|a| a.pubkey).collect();
    assert!(
        keys.contains(account),
        "{label}: the ArcherAccount is not in the account list at all"
    );
    assert!(
        keys.contains(authority),
        "{label}: nothing authorizes this instruction — a PDA cannot sign, so the \
         owner/delegate must appear. This is the failure `impl Into<Identity>` \
         cannot catch at compile time."
    );
    assert!(
        ix.accounts.iter().any(|a| a.pubkey == *authority && a.is_signer),
        "{label}: the authority is present but not marked as a signer"
    );
}

#[test]
fn place_bootstraps_a_book_for_an_archer_account() {
    let user = Pubkey::new_unique();
    let delegate = Pubkey::new_unique();
    let market = Pubkey::new_unique();
    let cfg = test_config();

    let (account, _) = pda::derive_archer_account(&user, DelegatedPlatform::TreadFi);
    let identity = Identity::archer_account_at(account, delegate);

    // No existing book: this emits init + deposit + update_book.
    let result = build_place(identity, &market, None, &orders(), Some(collateral()), &cfg).unwrap();

    let discs: Vec<u8> = result.instructions.iter().map(disc).collect();
    assert_eq!(
        discs,
        vec![
            ArcherInstruction::InitializeMakerBook as u8,
            ArcherInstruction::MakerDepositFunds as u8,
            ArcherInstruction::UpdateBook as u8,
        ]
    );

    for (ix, label) in result.instructions.iter().zip([
        "InitializeMakerBook",
        "MakerDepositFunds",
        "UpdateBook",
    ]) {
        assert_account_is_referenced(ix, &account, &delegate, label);
    }
}

#[test]
fn cancel_all_withdraws_for_an_archer_account() {
    let user = Pubkey::new_unique();
    let delegate = Pubkey::new_unique();
    let market = Pubkey::new_unique();
    let cfg = test_config();

    let (account, _) = pda::derive_archer_account(&user, DelegatedPlatform::TreadFi);
    let identity = Identity::archer_account_at(account, delegate);
    let book = lo_book(market, account, true);

    let result =
        build_cancel_all(identity, &market, &book, Some(collateral()), &cfg).unwrap();

    let discs: Vec<u8> = result.instructions.iter().map(disc).collect();
    assert!(
        discs.contains(&(ArcherInstruction::ClearBook as u8)),
        "cancel-all zeroes the book via ClearBook"
    );
    assert!(
        discs.contains(&(ArcherInstruction::MakerWithdrawFunds as u8)),
        "cancel-all with collateral must emit the withdrawal"
    );

    for ix in &result.instructions {
        assert_account_is_referenced(ix, &account, &delegate, "cancel_all");
    }
}

#[test]
fn wallet_lo_path_gains_no_accounts() {
    let owner = Pubkey::new_unique();
    let market = Pubkey::new_unique();
    let cfg = test_config();

    let wallet = build_place(owner, &market, None, &orders(), Some(collateral()), &cfg).unwrap();

    // The wallet is the maker AND the signer; nothing is appended.
    for ix in &wallet.instructions {
        assert!(
            ix.accounts.iter().any(|a| a.pubkey == owner && a.is_signer),
            "the wallet must sign directly"
        );
    }

    // Same call shape via an ArcherAccount produces strictly more accounts —
    // if these ever match, the authority stopped being appended.
    let (account, _) = pda::derive_archer_account(&owner, DelegatedPlatform::TreadFi);
    let delegated = build_place(
        Identity::archer_account_at(account, Pubkey::new_unique()),
        &market,
        None,
        &orders(),
        Some(collateral()),
        &cfg,
    )
    .unwrap();

    for (w, d) in wallet.instructions.iter().zip(delegated.instructions.iter()) {
        assert_eq!(disc(w), disc(d));
        assert!(
            d.accounts.len() > w.accounts.len(),
            "the ArcherAccount path must carry an extra account for instruction {}",
            disc(w)
        );
    }
}

#[test]
fn identity_is_checked_against_the_book() {
    let owner = Pubkey::new_unique();
    let market = Pubkey::new_unique();
    let (account, _) = pda::derive_archer_account(&owner, DelegatedPlatform::TreadFi);

    let account_book = lo_book(market, account, true);
    let wallet_book = lo_book(market, owner, false);

    let wallet_identity: Identity = owner.into();
    assert!(wallet_identity.check_against_book(&account_book).is_err());

    let account_identity = Identity::archer_account_at(account, Pubkey::new_unique());
    assert!(account_identity.check_against_book(&wallet_book).is_err());

    assert!(wallet_identity.check_against_book(&wallet_book).is_ok());
    assert!(account_identity.check_against_book(&account_book).is_ok());
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
