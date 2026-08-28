//! ArcherAccount: lifecycle, funding, and delegation.
//!
//! **Optionality note** Nothing here is an absolute requirement for users 
//! to make markets or take on Archer; it exists so a trading platform can act
//! for its users without ever holding their keys.
//!
//! An ArcherAccount is a program-owned account that owns token accounts, holds
//! SOL for book rent, and owns MakerBooks. Its owner authorizes a delegate once;
//! after that the delegate can fund, defund, rebalance, quote and take — but
//! **value only leaves the account under the owner's signature.**
//!
//! # Funding it
//!
//! There is no deposit instruction, by design. The account holds tokens in
//! ordinary associated token accounts, so funding is a plain SPL transfer and
//! funding its rent is a plain system transfer.

use crate::onchain::state::{ArcherAccount, DelegatedPlatform, MakerBook};
use solana_program::{instruction::Instruction, pubkey::Pubkey, system_instruction};

use crate::error::{ArcherSDKError, SdkResult};
use crate::pda::derive_archer_account;

pub use crate::identity::archer_account_token_address as token_address;

/// Create an ArcherAccount for `owner` under `platform`.
///
/// The owner signs — creating the account is the authorization event — but
/// `payer` is a **separate signer**, so a platform can onboard a user holding no
/// SOL. Pass the owner for both when the user pays for themselves.
pub fn create(owner: &Pubkey, payer: &Pubkey, platform: DelegatedPlatform) -> Instruction {
    crate::onchain::builders::create_initialize_archer_account_instruction(
        *owner, *payer, platform,
    )
}

/// Authorize a delegate, and set the ceiling on builder fees it may charge.
///
/// One signature covers every book the account owns, present and future — and
/// takes effect immediately on all of them, including books created before this
/// call.
///
/// `max_builder_fee_ppm` is an authority bound rather than a fee setting: it is
/// the only delegate-reachable path in the program that pays a third party.
/// `0` (the default) forbids any builder fee.
pub fn set_delegate(
    owner: &Pubkey,
    platform: DelegatedPlatform,
    delegate: &Pubkey,
    max_builder_fee_ppm: u32,
) -> Instruction {
    crate::onchain::builders::create_set_archer_account_delegate_instruction(
        *owner,
        platform,
        Some(*delegate),
        max_builder_fee_ppm,
    )
}

/// Revoke the delegate. Takes effect on the very next instruction, on every book
/// the account owns — nothing caches authority.
///
/// This also resets `max_builder_fee_ppm` to `0`, so a later re-delegation has to
/// state the ceiling again explicitly.
pub fn revoke_delegate(owner: &Pubkey, platform: DelegatedPlatform) -> Instruction {
    crate::onchain::builders::create_revoke_archer_account_delegate_instruction(
        *owner, platform,
    )
}

/// Move value out of the account. **Owner only.**
///
/// The destination must be a token account the *owner* holds.
/// A delegate cannot reach this instruction.
pub fn withdraw_tokens(
    owner: &Pubkey,
    platform: DelegatedPlatform,
    mint: &Pubkey,
    token_program: &Pubkey,
    amount: u64,
) -> Instruction {
    let (account, _) = derive_archer_account(owner, platform);

    crate::onchain::builders::create_archer_account_withdraw_instruction(
        *owner,
        platform,
        amount,
        0,
        Some(crate::onchain::builders::ArcherAccountWithdrawTokenLeg {
            token_program: *token_program,
            mint: *mint,
            source_token_account: token_address(&account, mint),
            destination_token_account:
                spl_associated_token_account::get_associated_token_address(owner, mint),
        }),
    )
}

/// Withdraw SOL down to the account's rent-exempt minimum. **Owner only.**
///
/// Rejects rather than clamps if `lamports` would take the account below rent
/// exemption — a silently short withdrawal is worse than a loud failure.
pub fn withdraw_sol(owner: &Pubkey, platform: DelegatedPlatform, lamports: u64) -> Instruction {
    crate::onchain::builders::create_archer_account_withdraw_instruction(
        *owner, platform, 0, lamports, None,
    )
}

/// Create the account's token account for a mint, if it does not exist.
///
/// Idempotent, and `payer` is separate from the owner so a platform can create
/// it for a user holding no SOL.
pub fn create_token_account(
    payer: &Pubkey,
    archer_account: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Instruction {
    spl_associated_token_account::instruction::create_associated_token_account_idempotent(
        payer,
        archer_account,
        mint,
        token_program,
    )
}

/// Transfer tokens from a wallet into an ArcherAccount.
///
/// `from_authority` signs, and is normally the wallet that owns `from`.
pub fn deposit_tokens(
    from: &Pubkey,
    from_authority: &Pubkey,
    archer_account: &Pubkey,
    mint: &Pubkey,
    amount: u64,
) -> SdkResult<Instruction> {
    spl_token::instruction::transfer(
        &spl_token::ID,
        from,
        &token_address(archer_account, mint),
        from_authority,
        &[],
        amount,
    )
    .map_err(|e| ArcherSDKError::DeserializationError(format!("spl transfer: {e}")))
}

/// Fund an ArcherAccount with SOL, which is what pays rent for the books it
/// creates. A plain system transfer.
pub fn deposit_sol(from: &Pubkey, archer_account: &Pubkey, lamports: u64) -> Instruction {
    system_instruction::transfer(from, archer_account, lamports)
}

/// Decode an ArcherAccount from raw account data.
pub fn parse(data: &[u8]) -> SdkResult<&ArcherAccount> {
    ArcherAccount::load(data)
        .map_err(|_| ArcherSDKError::InvalidDiscriminator { expected: "ACHRACC1" })
}

/// Guard against the `MakerBook.delegate` footgun.
///
/// On a book owned by an ArcherAccount that field does not grant access — it
/// *restricts* one of the keys the account already authorizes to a single book.
/// Setting it to anything other than the account's current delegate therefore
/// grants nobody anything and locks the delegate out of that book.
///
/// The program cannot catch this; it has no way to know the intent. The SDK,
/// holding both the book and the account, can.
pub fn check_book_delegate_change(
    book: &MakerBook,
    account: &ArcherAccount,
    new_delegate: &Pubkey,
) -> SdkResult<()> {
    if book.maker_is_archer_account == 0 {
        // Wallet-owned book: the field grants, as it always has.
        return Ok(());
    }

    if new_delegate == &Pubkey::default() || new_delegate == &account.delegate {
        return Ok(());
    }

    Err(ArcherSDKError::BookDelegateWouldNotGrant {
        attempted: *new_delegate,
        current: account.delegate,
    })
}
