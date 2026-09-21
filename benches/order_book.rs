use std::time::{Duration, Instant};

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkGroup, BenchmarkId, Criterion,
    measurement::WallTime,
};
use order_book::{BTreeOrderStore, BucketMapOrderStore, Order, OrderBook, Price, Quantity, Side};
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_distr::{Distribution, Normal};
use std::num::NonZeroU64;

/// Geometric sizes so a log–log plot can show the slope.
const SIZES: &[u64] = &[1_000, 4_000, 16_000, 64_000];
/// Operations timed back to back per batch. Small next to `SIZES`, so the book size stays close to `order_size`.
const BATCH_SIZE: u64 = 16;

/// Orders spread uniformly at random over 10,000 prices: buys on even `i`,
/// sells on odd `i`, price a hash of `i`, ranges from -5,000 to 4,999
fn sample_order_uniform(i: u64) -> Order {
    Order {
        id: NonZeroU64::new(i).unwrap(),
        side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
        price: Price::from_ticks(((i.wrapping_mul(2_654_435_761) >> 7) % 10_000) as i32 - 5_000).unwrap(),
        quantity: Quantity::from_ticks(1).unwrap(),
        timestamp: i as u64,
    }
}

/// Prices are normal (mean 100, sigma 2000 ticks), so the book is dense near
/// the mean and thin in the tails. Otherwise like `sample_order_uniform`.
///
/// Orders per price at the mean, m = n/10_000: 0.1, 0.4, 1.6, 6.4 for
/// n = 1k, 4k, 16k, 64k. It falls to x0.61 one sigma out and x0.14 two sigma out.
const NORMAL_MEAN: f64 = 100.0;
const NORMAL_SIGMA: f64 = 2_000.0;
fn sample_order_normal(i: u64) -> Order {
    let normal = Normal::new(NORMAL_MEAN, NORMAL_SIGMA).unwrap();
    let ticks = (normal.sample(&mut StdRng::seed_from_u64(i)).round() as i32).clamp(Price::MIN_TICKS, Price::MAX_TICKS);
    Order {
        id: NonZeroU64::new(i).unwrap(),
        side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
        price: Price::from_ticks(ticks).unwrap(),
        quantity: Quantity::from_ticks(1).unwrap(),
        timestamp: i,
    }
}

/// Hot prices: same shape as `sample_order_uniform`, but every order lands on
/// one of 4 prices per side, so each queue holds n/8 orders. Jitter cause orders arrive 
/// with a timestamp earlier than the current tail of their queue.
fn sample_order_hot(i: u64) -> Order {
    let jitter = i.wrapping_mul(2_654_435_761) >> 7 & 63;
    Order {
        id: NonZeroU64::new(i).unwrap(),
        side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
        price: Price::from_ticks((i % 8) as i32 - 4).unwrap(),
        quantity: Quantity::from_ticks(1).unwrap(),
        timestamp: i + 64 - jitter,
    }
}

/// An order generator, and the suffix that names its benchmark groups.
struct Workload {
    suffix: &'static str,
    order: fn(u64) -> Order,
}

const WORKLOADS: &[Workload] = &[
    Workload { suffix: "_uniform", order: sample_order_uniform },
    Workload { suffix: "_normal", order: sample_order_normal },
    Workload { suffix: "_hot", order: sample_order_hot },
];

/// Book of `order_size` orders (ids `1..=order_size`). Built with a little spare capacity (fill
/// then cancel a few extra ids) so a cloned template does not have to grow its
/// slab or hash tables on the timed insert.
fn preload<B: OrderBook + Default>(order_size: u64, order: fn(u64) -> Order) -> B {
    const HEADROOM: u64 = 64;
    let mut book = B::default();
    for i in 1..=order_size + HEADROOM {
        book.insert(order(i)).unwrap();
    }
    for i in order_size + 1..=order_size + HEADROOM {
        book.remove(NonZeroU64::new(i).unwrap()).unwrap();
    }
    book
}

fn group<'a>(c: &'a mut Criterion, name: &str) -> BenchmarkGroup<'a, WallTime> {
    let mut group = c.benchmark_group(name);
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(400));
    group.measurement_time(Duration::from_secs(1));
    group
}


/// Insert: each batch times `BATCH_SIZE` inserts of fresh samples (new ids, so new prices), then cancels the oldest `BATCH_SIZE` orders, untimed.
fn bench_insert<B: OrderBook + Default>(c: &mut Criterion, store: &str, w: &Workload) {
    let mut group = group(c, &format!("insert{}", w.suffix));
    for &order_size in SIZES {
        let mut book: B = preload(order_size, w.order);
        // Ids 1..=order_size are live; the next new id is order_size + 1.
        let (mut oldest, mut next) = (1, order_size + 1);
        group.bench_with_input(BenchmarkId::new(store, order_size), &order_size, |b, _| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                let mut done = 0;
                while done < iters {
                    let k = BATCH_SIZE.min(iters - done);
                    let orders: Vec<Order> = (next..next + k).map(w.order).collect();
                    let start = Instant::now();
                    for order in orders {
                        book.insert(black_box(order)).unwrap();
                    }
                    total += start.elapsed();
                    for id in oldest..oldest + k {
                        book.remove(NonZeroU64::new(id).unwrap()).unwrap();
                    }
                    next += k;
                    oldest += k;
                    done += k;
                }
                total
            })
        });
        assert_eq!(book.bids().len() + book.asks().len(), order_size as usize, "batches must keep the size");
    }
    group.finish();
}

