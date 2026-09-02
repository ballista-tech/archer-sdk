//! Who is acting, and on whose behalf.
//!
//! The type exists for the cases where a platform is acting for its users through
//! an [`ArcherAccount`]. There the *maker* is the account (a PDA, which cannot
//! sign) and the *signer* is its owner or delegate — two different keys, which a
//! bare `Pubkey` cannot express.
//!
//! Everything downstream takes `impl Into<Identity>`, so both shapes call the
//! same functions.

use crate::onchain::state::{ArcherAccount, DelegatedPlatform, MakerBook};
use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::pubkey::Pubkey;

/// The maker or taker an instruction acts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identity {
    /// A wallet acting for itself. It is both the maker/taker and the signer.
    ///
    /// This is the default and what a plain `Pubkey` becomes.
    Wallet(Pubkey),

    /// An [`ArcherAccount`] acting through an authorized key.
    ///
    /// `account` is the maker/taker of record and owns the collateral;
    /// `authority` is the owner or delegate that signs. A PDA cannot sign a
    /// transaction, which is why these are separate.
    ArcherAccount {
        account: Pubkey,
        authority: Pubkey,
    },
}

impl From<Pubkey> for Identity {
    /// A bare pubkey is a wallet acting for itself — the default for anyone not
    /// using ArcherAccount.
    fn from(wallet: Pubkey) -> Self {
        Identity::Wallet(wallet)
    }
}

impl From<&Pubkey> for Identity {
    fn from(wallet: &Pubkey) -> Self {
        Identity::Wallet(*wallet)
    }
}

impl Identity {
    /// A wallet acting for itself. Equivalent to passing the pubkey directly.
    #[inline]
    pub fn wallet(wallet: Pubkey) -> Self {
        Identity::Wallet(wallet)
    }

    /// An ArcherAccount identified by its owner and platform, acted on by
    /// `authority` (the account's owner or its delegate).
    ///
    /// Derives the account address; use [`Identity::archer_account_at`] if you
    /// already have it.
    #[inline]
    pub fn archer_account(
        owner: &Pubkey,
        platform: DelegatedPlatform,
        authority: Pubkey,
    ) -> Self {
        let (account, _) = crate::pda::derive_archer_account(owner, platform);
        Identity::ArcherAccount { account, authority }
    }

    /// An ArcherAccount whose address you already hold.
    #[inline]
    pub fn archer_account_at(account: Pubkey, authority: Pubkey) -> Self {
        Identity::ArcherAccount { account, authority }
    }

    /// Infer the identity from a book you have already fetched.
    ///
    /// The usual way to get one when you are operating on an existing book:
    /// reads the book's own record of what kind of maker owns it, so you cannot
    /// pick the wrong shape by hand.
    #[inline]
    pub fn for_book(book: &MakerBook, authority: Pubkey) -> Self {
        if book.maker_is_archer_account == 0 {
            Identity::Wallet(book.maker)
        } else {
            Identity::ArcherAccount {
                account: book.maker,
                authority,
            }
        }
    }

    /// The maker/taker of record — what goes in `MakerBook.maker`, and what owns
    /// the collateral and token accounts.
    #[inline]
    pub fn maker(&self) -> Pubkey {
        match self {
            Identity::Wallet(w) => *w,
            Identity::ArcherAccount { account, .. } => *account,
        }
    }

    /// The key that must sign.
    #[inline]
    pub fn authority(&self) -> Pubkey {
        match self {
            Identity::Wallet(w) => *w,
            Identity::ArcherAccount { authority, .. } => *authority,
        }
    }

    /// The ArcherAccount address, if this is one.
    #[inline]
    pub fn archer_account_key(&self) -> Option<Pubkey> {
        match self {
            Identity::Wallet(_) => None,
            Identity::ArcherAccount { account, .. } => Some(*account),
        }
    }

    #[inline]
    pub fn is_archer_account(&self) -> bool {
        matches!(self, Identity::ArcherAccount { .. })
    }

    /// Check this identity against a book before building an instruction.
    ///
    pub fn check_against_book(&self, book: &MakerBook) -> crate::error::SdkResult<()> {
        use crate::error::ArcherSDKError;

        if book.maker != self.maker() {
            return Err(ArcherSDKError::IdentityMismatch(format!(
                "book is owned by {} but this identity acts as {}",
                book.maker,
                self.maker()
            )));
        }

        match (book.maker_is_archer_account == 1, self) {
            (true, Identity::Wallet(_)) => Err(ArcherSDKError::IdentityMismatch(format!(
                "book {} is owned by ArcherAccount {} — build the identity with \
                 Identity::for_book or Identity::archer_account, not a bare wallet pubkey",
                book.market, book.maker
            ))),
            (false, Identity::ArcherAccount { .. }) => Err(ArcherSDKError::IdentityMismatch(
                format!(
                    "book is owned by wallet {} — pass the wallet pubkey directly",
                    book.maker
                ),
            )),
            _ => Ok(()),
        }
    }
}

/// Append the trailing authority signer to a **value or lifecycle** instruction
/// (create book, deposit, withdraw, set book delegate, close book).
///
/// Those instructions carry the maker — the ArcherAccount — in their existing
/// maker slot, and need the signer that authorizes it appended. A wallet identity
/// signs from the maker slot itself, so nothing is appended and the instruction
/// is byte-identical to what it has always been.
#[inline]
pub(crate) fn append_authority(mut ix: Instruction, identity: &Identity) -> Instruction {
    if let Identity::ArcherAccount { account, authority } = identity {
        for meta in ix.accounts.iter_mut() {
            if meta.pubkey == *account {
                meta.is_signer = false;
            }
        }
        ix.accounts
            .push(AccountMeta::new_readonly(*authority, true));
    }
    ix
}

/// Append the ArcherAccount to a **quote** instruction (`update_book`,
/// `update_mid_price`, `clear_book`, `update_expiry_in_slots`).
///
/// These are the other shape: the signer is already account 0, and what the
/// program needs appended is the account itself, because quote authority is read
/// live from it rather than from the book. That is what makes a revoked delegate
/// stop quoting immediately.
///
/// Nothing is appended for a wallet identity, so `update_mid_price` keeps the
/// exact 3-account layout its raw fast path requires.
#[inline]
pub(crate) fn append_archer_account(mut ix: Instruction, identity: &Identity) -> Instruction {
    if let Identity::ArcherAccount { account, .. } = identity {
        ix.accounts.push(AccountMeta::new_readonly(*account, false));
    }
    ix
}

/// Derive the canonical token account an [`ArcherAccount`] holds a mint in.
///
/// This is where a user's funds live once deposited, and it doubles as their
/// deposit address — it is an ordinary associated token account, so funds can
/// arrive from an exchange, a bridge, or any wallet with no Archer instruction
/// involved.
#[inline]
pub fn archer_account_token_address(archer_account: &Pubkey, mint: &Pubkey) -> Pubkey {
    spl_associated_token_account::get_associated_token_address(archer_account, mint)
}

/// Statically assert the SDK's view of `ArcherAccount` matches the program's.
///
/// The SDK mirrors the program's types rather than sharing them, so drift is the
/// standing risk. These are the same constants the program asserts on its side.
const _: () = {
    assert!(ArcherAccount::LEN == 176);
};
