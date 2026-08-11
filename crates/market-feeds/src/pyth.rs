//! Reading a time-weighted price out of a Pyth `TwapUpdate` account.
//!
//! Pyth's receiver checks the signatures; this checks what it leaves to the
//! caller, all of which matters because whoever settles creates the account: the
//! averaged window must match the one declared (signed history runs far back, so
//! a free choice of window is a profitable one), the confidence interval must be
//! narrow, and `down_slots_ratio` low — an average over a halted chain is not an
//! average over a market.

use market_math::Q64;

use crate::FeedError;

/// Total account size, discriminator included.
pub const TWAP_UPDATE_LEN: usize = 112;

/// `sha256("account:TwapUpdate")[..8]`, verified by a unit test.
const DISCRIMINATOR: [u8; 8] = [0x68, 0xc0, 0xbc, 0x48, 0xf6, 0xa6, 0x0c, 0x51];

// Borsh lays these out in declaration order with no padding.
const OFF_FEED_ID: usize = 40;
const OFF_START_TIME: usize = 72;
const OFF_END_TIME: usize = 80;
const OFF_PRICE: usize = 88;
const OFF_CONF: usize = 96;
const OFF_EXPONENT: usize = 104;
const OFF_DOWN_SLOTS_RATIO: usize = 108;

/// `down_slots_ratio` is expressed out of this.
const DOWN_SLOTS_SCALE: u32 = 1_000_000;

/// Basis points denominator, for the confidence test.
const BPS: u128 = 10_000;

/// How wide a window may be trusted, and how far it may drift from the one the
/// market asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PythLimits {
    /// Seconds either boundary may differ from the requested instant.
    ///
    /// Pyth's own messages carry publish times of their own, so demanding an
    /// exact second would make every feed permanently unreadable. A few seconds
    /// of slack is the difference between a check that binds and one that
    /// cannot be satisfied.
    pub window_tolerance: u32,
    /// Largest tolerated confidence interval, in basis points of the price.
    pub max_confidence_bps: u32,
    /// Largest tolerated share of missed slots, out of 1 000 000.
    pub max_down_slots_ratio: u32,
}

/// What a `TwapUpdate` said, once it has been believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PythReading {
    /// The average price over the window, as a Q64.64 value.
    pub price: Q64,
    /// The confidence interval, in basis points of the price.
    pub confidence_bps: u32,
    pub start_time: i64,
    pub end_time: i64,
    /// Raw price and exponent, kept so the reading can be re-derived from the
    /// archived numbers alone.
    pub raw_price: i64,
    pub raw_conf: u64,
    pub exponent: i32,
    pub down_slots_ratio: u32,
}

/// Reads the average price over `[from, to]` from a Pyth TWAP account.
///
/// `expected_feed_id` is the instrument the market named. Everything else is a
/// bound the market declared in advance, so that the party who supplies this
/// account cannot choose anything that matters.
pub fn pyth_twap(
    account_data: &[u8],
    expected_feed_id: &[u8; 32],
    from: i64,
    to: i64,
    limits: PythLimits,
) -> Result<PythReading, FeedError> {
    if to <= from {
        return Err(FeedError::EmptyWindow);
    }
    if account_data.len() < TWAP_UPDATE_LEN {
        return Err(FeedError::AccountTooSmall);
    }
    if account_data[..8] != DISCRIMINATOR {
        return Err(FeedError::WrongAccountType);
    }
    if &account_data[OFF_FEED_ID..OFF_FEED_ID + 32] != expected_feed_id.as_slice() {
        return Err(FeedError::FeedIdMismatch);
    }

    let start_time = read_i64(account_data, OFF_START_TIME);
    let end_time = read_i64(account_data, OFF_END_TIME);
    let raw_price = read_i64(account_data, OFF_PRICE);
    let raw_conf = u64::from_le_bytes(slice8(account_data, OFF_CONF));
    let exponent = i32::from_le_bytes(slice4(account_data, OFF_EXPONENT));
    let down_slots_ratio = u32::from_le_bytes(slice4(account_data, OFF_DOWN_SLOTS_RATIO));

    // The window the market asked for, not merely a recent one.
    let tolerance = i64::from(limits.window_tolerance);
    if (start_time - from).abs() > tolerance || (end_time - to).abs() > tolerance {
        return Err(FeedError::WindowMismatch);
    }
    if end_time <= start_time {
        return Err(FeedError::WindowMismatch);
    }

    if down_slots_ratio > limits.max_down_slots_ratio.min(DOWN_SLOTS_SCALE) {
        return Err(FeedError::TooManySlotsMissed);
    }

    if raw_price <= 0 {
        return Err(FeedError::NonPositivePrice);
    }
    let confidence_bps = u32::try_from(
        u128::from(raw_conf)
            .checked_mul(BPS)
            .ok_or(FeedError::ConfidenceTooWide)?
            / raw_price.unsigned_abs() as u128,
    )
    .unwrap_or(u32::MAX);
    if confidence_bps > limits.max_confidence_bps {
        return Err(FeedError::ConfidenceTooWide);
    }

    // Pyth reports `price * 10^exponent`; the exponent is negative in practice.
    let price = Q64::from_int(raw_price)
        .scale_pow10(exponent)
        .map_err(FeedError::from)?;
    if price <= Q64::ZERO {
        return Err(FeedError::NonPositivePrice);
    }
    // Held to the same band a pool price is: outside it an ulp is a payout
    // step, and composing feeds could fall off the type. The Raydium path gets
    // this from `normalized_price`; this is the Pyth path's copy of the rule.
    if !crate::price::in_representable_band(price) {
        return Err(FeedError::PriceOutOfRange);
    }

    Ok(PythReading {
        price,
        confidence_bps,
        start_time,
        end_time,
        raw_price,
        raw_conf,
        exponent,
        down_slots_ratio,
    })
}

