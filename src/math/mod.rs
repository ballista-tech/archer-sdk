pub mod fees;
pub mod levels;
pub mod lots;
pub mod ticks;

/// A single price level in human-readable terms.
///
/// `price` is in quote tokens per base token (e.g., 148.50 USDC/SOL).
/// `size` is in base token units (e.g., 10.5 SOL).
///
/// The SDK converts these into on-chain `MakerLevel` structs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quote {
    /// Price in quote tokens per base token.
    pub price: f64,
    /// Size in base token units.
    pub size: f64,
}

/// A full two-sided quote from a market maker.
///
/// Bids must be in strictly descending price order (best bid first).
/// Asks must be in strictly ascending price order (best ask first).
/// The book must not be crossed (best bid < best ask).
///
/// Maximum 16 levels per side.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoSidedQuote {
    /// Bid levels, best (highest) price first.
    pub bids: Vec<Quote>,
    /// Ask levels, best (lowest) price first.
    pub asks: Vec<Quote>,
}

/// Fully resolved book update ready for instruction building.
///
/// Produced by [`build_book_update`] or [`build_book_from_spread`].
/// Pass this to [`crate::ix_builder::maker::build_update_instructions`]
/// to get the actual Solana instructions.
#[derive(Debug, Clone)]
pub struct BookUpdate {
    /// The new mid price in ticks (may or may not have changed).
    pub new_mid_price_ticks: u64,

    /// Bid levels in on-chain format (offset from mid, size in base lots).
    /// Best bid first, strictly decreasing offsets (more negative = further from mid).
    pub bid_levels: Vec<crate::types::MakerLevel>,

    /// Ask levels in on-chain format (offset from mid, size in base lots).
    /// Best ask first, strictly increasing offsets (more positive = further from mid).
    pub ask_levels: Vec<crate::types::MakerLevel>,

    /// Whether the mid price changed from the previous value.
    /// Determines if an `UpdateMidPrice` instruction is needed.
    pub mid_price_changed: bool,

    /// Estimated base lots locked by ask levels (for margin checks).
    pub estimated_base_lots_locked: u64,

    /// Estimated quote lots locked by bid levels (for margin checks).
    pub estimated_quote_lots_locked: u64,
}

impl TwoSidedQuote {
    /// Create a new empty two-sided quote.
    pub fn new() -> Self {
        Self {
            bids: Vec::new(),
            asks: Vec::new(),
        }
    }

    /// Add a bid level.
    pub fn with_bid(mut self, price: f64, size: f64) -> Self {
        self.bids.push(Quote { price, size });
        self
    }

    /// Add an ask level.
    pub fn with_ask(mut self, price: f64, size: f64) -> Self {
        self.asks.push(Quote { price, size });
        self
    }

    /// Number of total levels (both sides).
    pub fn num_levels(&self) -> usize {
        self.bids.len() + self.asks.len()
    }

    /// Whether this quote has any levels.
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }
}

impl Default for TwoSidedQuote {
    fn default() -> Self {
        Self::new()
    }
}
