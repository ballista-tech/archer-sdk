//! Event definitions, for decoding.
//!
//! Emission is the program's business — these are here so an indexer or client
//! can recognise and deserialize what it emitted. Match the leading eight bytes
//! of a `Program data:` payload against the discriminators below, then
//! Borsh-deserialize the remainder into the matching struct.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

pub const EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";

pub const EVENT_AUTHORITY_PUBKEY: Pubkey =
    solana_program::pubkey!("Fzo6R5MrDSComspzpQNiieGaYVpNksqeLC27CKTPMTm1");
pub const EVENT_AUTHORITY_BUMP: u8 = 255;



#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct SyncFillEvent {
    pub user_wallet: Pubkey,
    pub market: Pubkey,
    pub side: u8,
    pub input_mint: Pubkey,
    pub output_mint: Pubkey,
    pub param_amount: u64,
    pub param_threshold: u64,
    pub exec_input_size: u64,
    pub exec_output_size: u64,
    pub builder_fee_wallet: Pubkey,
    pub builder_fee_atoms: u64,
}

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct MakerFillEvent {
    pub maker_index: u8,
    pub side: u8,
    pub absolute_price_ticks: u64,
    pub price_offset_ticks: i64,
    pub base_lots_filled: u64,
    pub quote_lots_filled: u64,
    pub maker_fee: i64,
    pub sequence_number: u64,
}

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct LimitOrderPlacedEvent {
    pub side: u8,
    pub absolute_price_ticks: u64,
    pub price_offset_ticks: i64,
    pub size_lots: u64,
    pub sequence_number: u64,
}

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct LimitOrderCanceledEvent {
    pub side: u8,
    pub absolute_price_ticks: u64,
    pub price_offset_ticks: i64,
    pub prev_size_lots: u64,
    pub sequence_number: u64,
}

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct LimitOrderResizedEvent {
    pub side: u8,
    pub absolute_price_ticks: u64,
    pub price_offset_ticks: i64,
    pub prev_size_lots: u64,
    pub new_size_lots: u64,
    pub sequence_number: u64,
}

/// Event discriminators: `sha256("event:<Name>")[..8]`, precomputed.
///
/// Every Archer event is emitted as `discriminator || borsh(event)`. To decode
/// a `Program data:` log line or the payload of a self-CPI event instruction,
/// match the first eight bytes against these and Borsh-deserialize the rest into
/// the corresponding struct above.
///
/// `SyncFillEvent` is the exception: its discriminator is computed at emit time
/// rather than precomputed, and equals `sha256("event:SyncFillEvent")[..8]`.
pub const MAKER_FILL_DISC: [u8; 8] = [60, 14, 66, 1, 204, 202, 42, 161];
pub const LO_PLACED_DISC: [u8; 8] = [102, 100, 70, 242, 177, 143, 91, 181];
pub const LO_CANCELED_DISC: [u8; 8] = [38, 110, 113, 150, 123, 237, 121, 105];
pub const LO_RESIZED_DISC: [u8; 8] = [133, 247, 99, 188, 194, 35, 12, 234];
