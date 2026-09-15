use archer_sdk::{
    limit_order::{
        actions::{build_cancel, build_modify, build_place, build_replace_all},
        CancelMode, CrossPolicy, LimitOrderId, NewLimitOrder, PostOnly,
    },
    math::{lots::base_amount_to_lots, ticks::price_to_ticks},
    onchain::{
        ArcherInstruction, ArcherUnit, BaseLots, LevelOp, MakerBook, MakerLevel, MakerRegistry,
        MarketStateHeader, Side, LEVEL_OPS_HEADER_SIZE, LEVEL_OP_EXPECTED_ANY, LEVEL_OP_SIZE,
        MAX_LEVELS, MAX_LEVEL_OPS,
    },
    pda,
};
use bytemuck::Zeroable;
use solana_program::{instruction::Instruction, pubkey::Pubkey};

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

fn post_only(market: Pubkey, policy: CrossPolicy) -> PostOnly {
    let (pk, _) = pda::derive_maker_registry(&market);
    let mut r = MakerRegistry::zeroed();
    r.market = market;
    r.num_makers = 1;
    r.makers[0] = Pubkey::new_unique();
    PostOnly::from_registry(pk, &r, policy)
}

/// An LO book (mid 0) with the given resting `(side, price_ticks, lots)`.
fn lo_book(market: Pubkey, maker: Pubkey, seq: u64, resting: &[(Side, u64, u64)]) -> MakerBook {
    let mut b = MakerBook::zeroed();
    b.discriminator = *archer_sdk::onchain::MAKER_BOOK_DISCRIMINATOR;
    b.maker = maker;
    b.market = market;
    b.kind = archer_sdk::onchain::MAKER_KIND_LO;
    b.status = 1;
    b.last_updated_sequence_number = seq;
    let (mut nb, mut na) = (0, 0);
    for (side, price, lots) in resting {
        let level = MakerLevel::new(BaseLots::new(*lots), *price as i64);
        match side {
            Side::Bid => {
                b.bid_levels[nb] = level;
                nb += 1;
            }
            Side::Ask => {
                b.ask_levels[na] = level;
                na += 1;
            }
        }
    }
    b
}

