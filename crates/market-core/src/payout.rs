//! Dividing the pot by the share a resolution yields.
//!
//! The fee is charged on the amount that changes hands, never on the whole pot,
//! so a winning side always recovers at least its principal and a side that
//! neither wins nor loses pays nothing.

use market_math::Q64;

use crate::{CoreError, BPS_DENOMINATOR};

/// Which side of a market a token belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Yes,
    No,
}

/// Stake recorded per side, in collateral base units.
///
/// These are the market's own counters, not the outcome mints' supplies.
/// Anyone holding an SPL token may burn it directly through the Token program,
/// so a supply is not something payout arithmetic can be anchored to: a holder
/// burning their tokens would silently inflate everyone else's claim and drain
/// the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stakes {
    pub yes: u64,
    pub no: u64,
}

impl Stakes {
    pub fn pot(&self) -> Result<u64, CoreError> {
        self.yes.checked_add(self.no).ok_or(CoreError::Overflow)
    }

    pub fn of(&self, side: Side) -> u64 {
        match side {
            Side::Yes => self.yes,
            Side::No => self.no,
        }
    }

    /// Applies a deposit, refusing to exceed the per-side cap.
    ///
    /// The cap is per side rather than on the total: capping the total would
    /// let one account fill it from one side, leaving the other empty, which
    /// both blocks every later bet and forces a void that refunds the blocker
    /// in full.
    pub fn deposit(&self, side: Side, amount: u64, cap_per_side: u64) -> Result<Self, CoreError> {
        let current = self.of(side);
        let updated = current.checked_add(amount).ok_or(CoreError::Overflow)?;
        if updated > cap_per_side {
            return Err(CoreError::CapExceeded);
        }
        Ok(match side {
            Side::Yes => Stakes {
                yes: updated,
                no: self.no,
            },
            Side::No => Stakes {
                yes: self.yes,
                no: updated,
            },
        })
    }
}

/// How the pot is split, fixed once at resolution.
///
/// Recorded in the market account so that claiming is a pure lookup. Deriving
/// it again per claim would let the numbers move as holders burn tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settlement {
    /// Total owed to YES holders collectively.
    pub pool_yes: u64,
    /// Total owed to NO holders collectively.
    pub pool_no: u64,
    /// Protocol fee, taken from the transferred amount only.
    pub fee: u64,
}

impl Settlement {
    pub fn pool_for(&self, side: Side) -> u64 {
        match side {
            Side::Yes => self.pool_yes,
            Side::No => self.pool_no,
        }
    }
}

/// Splits the pot according to `share`, the fraction owed to YES.
pub fn settle(share: Q64, stakes: Stakes, fee_bps: u16) -> Result<Settlement, CoreError> {
    if share < Q64::ZERO || share > Q64::ONE {
        return Err(CoreError::ShareOutOfRange);
    }
    if u64::from(fee_bps) > BPS_DENOMINATOR {
        return Err(CoreError::FeeRateOutOfRange);
    }
    let pot = stakes.pot()?;

    // Checked, not argued. `share.raw() as u128` on a negative would silently
    // become 3.4e38, and at `pot == u64::MAX` the product sits one ulp below
    // overflowing a u128.
    let share_raw = u128::try_from(share.raw()).map_err(|_| CoreError::ShareOutOfRange)?;
    let gross_yes = share_raw
        .checked_mul(u128::from(pot))
        .ok_or(CoreError::Overflow)?
        >> Q64::FRACTIONAL_BITS;
    let gross_yes = u64::try_from(gross_yes).map_err(|_| CoreError::Overflow)?;

    let transferred = gross_yes.abs_diff(stakes.yes);
    let fee = (u128::from(transferred) * u128::from(fee_bps) / u128::from(BPS_DENOMINATOR)) as u64;

    // The fee comes out of whichever side gained; the side that lost stake
    // pays nothing beyond the stake itself.
    let (pool_yes, pool_no) = if gross_yes > stakes.yes {
        (gross_yes - fee, pot - gross_yes)
    } else {
        (gross_yes, pot - gross_yes - fee)
    };

    Ok(Settlement {
        pool_yes,
        pool_no,
        fee,
    })
}

