pub mod btree;
pub mod bucket_map;
mod hier_bitset;
pub mod level_map;

pub use btree::BTreeOrderStore;
pub use bucket_map::BucketMapOrderStore;
pub use level_map::LevelMapOrderStore;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BookError;
    use crate::order_book::OrderBook;
    use crate::types::{Order, OrderId, Price, Quantity, Side};

    fn id(n: u64) -> OrderId {
        OrderId::new(n).unwrap()
    }

    fn order(n: u64, side: Side, price: i32, ts: u64) -> Order {
        Order {
            id: id(n),
            side,
            price: Price::from_ticks(price).unwrap(),
            quantity: Quantity::from_ticks(1).unwrap(),
            timestamp: ts,
        }
    }

    fn ids(v: Vec<&Order>) -> Vec<u64> {
        v.into_iter().map(|o| o.id.get()).collect()
    }

    // Runs the same behavioural suite against every store.
    macro_rules! book_tests {
        ($modname:ident, $store:ty) => {
            mod $modname {
                use super::*;

                fn book() -> $store {
                    <$store>::default()
                }

                #[test]
                fn empty_book_has_no_orders() {
                    let b = book();
                    assert!(b.bids().is_empty());
                    assert!(b.asks().is_empty());
                }

                #[test]
                fn insert_routes_by_side() {
                    let mut b = book();
                    b.insert(order(1, Side::Buy, 100, 0)).unwrap();
                    b.insert(order(2, Side::Sell, 101, 0)).unwrap();
                    assert_eq!(ids(b.bids()), [1]);
                    assert_eq!(ids(b.asks()), [2]);
                }

                #[test]
                fn duplicate_id_is_rejected_and_book_unchanged() {
                    let mut b = book();
                    b.insert(order(1, Side::Buy, 100, 0)).unwrap();
                    assert_eq!(
                        b.insert(order(1, Side::Sell, 5, 9)),
                        Err(BookError::DuplicateId(id(1)))
                    );
                    assert_eq!(ids(b.bids()), [1]);
                    assert!(b.asks().is_empty());
                }

                #[test]
                fn bids_sort_price_desc_then_time_then_id() {
                    let mut b = book();
                    b.insert(order(5, Side::Buy, 100, 2)).unwrap();
                    b.insert(order(4, Side::Buy, 101, 9)).unwrap();
                    b.insert(order(3, Side::Buy, 100, 1)).unwrap();
                    b.insert(order(2, Side::Buy, 100, 2)).unwrap();
                    b.insert(order(1, Side::Buy, -50, 0)).unwrap();
                    assert_eq!(ids(b.bids()), [4, 3, 2, 5, 1]);
                }

                #[test]
                fn asks_sort_price_asc_then_time_then_id() {
                    let mut b = book();
                    b.insert(order(5, Side::Sell, 100, 2)).unwrap();
                    b.insert(order(4, Side::Sell, 99, 9)).unwrap();
                    b.insert(order(3, Side::Sell, 100, 1)).unwrap();
                    b.insert(order(2, Side::Sell, 100, 2)).unwrap();
                    b.insert(order(1, Side::Sell, 5_000, 0)).unwrap();
                    assert_eq!(ids(b.asks()), [4, 3, 2, 5, 1]);
                }

                #[test]
                fn out_of_order_timestamps_are_placed_correctly() {
                    let mut b = book();
                    b.insert(order(1, Side::Buy, 100, 10)).unwrap();
                    b.insert(order(2, Side::Buy, 100, 30)).unwrap();
                    b.insert(order(3, Side::Buy, 100, 20)).unwrap();
                    b.insert(order(4, Side::Buy, 100, 5)).unwrap();
                    assert_eq!(ids(b.bids()), [4, 1, 3, 2]);
                }

                #[test]
                fn extreme_prices_are_supported() {
                    let mut b = book();
                    b.insert(order(1, Side::Buy, Price::MIN_TICKS, 0)).unwrap();
                    b.insert(order(2, Side::Buy, Price::MAX_TICKS, 0)).unwrap();
                    b.insert(order(3, Side::Sell, Price::MIN_TICKS, 0)).unwrap();
                    b.insert(order(4, Side::Sell, Price::MAX_TICKS, 0)).unwrap();
                    assert_eq!(ids(b.bids()), [2, 1]);
                    assert_eq!(ids(b.asks()), [3, 4]);
                }

                #[test]
                fn remove_returns_order_and_unknown_errors() {
                    let mut b = book();
                    let o = order(1, Side::Buy, 100, 0);
                    b.insert(o.clone()).unwrap();
                    assert_eq!(b.remove(id(1)), Ok(o));
                    assert!(b.bids().is_empty());
                    assert_eq!(b.remove(id(1)), Err(BookError::UnknownId(id(1))));
                }

                #[test]
                fn remove_head_middle_and_tail_of_a_level() {
                    let mut b = book();
                    for n in 1..=5 {
                        b.insert(order(n, Side::Sell, 100, n)).unwrap();
                    }
                    b.remove(id(3)).unwrap();
                    b.remove(id(1)).unwrap();
                    b.remove(id(5)).unwrap();
                    assert_eq!(ids(b.asks()), [2, 4]);
                }

                #[test]
                fn id_can_be_reused_after_remove() {
                    let mut b = book();
                    b.insert(order(1, Side::Buy, 100, 0)).unwrap();
                    b.remove(id(1)).unwrap();
                    b.insert(order(1, Side::Sell, 7, 3)).unwrap();
                    assert!(b.bids().is_empty());
                    assert_eq!(ids(b.asks()), [1]);
                }

                #[test]
                fn update_unknown_id_errors() {
                    let mut b = book();
                    assert_eq!(
                        b.update(order(9, Side::Buy, 1, 0)),
                        Err(BookError::UnknownId(id(9)))
                    );
                    assert!(b.bids().is_empty());
                }

                #[test]
                fn update_replaces_all_fields_and_repositions() {
                    let mut b = book();
                    b.insert(order(1, Side::Buy, 100, 0)).unwrap();
                    b.insert(order(2, Side::Buy, 100, 1)).unwrap();
                    // change price: 1 now beats 2
                    b.update(order(1, Side::Buy, 200, 5)).unwrap();
                    assert_eq!(ids(b.bids()), [1, 2]);
                    // change timestamp within the level: 2 is now behind 3
                    b.insert(order(3, Side::Buy, 100, 4)).unwrap();
                    b.update(order(2, Side::Buy, 100, 9)).unwrap();
                    assert_eq!(ids(b.bids()), [1, 3, 2]);
                    // change side
                    let mut moved = order(3, Side::Sell, 50, 0);
                    moved.quantity = Quantity::from_ticks(42).unwrap();
                    b.update(moved.clone()).unwrap();
                    assert_eq!(ids(b.bids()), [1, 2]);
                    assert_eq!(b.asks(), [&moved]);
                }


                #[test]
                fn emptied_level_is_forgotten() {
                    let mut b = book();
                    b.insert(order(1, Side::Buy, 100, 0)).unwrap();
                    b.remove(id(1)).unwrap();
                    b.insert(order(2, Side::Buy, 100, 0)).unwrap();
                    assert_eq!(ids(b.bids()), [2]);
                }
            }
        };
    }

    book_tests!(btree, BTreeOrderStore);
    book_tests!(bucket_map, BucketMapOrderStore);
    book_tests!(level_map, LevelMapOrderStore);

    #[test]
    fn trait_is_usable_generically() {
        fn fill<B: OrderBook>(b: &mut B) {
            b.insert(order(1, Side::Buy, 1, 0)).unwrap();
        }
        let mut a = BTreeOrderStore::new();
        let mut d = BucketMapOrderStore::new();
        let mut l = LevelMapOrderStore::new();
        fill(&mut a);
        fill(&mut d);
        fill(&mut l);
        assert_eq!(a.bids(), d.bids());
        assert_eq!(a.bids(), l.bids());
    }

    /// Random insert/update/remove stream; all stores must agree at each step.
    #[test]
    fn stores_agree_on_random_operations() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = move |m: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % m
        };
        let mut a = BTreeOrderStore::new();
        let mut d = BucketMapOrderStore::new();
        let mut l = LevelMapOrderStore::new();
        for _ in 0..5_000 {
            let n = rnd(40) + 1;
            let side = if rnd(2) == 0 { Side::Buy } else { Side::Sell };
            let o = order(n, side, rnd(7) as i32 - 3, rnd(10));
            match rnd(3) {
                0 => {
                    let r = a.insert(o.clone());
                    assert_eq!(r, d.insert(o.clone()));
                    assert_eq!(r, l.insert(o));
                }
                1 => {
                    let r = a.update(o.clone());
                    assert_eq!(r, d.update(o.clone()));
                    assert_eq!(r, l.update(o));
                }
                _ => {
                    let r = a.remove(id(n));
                    assert_eq!(r, d.remove(id(n)));
                    assert_eq!(r, l.remove(id(n)));
                }
            }
            assert_eq!(a.bids(), d.bids());
            assert_eq!(a.asks(), d.asks());
            assert_eq!(a.bids(), l.bids());
            assert_eq!(a.asks(), l.asks());
        }
    }
}