/// Decode an `UpdateBookLimit` payload into `(policy, seq, ops)`.
fn decode(ix: &Instruction) -> (u8, u64, Vec<LevelOp>) {
    assert_eq!(ix.data[0], ArcherInstruction::UpdateBookLimit as u8);
    let policy = ix.data[1];
    let seq = u64::from_le_bytes(ix.data[2..10].try_into().unwrap());
    let n = ix.data[10] as usize;
    assert_eq!(ix.data.len(), LEVEL_OPS_HEADER_SIZE + n * LEVEL_OP_SIZE);
    let ops = (0..n)
        .map(|i| {
            let at = LEVEL_OPS_HEADER_SIZE + i * LEVEL_OP_SIZE;
            LevelOp::decode(&ix.data[at..]).unwrap()
        })
        .collect();
    (policy, seq, ops)
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
fn place_emits_one_expect_empty_op_per_order() {
    let cfg = config();
    let market = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    let orders = [NewLimitOrder::bid(99.0, 1.0), NewLimitOrder::ask(101.0, 2.0)];

    let r = build_place(owner, &market, None, &orders, None, &post_only(market, CrossPolicy::Allow), &cfg).unwrap();
    let (policy, seq, ops) = decode(only_limit_ix(&r.instructions));

    assert_eq!(policy, CrossPolicy::Allow as u8);
    assert_eq!(seq, 1, "fresh book starts at sequence 1");
    assert_eq!(r.next_sequence_number, 1);
    let bid_ticks = price_to_ticks(99.0, &cfg).unwrap();
    let ask_ticks = price_to_ticks(101.0, &cfg).unwrap();
    assert_eq!(bid_ticks, 99_000, "fixture: 1 tick = 0.001 quote per base");
    assert_eq!(
        ops,
        vec![
            LevelOp::place(Side::Bid, bid_ticks, base_amount_to_lots(1.0, &cfg).unwrap()),
            LevelOp::place(Side::Ask, ask_ticks, base_amount_to_lots(2.0, &cfg).unwrap()),
        ]
    );
    assert!(ops.iter().all(|op| op.expected_size == 0), "a place expects an empty slot");
    assert_eq!(
        r.placed_ids,
        vec![LimitOrderId::new(Side::Bid, bid_ticks), LimitOrderId::new(Side::Ask, ask_ticks)]
    );
}

#[test]
fn place_over_a_resting_price_or_across_own_book_fails_locally() {
    let cfg = config();
    let market = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    let book = lo_book(market, owner, 3, &[(Side::Bid, 99_000, 1_000)]);
    let po = post_only(market, CrossPolicy::Reject);

    let dup = build_place(owner, &market, Some(&book), &[NewLimitOrder::bid(99.0, 1.0)], None, &po, &cfg);
    assert!(dup.is_err(), "same price twice is a duplicate order, not a merge");

    let cross = build_place(owner, &market, Some(&book), &[NewLimitOrder::ask(98.0, 1.0)], None, &po, &cfg);
    assert!(cross.is_err(), "an ask under the user's own bid is caught before sending");
}

#[test]
fn cancel_mode_selects_the_guard() {
    let cfg = config();
    let market = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    let book = lo_book(market, owner, 9, &[(Side::Ask, 101_000, 2_000)]);
    let id = LimitOrderId::new(Side::Ask, 101_000);
    let po = post_only(market, CrossPolicy::Reject);

    let any = build_cancel(owner, &market, &book, &[id], None, &po, CancelMode::Any, &cfg).unwrap();
    let (_, seq, ops) = decode(only_limit_ix(&any.instructions));
    assert_eq!(seq, 10);
    assert_eq!(ops, vec![LevelOp::new(Side::Ask, 101_000, LEVEL_OP_EXPECTED_ANY, 0)]);

    let strict = build_cancel(owner, &market, &book, &[id], None, &po, CancelMode::Strict, &cfg).unwrap();
    let (_, _, ops) = decode(only_limit_ix(&strict.instructions));
    assert_eq!(ops, vec![LevelOp::new(Side::Ask, 101_000, 2_000, 0)]);

    let missing = LimitOrderId::new(Side::Ask, 102_000);
    assert!(build_cancel(owner, &market, &book, &[missing], None, &po, CancelMode::Any, &cfg).is_err());
}

#[test]
fn modify_size_is_a_strict_resize_and_modify_price_is_cancel_plus_place() {
    let cfg = config();
    let market = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    let book = lo_book(market, owner, 4, &[(Side::Ask, 101_000, 2_000)]);
    let id = LimitOrderId::new(Side::Ask, 101_000);
    let po = post_only(market, CrossPolicy::Reject);

    // Same price: one op, guarded by the 2_000 lots we saw. A fill in between
    // would make the chain reject it rather than grow the order back.
    let r = build_modify(owner, &market, &book, id, 101.0, 1.5, &po, CancelMode::Any, &cfg).unwrap();
    let (_, seq, ops) = decode(only_limit_ix(&r.instructions));
    assert_eq!(seq, 5);
    assert_eq!(ops, vec![LevelOp::resize(Side::Ask, 101_000, 2_000, 1_500)]);
    assert_eq!(r.placed_ids, vec![id]);

    // New price: cancel the old level (any), place the new one (expect empty).
    let r = build_modify(owner, &market, &book, id, 102.0, 1.5, &po, CancelMode::Any, &cfg).unwrap();
    let (_, _, ops) = decode(only_limit_ix(&r.instructions));
    assert_eq!(
        ops,
        vec![LevelOp::cancel_any(Side::Ask, 101_000), LevelOp::place(Side::Ask, 102_000, 1_500)]
    );
    assert_eq!(r.placed_ids, vec![LimitOrderId::new(Side::Ask, 102_000)]);

    // Zero size is a cancel and places nothing.
    let r = build_modify(owner, &market, &book, id, 101.0, 0.0, &po, CancelMode::Strict, &cfg).unwrap();
    let (_, _, ops) = decode(only_limit_ix(&r.instructions));
    assert_eq!(ops, vec![LevelOp::resize(Side::Ask, 101_000, 2_000, 0)]);
    assert!(r.placed_ids.is_empty());
}

#[test]
fn replace_all_touches_only_the_levels_that_differ() {
    let cfg = config();
    let market = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    let book = lo_book(
        market,
        owner,
        6,
        &[(Side::Bid, 99_000, 1_000), (Side::Bid, 98_000, 1_000), (Side::Ask, 101_000, 1_000)],
    );
    let po = post_only(market, CrossPolicy::Reject);
    let desired = [
        NewLimitOrder::bid(99.0, 1.0), // unchanged
        NewLimitOrder::bid(98.0, 0.5), // shrink
        NewLimitOrder::ask(102.0, 1.0), // new; the 101 ask goes
    ];

    let r = build_replace_all(owner, &market, Some(&book), &desired, None, &po, CancelMode::Any, &cfg).unwrap();
    let (_, seq, ops) = decode(only_limit_ix(&r.instructions));
    assert_eq!(seq, 7);
    assert_eq!(
        ops,
        vec![
            LevelOp::cancel_any(Side::Ask, 101_000),
            LevelOp::resize(Side::Bid, 98_000, 1_000, 500),
            LevelOp::place(Side::Ask, 102_000, 1_000),
        ],
        "cancels, then resizes, then places"
    );
    assert_eq!(r.placed_ids.len(), 3);

    // Handing back the current state writes nothing.
    let same = [NewLimitOrder::bid(99.0, 1.0), NewLimitOrder::bid(98.0, 1.0), NewLimitOrder::ask(101.0, 1.0)];
    let r = build_replace_all(owner, &market, Some(&book), &same, None, &po, CancelMode::Any, &cfg).unwrap();
    assert!(r.instructions.is_empty());
    assert_eq!(r.next_sequence_number, 6);
}

#[test]
fn more_than_thirty_two_ops_split_across_consecutive_sequences() {
    let cfg = config();
    let market = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    // A full book on both sides...
    let resting: Vec<(Side, u64, u64)> = (0..MAX_LEVELS as u64)
        .flat_map(|i| [(Side::Bid, 90_000 - i * 1_000, 1_000), (Side::Ask, 110_000 + i * 1_000, 1_000)])
        .collect();
    let book = lo_book(market, owner, 20, &resting);
    // ...replaced by a full book at entirely different prices: 32 cancels + 32 places.
    let desired: Vec<NewLimitOrder> = (0..MAX_LEVELS as u64)
        .flat_map(|i| [NewLimitOrder::bid(70.0 - i as f64, 1.0), NewLimitOrder::ask(130.0 + i as f64, 1.0)])
        .collect();
    let po = post_only(market, CrossPolicy::Reject);

    let r = build_replace_all(owner, &market, Some(&book), &desired, None, &po, CancelMode::Any, &cfg).unwrap();
    assert_eq!(r.instructions.len(), 2);
    let (_, seq_a, ops_a) = decode(&r.instructions[0]);
    let (_, seq_b, ops_b) = decode(&r.instructions[1]);
    assert_eq!((seq_a, seq_b), (21, 22));
    assert_eq!(r.next_sequence_number, 22);
    assert_eq!(ops_a.len(), MAX_LEVEL_OPS);
    assert_eq!(ops_b.len(), MAX_LEVEL_OPS);
    assert!(ops_a.iter().all(LevelOp::is_cancel), "every cancel lands before any place");
    assert!(ops_b.iter().all(|op| !op.is_cancel()));
}
