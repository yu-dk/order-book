//! One `BTreeMap` of price levels per side; each level is a `BTreeMap` queue.
//!
//! - Levels are keyed by `PriceIdx` (0 is the best price, see `price_index`),
//!   so iterating a side's map yields levels best first.
//! - Each level has a `BTreeMap` keyed `(timestamp, id)`, so a queue is always
//!   in priority order.
//! - `index` maps an id to where its order lives, so cancels need no search.
//! - A level is removed as soon as its queue empties, so the map only holds
//!   live prices.
//!
//! | Op | Cost |
//! | --- | --- |
//! | `insert` / `remove` | O(log L + log m), L = live prices, m = orders at that price |
//! | `update` | O(log L + log m) whether or not the order moves |
//! | `bids` / `asks` | O(k) for k orders returned |

use std::collections::{BTreeMap, HashMap};

use super::bucket_map::{price_index, PriceIdx};
use crate::error::BookError;
use crate::order_book::OrderBook;
use crate::types::{Order, OrderId, Side, Timestamp};

/// Orders at one price, in priority order.
type Queue = BTreeMap<(Timestamp, OrderId), Order>;

/// Price index -> its queue, best price first. Never holds an empty queue.
type Levels = BTreeMap<PriceIdx, Queue>;

/// Where an order lives: enough to find its queue entry.
#[derive(Debug, Clone, Copy)]
struct Locator {
    side: Side,
    price_idx: PriceIdx,
    timestamp: Timestamp,
}

#[derive(Debug, Default, Clone)]
pub struct LevelMapOrderStore {
    index: HashMap<OrderId, Locator>,
    bids: Levels,
    asks: Levels,
}

impl LevelMapOrderStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn side_mut(&mut self, side: Side) -> &mut Levels {
        match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        }
    }

    /// Queues `order` at its price and records where it is.
    fn link(&mut self, order: Order) {
        let loc = Locator {
            side: order.side,
            price_idx: price_index(order.side, order.price),
            timestamp: order.timestamp,
        };
        self.index.insert(order.id, loc);
        self.side_mut(loc.side)
            .entry(loc.price_idx)
            .or_default()
            .insert((loc.timestamp, order.id), order);
    }

    /// Removes an order by id and returns it, or `None` if the id is unknown.
    fn unlink(&mut self, id: OrderId) -> Option<Order> {
        let loc = self.index.remove(&id)?;
        let levels = self.side_mut(loc.side);
        let queue = levels.get_mut(&loc.price_idx).expect("indexed order has a queue");
        let order = queue.remove(&(loc.timestamp, id)).expect("indexed order is queued");
        if queue.is_empty() {
            levels.remove(&loc.price_idx);
        }
        Some(order)
    }
}

/// Orders best price first: each level in key order, then its queue.
fn orders(levels: &Levels, capacity: usize) -> Vec<&Order> {
    let mut out = Vec::with_capacity(capacity);
    for queue in levels.values() {
        out.extend(queue.values());
    }
    out
}

impl OrderBook for LevelMapOrderStore {
    fn insert(&mut self, order: Order) -> Result<(), BookError> {
        if self.index.contains_key(&order.id) {
            return Err(BookError::DuplicateId(order.id));
        }
        self.link(order);
        Ok(())
    }

    fn update(&mut self, order: Order) -> Result<(), BookError> {
        let loc = *self.index.get(&order.id).ok_or(BookError::UnknownId(order.id))?;
        let same_place = order.side == loc.side
            && price_index(order.side, order.price) == loc.price_idx
            && order.timestamp == loc.timestamp;
        if same_place {
            // Only the quantity can differ, so the entry keeps its place.
            let entry = self
                .side_mut(loc.side)
                .get_mut(&loc.price_idx)
                .and_then(|q| q.get_mut(&(loc.timestamp, order.id)))
                .expect("indexed order is queued");
            *entry = order;
            return Ok(());
        }
        self.unlink(order.id);
        self.link(order);
        Ok(())
    }

    fn remove(&mut self, id: OrderId) -> Result<Order, BookError> {
        self.unlink(id).ok_or(BookError::UnknownId(id))
    }

    fn bids(&self) -> Vec<&Order> {
        orders(&self.bids, self.index.len())
    }

    fn asks(&self) -> Vec<&Order> {
        orders(&self.asks, self.index.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Price, Quantity};

    fn order(n: u64, side: Side, price: i32, ts: u64) -> Order {
        Order {
            id: OrderId::new(n).unwrap(),
            side,
            price: Price::from_ticks(price).unwrap(),
            quantity: Quantity::from_ticks(1).unwrap(),
            timestamp: ts,
        }
    }

    #[test]
    fn emptied_price_removes_its_level() {
        let mut s = LevelMapOrderStore::new();
        s.insert(order(1, Side::Buy, 100, 1)).unwrap();
        s.insert(order(2, Side::Buy, 100, 2)).unwrap();
        s.remove(OrderId::new(1).unwrap()).unwrap();
        assert_eq!(s.bids.len(), 1, "level still has order 2");
        s.remove(OrderId::new(2).unwrap()).unwrap();
        assert!(s.bids.is_empty(), "empty level is dropped");
    }

    #[test]
    fn repricing_moves_order_between_levels() {
        let mut s = LevelMapOrderStore::new();
        s.insert(order(1, Side::Sell, 100, 1)).unwrap();
        s.update(order(1, Side::Sell, 90, 1)).unwrap();
        let keys: Vec<_> = s.asks.keys().copied().collect();
        assert_eq!(keys, [price_index(Side::Sell, Price::from_ticks(90).unwrap())]);
    }

    #[test]
    fn update_quantity_only_keeps_place() {
        let mut s = LevelMapOrderStore::new();
        s.insert(order(1, Side::Buy, 100, 1)).unwrap();
        s.insert(order(2, Side::Buy, 100, 2)).unwrap();
        let mut changed = order(1, Side::Buy, 100, 1);
        changed.quantity = Quantity::from_ticks(7).unwrap();
        s.update(changed.clone()).unwrap();
        assert_eq!(s.bids(), [&changed, &order(2, Side::Buy, 100, 2)]);
    }
}
