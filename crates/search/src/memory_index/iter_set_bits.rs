pub fn iter_set_bits(mut bitset: u64) -> impl Iterator<Item = usize> {
    std::iter::from_coroutine(
        #[coroutine]
        move || {
            while bitset != 0 {
                let t = bitset.isolate_lowest_one();
                yield bitset.trailing_zeros() as usize;
                bitset ^= t;
            }
        },
    )
}
