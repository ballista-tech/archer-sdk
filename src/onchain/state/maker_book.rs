use bytemuck::{Pod, Zeroable};
use solana_program::{program_error::ProgramError, pubkey::Pubkey};

use crate::onchain::{ArcherError, ArcherUnit, BaseLots, QuoteLots};

pub const MAKER_BOOK_DISCRIMINATOR: &[u8; 8] = b"ACHRMKR1";
pub const MAKER_BOOK_SEED_PREFIX: &[u8] = b"maker";

pub const MAX_LEVELS: usize = 16;

pub const MAX_SEQUENCE_JUMP: u64 = u16::MAX as u64;

/// MakerBook role discriminator (`MakerBook::kind`).
/// `MM` is the default (all pre-existing books read as MM since their padding is
/// zeroed). `LO` pins `mid_price_ticks` to 0 so each level's
/// `price_offset_ticks` IS its absolute price tick, giving limit orders a
/// structurally immutable price-keyed id.
pub const MAKER_KIND_MM: u8 = 0;
pub const MAKER_KIND_LO: u8 = 1;

pub const MAKER_LEVEL_SIZE: usize = core::mem::size_of::<MakerLevel>();

pub const LEVELS_ARRAY_SIZE: usize = MAX_LEVELS * MAKER_LEVEL_SIZE;
pub const UPDATE_BOOK_DATA_LEN: usize = 8 + 8 + 8 + (MAX_LEVELS * 2 * MAKER_LEVEL_SIZE);

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MakerBookStatus {
    Active = 1,
    Suspended = 2,
}

impl MakerBookStatus {
    #[inline(always)]
    pub fn from_u8(value: u8) -> Result<Self, ProgramError> {
        match value {
            1 => Ok(Self::Active),
            2 => Ok(Self::Suspended),
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    pub fn as_u8(&self) -> u8 {
        match self {
            MakerBookStatus::Active => 1,
            MakerBookStatus::Suspended => 2,
        }
    }

    #[inline(always)]
    pub fn can_participate_in_auction(&self) -> bool {
        matches!(self, Self::Active)
    }
}

/// A single resting price level. On-chain layout is identical for both book
/// kinds; only the interpretation of `price_offset_ticks` differs:
/// - MM book (`kind == MAKER_KIND_MM`): signed offset from the moving
///   `mid_price_ticks`. Absolute price = `mid_price_ticks + price_offset_ticks`.
/// - LO book (`kind == MAKER_KIND_LO`): the absolute price tick directly, since
///   `mid_price_ticks` is pinned at 0 (`0 + offset == offset`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct MakerLevel {
    pub size_in_base_lots: BaseLots,
    pub price_offset_ticks: i64,
}

impl MakerLevel {
    pub const LEN: usize = std::mem::size_of::<Self>();

