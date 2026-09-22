# Benchmarks

## Setup

Each benchmark runs on a preloaded book of \(n \in \{1000, 4000, 16000, 64000, 256000, 1024000, 2000000\}\) orders(warm cache), for `btree`, `bucket_map` and `level_map`, with three price distributions:

- `_uniform` — orders spread uniformly at random over 6,000 prices.
- `_normal` — prices are normal (mean 100, sigma 1,000 ticks), so the book is dense near the mean and thin in the tails. 3σ (99.7%) covers about 6,000 distinct ticks, close to real usage.
- `_hot` — every order lands on one of 16 prices per side, so each queue holds \(n/32\) orders. Jitter causes some orders to arrive with a timestamp earlier than the current tail of their queue.

`insert`, `update` and `remove` run on a book that stays at about \(n\) orders. `bids` and `asks` read a preloaded book of \(n\) orders. `BATCH_SIZE` is defined in `benches/order_book.rs`:

| Operation | Group name | How it is timed |
|---|---|---|
| `insert` | `insert_<workload>` | Each batch times `BATCH_SIZE` inserts of fresh samples of the workload (new ids, so new prices), then cancels the `BATCH_SIZE` oldest orders |
| `update` (quantity only) | `update_quantity_only_<workload>` | Each batch times `BATCH_SIZE` updates of random live orders that change only the quantity. A change of side, price or timestamp is a remove plus an insert, which is covered by other benchmarks. |
| `remove` | `remove_<workload>` | Each batch times `BATCH_SIZE` cancels of the oldest orders, then inserts `BATCH_SIZE` fresh samples. |
| `bids`, `asks` | `bids_<workload>`, `asks_<workload>` | One full read of that side of the book. |

## Command and output
```sh
cargo bench
cargo bench --bench order_book -- /btree/
cargo bench --bench order_book -- /bucket_map/
cargo bench --bench order_book -- /level_map/
```

The filter is a regex, so it can also pick one operation, workload or size, for example `-- "^insert_.*/bucket_map/"` or `-- "_hot/bucket_map/64000"`. `-- --list` shows what would run. 

Criterion writes HTML (including time vs n per operation and store) to: `target/criterion/report/index.html`

To draw the plots:

```sh
python3 -m pip install -r scripts/requirements-plot.txt
python3 scripts/plot_benches.py
```
That reads `target/criterion/<op>_<workload>/<store>/<n>/new/estimates.json` and writes `plots/*.png`.

## Time complexity

\(n\) is the number of orders in the book, \(m\) the number of orders at one price, \(L\) the number of prices that have orders. Hash map lookups are \(O(1)\) (expected) and are included in every cost.

| Operation | `btree` | `bucket_map` | `level_map` |
|---|---|---|---|
| `insert` | \(O(\log n)\) | \(O(\log m)\) | \(O(\log L + \log m)\) |
| `update` (side, price or timestamp changes) | \(O(\log n)\) | \(O(\log m)\) | \(O(\log L + \log m)\) |
| `update` (only the quantity changes) | \(O(1)\) | \(O(\log m)\) | \(O(\log L + \log m)\) |
| `remove` | \(O(\log n)\) | \(O(\log m)\) | \(O(\log L + \log m)\) |
| `bids` / `asks` | \(O(n)\) | \(O(n)\) | \(O(n)\) |

- In `_hot`, \(m = n/32\) exactly, so \(\log m\) tracks \(\log n\) closely — that workload exists specifically to make `bucket_map`'s \(\log m\) curve visible (see `plots/bucket_map.png`).

### Why `btree`'s `O(1)` update isn't flat at large \(n\)

`update_quantity_only`'s fast path is one `HashMap` lookup and in-place write: genuinely \(O(1)\) in operation count. But each call picks a uniformly random id, so there's no cache locality — the *wall-clock* cost depends on whether that lookup hits cache. `HashMap<OrderId, Order>` is roughly 45 bytes/order — ~12 MB at \(n=256{,}000\), close to this machine's 16 MB L2 (Apple A18 Pro; no separate L3).