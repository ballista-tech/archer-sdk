//! The v2.1 registry surface: layout, the `UpdateRegistry` builder, and the
//! account shape an `UpdateBookLimit` takes depending on whether it may seat.

use archer_sdk::{
    ix_builder::market::build_update_registry_ix,
    limit_order::{
        actions::{build_cancel, build_modify, build_place},
        CancelMode, CrossPolicy, LimitOrderId, NewLimitOrder, PostOnly,
    },
    onchain::{
        builders::{create_update_book_limit_instruction, create_update_registry_instruction},
        events::{
            LO_CANCELED_DISC, LO_PLACED_DISC, LO_RESIZED_DISC, MAKER_FILL_DISC, REGISTRY_SEAT_DISC,
        },
        level_ops_may_seat, ArcherInstruction, ArcherUnit, BaseLots, LevelOp, MakerBook,
        MakerLevel, MakerRegistry, MarketStateHeader, Side, LEVEL_OP_EXPECTED_ANY,
        MAKER_REGISTRY_DISCRIMINATOR, MAX_MAKER_BOOKS_PER_AUCTION, MAX_REGISTRY_MAKERS,
        MAX_SEATED_MAKERS,
    },
    pda,
};
use bytemuck::Zeroable;
use solana_program::{instruction::Instruction, pubkey::Pubkey};

#[test]
fn registry_keeps_its_size_and_carries_version_at_byte_76() {
    assert_eq!(MakerRegistry::LEN, 2128);
    assert_eq!(MAX_SEATED_MAKERS, 48);
    assert_eq!(
        MAX_MAKER_BOOKS_PER_AUCTION,
        MAX_SEATED_MAKERS,
        "a swap must be able to carry every seat"
    );

    let mut r = MakerRegistry::zeroed();
    r.discriminator = *MAKER_REGISTRY_DISCRIMINATOR;
    r.num_makers = 2;
    r.version = 7u32.to_le_bytes();
    r.makers[0] = Pubkey::new_unique();
    r.makers[1] = Pubkey::new_unique();

    let bytes = bytemuck::bytes_of(&r);
    assert_eq!(bytes[72], 2, "num_makers at 72");
    assert_eq!(&bytes[73..76], &[0, 0, 0], "padding at 73");
    assert_eq!(&bytes[76..80], &7u32.to_le_bytes(), "version at 76");
    assert_eq!(&bytes[80..112], r.makers[0].as_ref(), "makers at 80");

    let parsed = MakerRegistry::load(bytes).unwrap();
    assert_eq!(parsed.version(), 7);
    assert_eq!(parsed.get_makers(), &r.makers[..2]);
    assert!(parsed.contains(&r.makers[1]));

    // A registry written before v2.1 has zeroed padding, so it reads as
    // version 0 rather than garbage.
    let mut legacy = r;
    legacy.version = [0; 4];
    assert_eq!(MakerRegistry::load(bytemuck::bytes_of(&legacy)).unwrap().version(), 0);

    // `num_makers` is on-chain-controlled; a value past the array is clamped
    // instead of panicking.
    let mut bad = r;
    bad.num_makers = 200;
    assert_eq!(
        MakerRegistry::load(bytemuck::bytes_of(&bad)).unwrap().get_makers().len(),
        MAX_REGISTRY_MAKERS
    );
}

#[test]
fn event_discriminators_are_sha256_of_the_event_name() {
    let disc = |name: &str| {
        let h = solana_program::hash::hash(format!("event:{name}").as_bytes());
        let mut out = [0u8; 8];
        out.copy_from_slice(&h.to_bytes()[..8]);
        out
    };
    assert_eq!(disc("RegistrySeatEvent"), REGISTRY_SEAT_DISC);
    assert_eq!(disc("MakerFillEvent"), MAKER_FILL_DISC);
    assert_eq!(disc("LimitOrderPlacedEvent"), LO_PLACED_DISC);
    assert_eq!(disc("LimitOrderCanceledEvent"), LO_CANCELED_DISC);
    assert_eq!(disc("LimitOrderResizedEvent"), LO_RESIZED_DISC);
}

