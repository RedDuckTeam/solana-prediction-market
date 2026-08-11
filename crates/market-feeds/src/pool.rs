//! Reading the fields a feed registration needs out of Raydium's `PoolState`.
//!
//! Registration used to take the pair's mints and decimals as arguments and
//! trust governance to have copied them correctly. Every one of those values
//! already sits in the pool account the caller must supply, so they are read
//! from it instead: a transposed pair or a wrong decimal count is a price
//! normalised upside down, discovered only when a market settles wrong.

use crate::FeedError;

/// Total account size, discriminator included. Fixed: `PoolState` is
/// `#[repr(C, packed)]` with no variable-length fields.
pub const POOL_STATE_LEN: usize = 1544;

/// `sha256("account:PoolState")[..8]`, verified by a unit test.
const DISCRIMINATOR: [u8; 8] = [0xf7, 0xed, 0xe3, 0xf5, 0xd7, 0xc3, 0xde, 0x46];

// Byte offsets into the account: 8 discriminator, `bump` (1), `amm_config`
// (32), `owner` (32), then the fields below. Verified against recorded
// mainnet accounts in `tests/mainnet.rs`.
const OFF_MINT0: usize = 73;
const OFF_MINT1: usize = 105;
const OFF_OBSERVATION_KEY: usize = 201;
const OFF_MINT_DECIMALS0: usize = 233;
const OFF_MINT_DECIMALS1: usize = 234;

/// The slice of `PoolState` a registration cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolInfo {
    pub mint0: [u8; 32],
    pub mint1: [u8; 32],
    /// The observation ring this pool writes. Registration must check it names
    /// the ring being registered, or a feed could pair one pool's identity
    /// with another pool's prices.
    pub observation_key: [u8; 32],
    pub mint_decimals0: u8,
    pub mint_decimals1: u8,
}

/// Parses a `PoolState` account.
pub fn read_pool_state(data: &[u8]) -> Result<PoolInfo, FeedError> {
    if data.len() < POOL_STATE_LEN {
        return Err(FeedError::AccountTooSmall);
    }
    if data[..8] != DISCRIMINATOR {
        return Err(FeedError::WrongAccountType);
    }

    let key = |offset: usize| -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&data[offset..offset + 32]);
        out
    };

    Ok(PoolInfo {
        mint0: key(OFF_MINT0),
        mint1: key(OFF_MINT1),
        observation_key: key(OFF_OBSERVATION_KEY),
        mint_decimals0: data[OFF_MINT_DECIMALS0],
        mint_decimals1: data[OFF_MINT_DECIMALS1],
    })
}

/// Lays out a `PoolState` holding the given fields, for tests that need a pool
/// account without a chain to read one from. The other fields stay zero; the
/// parser never looks at them.
pub fn write_pool_state(
    mint0: &[u8; 32],
    mint1: &[u8; 32],
    observation_key: &[u8; 32],
    mint_decimals0: u8,
    mint_decimals1: u8,
) -> [u8; POOL_STATE_LEN] {
    let mut data = [0u8; POOL_STATE_LEN];
    data[..8].copy_from_slice(&DISCRIMINATOR);
    data[OFF_MINT0..OFF_MINT0 + 32].copy_from_slice(mint0);
    data[OFF_MINT1..OFF_MINT1 + 32].copy_from_slice(mint1);
    data[OFF_OBSERVATION_KEY..OFF_OBSERVATION_KEY + 32].copy_from_slice(observation_key);
    data[OFF_MINT_DECIMALS0] = mint_decimals0;
    data[OFF_MINT_DECIMALS1] = mint_decimals1;
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn discriminator_matches_anchors_derivation() {
        let expected: [u8; 8] = sha2::Sha256::digest(b"account:PoolState")[..8]
            .try_into()
            .expect("eight bytes");
        assert_eq!(DISCRIMINATOR, expected);
    }

    #[test]
    fn a_written_pool_reads_back_exactly() {
        let data = write_pool_state(&[0x11; 32], &[0x22; 32], &[0x33; 32], 9, 6);
        assert_eq!(
            read_pool_state(&data),
            Ok(PoolInfo {
                mint0: [0x11; 32],
                mint1: [0x22; 32],
                observation_key: [0x33; 32],
                mint_decimals0: 9,
                mint_decimals1: 6,
            })
        );
    }

    #[test]
    fn malformed_accounts_are_refused() {
        let data = write_pool_state(&[0x11; 32], &[0x22; 32], &[0x33; 32], 9, 6);
        assert_eq!(
            read_pool_state(&data[..POOL_STATE_LEN - 1]),
            Err(FeedError::AccountTooSmall)
        );

        let mut wrong_type = data;
        wrong_type[0] ^= 0xff;
        assert_eq!(
            read_pool_state(&wrong_type),
            Err(FeedError::WrongAccountType)
        );
    }
}
