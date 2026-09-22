//! One `BTreeMap` per side from (price, timestamp, id) to the order, plus a
//! `HashMap` from order id to what is needed to rebuild that key.
//!
//! - Bids are keyed `(Reverse(price), timestamp, id)` and asks `(price,
//!   timestamp, id)`, so iterating a map yields orders best first.
//! - The maps own the orders, so `bids` / `asks` read them in order with no
//!   per-order lookup.
//!
//! | Op | Cost |
//! | --- | --- |
//! | `insert` / `remove` / `update` | O(log n), n = orders on that side |
//! | `bids` / `asks` | O(k) for k orders returned |

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use crate::error::BookError;
use crate::order_book::OrderBook;
use crate::types::{Order, OrderId, Price, Side, Timestamp};

type BidKey = (Reverse<Price>, Timestamp, OrderId);
type AskKey = (Price, Timestamp, OrderId);

/// Enough to rebuild an order's key in its side's map.
#[derive(Debug, Clone, Copy)]
struct Locator {
    side: Side,
    price: Price,
    timestamp: Timestamp,
}

#[derive(Debug, Default, Clone)]
pub struct BTreeOrderStore {
    index: HashMap<OrderId, Locator>,
    bids: BTreeMap<BidKey, Order>,
    asks: BTreeMap<AskKey, Order>,
}

impl BTreeOrderStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `order` to its side and records where it is.
    fn link(&mut self, order: Order) {
        let loc = Locator { side: order.side, price: order.price, timestamp: order.timestamp };
        self.index.insert(order.id, loc);
        match order.side {
            Side::Buy => self.bids.insert((Reverse(order.price), order.timestamp, order.id), order),
            Side::Sell => self.asks.insert((order.price, order.timestamp, order.id), order),
        };
    }

    /// Removes an order by id and returns it, or `None` if the id is unknown.
    fn unlink(&mut self, id: OrderId) -> Option<Order> {
        let loc = self.index.remove(&id)?;
        let order = match loc.side {
            Side::Buy => self.bids.remove(&(Reverse(loc.price), loc.timestamp, id)),
            Side::Sell => self.asks.remove(&(loc.price, loc.timestamp, id)),
        };
        Some(order.expect("indexed order is in its map"))
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
        self.bids.values().collect()
    }

    fn asks(&self) -> Vec<&Order> {
        self.asks.values().collect()
    }
}