/// What `burned` tokens of `side` redeem for.
///
/// Rounds down, so the sum over every holder can only fall short of the pool,
/// never exceed it. The remainder stays in the vault and reaches the treasury
/// through `sweep_dust`, months later.
pub fn payout_for(
    burned: u64,
    side: Side,
    stakes: Stakes,
    settlement: Settlement,
) -> Result<u64, CoreError> {
    let staked = stakes.of(side);
    if staked == 0 {
        return Err(CoreError::EmptySide);
    }
    if burned > staked {
        return Err(CoreError::BurnExceedsStake);
    }
    let pool = settlement.pool_for(side);
    Ok((u128::from(burned) * u128::from(pool) / u128::from(staked)) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn share(numerator: u64, denominator: u64) -> Q64 {
        Q64::from_uint(numerator)
            .div(Q64::from_uint(denominator))
            .expect("denominator is non-zero")
    }

    #[test]
    fn a_clean_yes_pays_the_losing_pool_minus_fee() {
        let stakes = Stakes {
            yes: 1_000,
            no: 4_000,
        };
        let settlement = settle(Q64::ONE, stakes, 200).unwrap();

        // 2% of the 4000 that changed hands.
        assert_eq!(settlement.fee, 80);
        assert_eq!(settlement.pool_yes, 4_920);
        assert_eq!(settlement.pool_no, 0);
        assert_eq!(payout_for(1_000, Side::Yes, stakes, settlement), Ok(4_920));
    }

    #[test]
    fn a_clean_no_is_the_mirror_image() {
        let stakes = Stakes {
            yes: 1_000,
            no: 4_000,
        };
        let settlement = settle(Q64::ZERO, stakes, 200).unwrap();

        assert_eq!(settlement.fee, 20); // 2% of the 1000 that moved
        assert_eq!(settlement.pool_yes, 0);
        assert_eq!(settlement.pool_no, 4_980);
        assert_eq!(payout_for(4_000, Side::No, stakes, settlement), Ok(4_980));
    }

    #[test]
    fn a_share_that_moves_nothing_charges_nothing() {
        // Half the pot to YES when YES already staked half: no transfer, so no
        // fee. A ramp landing exactly on the current split is a no-op.
        let stakes = Stakes {
            yes: 2_500,
            no: 2_500,
        };
        let settlement = settle(share(1, 2), stakes, 200).unwrap();

        assert_eq!(settlement.fee, 0);
        assert_eq!(settlement.pool_yes, 2_500);
        assert_eq!(settlement.pool_no, 2_500);
    }

    #[test]
    fn a_partial_share_splits_the_pot_and_charges_only_the_transfer() {
        // YES staked 1000 of a 5000 pot but the ramp awards it 60%.
        let stakes = Stakes {
            yes: 1_000,
            no: 4_000,
        };
        let settlement = settle(share(3, 5), stakes, 200).unwrap();

        // Three fifths is not binary-representable, so 60% of 5000 floors to
        // 2999. Every rounding here leans toward the vault, by construction.
        assert_eq!(
            settlement.pool_yes + settlement.pool_no + settlement.fee,
            5_000
        );
        assert_eq!(settlement.fee, 39); // 2% of the 1999 transferred
        assert_eq!(settlement.pool_yes, 2_960);
        assert_eq!(settlement.pool_no, 2_001);
        // Each side still redeems pro rata within itself.
        assert_eq!(payout_for(500, Side::Yes, stakes, settlement), Ok(1_480));
        assert_eq!(payout_for(1_000, Side::No, stakes, settlement), Ok(500));
    }

    #[test]
    fn an_exactly_representable_share_leaves_no_residue() {
        // The same market at a share of 5/8, which binary represents exactly.
        let stakes = Stakes {
            yes: 1_000,
            no: 4_000,
        };
        let settlement = settle(share(5, 8), stakes, 200).unwrap();

        assert_eq!(settlement.fee, 42); // 2% of the 2125 transferred
        assert_eq!(settlement.pool_yes, 3_083);
        assert_eq!(settlement.pool_no, 1_875);
        assert_eq!(
            settlement.pool_yes + settlement.pool_no + settlement.fee,
            5_000
        );
    }

    #[test]
    fn burning_more_than_a_side_holds_is_refused() {
        let stakes = Stakes { yes: 100, no: 100 };
        let settlement = settle(Q64::ONE, stakes, 0).unwrap();
        assert_eq!(
            payout_for(101, Side::Yes, stakes, settlement),
            Err(CoreError::BurnExceedsStake)
        );
    }

    #[test]
    fn an_empty_side_has_no_payout_to_compute() {
        let stakes = Stakes { yes: 0, no: 100 };
        let settlement = settle(Q64::ONE, stakes, 0).unwrap();
        assert_eq!(
            payout_for(0, Side::Yes, stakes, settlement),
            Err(CoreError::EmptySide)
        );
    }

    #[test]
    fn a_cap_is_enforced_per_side_not_on_the_total() {
        let stakes = Stakes { yes: 900, no: 0 };
        // Filling YES to the cap must not block NO.
        assert_eq!(
            stakes.deposit(Side::Yes, 200, 1_000),
            Err(CoreError::CapExceeded)
        );
        assert_eq!(
            stakes.deposit(Side::No, 1_000, 1_000),
            Ok(Stakes {
                yes: 900,
                no: 1_000
            })
        );
    }

    #[test]
    fn out_of_range_inputs_are_refused() {
        let stakes = Stakes { yes: 1, no: 1 };
        assert_eq!(
            settle(Q64::from_int(2), stakes, 0),
            Err(CoreError::ShareOutOfRange)
        );
        assert_eq!(
            settle(Q64::from_int(-1), stakes, 0),
            Err(CoreError::ShareOutOfRange)
        );
        assert_eq!(
            settle(Q64::ONE, stakes, 10_001),
            Err(CoreError::FeeRateOutOfRange)
        );
        assert_eq!(
            settle(
                Q64::ONE,
                Stakes {
                    yes: u64::MAX,
                    no: 1
                },
                0
            ),
            Err(CoreError::Overflow)
        );
    }

    #[test]
    fn the_largest_possible_pot_does_not_overflow_the_split() {
        // The product `share * pot` sits one ulp below overflowing a u128 at
        // this size, so it is checked rather than argued about.
        let stakes = Stakes {
            yes: u64::MAX,
            no: 0,
        };
        let settlement = settle(Q64::ONE, stakes, 200).unwrap();
        assert_eq!(settlement.fee, 0, "nothing changed hands");
        assert_eq!(settlement.pool_yes, u64::MAX);

        let stakes = Stakes {
            yes: 0,
            no: u64::MAX,
        };
        let settlement = settle(Q64::ONE, stakes, 200).unwrap();
        assert_eq!(
            settlement.pool_yes + settlement.pool_no + settlement.fee,
            u64::MAX
        );
    }

    proptest! {
        /// The protocol never owes more than it holds, and never keeps what it
        /// did not charge as fee.
        #[test]
        fn the_pot_is_conserved_exactly(
            yes in 0u64..u64::MAX / 4,
            no in 0u64..u64::MAX / 4,
            share_raw in 0i128..=(1i128 << 64),
            fee_bps in 0u16..=10_000,
        ) {
            let stakes = Stakes { yes, no };
            let settlement = settle(Q64::from_raw(share_raw), stakes, fee_bps).unwrap();
            let pot = stakes.pot().unwrap();
            prop_assert_eq!(
                settlement.pool_yes as u128 + settlement.pool_no as u128 + settlement.fee as u128,
                pot as u128
            );
        }

        /// A side that gains stake never ends up below its principal, whatever
        /// the fee rate. This is the promise that makes the fee comprehensible:
        /// it is charged on winnings, not on the bet.
        #[test]
        fn a_gaining_side_keeps_at_least_its_principal(
            yes in 1u64..u64::MAX / 4,
            no in 1u64..u64::MAX / 4,
            share_raw in 0i128..=(1i128 << 64),
            fee_bps in 0u16..=10_000,
        ) {
            let stakes = Stakes { yes, no };
            let settlement = settle(Q64::from_raw(share_raw), stakes, fee_bps).unwrap();
            let pot = stakes.pot().unwrap();
            let gross_yes = ((share_raw as u128 * u128::from(pot)) >> 64) as u64;

            // Branch on where the transfer moved stake, not on the answer:
            // `pool_yes > yes` would make the assertion restate its condition.
            if gross_yes > yes {
                prop_assert!(
                    settlement.pool_yes >= yes,
                    "YES gained {gross_yes} but ended below its {yes} principal"
                );
            } else if gross_yes < yes {
                prop_assert!(
                    settlement.pool_no >= no,
                    "NO gained but ended below its {no} principal"
                );
            } else {
                prop_assert_eq!(settlement.fee, 0, "nothing moved, so nothing is owed");
            }
        }

        /// Every holder claiming in full cannot overdraw the pool. Checked by
        /// splitting a side into many holders and summing the floors.
        #[test]
        fn the_sum_of_claims_never_exceeds_the_pool(
            yes in 1u64..1_000_000_000,
            no in 1u64..1_000_000_000,
            share_raw in 0i128..=(1i128 << 64),
            fee_bps in 0u16..=1_000,
            holders in 1usize..64,
        ) {
            let stakes = Stakes { yes, no };
            let settlement = settle(Q64::from_raw(share_raw), stakes, fee_bps).unwrap();

            for side in [Side::Yes, Side::No] {
                let staked = stakes.of(side);
                let each = staked / holders as u64;
                if each == 0 {
                    continue;
                }
                let mut total = 0u128;
                for _ in 0..holders {
                    total += u128::from(payout_for(each, side, stakes, settlement).unwrap());
                }
                // The final holder also sweeps whatever the split left over.
                let remainder = staked - each * holders as u64;
                total += u128::from(payout_for(remainder, side, stakes, settlement).unwrap());
                prop_assert!(
                    total <= u128::from(settlement.pool_for(side)),
                    "{} claimed over a pool of {}", total, settlement.pool_for(side)
                );
            }
        }

        /// Nothing here panics, whatever it is handed.
        ///
        /// The share is drawn mostly from the range `settle` actually accepts:
        /// a uniform `i128` is rejected on the first line almost every time,
        /// leaving the arithmetic below untouched.
        #[test]
        fn never_panics(
            yes in any::<u64>(),
            no in any::<u64>(),
            share_raw in prop_oneof![
                0i128..=(1i128 << 64),
                Just(0i128), Just(1i128 << 64), Just(-1i128), Just(i128::MAX),
                any::<i128>(),
            ],
            fee_bps in prop_oneof![0u16..=10_000, any::<u16>()],
            burned in any::<u64>(),
        ) {
            let stakes = Stakes { yes, no };
            if let Ok(settlement) = settle(Q64::from_raw(share_raw), stakes, fee_bps) {
                let _ = payout_for(burned, Side::Yes, stakes, settlement);
                let _ = payout_for(burned, Side::No, stakes, settlement);
            }
            let _ = stakes.deposit(Side::Yes, burned, u64::MAX);
        }
    }
}
