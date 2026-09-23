//! Like [`BTreeOrderStore`](super::BTreeOrderStore), but each order is stored
//! once: the order *is* the key of a `BTreeSet`, instead of a copy of its
//! `(price, timestamp, id)` being the key of a `BTreeMap` whose value is the
//! order again.
//!
//! - [`OrderByPriority`] wraps an [`Order`] and orders it by
//!   [`Order::cmp_book_priority`], so iterating a side's set yields orders
//!   best first. Each set holds one side only, so the comparison is always
//!   between same-side orders.
//! - To find an order by id, the `HashMap` index keeps its side, price and
//!   timestamp. Removal builds a probe order from those (quantity is ignored
//!   by the comparison) and looks it up in the set.
//!
//! Memory per order decreases from 80 B to 56 B.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use crate::error::BookError;
use crate::order_book::OrderBook;
use crate::types::{Order, OrderId, Price, Quantity, Side, Timestamp};

/// An order ordered by book priority. Only compare orders of the same side.
#[derive(Debug, Clone)]
#[repr(transparent)]
struct OrderByPriority(Order);

impl Ord for OrderByPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp_book_priority(&other.0)
    }
}

impl PartialOrd for OrderByPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Equality must agree with `Ord`, so it ignores quantity like `cmp` does.
impl PartialEq for OrderByPriority {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for OrderByPriority {}

/// Enough to rebuild an order's position in its side's set.
#[derive(Debug, Clone, Copy)]
struct Locator {
    side: Side,
    price: Price,
    timestamp: Timestamp,
}

/// Any quantity will do for a probe: the comparison ignores it.
const PROBE_QUANTITY: Quantity = match Quantity::from_ticks(1) {
    Some(q) => q,
    None => unreachable!(),
};

#[derive(Debug, Default, Clone)]
pub struct BTreeMemoryOrderStore {
    index: HashMap<OrderId, Locator>,
    bids: BTreeSet<OrderByPriority>,
    asks: BTreeSet<OrderByPriority>,
}

impl BTreeMemoryOrderStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn side_mut(&mut self, side: Side) -> &mut BTreeSet<OrderByPriority> {
        match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        }
    }

    /// Adds `order` to its side and records where it is.
    fn link(&mut self, order: Order) {
        let loc = Locator { side: order.side, price: order.price, timestamp: order.timestamp };
        self.index.insert(order.id, loc);
        self.side_mut(order.side).insert(OrderByPriority(order));
    }

    /// Removes an order by id and returns it, or `None` if the id is unknown.
    fn unlink(&mut self, id: OrderId) -> Option<Order> {
        let loc = self.index.remove(&id)?;
        let probe = OrderByPriority(Order {
            id,
            side: loc.side,
            price: loc.price,
            quantity: PROBE_QUANTITY,
            timestamp: loc.timestamp,
        });
        let order = self.side_mut(loc.side).take(&probe);
        Some(order.expect("indexed order is in its set").0)
    }
}

impl OrderBook for BTreeMemoryOrderStore {
    fn insert(&mut self, order: Order) -> Result<(), BookError> {
        if self.index.contains_key(&order.id) {
            return Err(BookError::DuplicateId(order.id));
        }
        self.link(order);
        Ok(())
    }

    fn update(&mut self, order: Order) -> Result<(), BookError> {
        // An update always carries a new timestamp, so the order always moves.
        self.unlink(order.id).ok_or(BookError::UnknownId(order.id))?;
        self.link(order);
        Ok(())
    }

    fn remove(&mut self, id: OrderId) -> Result<Order, BookError> {
        self.unlink(id).ok_or(BookError::UnknownId(id))
    }

    fn bids(&self) -> Vec<&Order> {
        self.bids.iter().map(|o| &o.0).collect()
    }

    fn asks(&self) -> Vec<&Order> {
        self.asks.iter().map(|o| &o.0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_entry_is_one_order() {
        assert_eq!(std::mem::size_of::<OrderByPriority>(), std::mem::size_of::<Order>());
    }
}
