//! Integration test: replay a stream of operations on every store and check
//! the resulting book. To watch the book change step by step, run:
//!
//!     cargo test --test stream -- --nocapture

use order_book::{
    BTreeMemoryOrderStore, BTreeOrderStore, BookError, BucketMapOrderStore, LevelMapOrderStore, Order, OrderBook, OrderId, Price, Quantity, Side,
};

// ---------------------------------------------------------------- the stream

/// Each step: a description, the operation, and the expected outcome.
///
/// A step's `expect` is `None` when its error case is already covered by a
/// dedicated test in `store/mod.rs`; it still runs and prints, just without
/// a redundant per-step assert.
fn steps() -> Vec<Step> {
    use Side::{Buy, Sell};
    vec![
        step("buy  10 @ 100.000", insert(1, Buy, "100", "10", 1), Some(Ok(()))),
        step("buy   5 @ 100.000 (later, so behind #1)", insert(2, Buy, "100", "5", 2), Some(Ok(()))),
        step("buy   7 @ 101.500 (new best bid)", insert(3, Buy, "101.5", "7", 3), Some(Ok(()))),
        step("sell  4 @ 102.000", insert(4, Sell, "102", "4", 4), Some(Ok(()))),
        step("sell  6 @ 101.999 (new best ask)", insert(5, Sell, "101.999", "6", 5), Some(Ok(()))),
        step("insert with existing id 3", insert(3, Sell, "1", "1", 6), None),
        step("amend #2 to 100.250 (now ahead of #1)", update(2, Buy, "100.25", "5", 7), Some(Ok(()))),
        step("amend #4 into a buy @ 99.000", update(4, Buy, "99", "4", 8), Some(Ok(()))),
        step("cancel #3 (the best bid)", Op::Remove(id(3)), Some(Ok(()))),
        step("cancel unknown id 99", Op::Remove(id(99)), None),
        step("amend unknown id 98", update(98, Buy, "1", "1", 9), None),
    ]
}

/// Ids expected in each book once every step has run.
const FINAL_BIDS: [u64; 3] = [2, 1, 4];
const FINAL_ASKS: [u64; 1] = [5];

// --------------------------------------------------------------------- tests

#[test]
fn replay_on_btree_store() {
    let book = replay::<BTreeOrderStore>("BTreeOrderStore");
    assert_eq!((ids(book.bids()), ids(book.asks())), (FINAL_BIDS.to_vec(), FINAL_ASKS.to_vec()));
}

#[test]
fn replay_on_btree_memory_store() {
    let book = replay::<BTreeMemoryOrderStore>("BTreeMemoryOrderStore");
    assert_eq!((ids(book.bids()), ids(book.asks())), (FINAL_BIDS.to_vec(), FINAL_ASKS.to_vec()));
}

#[test]
fn replay_on_bucket_map_store() {
    let book = replay::<BucketMapOrderStore>("BucketMapOrderStore");
    assert_eq!((ids(book.bids()), ids(book.asks())), (FINAL_BIDS.to_vec(), FINAL_ASKS.to_vec()));
}

#[test]
fn replay_on_level_map_store() {
    let book = replay::<LevelMapOrderStore>("LevelMapOrderStore");
    assert_eq!((ids(book.bids()), ids(book.asks())), (FINAL_BIDS.to_vec(), FINAL_ASKS.to_vec()));
}

#[test]
fn duplicate_error_message_names_the_id() {
    assert_eq!(dup(1).to_string(), "order 1 already exists");
    assert_eq!(unknown(2).to_string(), "order 2 not found");
}

// ------------------------------------------------------------------- replay

/// Applies every step, checks its result, and prints the book afterwards.
fn replay<B: OrderBook + Default>(name: &str) -> B {
    println!("\n=== {name}");
    let mut book = B::default();
    for Step { label, op, expect } in steps() {
        let result = match op {
            Op::Insert(order) => book.insert(order),
            Op::Update(order) => book.update(order),
            Op::Remove(id) => book.remove(id).map(|_| ()),
        };
        if let Some(expect) = expect {
            assert_eq!(result, expect, "step: {label}");
        }

        println!("\n{label}  ->  {}", if result.is_ok() { "ok".into() } else { format!("{:?}", result.unwrap_err()) });
        print_side("bids", book.bids());
        print_side("asks", book.asks());
    }
    book
}

fn print_side(name: &str, orders: Vec<&Order>) {
    println!("  {name}:");
    if orders.is_empty() {
        println!("    (empty)");
    }
    for o in orders {
        println!("    #{}  {} @ {}  (t={})", o.id, o.quantity, o.price, o.timestamp);
    }
}

fn ids(orders: Vec<&Order>) -> Vec<u64> {
    orders.iter().map(|o| o.id.get()).collect()
}

// ------------------------------------------------------- building the steps

enum Op {
    Insert(Order),
    Update(Order),
    Remove(OrderId),
}

struct Step {
    label: &'static str,
    op: Op,
    /// `None` when the error case is already covered by a `store/mod.rs` test.
    expect: Option<Result<(), BookError>>,
}

fn step(label: &'static str, op: Op, expect: Option<Result<(), BookError>>) -> Step {
    Step { label, op, expect }
}

fn insert(id: u64, side: Side, price: &str, qty: &str, ts: u64) -> Op {
    Op::Insert(order(id, side, price, qty, ts))
}

fn update(id: u64, side: Side, price: &str, qty: &str, ts: u64) -> Op {
    Op::Update(order(id, side, price, qty, ts))
}

fn id(n: u64) -> OrderId {
    OrderId::new(n).unwrap()
}

fn dup(n: u64) -> BookError {
    BookError::DuplicateId(id(n))
}

fn unknown(n: u64) -> BookError {
    BookError::UnknownId(id(n))
}

/// Builds an order from human-readable decimals, e.g. price "101.5", qty "7".
fn order(n: u64, side: Side, price: &str, qty: &str, timestamp: u64) -> Order {
    Order {
        id: id(n),
        side,
        price: Price::from_ticks(to_ticks(price, 1_000) as i32).unwrap(),
        quantity: Quantity::from_ticks(to_ticks(qty, 1_000_000) as u64).unwrap(),
        timestamp,
    }
}

/// "101.5" with denominator 1000 -> 101_500. Exact decimal parsing, no floats.
fn to_ticks(text: &str, denominator: i64) -> i64 {
    let digits = denominator.ilog10() as usize;
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    let frac = format!("{frac:0<digits$}");
    whole.parse::<i64>().unwrap() * denominator + frac[..digits].parse::<i64>().unwrap()
}
