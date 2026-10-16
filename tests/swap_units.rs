//! Wire-level checks for the taker swap builders: lots on the wire, rounding
//! direction, and the account tail.
//!
//! Market shape is mainnet SOL/USDC: 9/6 decimals, 1_000_000 atoms per base
//! lot (0.001 SOL), 1 atom per quote lot, 1_000-atom ticks.

use archer_sdk::{
    config::MarketConfig,
    error::ArcherSDKError,
    identity::Identity,
    ix_builder::swap,
    onchain::{
        state::DelegatedPlatform, ArcherInstruction, ArcherUnit, BaseAtomsPerLot, MarketStateHeader,
        QuoteAtomsPerBaseUnitPerTick, QuoteAtomsPerLot, Side, SwapMode, SwapParams,
        EVENT_AUTHORITY_PUBKEY, MAX_BUILDER_FEE_PPM,
    },
};
use borsh::BorshDeserialize;
use bytemuck::Zeroable;
use solana_program::{instruction::Instruction, pubkey::Pubkey};

const BASE_ATOMS_PER_LOT: u64 = 1_000_000;
const QUOTE_ATOMS_PER_LOT: u64 = 1;

fn sol_usdc() -> MarketConfig {
    let mut header = MarketStateHeader::zeroed();
    header.base_mint = Pubkey::new_unique();
    header.quote_mint = Pubkey::new_unique();
    header.base_vault = Pubkey::new_unique();
    header.quote_vault = Pubkey::new_unique();
    header.base_atoms_per_base_lot = BaseAtomsPerLot::new(BASE_ATOMS_PER_LOT);
    header.quote_atoms_per_quote_lot = QuoteAtomsPerLot::new(QUOTE_ATOMS_PER_LOT);
    header.tick_size_in_quote_atoms_per_base_unit = QuoteAtomsPerBaseUnitPerTick::new(1_000);
    header.raw_base_units_per_base_unit = 1;
    header.base_decimals = 9;
    header.quote_decimals = 6;

    MarketConfig::from_header(
        Pubkey::new_unique(),
        &header,
        9,
        6,
        spl_token::ID,
        spl_token::ID,
    )
}

struct Fixture {
    cfg: MarketConfig,
    market: Pubkey,
    fee_wallet: Pubkey,
    base_ata: Pubkey,
    quote_ata: Pubkey,
    books: Vec<Pubkey>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            cfg: sol_usdc(),
            market: Pubkey::new_unique(),
            fee_wallet: Pubkey::new_unique(),
            base_ata: Pubkey::new_unique(),
            quote_ata: Pubkey::new_unique(),
            books: vec![Pubkey::new_unique(), Pubkey::new_unique()],
        }
    }

    fn generic(
        &self,
        taker: impl Into<Identity>,
        side: Side,
        mode: SwapMode,
        amount: f64,
        threshold: f64,
        fee_ppm: u32,
    ) -> Result<Instruction, ArcherSDKError> {
        swap::build_swap_ix(
            taker,
            &self.market,
            &self.fee_wallet,
            side,
            mode,
            amount,
            threshold,
            &self.base_ata,
            &self.quote_ata,
            &spl_token::ID,
            &spl_token::ID,
            &self.books,
            &self.cfg,
            fee_ppm,
        )
    }

    fn buy_max_in(&self, quote: f64, min_base: f64) -> Result<Instruction, ArcherSDKError> {
        swap::build_buy_max_amount_in(
            Pubkey::new_unique(),
            &self.market,
            &self.fee_wallet,
            quote,
            min_base,
            &self.base_ata,
            &self.quote_ata,
            &spl_token::ID,
            &spl_token::ID,
            &self.books,
            &self.cfg,
            0,
        )
    }

    fn buy_min_out(&self, base: f64, max_quote: f64) -> Result<Instruction, ArcherSDKError> {
        swap::build_buy_min_amount_out(
            Pubkey::new_unique(),
            &self.market,
            &self.fee_wallet,
            base,
            max_quote,
            &self.base_ata,
            &self.quote_ata,
            &spl_token::ID,
            &spl_token::ID,
            &self.books,
            &self.cfg,
            0,
        )
    }

    fn sell_max_in(&self, base: f64, min_quote: f64) -> Result<Instruction, ArcherSDKError> {
        swap::build_sell_max_amount_in(
            Pubkey::new_unique(),
            &self.market,
            &self.fee_wallet,
            base,
            min_quote,
            &self.base_ata,
            &self.quote_ata,
            &spl_token::ID,
            &spl_token::ID,
            &self.books,
            &self.cfg,
            0,
        )
    }

    fn sell_min_out(&self, quote: f64, max_base: f64) -> Result<Instruction, ArcherSDKError> {
        swap::build_sell_min_amount_out(
            Pubkey::new_unique(),
            &self.market,
            &self.fee_wallet,
            quote,
            max_base,
            &self.base_ata,
            &self.quote_ata,
            &spl_token::ID,
            &spl_token::ID,
            &self.books,
            &self.cfg,
            0,
        )
    }
}

