use std::fmt;
use std::num::NonZeroU64;

pub const PRICE_TICK_DENOMINATOR: i32 = 1_000;
pub const PRICE_DECIMALS: usize = 3;
pub const PRICE_ABS_BOUND: i32 = 10_000;

pub const QUANTITY_TICK_DENOMINATOR: u64 = 1_000_000;
pub const QUANTITY_DECIMALS: usize = 6;

pub type OrderId = NonZeroU64;  /// `I = ℕ>0`
pub type Timestamp = u64;

#[derive(Debug, Clone, Copy, PartialEq,Eq)]
pub enum Side {
    Buy,
    Sell,
}

// Price is represented in 1/1000 ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Price(i32);

// Quantity as a positive in 1/QUANTITY_TICK_DENOMINATOR ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quantity(NonZeroU64);


impl Price {
    pub const MIN_TICKS: i32 = -(PRICE_ABS_BOUND * PRICE_TICK_DENOMINATOR - 1);
    pub const MAX_TICKS: i32 = PRICE_ABS_BOUND * PRICE_TICK_DENOMINATOR - 1;

    pub const fn from_ticks(ticks: i32) -> Option<Self> {
        if ticks >= Self::MIN_TICKS && ticks <= Self::MAX_TICKS {
            Some(Self(ticks))
        } else {
            None
        }
    }

    pub const fn ticks(self) -> i32 {
        self.0
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_fixed_i32(f, self.0, PRICE_TICK_DENOMINATOR, PRICE_DECIMALS)
    }
}


impl Quantity {
    pub const fn from_ticks(ticks: u64) -> Option<Self> {
        match NonZeroU64::new(ticks) {
            Some(n) => Some(Self(n)),
            None => None,
        }
    }

    pub const fn ticks(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_fixed_u64(f, self.0.get(), QUANTITY_TICK_DENOMINATOR, QUANTITY_DECIMALS)
    }
}

// Memory: 32 bytes with 8-byte alignment.
//   id 8 (NonZeroU64) + quantity 8 (NonZeroU64) + timestamp 8 (u64)
//   + price 4 (i32) + side 1 = 29 bytes, padded to 32.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    pub price: Price,
    pub quantity: Quantity,
    pub timestamp: Timestamp,
}

impl Order {
    /// Buy: higher price first; Sell: lower price first; then earlier time; then smaller id.
    pub fn cmp_book_priority(&self, other: &Self) -> std::cmp::Ordering {
        debug_assert_eq!(self.side, other.side);
        let price_ord = match self.side {
            Side::Buy => other.price.cmp(&self.price),
            Side::Sell => self.price.cmp(&other.price),
        };
        price_ord
            .then(self.timestamp.cmp(&other.timestamp))
            .then(self.id.cmp(&other.id))
    }
}

fn write_fixed_i32(
    f: &mut fmt::Formatter<'_>,
    ticks: i32,
    denom: i32,
    decimals: usize,
) -> fmt::Result {
    if ticks < 0 {
        write!(f, "-")?;
    }
    write_fixed_u64(f, ticks.unsigned_abs() as u64, denom as u64, decimals)
}

fn write_fixed_u64(
    f: &mut fmt::Formatter<'_>,
    ticks: u64,
    denom: u64,
    decimals: usize,
) -> fmt::Result {
    let whole = ticks / denom;
    let frac = ticks % denom;
    write!(f, "{whole}.{frac:0decimals$}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_32_bytes() {
        assert_eq!(std::mem::size_of::<Order>(), 32);
    }

    #[test]
    fn tick_constants_are_powers_of_ten() {
        assert_eq!(10i32.pow(PRICE_DECIMALS as u32), PRICE_TICK_DENOMINATOR);
        assert_eq!(
            10u64.pow(QUANTITY_DECIMALS as u32),
            QUANTITY_TICK_DENOMINATOR
        );
    }

    #[test]
    fn price_tick_bounds_are_open_interval() {
        assert_eq!(Price::MIN_TICKS, -9_999_999);
        assert_eq!(Price::MAX_TICKS, 9_999_999);
        assert!(Price::from_ticks(Price::MIN_TICKS).is_some());
        assert!(Price::from_ticks(Price::MAX_TICKS).is_some());
        assert!(Price::from_ticks(Price::MIN_TICKS - 1).is_none());
        assert!(Price::from_ticks(Price::MAX_TICKS + 1).is_none());
    }

    #[test]
    fn quantity_rejects_zero() {
        assert!(Quantity::from_ticks(0).is_none());
        assert!(Quantity::from_ticks(1).is_some());
    }

    #[test]
    fn display_uses_tick_size() {
        assert_eq!(
            Price::from_ticks(1).unwrap().to_string(),
            "0.001"
        );
        assert_eq!(
            Price::from_ticks(-1_234).unwrap().to_string(),
            "-1.234"
        );
        assert_eq!(
            Quantity::from_ticks(1).unwrap().to_string(),
            "0.000001"
        );
        assert_eq!(
            Quantity::from_ticks(QUANTITY_TICK_DENOMINATOR).unwrap().to_string(),
            "1.000000"
        );
    }

    fn ord(id: u64, side: Side, price: i32, ts: u64) -> Order {
        Order {
            id: OrderId::new(id).unwrap(),
            side,
            price: Price::from_ticks(price).unwrap(),
            quantity: Quantity::from_ticks(1).unwrap(),
            timestamp: ts,
        }
    }

    #[test]
    fn priority_price_depends_on_side() {
        use std::cmp::Ordering::*;
        let hi = ord(1, Side::Buy, 200, 0);
        let lo = ord(1, Side::Buy, 100, 0);
        assert_eq!(hi.cmp_book_priority(&lo), Less);
        let lo = ord(1, Side::Sell, 100, 0);
        let hi = ord(1, Side::Sell, 200, 0);
        assert_eq!(lo.cmp_book_priority(&hi), Less);
    }

    #[test]
    fn priority_ties_break_on_timestamp_then_id() {
        use std::cmp::Ordering::*;
        let a = ord(2, Side::Buy, 100, 1);
        let b = ord(1, Side::Buy, 100, 2);
        assert_eq!(a.cmp_book_priority(&b), Less);
        let c = ord(1, Side::Buy, 100, 1);
        assert_eq!(c.cmp_book_priority(&a), Less);
        assert_eq!(a.cmp_book_priority(&a), Equal);
    }

    #[test]
    fn price_and_quantity_round_trip_ticks() {
        assert_eq!(Price::from_ticks(-7).unwrap().ticks(), -7);
        assert_eq!(Quantity::from_ticks(9).unwrap().ticks(), 9);
        assert!(Price::from_ticks(-1).unwrap() < Price::from_ticks(0).unwrap());
    }

    #[test]
    fn display_zero_and_extremes() {
        assert_eq!(Price::from_ticks(0).unwrap().to_string(), "0.000");
        assert_eq!(Price::from_ticks(Price::MAX_TICKS).unwrap().to_string(), "9999.999");
        assert_eq!(Price::from_ticks(Price::MIN_TICKS).unwrap().to_string(), "-9999.999");
    }
}
