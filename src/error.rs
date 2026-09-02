use crate::onchain::Side;
use solana_program::pubkey::Pubkey;

#[derive(Debug, thiserror::Error)]
pub enum ArcherSDKError {
    #[error("identity mismatch: {0}")]
    IdentityMismatch(String),

    #[error("would restrict book to {attempted}, which is not the ArcherAccount's delegate ({current}) — on an ArcherAccount-owned book this field narrows access, it does not grant it")]
    BookDelegateWouldNotGrant { attempted: Pubkey, current: Pubkey },

    #[error("price {0} converts to zero ticks — below minimum tick resolution")]
    PriceBelowResolution(f64),

    #[error("size {0} converts to zero lots — below minimum lot size")]
    SizeBelowResolution(f64),

    #[error("invalid price: {0} (must be finite and positive)")]
    InvalidPrice(f64),

    #[error("invalid size: {0} (must be finite and non-negative)")]
    InvalidSize(f64),

    #[error("tick offset overflow: price {price} too far from mid {mid}")]
    OffsetOverflow { price: f64, mid: f64 },

    #[error("arithmetic overflow in {operation}")]
    ArithmeticOverflow { operation: &'static str },

    #[error("crossed book: best bid {bid} >= best ask {ask}")]
    CrossedBook { bid: f64, ask: f64 },

    #[error("too many levels: {count} exceeds maximum of 16 per side")]
    TooManyLevels { count: usize },

    #[error("bid levels not strictly descending at index {index}")]
    BidsNotDescending { index: usize },

    #[error("ask levels not strictly ascending at index {index}")]
    AsksNotAscending { index: usize },

    #[error("duplicate tick offset {offset} at level indices {a} and {b}")]
    DuplicateOffset { offset: i64, a: usize, b: usize },

    #[error("insufficient {token} balance: need {required}, have {available}")]
    InsufficientBalance {
        required: f64,
        available: f64,
        token: String,
    },

    #[error("account not found: {0}")]
    AccountNotFound(Pubkey),

    #[error("deserialization failed: {0}")]
    DeserializationError(String),

    #[error("invalid discriminator: expected {expected}")]
    InvalidDiscriminator { expected: &'static str },

    #[error("market is not active (status: {0})")]
    MarketNotActive(u8),

    #[error("rpc error: {0}")]
    #[cfg(feature = "client")]
    RpcError(#[from] solana_client::client_error::ClientError),

    #[error("simulation failed: {0}")]
    SimulationFailed(String),

    #[error("limit order not found at price_ticks {price_ticks} on {side:?}")]
    LimitOrderNotFound { side: Side, price_ticks: u64 },

    #[error("limit order already exists at price_ticks {price_ticks} on {side:?}")]
    LimitOrderAlreadyExists { side: Side, price_ticks: u64 },

    #[error("limit order book full: 16 active levels already on {side:?}")]
    BookFull { side: Side },

    #[error("no MakerBook on chain for this (market, owner); call place to bootstrap")]
    NoMakerBook,

    #[error("crossed book offsets: best bid offset {bid_offset} >= best ask offset {ask_offset}")]
    CrossedBookOffsets { bid_offset: i64, ask_offset: i64 },

    #[error("anchor mid price is zero — book never initialized with levels")]
    AnchorMidUninitialized,

    #[error("no orders provided")]
    EmptyOrderList,

    #[error("invalid market parameters: program error {0}")]
    InvalidMarketParams(u32),

    #[error("permissionless markets must launch at exactly maker {} ppm / taker {} ppm, got maker {maker_ppm} / taker {taker_ppm}", crate::onchain::PERMISSIONLESS_MAKER_FEE_PPM, crate::onchain::PERMISSIONLESS_TAKER_FEE_PPM)]
    FixedFeeConfigRequired { maker_ppm: i32, taker_ppm: i32 },

    #[error("builder fee of {requested} ppm exceeds the protocol maximum of {max} ppm")]
    BuilderFeeTooHigh { requested: u32, max: u32 },

    #[error("builder fee of {requested} ppm exceeds the ArcherAccount's owner-set cap of {cap} ppm")]
    BuilderFeeExceedsAccountCap { requested: u32, cap: u32 },

    #[error("sequence number {proposed} is {} ahead of the book's current {current}; the maximum jump is {max}", proposed - current)]
    SequenceNumberTooFarAhead {
        current: u64,
        proposed: u64,
        max: u64,
    },

    #[error("sequence number {proposed} is not ahead of the book's current {current}")]
    StaleSequenceNumber { current: u64, proposed: u64 },
}

pub type SdkResult<T> = Result<T, ArcherSDKError>;
