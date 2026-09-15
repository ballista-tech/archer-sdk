//! Wire format of one `UpdateBookLimit` level op.

use solana_program::program_error::ProgramError;

use crate::onchain::{Side, MAX_LEVELS};

/// `cross_policy` values for `UpdateBookLimit`.
pub const CROSS_POLICY_REJECT: u8 = 0;
pub const CROSS_POLICY_ALLOW: u8 = 1;

pub const LEVEL_OP_SIDE_BID: u8 = 0;
pub const LEVEL_OP_SIDE_ASK: u8 = 1;

/// `expected_size` value that skips the compare-and-set check.
pub const LEVEL_OP_EXPECTED_ANY: u64 = u64::MAX;

/// Wire size of one [`LevelOp`].
pub const LEVEL_OP_SIZE: usize = 1 + 8 + 8 + 8;
/// Fixed header before the ops: discriminator, cross policy, sequence, count.
pub const LEVEL_OPS_HEADER_SIZE: usize = 1 + 1 + 8 + 1;
/// At most every slot on both sides.
pub const MAX_LEVEL_OPS: usize = MAX_LEVELS * 2;

/// One place / resize / cancel on an LO book, guarded by the resting size the
/// client last observed.
///
/// Ops within one instruction are applied in order. Cancels and decreases are
/// never subject to the post-only check; places and increases are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelOp {
    /// [`LEVEL_OP_SIDE_BID`] or [`LEVEL_OP_SIDE_ASK`].
    pub side: u8,
    /// Absolute price in ticks, `> 0`. LO books are anchored at mid 0, so
    /// this is also the level's `price_offset_ticks`.
    pub price_ticks: u64,
    /// Resting size the client last saw: `0` for "no level", or
    /// [`LEVEL_OP_EXPECTED_ANY`].
    pub expected_size: u64,
    /// Lots to rest after the op; `0` cancels.
    pub new_size: u64,
}

impl LevelOp {
    #[inline]
    pub const fn new(side: Side, price_ticks: u64, expected_size: u64, new_size: u64) -> Self {
        Self {
            side: match side {
                Side::Bid => LEVEL_OP_SIDE_BID,
                Side::Ask => LEVEL_OP_SIDE_ASK,
            },
            price_ticks,
            expected_size,
            new_size,
        }
    }

    /// Place at a price where nothing rests (`expected_size = 0`).
    #[inline]
    pub const fn place(side: Side, price_ticks: u64, size: u64) -> Self {
        Self::new(side, price_ticks, 0, size)
    }

    /// Cancel whatever rests, if anything (`expected_size = ANY`).
    #[inline]
    pub const fn cancel_any(side: Side, price_ticks: u64) -> Self {
        Self::new(side, price_ticks, LEVEL_OP_EXPECTED_ANY, 0)
    }

    /// Cancel or resize a level whose size the client observed.
    #[inline]
    pub const fn resize(side: Side, price_ticks: u64, observed: u64, new_size: u64) -> Self {
        Self::new(side, price_ticks, observed, new_size)
    }

    #[inline]
    pub fn side(&self) -> Result<Side, ProgramError> {
        match self.side {
            LEVEL_OP_SIDE_BID => Ok(Side::Bid),
            LEVEL_OP_SIDE_ASK => Ok(Side::Ask),
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }

    #[inline]
    pub fn is_cancel(&self) -> bool {
        self.new_size == 0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        if bytes.len() < LEVEL_OP_SIZE {
            return Err(ProgramError::InvalidInstructionData);
        }
        let u64_at = |at: usize| -> Result<u64, ProgramError> {
            Ok(u64::from_le_bytes(
                bytes[at..at + 8]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidInstructionData)?,
            ))
        };
        Ok(Self {
            side: bytes[0],
            price_ticks: u64_at(1)?,
            expected_size: u64_at(9)?,
            new_size: u64_at(17)?,
        })
    }

    pub fn encode(&self) -> [u8; LEVEL_OP_SIZE] {
        let mut out = [0u8; LEVEL_OP_SIZE];
        out[0] = self.side;
        out[1..9].copy_from_slice(&self.price_ticks.to_le_bytes());
        out[9..17].copy_from_slice(&self.expected_size.to_le_bytes());
        out[17..25].copy_from_slice(&self.new_size.to_le_bytes());
        out
    }
}

/// Encode a full `UpdateBookLimit` payload: `[36, cross_policy, seq u64 LE,
/// num_ops u8, ops…]`. Panics if `ops` is empty or longer than
/// [`MAX_LEVEL_OPS`] — callers chunk before they get here.
pub fn encode_level_ops(cross_policy: u8, sequence_number: u64, ops: &[LevelOp]) -> Vec<u8> {
    assert!(
        !ops.is_empty() && ops.len() <= MAX_LEVEL_OPS,
        "UpdateBookLimit carries 1..={MAX_LEVEL_OPS} ops, got {}",
        ops.len()
    );
    let mut data = Vec::with_capacity(LEVEL_OPS_HEADER_SIZE + ops.len() * LEVEL_OP_SIZE);
    data.push(crate::onchain::ArcherInstruction::UpdateBookLimit as u8);
    data.push(cross_policy);
    data.extend_from_slice(&sequence_number.to_le_bytes());
    data.push(ops.len() as u8);
    for op in ops {
        data.extend_from_slice(&op.encode());
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_round_trips_and_matches_the_program_layout() {
        let op = LevelOp::resize(Side::Ask, 100_050, 100, 60);
        let bytes = op.encode();
        assert_eq!(bytes[0], LEVEL_OP_SIDE_ASK);
        assert_eq!(u64::from_le_bytes(bytes[1..9].try_into().unwrap()), 100_050);
        assert_eq!(u64::from_le_bytes(bytes[9..17].try_into().unwrap()), 100);
        assert_eq!(u64::from_le_bytes(bytes[17..25].try_into().unwrap()), 60);
        assert_eq!(LevelOp::decode(&bytes).unwrap(), op);
    }

    #[test]
    fn payload_header_is_disc_policy_seq_count() {
        let ops = [LevelOp::place(Side::Bid, 99_900, 10), LevelOp::cancel_any(Side::Ask, 100_100)];
        let data = encode_level_ops(CROSS_POLICY_ALLOW, 7, &ops);
        assert_eq!(data.len(), LEVEL_OPS_HEADER_SIZE + 2 * LEVEL_OP_SIZE);
        assert_eq!(data[0], 36);
        assert_eq!(data[1], CROSS_POLICY_ALLOW);
        assert_eq!(u64::from_le_bytes(data[2..10].try_into().unwrap()), 7);
        assert_eq!(data[10], 2);
        assert_eq!(LevelOp::decode(&data[11..]).unwrap(), ops[0]);
        assert_eq!(LevelOp::decode(&data[11 + LEVEL_OP_SIZE..]).unwrap(), ops[1]);
        assert_eq!(ops[1].expected_size, LEVEL_OP_EXPECTED_ANY);
    }
}