fn slice8(data: &[u8], offset: usize) -> [u8; 8] {
    let mut out = [0u8; 8];
    out.copy_from_slice(&data[offset..offset + 8]);
    out
}

fn slice4(data: &[u8], offset: usize) -> [u8; 4] {
    let mut out = [0u8; 4];
    out.copy_from_slice(&data[offset..offset + 4]);
    out
}

fn read_i64(data: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(slice8(data, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use sha2::Digest;
    use std::vec;
    use std::vec::Vec;

    const FEED_ID: [u8; 32] = [0x11; 32];
    const FROM: i64 = 1_800_000_000;
    const TO: i64 = FROM + 300;

    fn limits() -> PythLimits {
        PythLimits {
            window_tolerance: 5,
            max_confidence_bps: 100,
            max_down_slots_ratio: 50_000,
        }
    }

    /// A `TwapUpdate` account, laid out as Borsh writes one.
    struct Update {
        feed_id: [u8; 32],
        start_time: i64,
        end_time: i64,
        price: i64,
        conf: u64,
        exponent: i32,
        down_slots_ratio: u32,
        discriminator: [u8; 8],
    }

    impl Default for Update {
        fn default() -> Self {
            Update {
                feed_id: FEED_ID,
                start_time: FROM,
                end_time: TO,
                // 20_000_000_000 * 10^-8 = 200.0
                price: 20_000_000_000,
                conf: 20_000_000, // 10 bps of the price
                exponent: -8,
                down_slots_ratio: 0,
                discriminator: DISCRIMINATOR,
            }
        }
    }

    impl Update {
        fn encode(&self) -> Vec<u8> {
            let mut data = vec![0u8; TWAP_UPDATE_LEN];
            data[..8].copy_from_slice(&self.discriminator);
            data[8..40].copy_from_slice(&[0x99u8; 32]); // write_authority, unread
            data[OFF_FEED_ID..OFF_FEED_ID + 32].copy_from_slice(&self.feed_id);
            data[OFF_START_TIME..OFF_START_TIME + 8]
                .copy_from_slice(&self.start_time.to_le_bytes());
            data[OFF_END_TIME..OFF_END_TIME + 8].copy_from_slice(&self.end_time.to_le_bytes());
            data[OFF_PRICE..OFF_PRICE + 8].copy_from_slice(&self.price.to_le_bytes());
            data[OFF_CONF..OFF_CONF + 8].copy_from_slice(&self.conf.to_le_bytes());
            data[OFF_EXPONENT..OFF_EXPONENT + 4].copy_from_slice(&self.exponent.to_le_bytes());
            data[OFF_DOWN_SLOTS_RATIO..OFF_DOWN_SLOTS_RATIO + 4]
                .copy_from_slice(&self.down_slots_ratio.to_le_bytes());
            data
        }
    }

    fn read(update: &Update) -> Result<PythReading, FeedError> {
        pyth_twap(&update.encode(), &FEED_ID, FROM, TO, limits())
    }

    #[test]
    fn discriminator_matches_anchors_derivation() {
        let expected: [u8; 8] = sha2::Sha256::digest(b"account:TwapUpdate")[..8]
            .try_into()
            .expect("eight bytes");
        assert_eq!(DISCRIMINATOR, expected);
    }

    #[test]
    fn a_well_formed_update_reads_as_its_price() {
        let reading = read(&Update::default()).expect("valid update");
        assert_eq!(reading.price, Q64::from_int(200));
        assert_eq!(reading.confidence_bps, 10);
        assert_eq!(reading.start_time, FROM);
        assert_eq!(reading.end_time, TO);
    }

    #[test]
    fn the_window_must_be_the_one_the_market_asked_for() {
        // A poster with access to signed history would otherwise average over
        // whichever stretch of time suited them.
        let shifted_start = Update {
            start_time: FROM - 6,
            ..Update::default()
        };
        assert_eq!(read(&shifted_start), Err(FeedError::WindowMismatch));

        let shifted_end = Update {
            end_time: TO + 6,
            ..Update::default()
        };
        assert_eq!(read(&shifted_end), Err(FeedError::WindowMismatch));

        // Inside the tolerance is fine: Pyth's messages carry their own publish
        // times, and demanding an exact second would make the feed unusable.
        let nudged = Update {
            start_time: FROM + 4,
            end_time: TO - 4,
            ..Update::default()
        };
        assert!(read(&nudged).is_ok());

        let inverted = Update {
            start_time: TO,
            end_time: FROM,
            ..Update::default()
        };
        assert_eq!(read(&inverted), Err(FeedError::WindowMismatch));
    }

    #[test]
    fn a_disorderly_market_is_refused_rather_than_averaged() {
        // 200 bps of disagreement between publishers, against a 100 bps limit.
        let wide = Update {
            conf: 400_000_000,
            ..Update::default()
        };
        assert_eq!(read(&wide), Err(FeedError::ConfidenceTooWide));

        // Exactly at the limit is still acceptable.
        let borderline = Update {
            conf: 200_000_000,
            ..Update::default()
        };
        assert_eq!(
            read(&borderline).map(|reading| reading.confidence_bps),
            Ok(100)
        );
    }

    #[test]
    fn an_average_over_a_halted_chain_is_refused() {
        let halted = Update {
            down_slots_ratio: 50_001,
            ..Update::default()
        };
        assert_eq!(read(&halted), Err(FeedError::TooManySlotsMissed));
        let brief = Update {
            down_slots_ratio: 50_000,
            ..Update::default()
        };
        assert!(read(&brief).is_ok());
    }

    #[test]
    fn another_instrument_is_not_this_one() {
        let wrong = Update {
            feed_id: [0x22; 32],
            ..Update::default()
        };
        assert_eq!(read(&wrong), Err(FeedError::FeedIdMismatch));
    }

    #[test]
    fn malformed_accounts_are_refused_before_anything_is_believed() {
        let update = Update::default();
        assert_eq!(
            pyth_twap(
                &update.encode()[..TWAP_UPDATE_LEN - 1],
                &FEED_ID,
                FROM,
                TO,
                limits()
            ),
            Err(FeedError::AccountTooSmall)
        );
        let wrong_type = Update {
            discriminator: [0u8; 8],
            ..Update::default()
        };
        assert_eq!(read(&wrong_type), Err(FeedError::WrongAccountType));
        assert_eq!(
            pyth_twap(&update.encode(), &FEED_ID, TO, FROM, limits()),
            Err(FeedError::EmptyWindow)
        );
    }

    #[test]
    fn a_non_positive_price_is_not_a_price() {
        for price in [0i64, -1, i64::MIN] {
            let broken = Update {
                price,
                ..Update::default()
            };
            assert_eq!(
                read(&broken),
                Err(FeedError::NonPositivePrice),
                "price {price}"
            );
        }
    }

    #[test]
    fn a_price_outside_the_representable_band_is_refused() {
        // 2e10 * 10^8 is far above 2^56; believing it would let one absurd
        // reading push a composed predicate into overflow-and-void territory.
        let enormous = Update {
            exponent: 8,
            ..Update::default()
        };
        assert_eq!(read(&enormous), Err(FeedError::PriceOutOfRange));

        // 2e10 * 10^-24 is far below 2^-40, where an ulp is a payout step.
        let vanishing = Update {
            exponent: -24,
            conf: 0,
            ..Update::default()
        };
        assert_eq!(read(&vanishing), Err(FeedError::PriceOutOfRange));
    }

    #[test]
    fn the_exponent_scales_the_price() {
        let cases = [
            (-8i32, 20_000_000_000i64, 200i64),
            (-2, 20_000, 200),
            (0, 200, 200),
        ];
        for (exponent, price, expected) in cases {
            let update = Update {
                exponent,
                price,
                conf: 0,
                ..Update::default()
            };
            assert_eq!(
                read(&update).map(|reading| reading.price),
                Ok(Q64::from_int(expected)),
                "exponent {exponent}"
            );
        }
    }

    proptest! {
        /// Arbitrary bytes must never panic the parser.
        #[test]
        fn parsing_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
            let _ = pyth_twap(&bytes, &FEED_ID, FROM, TO, limits());
        }

        /// Nor must any window, however extreme.
        #[test]
        fn any_window_is_total(from in any::<i64>(), to in any::<i64>()) {
            let _ = pyth_twap(&Update::default().encode(), &FEED_ID, from, to, limits());
        }
    }
}