    #[inline(always)]
    pub const fn new(size_in_base_lots: BaseLots, price_offset_ticks: i64) -> Self {
        Self {
            size_in_base_lots,
            price_offset_ticks,
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.size_in_base_lots.as_u64() > 0
    }

    #[inline(always)]
    pub fn absolute_price(&self, reference_price_ticks: u64) -> Option<u64> {
        let ref_i64 = i64::try_from(reference_price_ticks).ok()?;
        let abs = ref_i64.checked_add(self.price_offset_ticks)?;
        if abs <= 0 {
            None
        } else {
            Some(abs as u64)
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct UpdateBookData {
    pub discriminator: u8,
    sequence_number_le: [u8; 8],
    mid_price_ticks_le: [u8; 8],
    pub num_bids: u8,
    pub num_asks: u8,
    pub _padding: [u8; 5],
    pub levels: [MakerLevel; MAX_LEVELS * 2],
}

impl UpdateBookData {
    pub const LEN: usize = core::mem::size_of::<Self>();

    pub const BIDS_OFFSET: usize = 24;

    pub const ASKS_OFFSET: usize = Self::BIDS_OFFSET + LEVELS_ARRAY_SIZE;

    #[inline(always)]
    pub fn load(data: &[u8]) -> Result<&Self, ProgramError> {
        if data.len() < Self::LEN {
            return Err(ProgramError::InvalidInstructionData);
        }
        bytemuck::try_from_bytes(&data[..Self::LEN])
            .map_err(|_| ProgramError::InvalidInstructionData)
    }

    #[inline(always)]
    pub fn sequence_number(&self) -> u64 {
        u64::from_le_bytes(self.sequence_number_le)
    }

    #[inline(always)]
    pub fn mid_price_ticks(&self) -> u64 {
        u64::from_le_bytes(self.mid_price_ticks_le)
    }

    #[inline(always)]
    pub fn bids(&self) -> &[MakerLevel] {
        let n = (self.num_bids as usize).min(MAX_LEVELS);
        &self.levels[..n]
    }

    #[inline(always)]
    pub fn asks(&self) -> &[MakerLevel] {
        let n = (self.num_asks as usize).min(MAX_LEVELS);
        &self.levels[MAX_LEVELS..MAX_LEVELS + n]
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct MakerBook {
    /// Discriminator
    pub discriminator: [u8; 8],

    /// Maker pubkey owning the maker book
    pub maker: Pubkey,

    /// Market pubkey
    pub market: Pubkey,

    /// Optional delegate to manage orders
    /// Cannot deposit/withdraw funds. Only clear book, update mid price / levels
    pub delegate: Pubkey,

    /// Mid price in ticks around which all the orders are offsetted
    pub mid_price_ticks: u64,

    /// Internal cache utility
    pub quote_delta_per_tick: u64,

    /// Internal cache utility
    pub min_mid_price_ticks: u64,

    /// Quote lots locked for existing bid orders
    pub quote_locked: QuoteLots,

    /// Quote lots free for new bid orders or withdrawals
    pub quote_free: QuoteLots,

    /// Base lots locked for existing ask orders
    pub base_locked: BaseLots,

    /// Base lots free for new ask orders or withdrawals
    pub base_free: BaseLots,

    /// Status: Active/Suspended
    pub status: u8,

    /// PDA bump
    pub maker_book_bump: u8,

    /// Reserved padding
    pub reserved_padding_1: u16,

    /// Role discriminator: `MAKER_KIND_MM` (0) or `MAKER_KIND_LO` (1).
    /// Init-only and immutable thereafter.
    pub kind: u8,

    /// `1` when [`Self::maker`] is an `ArcherAccount` PDA; `0` when it is a
    /// wallet or any other PDA. Init-only.
    pub maker_is_archer_account: u8,

    /// Reserved padding
    pub reserved_padding_2: [u8; 2],

    /// Monotonically increasing sequence number counting all updates.
    /// Useful for makers to update book sequentially and avoid stale updates
    pub last_updated_sequence_number: u64,

    //// Total bid size in base lots
    pub total_bid_base_lots: BaseLots,

    /// Internal cache utility
    pub tick_conversion_num: u64,

    /// Internal cache utility
    pub tick_conversion_den: u64,

    /// All bid order levels. All orders are offset from the mid price
    pub bid_levels: [MakerLevel; MAX_LEVELS],

    /// All ask order levels. All orders are offset from the mid price
    pub ask_levels: [MakerLevel; MAX_LEVELS],

    /// Slot of the most recent update_book / update_mid_price.
    /// Combined with `expiry_in_slots`, the aggregator will skip books whose
    /// quotes are staler than allowed.
    pub last_updated_slot: u64,

    /// Maximum slots a book may remain unupdated before the aggregator skips it.
    /// `0` disables the expiry check.
    pub expiry_in_slots: u64,

    /// Deferred-rebalancing anchor: the `mid_price_ticks` at which
    /// `quote_locked` / `quote_free` were last accurate. `update_mid_price`
    /// defers the quote rebalance (it only moves the reference price); consumers
    /// that read the quote balances replay the pending shift lazily via
    /// [`MakerBook::sync_quote_balances`]. `0` is a sentinel meaning "balances
    /// are already accurate at the current mid" — carved from the front of
    /// `reserved_padding_3`, so existing on-chain books (zeroed) read as already-synced.
    pub mid_at_last_sync: u64,

    /// Reserved for future fields.
    pub reserved_padding_3: [u64; 5],
}

impl MakerBook {
    pub const LEN: usize = core::mem::size_of::<Self>();

    #[inline(always)]
    pub fn load(data: &[u8]) -> Result<&Self, ArcherError> {
        if data.len() < Self::LEN {
            return Err(ArcherError::InvalidMakerBook);
        }

        let book = bytemuck::try_from_bytes::<Self>(&data[..Self::LEN])
            .map_err(|_| ArcherError::InvalidMakerBook)?;

        if &book.discriminator != MAKER_BOOK_DISCRIMINATOR {
            return Err(ArcherError::InvalidMakerBook);
        }

        Ok(book)
    }

    #[inline(always)]
    pub fn get_status(&self) -> Result<MakerBookStatus, ProgramError> {
        MakerBookStatus::from_u8(self.status)
    }

    #[inline(always)]
    pub fn is_frozen(&self) -> bool {
        self.status >= MakerBookStatus::Suspended as u8
    }

    #[inline(always)]
    pub fn is_authorized(&self, signer: &Pubkey) -> bool {
        self.maker == *signer || (self.delegate != Pubkey::default() && self.delegate == *signer)
    }

    #[inline(always)]
    pub fn base_lots_available(&self) -> BaseLots {
        self.base_free
    }

    #[inline(always)]
    pub fn quote_lots_available(&self) -> QuoteLots {
        self.quote_free
    }

    #[inline(always)]
    pub fn base_lots_total(&self) -> Result<u64, ArcherError> {
        self.base_free
            .as_u64()
            .checked_add(self.base_locked.as_u64())
            .ok_or(ArcherError::ArithmeticOverflow)
    }

    #[inline(always)]
    pub fn quote_lots_total(&self) -> Result<u64, ArcherError> {
        self.quote_free
            .as_u64()
            .checked_add(self.quote_locked.as_u64())
            .ok_or(ArcherError::ArithmeticOverflow)
    }

    #[inline]
    pub fn validate_level_size(
        &self,
        is_bid_side: bool,
        level_idx: u8,
        base_lots: u64,
    ) -> Result<(), ArcherError> {
        let levels = if is_bid_side {
            &self.bid_levels
        } else {
            &self.ask_levels
        };

        let idx = level_idx as usize;
        if idx >= MAX_LEVELS {
            return Err(ArcherError::InvalidOrderSequence);
        }

        let actual_size = levels[idx].size_in_base_lots.as_u64();
        if base_lots > actual_size {
            return Err(ArcherError::InvalidSizeAtLevel);
        }

        Ok(())
    }

    #[inline]
    /// Quote lots needed to back `base_lots` resting at `price_ticks`, rounded
    /// up — the program's own collateral arithmetic.
    pub fn compute_quote_lots_ceiling(
        &self,
        base_lots: u64,
        price_ticks: u64,
    ) -> Result<u64, ArcherError> {
        let numerator = (base_lots as u128)
            .checked_mul(price_ticks as u128)
            .ok_or(ArcherError::ArithmeticOverflow)?
            .checked_mul(self.tick_conversion_num as u128)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        let den = self.tick_conversion_den as u128;
        let adjustment = den.checked_sub(1).ok_or(ArcherError::ArithmeticUnderflow)?;

        let result = numerator
            .checked_add(adjustment)
            .ok_or(ArcherError::ArithmeticOverflow)?
            .checked_div(den)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        u64::try_from(result).map_err(|_| ArcherError::ArithmeticOverflow)
    }

    /// `(quote_locked, quote_free)` as they will stand once any deferred
    /// `update_mid_price` rebalance is replayed — i.e. the true balances, as
    /// opposed to the possibly-stale values in the struct.
    ///
    /// Pure: reads only. This is the single definition of the deferred-rebalance
    /// arithmetic — [`Self::sync_quote_balances`] applies it on-chain, the
    /// aggregator uses it to decide whether a book is fundable, and off-chain
    /// consumers should call it instead of reading the raw fields, so there is no
    /// second implementation to drift.
    ///
    /// Returns `InsufficientBalance` when the maker repriced further than their
    /// quote balance can back. The reprice itself is allowed to succeed (that is
    /// the point of deferring), so this is the first place the shortfall is
    /// observable.
    ///
    /// `quote_delta_per_tick` is only changed by `update_book` / `clear_book` /
    /// settle, all of which re-anchor, so it is constant across the reprices
    /// being replayed here.
    #[inline(always)]
    pub fn projected_quote_balances(&self) -> Result<(u64, u64), ArcherError> {
        let cur = self.mid_price_ticks;
        let anchor = self.mid_at_last_sync;

        let q_locked = self.quote_locked.as_u64();
        let q_free = self.quote_free.as_u64();

        // `0` is the "already accurate" sentinel; see `mid_at_last_sync`.
        if anchor == 0 || anchor == cur {
            return Ok((q_locked, q_free));
        }

        let (delta_price, is_increase) = if cur > anchor {
            (cur - anchor, true)
        } else {
            (anchor - cur, false)
        };

        let quote_delta = self
            .quote_delta_per_tick
            .checked_mul(delta_price)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        if is_increase {
            let nl = q_locked
                .checked_add(quote_delta)
                .ok_or(ArcherError::ArithmeticOverflow)?;
            let nf = q_free
                .checked_sub(quote_delta)
                .ok_or(ArcherError::InsufficientBalance)?;
            Ok((nl, nf))
        } else {
            let nl = q_locked
                .checked_sub(quote_delta)
                .ok_or(ArcherError::InsufficientBalance)?;
            let nf = q_free
                .checked_add(quote_delta)
                .ok_or(ArcherError::ArithmeticOverflow)?;
            Ok((nl, nf))
        }
    }

    /// True when a deferred rebalance is pending and cannot be funded, so the
    /// book must not participate in an auction. Cheaper to ask than to handle a
    /// mid-settlement failure, and unlike [`Self::sync_quote_balances`] it needs
    /// no write access.
    #[inline(always)]
    pub fn is_quote_sync_fundable(&self) -> bool {
        self.projected_quote_balances().is_ok()
    }

    /// Whether the aggregator would skip this book as stale at `current_slot`.
    ///
    /// Mirrors the rule the matching engine applies before considering a book:
    /// `expiry_in_slots == 0` disables the check, otherwise a book untouched for
    /// that many slots is passed over. Quoters must apply the same rule or they
    /// will offer liquidity the program will not fill.
    #[inline]
    pub fn is_stale(&self, current_slot: u64) -> bool {
        self.expiry_in_slots > 0
            && current_slot.saturating_sub(self.last_updated_slot) >= self.expiry_in_slots
    }

    #[inline(always)]
    pub fn best_bid_price(&self) -> Option<u64> {
        if self.bid_levels[0].is_active() {
            self.bid_levels[0].absolute_price(self.mid_price_ticks)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn best_ask_price(&self) -> Option<u64> {
        if self.ask_levels[0].is_active() {
            self.ask_levels[0].absolute_price(self.mid_price_ticks)
        } else {
            None
        }
    }

    #[inline]
    pub fn active_bid_count(&self) -> usize {
        self.bid_levels.iter().filter(|l| l.is_active()).count()
    }

    #[inline]
    pub fn active_ask_count(&self) -> usize {
        self.ask_levels.iter().filter(|l| l.is_active()).count()
    }

    #[inline(always)]
    pub fn total_bid_size(&self) -> BaseLots {
        self.total_bid_base_lots
    }

    #[inline]
    pub fn total_ask_size(&self) -> Result<BaseLots, ArcherError> {
        let mut total = 0u64;
        for l in &self.ask_levels {
            total = total
                .checked_add(l.size_in_base_lots.as_u64())
                .ok_or(ArcherError::ArithmeticOverflow)?;
        }
        Ok(BaseLots::new(total))
    }
}