fn params(ix: &Instruction) -> SwapParams {
    SwapParams::try_from_slice(&ix.data[1..]).expect("22-byte SwapParams after the discriminator")
}

fn amounts(ix: &Instruction) -> (u64, u64) {
    let p = params(ix);
    (p.amount, p.threshold)
}

#[test]
fn sell_max_amount_in_puts_lots_on_the_wire() {
    let f = Fixture::new();
    // The bug-bounty reproduction: 0.001 SOL for at least 0.10 USDC.
    let ix = f.sell_max_in(0.001, 0.10).unwrap();

    assert_eq!(ix.data.len(), 23, "1 discriminator + 22 bytes of SwapParams");
    assert_eq!(ix.data[0], ArcherInstruction::Swap as u8);

    let p = params(&ix);
    assert_eq!(p.amount, 1, "0.001 SOL is exactly one base lot, not 1_000_000 atoms");
    assert_eq!(p.threshold, 100_000, "0.10 USDC is 100_000 atoms = 100_000 quote lots");
    assert_eq!(p.side, Side::Ask as u8);
    assert_eq!(p.mode, SwapMode::MaxAmountIn as u8);
    assert_eq!(p.builder_fee_ppm, 0);

    let mut expected = vec![ArcherInstruction::Swap as u8];
    expected.extend_from_slice(&1u64.to_le_bytes());
    expected.extend_from_slice(&100_000u64.to_le_bytes());
    expected.extend_from_slice(&[Side::Ask as u8, SwapMode::MaxAmountIn as u8]);
    expected.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(ix.data, expected);
}

#[test]
fn buy_max_amount_in_puts_lots_on_the_wire() {
    let f = Fixture::new();
    // Spend at most 100 USDC, receive at least 0.6 SOL (the README example).
    let ix = f.buy_max_in(100.0, 0.6).unwrap();
    assert_eq!(amounts(&ix), (100_000_000, 600));
    let p = params(&ix);
    assert_eq!((p.side, p.mode), (Side::Bid as u8, SwapMode::MaxAmountIn as u8));
}

#[test]
fn buy_min_amount_out_puts_lots_on_the_wire() {
    let f = Fixture::new();
    // Receive at least 0.6 SOL, spend at most 100 USDC.
    let ix = f.buy_min_out(0.6, 100.0).unwrap();
    assert_eq!(amounts(&ix), (600, 100_000_000));
    let p = params(&ix);
    assert_eq!((p.side, p.mode), (Side::Bid as u8, SwapMode::MinAmountOut as u8));
}

#[test]
fn sell_min_amount_out_puts_lots_on_the_wire() {
    let f = Fixture::new();
    // Receive at least 0.10 USDC, deliver at most 0.001 SOL.
    let ix = f.sell_min_out(0.10, 0.001).unwrap();
    assert_eq!(amounts(&ix), (100_000, 1));
    let p = params(&ix);
    assert_eq!((p.side, p.mode), (Side::Ask as u8, SwapMode::MinAmountOut as u8));
}

#[test]
fn generic_builder_matches_the_convenience_builders() {
    let f = Fixture::new();
    let taker = Pubkey::new_unique();

    let cases: [(Side, SwapMode, f64, f64, Instruction); 4] = [
        (Side::Bid, SwapMode::MaxAmountIn, 100.0, 0.6, f.buy_max_in(100.0, 0.6).unwrap()),
        (Side::Bid, SwapMode::MinAmountOut, 0.6, 100.0, f.buy_min_out(0.6, 100.0).unwrap()),
        (Side::Ask, SwapMode::MaxAmountIn, 0.001, 0.10, f.sell_max_in(0.001, 0.10).unwrap()),
        (Side::Ask, SwapMode::MinAmountOut, 0.10, 0.001, f.sell_min_out(0.10, 0.001).unwrap()),
    ];

    for (side, mode, amount, threshold, convenience) in cases {
        let generic = f.generic(taker, side, mode, amount, threshold, 0).unwrap();
        assert_eq!(generic.data, convenience.data, "{side:?}/{mode:?}");
    }
}

// ───── Rounding ─────

#[test]
fn maximum_input_rounds_down() {
    let f = Fixture::new();
    // 0.0015 SOL is one and a half base lots: spend at most one.
    assert_eq!(amounts(&f.sell_max_in(0.0015, 0.0).unwrap()).0, 1);
    // 0.0015 SOL as the max base to deliver in MinAmountOut mode: same rule.
    assert_eq!(amounts(&f.sell_min_out(1.0, 0.0015).unwrap()).1, 1);
}

#[test]
fn minimum_output_rounds_up() {
    let f = Fixture::new();
    // Require at least 0.0015 SOL back: that is two lots, never one.
    assert_eq!(amounts(&f.buy_max_in(1.0, 0.0015).unwrap()).1, 2);
    // Require at least 0.0015 SOL as the MinAmountOut amount: same rule.
    assert_eq!(amounts(&f.buy_min_out(0.0015, 1.0).unwrap()).0, 2);
}

