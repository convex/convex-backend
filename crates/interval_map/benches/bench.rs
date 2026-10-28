use std::hint;

use common::interval::Interval;
use interval_map::IntervalMap;
use tikv_jemallocator::Jemalloc;

#[global_allocator]
static ALLOC: Jemalloc = Jemalloc;

fn main() {
    divan::main();
}

#[divan::bench(args = [10, 100, 1000, 10000, 100000])]
fn equal_keys(n: usize) -> IntervalMap {
    let mut map = IntervalMap::new();
    map.insert(
        1,
        std::iter::repeat_n(Interval::prefix(vec![1; 64].into()), n),
    )
    .unwrap();
    map
}

#[divan::bench(args = [10, 100, 1000, 10000, 100000])]
fn integers(n: u32) -> IntervalMap {
    let mut map = IntervalMap::new();
    map.insert(
        1,
        (0..n).map(|i| Interval::prefix(i.to_le_bytes().to_vec().into())),
    )
    .unwrap();
    for i in (0..n * 5).step_by(5) {
        map.query(&i.to_le_bytes(), |id| {
            hint::black_box(id);
        });
    }
    map
}
