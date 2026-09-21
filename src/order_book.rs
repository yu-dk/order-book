use crate::error::BookError;
use crate::types::{Order, OrderId};

// order store with price/time/id priority on each side.
pub trait OrderBook {
    fn insert(&mut self, order: Order) -> Result<(), BookError>;
    // replaces all fields of an existing order (`S`, `P`, `Q`, `T`),  missing ids returns error
    fn update(&mut self, order: Order) -> Result<(), BookError>;
    fn remove(&mut self, id: OrderId) -> Result<Order, BookError>;
    // All resting buys, desc price, then timestamp, then id.
    fn bids(&self) -> Vec<&Order>;
    // All resting sells, asc price, then timestamp, then id.
    fn asks(&self) -> Vec<&Order>;
}
