//! One `BTreeMap` queue per price, found through a bitset over the price grid.
//!
//! - A bitset over all prices finds the next non-empty price.
//! - Each price has a `BTreeMap` keyed `(timestamp, id)`, so a queue is always in
//!   priority order and there are no links to maintain.
//! - `index` maps an id to where its order lives, so cancels need no search.
//!
//! | Op | Cost |
//! | --- | --- |
//! | `insert` / `remove` | O(log m), m = orders at that price |
//! | `update` | O(log m) whether or not the order moves |
//! | `bids` / `asks` | O(k) for k orders returned |

use std::collections::{BTreeMap, HashMap};

use super::hier_bitset::HierBitset;
use crate::error::BookError;
use crate::order_book::OrderBook;
use crate::types::{Order, OrderId, Price, Side, Timestamp};

/// Number of distinct prices on one side.
const PRICE_SPAN: usize = (Price::MAX_TICKS as i64 - Price::MIN_TICKS as i64 + 1) as usize;

/// Position of a price on one side; 0 is the best price (see `price_index`).
type PriceIdx = u32;

/// Maps a price to its position on `side`, so index 0 is always the best
/// price: lowest for sells, highest for buys.
fn price_index(side: Side, price: Price) -> PriceIdx {
    match side {
        Side::Sell => (price.ticks() - Price::MIN_TICKS) as u32,
        Side::Buy => (Price::MAX_TICKS - price.ticks()) as u32,
    }
}

/// Orders at one price, in priority order.
type Queue = BTreeMap<(Timestamp, OrderId), Order>;

#[derive(Debug, Clone)]
struct BookSide {
    /// One bit per price; set exactly when that price has at least one order.
    bits: HierBitset,
    /// Price index -> its queue. May hold empty queues; `bits` is the truth.
    queues: HashMap<PriceIdx, Queue>,
}

impl BookSide {
    fn new() -> Self {
        Self { bits: HierBitset::new(PRICE_SPAN), queues: HashMap::new() }
    }

    /// Orders best price first: visit each set bit, then that price's queue.
    fn orders(&self, capacity: usize) -> Vec<&Order> {
        let mut out = Vec::with_capacity(capacity);
        let mut next = self.bits.next(0);
        while let Some(price_idx) = next {
            out.extend(self.queues[&(price_idx as PriceIdx)].values());
            next = self.bits.next(price_idx + 1);
        }
        out
    }
}

/// Where an order lives: enough to find its queue entry.
#[derive(Debug, Clone, Copy)]
struct Locator {
    side: Side,
    price_idx: PriceIdx,
    timestamp: Timestamp,
}

#[derive(Debug, Clone)]
pub struct BucketMapOrderStore {
    index: HashMap<OrderId, Locator>,
    bids: BookSide,
    asks: BookSide,
}

impl Default for BucketMapOrderStore {
    fn default() -> Self {
        Self::new()
    }
}

impl BucketMapOrderStore {
    pub fn new() -> Self {
        Self { index: HashMap::new(), bids: BookSide::new(), asks: BookSide::new() }
    }

    fn side_mut(&mut self, side: Side) -> &mut BookSide {
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
        let book = self.side_mut(loc.side);
        book.queues.entry(loc.price_idx).or_default().insert((loc.timestamp, order.id), order);
        book.bits.set(loc.price_idx as usize);
    }

    /// Removes an order by id and returns it, or `None` if the id is unknown.
    fn unlink(&mut self, id: OrderId) -> Option<Order> {
        let loc = self.index.remove(&id)?;
        let book = self.side_mut(loc.side);
        let queue = book.queues.get_mut(&loc.price_idx).expect("indexed order has a queue");
        let order = queue.remove(&(loc.timestamp, id)).expect("indexed order is queued");
        if queue.is_empty() {
            // TRADEOFF: empty only clears its bit. Its empty map stays in `queues` and is reused when the price gets an order again, 
            // it may allocate lots of memory in long term.
            book.bits.clear(loc.price_idx as usize); // the empty queue stays for reuse
        }
        Some(order)
    }
}

impl OrderBook for BucketMapOrderStore {
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
                .queues
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
        self.bids.orders(self.index.len())
    }

    fn asks(&self) -> Vec<&Order> {
        self.asks.orders(self.index.len())
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
    fn emptied_price_clears_its_bit_but_keeps_its_queue() {
        let mut s = BucketMapOrderStore::new();
        let o = order(1, Side::Buy, 100, 1);
        let price_idx = price_index(Side::Buy, o.price);
        s.insert(o.clone()).unwrap();
        s.remove(o.id).unwrap();

        assert!(s.bids().is_empty());
        assert_eq!(s.bids.bits.next(0), None, "bit is cleared");
        assert!(s.bids.queues[&price_idx].is_empty(), "queue is kept");

        s.insert(order(2, Side::Buy, 100, 2)).unwrap();
        assert_eq!(s.bids().len(), 1);
        assert_eq!(s.bids.queues.len(), 1, "the kept queue was reused");
    }

    #[test]
    fn update_quantity_only_keeps_place() {
        let mut s = BucketMapOrderStore::new();
        s.insert(order(1, Side::Buy, 100, 1)).unwrap();
        s.insert(order(2, Side::Buy, 100, 2)).unwrap();
        let mut changed = order(1, Side::Buy, 100, 1);
        changed.quantity = Quantity::from_ticks(7).unwrap();
        s.update(changed.clone()).unwrap();
        assert_eq!(s.bids(), [&changed, &order(2, Side::Buy, 100, 2)]);
    }
}
