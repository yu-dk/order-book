# order-book

A local orderbook implementation which handles a stream of order events and maintains the state of the orderbook.

Assumptions:
- No assumed RPS; the book is single-threaded.
- Memory is not a constraint (32 bytes per order); we optimise for time.
- In-memory only: no persistence or crash recovery. State lives for the process's lifetime.

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

Prices and quantities are discrete, not floating-point money. The tick sizes are constants (`PRICE_TICK_DENOMINATOR = 1000`, `QUANTITY_TICK_DENOMINATOR = 10^6`). Price is stored as an integer number of 0.001 ticks in \([-9\,999\,999, 9\,999\,999]\) — the open interval \((-10000, 10000)\) — and quantity as a positive count of \(10^{-6}\) units.

The store must support:

- **`insert`** — add a new order; duplicate ids are rejected
- **`update`** — if the id exists, replace \(S\), \(P\), \(Q\), and \(T\); priority follows the new values. An update always changes \(T\), so the order always loses its place in the queue and the stores implement it as a remove plus an insert. Unknown ids error
- **`remove`** — cancel by id
- **`bids` / `asks`** — return ordered orders on that side, the best bid and best ask are the first elements of those lists

Resting orders are ordered by:
1. Price (buys: highest first; sells: lowest first)
2. Timestamp (earlier first)
3. Order id (smaller first)

## File structure

### Implementations

Three implementations of `OrderBook` live in `src/store`:

**Baseline: `btree.rs`.** One `BTreeSet` of whole orders per side, sorted by (price, timestamp, id), plus a `HashMap<OrderId, Locator>` that keeps each order's side, price and timestamp so it can be found in its set in O(log n). We used Rust's `BTreeMap` instead of red-black binary tree.

**Price-level stores.** When n is large, the tree gets deeper and its nodes no longer fit in L1/L2 cache, so each jump between nodes gets expensive. The next two implementations first find the order's price level, then work on the m orders queued at that price. Both keep each price's queue as a `BTreeMap` keyed by (timestamp, id), and store orders inside the queue.

- `bucket_map.rs`: per side, queues live in a `HashMap<PriceIdx, Queue>`, and a hierarchical bitset over the whole price grid keeps prices in order. Insert/remove/update cost O(log m), independent of L, the number of live prices.
- `level_map.rs`: per side, queues live in a `BTreeMap<PriceIdx, Queue>`, which keeps prices in order by itself. Insert/remove/update cost O(log L + log m).

How the two price-level stores differ:

| | `bucket_map` | `level_map` |
| --- | --- | --- |
| Find a price's queue | `HashMap` lookup, O(1) | tree search, O(log L) |
| Price order for reads | bitset (set/clear a bit, find next set bit) | the map's own key order |
| Reads to find the next non-empty price | 5 levels of the ~20M-price bitset. 1–9 word reads: up, then down | first price: 4–5 nodes for L = 6,000; each next price: O(1) on average |
| Structures to keep in sync | id index, queue `HashMap` and bitset | id index and one map (simpler) |

All implementations satisfy the same trait, so they can be swapped in and checked against each other.

### Tests

Unit tests (a shared behavioral suite plus per-implementation white-box tests) live in `src/store/mod.rs`, `src/store/btree.rs`, `src/store/bucket_map.rs` and `src/store/level_map.rs`. An integration test replaying a realistic order stream against every store is in `tests/stream.rs`.

```sh
cargo test
```

### Benches

To verify the theoretical time complexity of each operation, we use Criterion to run experiments.

See [BENCHMARKS.md](benches/BENCHMARKS.md) for details. Benchmark plots are in `plots/*.png`.

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
