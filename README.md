# order-book

A local in-memory store of buy and sell orders, kept in matching-engine priority so you can insert, amend, cancel, and read the full bid and ask books.

This is not an exchange: there is no matching, no trades, and no networking. It is the structure underneath those systems. Given a stream of new, amended, and cancelled limits, it maintains two sorted books.

## The problem

Markets advertise a book of unfilled limit orders. Each order is a tuple

\[
O = I \times S \times P \times Q \times T
\]

| Symbol | Meaning | Domain |
| --- | --- | --- |
| \(I\) | Order id | \(\mathbb{N}_{>0}\) (`NonZeroU64`) |
| \(S\) | Side | \(\{\mathrm{Buy}, \mathrm{Sell}\}\) |
| \(P\) | Price | \((-10000, 10000) \cap (\tfrac{1}{1000}\mathbb{Z})\) |
| \(Q\) | Quantity | \(\tfrac{1}{10^{6}}\mathbb{N}_{>0}\) |
| \(T\) | Timestamp | \(\mathbb{N}_0\) |

Prices and quantities are discrete, not floating-point money. The tick sizes are crate constants (`PRICE_TICK_DENOMINATOR = 1000`, `QUANTITY_TICK_DENOMINATOR = 10^6`). Price is stored as milli-ticks in \([-9\,999\,999, 9\,999\,999]\) — the open interval \((-10000, 10000)\) — and quantity as a positive count of \(10^{-6}\) units.

The store must support:

- **`insert`** — add a new order; duplicate ids are rejected
- **`update`** — if the id exists, replace \(S\), \(P\), \(Q\), and \(T\); priority follows the new values. Unknown ids error
- **`remove`** — cancel by id
- **`bids` / `asks`** — return every resting order on that side

Resting orders are ordered by:
1. Price (buys: highest first; sells: lowest first)
2. Timestamp (earlier first)
3. Order id (smaller first)

The best bid and best ask are the first elements of those lists.

## Implementations

Both stores implement `OrderBook`, so they can be swapped and checked against each other. They differ in how they find the best price and how they keep orders at one price in time order.


## Usage

```rust
use order_book::{BTreeOrderStore, Order, OrderBook, Price, Quantity, Side};
use std::num::NonZeroU64;

let mut book = BTreeOrderStore::new();
book.insert(Order {
    id: NonZeroU64::new(1).unwrap(),
    side: Side::Buy,
    price: Price::from_ticks(100_000).unwrap(), // 100.000
    quantity: Quantity::from_ticks(1_000_000).unwrap(), // 1.000000
    timestamp: 0,
})?;

let _bids = book.bids(); // all buys, best first
let _asks = book.asks(); // all sells, best first
```

## Tests and benches

```sh
cargo test
```

See [BENCHMARKS.md](benches/BENCHMARKS.md) for what is measured, how to plot it, and results.
