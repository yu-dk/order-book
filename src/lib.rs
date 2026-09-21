//! In-memory order store for resting buy and sell limits.
//!
//! An order is a tuple `O = I × S × P × Q × T` with discrete, bounded prices
//! and discrete positive quantities. Each side is sorted by price (desc for
//! buys, asc for sells), then timestamp, then order id.

pub mod error;
pub mod order_book;
pub mod store;
pub mod types;

pub use order_book::OrderBook;
pub use error::BookError;
pub use store::{BTreeOrderStore, BucketMapOrderStore};
pub use types::{
    Order, OrderId, Price, Quantity, Side, Timestamp, PRICE_ABS_BOUND, PRICE_DECIMALS,
    PRICE_TICK_DENOMINATOR, QUANTITY_DECIMALS, QUANTITY_TICK_DENOMINATOR,
};