#[test]
fn sub_lot_maximum_input_is_rejected() {
    let f = Fixture::new();
    // Half a base lot cannot be spent; the program would reject a zero bound.
    assert!(matches!(
        f.sell_max_in(0.0005, 0.0),
        Err(ArcherSDKError::SizeBelowResolution(_))
    ));
    // A tenth of a quote atom as the max quote to spend.
    assert!(matches!(
        f.buy_min_out(0.001, 0.000_000_1),
        Err(ArcherSDKError::SizeBelowResolution(_))
    ));
}

#[test]
fn sub_lot_minimum_output_becomes_one_lot_and_zero_stays_zero() {
    let f = Fixture::new();
    // Asking for at least 100 atoms of SOL back means at least one lot.
    assert_eq!(amounts(&f.buy_max_in(1.0, 0.000_000_1).unwrap()).1, 1);
    // No minimum at all is still allowed in MaxAmountIn mode.
    assert_eq!(amounts(&f.buy_max_in(1.0, 0.0).unwrap()).1, 0);
    assert_eq!(amounts(&f.sell_max_in(0.001, 0.0).unwrap()).1, 0);
}

#[test]
fn float_noise_does_not_move_a_lot() {
    let f = Fixture::new();
    // 0.067 * 1e9 is 67_000_000.00000001 in f64: a ceil straight off the float
    // would demand 68 lots. Rounding to the nearest atom first gives 67.
    assert_eq!(amounts(&f.buy_max_in(10.0, 0.067).unwrap()).1, 67, "min output");
    // 1.001 * 1e6 is 1_000_999.9999999999 in f64: a floor straight off the
    // float would cap spending at 1_000_999 atoms. Nearest atom is 1_001_000.
    assert_eq!(amounts(&f.buy_min_out(0.001, 1.001).unwrap()).1, 1_001_000, "max input");
}

#[test]
fn negative_nan_and_infinite_amounts_are_rejected() {
    let f = Fixture::new();
    for bad in [-0.001, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            f.sell_max_in(bad, 0.0),
            Err(ArcherSDKError::InvalidSize(_))
        ));
        assert!(matches!(
            f.sell_max_in(0.001, bad),
            Err(ArcherSDKError::InvalidSize(_))
        ));
    }
}

// ───── Builder fee ─────

#[test]
fn builder_fee_above_the_protocol_maximum_is_rejected_client_side() {
    let f = Fixture::new();
    let taker = Pubkey::new_unique();

    let ok = f
        .generic(taker, Side::Ask, SwapMode::MaxAmountIn, 0.001, 0.0, MAX_BUILDER_FEE_PPM)
        .unwrap();
    assert_eq!(params(&ok).builder_fee_ppm, MAX_BUILDER_FEE_PPM);

    assert!(matches!(
        f.generic(taker, Side::Ask, SwapMode::MaxAmountIn, 0.001, 0.0, MAX_BUILDER_FEE_PPM + 1),
        Err(ArcherSDKError::BuilderFeeTooHigh { requested, max })
            if requested == MAX_BUILDER_FEE_PPM + 1 && max == MAX_BUILDER_FEE_PPM
    ));
}

#[test]
fn wallet_swap_ends_with_the_maker_books_and_no_event_authority() {
    let f = Fixture::new();
    let ix = f.sell_max_in(0.001, 0.10).unwrap();

    assert_eq!(ix.data[0], ArcherInstruction::Swap as u8);
    assert_eq!(ix.accounts.len(), 11 + f.books.len());
    let tail: Vec<Pubkey> = ix.accounts[11..].iter().map(|m| m.pubkey).collect();
    assert_eq!(tail, f.books);
    assert!(
        ix.accounts.iter().all(|m| m.pubkey != EVENT_AUTHORITY_PUBKEY),
        "Swap has no slot for the program account the event self-CPI needs; \
         appending the event authority fails with MissingAccount on mainnet"
    );
}

#[test]
fn archer_account_swap_ends_with_the_maker_books_and_no_event_authority() {
    let f = Fixture::new();
    let user = Pubkey::new_unique();
    let delegate = Pubkey::new_unique();
    let identity = Identity::archer_account(&user, DelegatedPlatform::TreadFi, delegate);

    let ix = f
        .generic(identity, Side::Ask, SwapMode::MaxAmountIn, 0.001, 0.10, 0)
        .unwrap();

    assert_eq!(ix.data[0], ArcherInstruction::SwapFromArcherAccount as u8);
    assert_eq!(amounts(&ix), (1, 100_000), "same units as the wallet path");
    assert_eq!(ix.accounts.len(), 12 + f.books.len());
    let tail: Vec<Pubkey> = ix.accounts[12..].iter().map(|m| m.pubkey).collect();
    assert_eq!(tail, f.books);
    assert!(ix.accounts.iter().all(|m| m.pubkey != EVENT_AUTHORITY_PUBKEY));
}