#[test]
fn update_registry_passes_the_set_as_accounts_and_the_version_as_data() {
    let (admin, market) = (Pubkey::new_unique(), Pubkey::new_unique());
    let books = [Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique()];

    let ix = create_update_registry_instruction(admin, market, &books, 41);
    assert_eq!(ix.program_id, archer_sdk::ARCHER_V1_PROGRAM_ID);
    assert_eq!(ArcherInstruction::UpdateRegistry as u8, 38);
    assert_eq!(ix.data, vec![38, 41, 0, 0, 0], "disc then expected_version u32 LE");

    let (registry, _) = pda::derive_maker_registry(&market);
    let metas: Vec<(Pubkey, bool, bool)> = ix
        .accounts
        .iter()
        .map(|m| (m.pubkey, m.is_signer, m.is_writable))
        .collect();
    assert_eq!(
        metas[..3],
        [(admin, true, false), (market, false, false), (registry, false, true)]
    );
    let expected_books: Vec<_> = books.iter().map(|b| (*b, false, false)).collect();
    assert_eq!(metas[3..], expected_books[..], "books follow, read-only, in seat order");

    // The ix_builder wrapper is the same instruction.
    assert_eq!(build_update_registry_ix(&market, &admin, &books, 41), ix);

    // An empty set is a valid call: it empties the registry.
    let empty = create_update_registry_instruction(admin, market, &[], u32::MAX);
    assert_eq!(empty.accounts.len(), 3);
    assert_eq!(empty.data, vec![38, 255, 255, 255, 255]);
}

fn limit_ix(ops: &[LevelOp], books: &[Pubkey]) -> Instruction {
    let (maker, market, book, registry) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    create_update_book_limit_instruction(maker, market, book, registry, books, 0, 1, ops)
}

#[test]
fn registry_is_writable_iff_an_op_may_place_or_grow() {
    let books = [Pubkey::new_unique(), Pubkey::new_unique()];
    let cases = [
        (LevelOp::place(Side::Bid, 99_900, 10), true),
        (LevelOp::resize(Side::Ask, 100_100, 10, 15), true),
        (LevelOp::new(Side::Ask, 100_100, LEVEL_OP_EXPECTED_ANY, 15), true),
        (LevelOp::resize(Side::Ask, 100_100, 15, 10), false),
        (LevelOp::resize(Side::Ask, 100_100, 15, 0), false),
        (LevelOp::cancel_any(Side::Bid, 99_900), false),
    ];
    for (op, may_seat) in cases {
        assert_eq!(op.may_place_or_grow(), may_seat, "{op:?}");
        assert_eq!(level_ops_may_seat(&[op]), may_seat, "{op:?}");
        let ix = limit_ix(&[op], &books);
        assert_eq!(ix.accounts[3].is_writable, may_seat, "registry flag for {op:?}");
        // The builder attaches exactly the books it is given, either way.
        assert_eq!(ix.accounts.len(), 4 + books.len());
        assert!(ix.accounts[4..].iter().all(|m| !m.is_writable && !m.is_signer));
    }

    // One placing op among cancels makes the whole call a placing call.
    let mixed = [
        LevelOp::cancel_any(Side::Bid, 99_900),
        LevelOp::place(Side::Bid, 99_800, 1),
    ];
    assert!(level_ops_may_seat(&mixed));
    assert!(limit_ix(&mixed, &books).accounts[3].is_writable);
    assert!(!level_ops_may_seat(&[]));

    // A cancel-only call with no books is the registry-free shape.
    let ix = limit_ix(&[LevelOp::cancel_any(Side::Bid, 99_900)], &[]);
    assert_eq!(ix.accounts.len(), 4);
    assert!(!ix.accounts[3].is_writable);
}