/// Update, quantity only: each batch times `BATCH_SIZE` updates of random live orders that change only the quantity.
/// A price or timestamp change is a remove plus an insert, which are benchmarked on their own.
fn bench_update_quantity_only<B: OrderBook + Default>(c: &mut Criterion, store: &str, w: &Workload) {
    let mut group = group(c, &format!("update_quantity_only{}", w.suffix));
    for &order_size in SIZES {
        let mut book: B = preload(order_size, w.order);
        let mut rng = StdRng::seed_from_u64(order_size);
        let mut quantity = 2; // differs from the preloaded quantity of 1
        group.bench_with_input(BenchmarkId::new(store, order_size), &order_size, |b, _| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                let mut done = 0;
                while done < iters {
                    let k = BATCH_SIZE.min(iters - done);
                    let orders: Vec<Order> = (0..k)
                        .map(|_| {
                            quantity += 1;
                            let id = rng.gen_range(1..=order_size);
                            Order { quantity: Quantity::from_ticks(quantity).unwrap(), ..(w.order)(id) }
                        })
                        .collect();
                    let start = Instant::now();
                    for order in orders {
                        book.update(black_box(order)).unwrap();
                    }
                    total += start.elapsed();
                    done += k;
                }
                total
            })
        });
        assert_eq!(book.bids().len() + book.asks().len(), order_size as usize, "updates must keep the size");
    }
    group.finish();
}

/// Remove: each batch times `BATCH_SIZE` cancels of the oldest orders, then inserts fresh samples, untimed.
fn bench_remove<B: OrderBook + Default>(c: &mut Criterion, store: &str, w: &Workload) {
    let mut group = group(c, &format!("remove{}", w.suffix));
    for &order_size in SIZES {
        let mut book: B = preload(order_size, w.order);
        // Ids 1..=order_size are live; the next new id is order_size + 1.
        let (mut oldest, mut next) = (1, order_size + 1);
        group.bench_with_input(BenchmarkId::new(store, order_size), &order_size, |b, _| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                let mut done = 0;
                while done < iters {
                    let k = BATCH_SIZE.min(iters - done);
                    let start = Instant::now();
                    for id in oldest..oldest + k {
                        black_box(book.remove(NonZeroU64::new(id).unwrap()).unwrap());
                    }
                    total += start.elapsed();
                    for i in next..next + k {
                        book.insert((w.order)(i)).unwrap();
                    }
                    next += k;
                    oldest += k;
                    done += k;
                }
                total
            })
        });
        assert_eq!(book.bids().len() + book.asks().len(), order_size as usize, "batches must keep the size");
    }
    group.finish();
}

/// Full read of the buy side of a preloaded book.
fn bench_bids<B: OrderBook + Default>(c: &mut Criterion, store: &str, w: &Workload) {
    let mut group = group(c, &format!("bids{}", w.suffix));
    for &order_size in SIZES {
        let book: B = preload(order_size, w.order);
        group.bench_with_input(BenchmarkId::new(store, order_size), &order_size, |b, _| b.iter(|| black_box(book.bids())));
    }
    group.finish();
}

/// Same for the sell side.
fn bench_asks<B: OrderBook + Default>(c: &mut Criterion, store: &str, w: &Workload) {
    let mut group = group(c, &format!("asks{}", w.suffix));
    for &order_size in SIZES {
        let book: B = preload(order_size, w.order);
        group.bench_with_input(BenchmarkId::new(store, order_size), &order_size, |b, _| b.iter(|| black_box(book.asks())));
    }
    group.finish();
}

fn bench_store<B: OrderBook + Default>(c: &mut Criterion, store: &str, w: &Workload) {
    bench_insert::<B>(c, store, w);
    bench_update_quantity_only::<B>(c, store, w);
    bench_remove::<B>(c, store, w);
    bench_bids::<B>(c, store, w);
    bench_asks::<B>(c, store, w);
}

fn bench_all(c: &mut Criterion) {
    for w in WORKLOADS {
        bench_store::<BTreeOrderStore>(c, "btree", w);
        bench_store::<BucketMapOrderStore>(c, "bucket_map", w);
    }
}

criterion_group!(benches, bench_all);
criterion_main!(benches);
