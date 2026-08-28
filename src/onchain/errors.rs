use num_enum::IntoPrimitive;
use solana_program::program_error::ProgramError;
use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, IntoPrimitive, PartialEq)]
#[repr(u32)]
pub enum ArcherError {
    #[error("Invalid Archer treasury")]
    InvalidArcherTreasury = 0,

    // 1xx — Market initialization & configuration
    #[error("Invalid market parameters")]
    InvalidParams = 100,

    #[error("Market is already initialized")]
    MarketAlreadyInitialized = 101,

    #[error("Invalid lot size")]
    InvalidLotSize = 102,

    #[error("Invalid tick size")]
    InvalidTickSize = 103,

    #[error("Market is not active")]
    MarketNotActive = 107,

    #[error("Invalid status transition")]
    InvalidStatusTransition = 108,

    #[error("Market is not paused")]
    MarketNotPaused = 109,

    // 2xx — Account validation
    #[error("Invalid account")]
    InvalidAccount = 200,

    #[error("Account is not writable")]
    AccountNotWritable = 201,

    #[error("Account is not a signer")]
    AccountNotSigner = 202,

    #[error("Invalid PDA derivation")]
    InvalidPda = 203,

    #[error("Account data size mismatch")]
    AccountSizeMismatch = 204,

    #[error("Invalid admin pubkey")]
    InvalidAdmin = 205,

    #[error("Unauthorized")]
    Unauthorized = 206,

    #[error("Invalid market ID")]
    InvalidMarketId = 207,

    #[error("Account has wrong owner")]
    WrongOwner = 208,

    #[error("Account must be executable")]
    AccountNotExecutable = 209,

    #[error("Invalid account discriminator")]
    InvalidDiscriminator = 210,

    #[error("Account is not initialized")]
    AccountNotInitialized = 211,

    #[error("Account is already initialized")]
    AccountAlreadyInitialized = 212,

    // 3xx — Token, mint & vault validation
    #[error("Invalid mint account")]
    InvalidMint = 300,

    #[error("Invalid vault account")]
    InvalidVault = 301,

    #[error("Invalid vault address")]
    InvalidVaultAddress = 302,

    #[error("Invalid token account")]
    InvalidTokenAccount = 303,

    #[error("Invalid token program")]
    InvalidTokenProgram = 304,

    #[error("Invalid associated token program")]
    InvalidAssociatedTokenProgram = 305,

    #[error("Invalid system program")]
    InvalidSystemProgram = 306,

    #[error("Invalid rent")]
    InvalidRent = 307,

    #[error("Token-2022 mint carries an extension unsupported by Archer")]
    UnsupportedMintExtension = 308,

    #[error("Declared decimals do not match the mint's decimals")]
    MintDecimalsMismatch = 309,

    // 4xx — Arithmetic
    #[error("Arithmetic overflow")]
    ArithmeticOverflow = 400,

    #[error("Arithmetic underflow")]
    ArithmeticUnderflow = 401,

    #[error("Insufficient balance")]
    InsufficientBalance = 402,

    // 5xx — Maker book
    #[error("Invalid maker book")]
    InvalidMakerBook = 500,

    #[error("Maker book cannot be updated")]
    MakerBookFrozen = 501,

    #[error("Unauthorized market maker")]
    UnauthorizedMaker = 502,

    #[error("Invalid price")]
    InvalidPrice = 503,

    #[error("Invalid order sequence")]
    InvalidOrderSequence = 504,

    #[error("Crossing order book levels")]
    CrossingOrderLevels = 505,

    #[error("Invalid size at level")]
    InvalidSizeAtLevel = 506,

    #[error("No liquidity levels found")]
    NoMatchingLiquidity = 507,

    #[error("Too many maker books per swap")]
    TooManyMakerBooks = 508,

    #[error("Stale sequence number")]
    StaleSequenceNumber = 509,

    #[error("Maker registry is full")]
    MakerRegistryFull = 510,