fn config() -> archer_sdk::config::MarketConfig {
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

fn post_only(market: Pubkey) -> PostOnly {
    let (pk, _) = pda::derive_maker_registry(&market);
    let mut r = MakerRegistry::zeroed();
    r.market = market;
    r.num_makers = 2;
    r.makers[0] = Pubkey::new_unique();
    r.makers[1] = Pubkey::new_unique();
    PostOnly::from_registry(pk, &r, CrossPolicy::Reject)
}

/// An LO book (mid 0) with one resting bid of `lots` at `price_ticks`.
fn lo_book(market: Pubkey, maker: Pubkey, price_ticks: u64, lots: u64) -> MakerBook {
    let mut b = MakerBook::zeroed();
    b.discriminator = *archer_sdk::onchain::MAKER_BOOK_DISCRIMINATOR;
    b.maker = maker;
    b.market = market;
    b.kind = archer_sdk::onchain::MAKER_KIND_LO;
    b.status = 1;
    b.last_updated_sequence_number = 3;
    b.bid_levels[0] = MakerLevel::new(BaseLots::new(lots), price_ticks as i64);
    b
}

fn only_limit_ix(ixs: &[Instruction]) -> &Instruction {
    let mut found = ixs
        .iter()
        .filter(|ix| ix.data[0] == ArcherInstruction::UpdateBookLimit as u8);
    let ix = found.next().expect("one UpdateBookLimit");
    assert!(found.next().is_none(), "exactly one UpdateBookLimit");
    ix
}

#[test]
fn actions_attach_the_registry_books_only_on_calls_that_may_seat() {
    let cfg = config();
    let (market, maker) = (Pubkey::new_unique(), Pubkey::new_unique());
    let po = post_only(market);
    // Price 99.0 is 99_000 ticks and 1.0 is 1_000 lots on this market.
    let book = lo_book(market, maker, 99_000, 1_000);
    let resting = LimitOrderId {
        side: Side::Bid,
        price_ticks: 99_000,
    };
    let fixed_accounts = 4; // signer, book, market, registry (wallet identity)

    let shape = |ixs: &[Instruction]| {
        let ix = only_limit_ix(ixs);
        (ix.accounts[3].is_writable, ix.accounts.len() - fixed_accounts)
    };

    // Place: writable registry, every registered book attached.
    let place = build_place(
        maker,
        &market,
        Some(&book),
        &[NewLimitOrder {
            side: Side::Bid,
            price: 98.0,
            size: 1.0,
        }],
        None,
        &po,
        &cfg,
    )
    .unwrap();
    assert_eq!(shape(&place.instructions), (true, 2));

    // Cancel: read-only registry, no books.
    let cancel = build_cancel(
        maker,
        &market,
        &book,
        &[resting],
        None,
        &po,
        CancelMode::Any,
        &cfg,
    )
    .unwrap();
    assert_eq!(shape(&cancel.instructions), (false, 0));
    let strict = build_cancel(
        maker,
        &market,
        &book,
        &[resting],
        None,
        &po,
        CancelMode::Strict,
        &cfg,
    )
    .unwrap();
    assert_eq!(shape(&strict.instructions), (false, 0));

    // Modify down: a strict shrink, still registry-free.
    let shrink = build_modify(
        maker,
        &market,
        &book,
        resting,
        99.0,
        0.5,
        &po,
        CancelMode::Any,
        &cfg,
    )
    .unwrap();
    assert_eq!(shape(&shrink.instructions), (false, 0));

    // Modify up: a grow, so the registry is writable and the books come along.
    let grow = build_modify(
        maker,
        &market,
        &book,
        resting,
        99.0,
        2.0,
        &po,
        CancelMode::Any,
        &cfg,
    )
    .unwrap();
    assert_eq!(shape(&grow.instructions), (true, 2));

    // Modify price: cancel + place in one instruction, which may seat.
    let reprice = build_modify(
        maker,
        &market,
        &book,
        resting,
        98.0,
        1.0,
        &po,
        CancelMode::Any,
        &cfg,
    )
    .unwrap();
    assert_eq!(shape(&reprice.instructions), (true, 2));
}
