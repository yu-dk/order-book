//! One `BTreeSet` per side, plus a `HashMap` from order id to order.
//! 
//! - Bids are keyed `(Reverse(price), timestamp, id)` and asks `(price,
//!   timestamp, id)`.
//!
//! | Op | Cost |
//! | --- | --- |
//! | `insert` / `remove` | O(log n) |
//! | `update` | O(log 1)  or  O(log n) 
//! | `bids` / `asks` | O(k) for k orders returned |

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};

use crate::error::BookError;
use crate::order_book::OrderBook;
use crate::types::{Order, OrderId, Price, Side, Timestamp};

type BidKey = (Reverse<Price>, Timestamp, OrderId);
type AskKey = (Price, Timestamp, OrderId);

#[derive(Debug, Default, Clone)]
pub struct BTreeOrderStore {
    orders: HashMap<OrderId, Order>,
    bids: BTreeSet<BidKey>,
    asks: BTreeSet<AskKey>,
}

impl BTreeOrderStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn link(&mut self, o: &Order) {
        match o.side {
            Side::Buy => self.bids.insert((Reverse(o.price), o.timestamp, o.id)),
            Side::Sell => self.asks.insert((o.price, o.timestamp, o.id)),
        };
    }

    fn unlink(&mut self, o: &Order) {
        match o.side {
            Side::Buy => self.bids.remove(&(Reverse(o.price), o.timestamp, o.id)),
            Side::Sell => self.asks.remove(&(o.price, o.timestamp, o.id)),
        };
    }
}

impl OrderBook for BTreeOrderStore {
    fn insert(&mut self, order: Order) -> Result<(), BookError> {
        if self.orders.contains_key(&order.id) {
            return Err(BookError::DuplicateId(order.id));
        }
        self.link(&order);
        self.orders.insert(order.id, order);
        Ok(())
    }

    fn update(&mut self, order: Order) -> Result<(), BookError> {
        let slot = self
            .orders
            .get_mut(&order.id)
            .ok_or(BookError::UnknownId(order.id))?;

        // The index keys don't include quantity, so if side, price and
        // timestamp are unchanged the existing index entry is still valid.
        if slot.side == order.side
            && slot.price == order.price
            && slot.timestamp == order.timestamp
        {
            *slot = order;
            return Ok(());
        }

        let old = std::mem::replace(slot, order.clone());
        self.unlink(&old);
        self.link(&order);
        Ok(())
    }

    fn remove(&mut self, id: OrderId) -> Result<Order, BookError> {
        let old = self.orders.remove(&id).ok_or(BookError::UnknownId(id))?;
        self.unlink(&old);
        Ok(old)
    }

    fn bids(&self) -> Vec<&Order> {
        self.bids.iter().map(|&(_, _, id)| &self.orders[&id]).collect()
    }

    fn asks(&self) -> Vec<&Order> {
        self.asks.iter().map(|&(_, _, id)| &self.orders[&id]).collect()
    }
}