    #[error("Maker already registered in registry")]
    MakerAlreadyRegistered = 511,

    #[error("Maker not registered in registry")]
    MakerNotRegistered = 512,

    #[error("Incomplete maker books: not all registered makers provided")]
    IncompleteMakerBooks = 513,

    #[error("Invalid maker registry")]
    InvalidMakerRegistry = 514,

    #[error("Maker book has locked balances; quotes must be cleared before closing")]
    MakerBookHasLockedBalance = 515,

    #[error("Maker book has free balances; funds must be withdrawn before closing")]
    MakerBookHasFreeBalance = 516,

    #[error("Maker book has active levels; book must be cleared before closing")]
    MakerBookHasActiveLevels = 517,

    #[error("Invalid maker book kind (must be 0 = MM or 1 = LO)")]
    InvalidMakerKind = 518,

    #[error("Mid price is immutable for a limit-order (LO) maker book; must stay 0")]
    MidPriceImmutableForLimitOrderBook = 519,

    #[error("Duplicate maker book account supplied")]
    DuplicateMakerBook = 521,

    #[error("Sequence number jumps too far ahead of the book's current one")]
    SequenceNumberTooFarAhead = 522,

    // 6xx — Fee
    #[error("Invalid maker fee")]
    InvalidFee = 600,

    #[error("Invalid fee recipient")]
    InvalidFeeRecipient = 601,

    #[error("Fee overflow")]
    FeeOverflow = 602,

    #[error("Insufficient protocol fees to cover rebates")]
    InsufficientProtocolFees = 603,

    #[error("Builder fee exceeds the maximum allowed")]
    BuilderFeeTooHigh = 606,

    #[error("Invalid builder fee wallet")]
    InvalidBuilderFeeWallet = 607,

    #[error("Permissionless markets must launch with the fixed protocol fee config")]
    InvalidPermissionlessFeeConfig = 608,

    // 7xx — Swap execution
    #[error("Invalid swap side")]
    InvalidSwapSide = 700,

    #[error("Invalid swap mode")]
    InvalidSwapMode = 701,

    #[error("Swap amount cannot be zero")]
    ZeroSwapAmount = 702,

    #[error("Slippage tolerance exceeded")]
    SlippageToleranceExceeded = 703,

    #[error("Quote overflow")]
    QuoteOverflow = 704,

    #[error("Price overflow")]
    PriceOverflow = 705,

    // 9xx — Invariant violations (post-execution checks)
    #[error("Fill quantity exceeds maker available liquidity")]
    FillExceedsMakerAvailable = 900,

    #[error("Fill produced zero output")]
    ZeroFill = 901,

    #[error("Price priority ordering violated across levels")]
    PricePriorityViolated = 902,

    #[error("Pro-rata distribution exceeds available at level")]
    DustExceedsLevel = 903,

    #[error("Fee solvency violated: net protocol revenue is negative")]
    FeeSolvencyViolated = 904,

    #[error("Token conservation violated: vault balance mismatch")]
    TokenConservationViolated = 905,

    #[error("Swap produced zero output atoms from nonzero lot trade")]
    ZeroOutput = 906,

    // 10xx — ArcherAccount
    #[error("Invalid ArcherAccount")]
    InvalidArcherAccount = 1000,

    #[error("Signer is neither the ArcherAccount's owner nor its delegate")]
    UnauthorizedAccountDelegate = 1001,

    #[error("Token account is not owned by the ArcherAccount")]
    InvalidArcherAccountTokenAccount = 1002,

    #[error("Builder fee exceeds the ArcherAccount's owner-set cap")]
    BuilderFeeExceedsAccountCap = 1003,

    #[error("Withdrawal would leave the ArcherAccount below rent exemption")]
    InsufficientArcherAccountLamports = 1004,

    #[error("Unknown delegated platform")]
    InvalidDelegatedPlatform = 1005,
}

impl From<ArcherError> for ProgramError {
    fn from(e: ArcherError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
