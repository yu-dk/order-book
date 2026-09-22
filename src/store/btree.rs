//! One `BTreeSet` of orders per side, plus a `HashMap` from order id to where
//! the order sits.
//!
//! - Each set holds the orders themselves, sorted by `Order::cmp_book_priority`
//!   (price best first, then timestamp, then id), so `bids` / `asks` read the
//!   tree in order with no per-order lookup.
//! - `index` keeps only the fields that sort an order, enough to rebuild a
//!   probe that finds it in its set.
//!
//! | Op | Cost |
//! | --- | --- |
//! | `insert` / `remove` / `update` | O(log n), n = orders on that side |
//! | `bids` / `asks` | O(k) for k orders returned |

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use crate::error::BookError;
use crate::order_book::OrderBook;
use crate::types::{Order, OrderId, Price, Quantity, Side, Timestamp};

/// An order in its side's set. Ordered by `cmp_book_priority`, which never
/// looks at quantity, so a probe with any quantity finds the stored order.
#[derive(Debug, Clone)]
struct Entry(Order);

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp_book_priority(&other.0)
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Entry {}

/// Where an order lives: the fields `Entry` sorts by.
#[derive(Debug, Clone, Copy)]
struct Locator {
    side: Side,
    price: Price,
    timestamp: Timestamp,
}

/// Placeholder quantity for probes; ignored by `Entry`'s ordering.
const PROBE_QUANTITY: Quantity = match Quantity::from_ticks(1) {
    Some(q) => q,
    None => unreachable!(),
};

#[derive(Debug, Default, Clone)]
pub struct BTreeOrderStore {
    index: HashMap<OrderId, Locator>,
    bids: BTreeSet<Entry>,
    asks: BTreeSet<Entry>,
}

impl BTreeOrderStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn side_mut(&mut self, side: Side) -> &mut BTreeSet<Entry> {
        match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        }
    }

    /// Adds `order` to its side and records where it is.
    fn link(&mut self, order: Order) {
        let loc = Locator { side: order.side, price: order.price, timestamp: order.timestamp };
        self.index.insert(order.id, loc);
        self.side_mut(loc.side).insert(Entry(order));
    }

    /// Removes an order by id and returns it, or `None` if the id is unknown.
    fn unlink(&mut self, id: OrderId) -> Option<Order> {
        let loc = self.index.remove(&id)?;
        let probe = Entry(Order {
            id,
            side: loc.side,
            price: loc.price,
            quantity: PROBE_QUANTITY,
            timestamp: loc.timestamp,
        });
        let Entry(order) = self.side_mut(loc.side).take(&probe).expect("indexed order is in its set");
        Some(order)
    }
}

impl OrderBook for BTreeOrderStore {
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
        self.bids.iter().map(|Entry(o)| o).collect()
    }

    fn asks(&self) -> Vec<&Order> {
        self.asks.iter().map(|Entry(o)| o).collect()
    }
}
