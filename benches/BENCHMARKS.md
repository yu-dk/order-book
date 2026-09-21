# Benchmarks

## Setup

Each benchmark runs on a preloaded book of \(n \in \{1000, 4000, 16000, 64000\}\) orders, for `btree` and `bucket_map`, with three price distributions: `_uniform`, `_normal` and `_hot`.

`insert`, `update` and `remove` run on a book that stays at about \(n\) orders. `bids` and `asks` read a preloaded book of \(n\) orders:

| Operation | Group name | How it is timed |
|---|---|---|
| `insert` | `insert_<workload>` | Each batch times 16 inserts of fresh samples of the workload (new ids, so new prices), then cancels the 16 oldest orders, untimed. |
| `update` (quantity only) | `update_quantity_only_<workload>` | Each batch times 16 updates of random live orders that change only the quantity. A change of side, price or timestamp is a remove plus an insert, so it is covered by those benchmarks. |
| `remove` | `remove_<workload>` | Each batch times 16 cancels of the oldest orders, then inserts 16 fresh samples, untimed. |
| `bids`, `asks` | `bids_<workload>`, `asks_<workload>` | One full read of that side of the book. |

```sh
cargo bench
```

To run one store only, filter on the benchmark ID (`<group>/<store>/<n>`). Results of the other store in `target/criterion` are kept, so after changing one store you only need to rerun that one:

```sh
cargo bench --bench order_book -- /btree/
cargo bench --bench order_book -- /bucket_map/
```

The filter is a regex, so it can also pick one operation, workload or size, for example `-- "^insert_.*/bucket_map/"` or `-- "_hot/bucket_map/64000"`. `-- --list` shows what would run. 

Criterion writes HTML (including time vs n per operation and store) to:

`target/criterion/report/index.html`

Open that in a browser after the run.

To draw the plots:

```sh
python3 -m pip install -r scripts/requirements-plot.txt
python3 scripts/plot_benches.py
```

That reads `target/criterion/<op>_<workload>/<store>/<n>/new/estimates.json` and writes `plots/btree.png` and `plots/bucket_map.png`. 

## Time complexity

\(n\) is the number of orders in the book, \(m\) the number of orders at one price, and \(k\) the number of orders returned by `bids` / `asks` (up to \(n\)). Hash map lookups are \(O(1)\) (expected) and are included in every cost.

| Operation | `btree` | `bucket_map` |
|---|---|---|
| `insert` | \(O(\log n)\) | \(O(\log m)\) |
| `update` (side, price or timestamp changes) | \(O(\log n)\) | \(O(\log m)\) |
| `update` (only the quantity changes) | \(O(1)\) | \(O(\log m)\) |
| `remove` | \(O(\log n)\) | \(O(\log m)\) |
| `bids` / `asks` | \(O(k)\) | \(O(k)\) |

- `btree` keeps every price of a side in one sorted set, hence \(\log n\). Its quantity-only update only overwrites the hash map entry.
- `bucket_map` has a small sorted map per price, hence \(\log m\), and a bitset over prices with a fixed number of steps. A quantity-only update still searches that map.
- \(m\) is single digits in `_uniform` and `_normal`, so \(\log m\) is tiny there. In `_hot`, \(m = n/8\), so \(\log m\) grows with \(n\).
