/// Hierarchical bitset used by the bucket_map store to find the next non-empty price.
/// 64-ary tree of bitmaps over `0..n`; `next` finds the smallest set bit >= i. 
/// With about 20 million bits (the price range), it has 5 levels.
/// `levels[0]` has one bit per index. Each higher level has one bit per word
/// of the level below, set when that word is non-zero. Searching climbs up
/// until it finds a set bit, then descends, so it takes at most one word
/// lookup per level
/// The price range is about 20 million ticks, so one side's bitset is about 20M / 8 = 2.5 MB (5 MB for both sides).

#[derive(Debug, Clone)]
pub(super) struct HierBitset {
    levels: Vec<Vec<u64>>,
}

impl HierBitset {
    pub(super) fn new(n: usize) -> Self {
        let mut levels = Vec::new();
        let mut words = n.div_ceil(64).max(1);
        loop {
            levels.push(vec![0u64; words]);
            if words == 1 {
                break;
            }
            words = words.div_ceil(64);
        }
        Self { levels }
    }

    /// Sets bit `i` and marks its word as non-zero in every level above.
    pub(super) fn set(&mut self, mut i: usize) {
        for level in &mut self.levels {
            level[i >> 6] |= 1u64 << (i & 63);
            i >>= 6;
        }
    }

    /// Clears bit `i`; a level's word only clears the level above if it became zero.
    pub(super) fn clear(&mut self, mut i: usize) {
        for level in &mut self.levels {
            level[i >> 6] &= !(1u64 << (i & 63));
            if level[i >> 6] != 0 {
                // if it still contains other set bits, stop clearing bits in the higher levels
                break;
            }
            i >>= 6;
        }
    }

    /// Smallest set bit at or after `from`.
    pub(super) fn next(&self, from: usize) -> Option<usize> {
        self.next_at(0, from)
    }

    fn next_at(&self, lvl: usize, from: usize) -> Option<usize> {
        let word_idx = from >> 6;
        let words = self.levels.get(lvl)?;
        let word = *words.get(word_idx)?;

        // A set bit in this word at or after `from`?
        let bits = word & (!0u64 << (from & 63));
        if bits != 0 {
            return Some(word_idx << 6 | bits.trailing_zeros() as usize);
        }
        // Otherwise ask the level above for the next non-empty word, then
        // take the first set bit inside it.
        let next_word = self.next_at(lvl + 1, word_idx + 1)?;
        Some(next_word << 6 | words[next_word].trailing_zeros() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same size as the bucket_map store's price range.
    const SPAN: usize = 19_999_999;

    #[test]
    fn bitset_next_finds_smallest_set_bit_at_or_after() {
        let mut b = HierBitset::new(SPAN);
        assert_eq!(b.next(0), None);
        for i in [0, 63, 64, 4_095, 4_096, 262_144, SPAN - 1] {
            b.set(i);
        }
        assert_eq!(b.next(0), Some(0));
        assert_eq!(b.next(1), Some(63));
        assert_eq!(b.next(64), Some(64));
        assert_eq!(b.next(65), Some(4_095));
        assert_eq!(b.next(4_097), Some(262_144));
        assert_eq!(b.next(262_145), Some(SPAN - 1));
        assert_eq!(b.next(SPAN), None);
    }

    #[test]
    fn bitset_clear_removes_bits_and_empty_summaries() {
        let mut b = HierBitset::new(SPAN);
        b.set(10);
        b.set(5_000_000);
        b.clear(10);
        assert_eq!(b.next(0), Some(5_000_000));
        b.clear(5_000_000);
        assert_eq!(b.next(0), None);
        b.clear(7); // clearing an unset bit is harmless
    }

    #[test]
    fn bitset_tiny_sizes() {
        let mut b = HierBitset::new(1);
        b.set(0);
        assert_eq!(b.next(0), Some(0));
        assert_eq!(b.next(1), None);
    }

    /// True if bit `bit` of word `word` at tree level `lvl` is set.
    fn bit_at(b: &HierBitset, lvl: usize, word: usize, bit: usize) -> bool {
        b.levels[lvl][word] >> bit & 1 == 1
    }

    #[test]
    fn bitset_new_has_five_levels_for_the_price_span() {
        let b = HierBitset::new(SPAN);
        let words: Vec<usize> = b.levels.iter().map(Vec::len).collect();
        assert_eq!(words, [312_500, 4_883, 77, 2, 1]);
        assert!(b.levels.iter().all(|l| l.iter().all(|&w| w == 0)));
    }

    #[test]
    fn bitset_set_marks_every_level_above() {
        // Bid 100 from the module docs: bit 9_999_999 - 100.
        let mut b = HierBitset::new(SPAN);
        b.set(9_999_899);
        assert!(bit_at(&b, 0, 156_248, 27));
        assert!(bit_at(&b, 1, 2_441, 24));
        assert!(bit_at(&b, 2, 38, 9));
        assert!(bit_at(&b, 3, 0, 38));
        assert!(bit_at(&b, 4, 0, 0));
        let set_bits: u32 = b.levels.iter().flatten().map(|w| w.count_ones()).sum();
        assert_eq!(set_bits, 5);
    }

    #[test]
    fn bitset_set_is_idempotent() {
        let mut b = HierBitset::new(1_000);
        b.set(700);
        let snapshot = b.levels.clone();
        b.set(700);
        assert_eq!(b.levels, snapshot);
        assert_eq!(b.next(0), Some(700));
    }

    #[test]
    fn bitset_set_in_same_word_shares_summary_bit() {
        let mut b = HierBitset::new(SPAN);
        b.set(64);
        b.set(65);
        assert_eq!(b.levels[0][1], 0b11);
        assert_eq!(b.levels[1][0], 0b10); // one summary bit for word 1
        assert_eq!((b.next(0), b.next(65), b.next(66)), (Some(64), Some(65), None));
    }

    #[test]
    fn bitset_clear_only_bit_in_word_clears_all_summary_levels() {
        let mut b = HierBitset::new(SPAN);
        b.set(9_999_899);
        b.clear(9_999_899);
        assert!(b.levels.iter().all(|l| l.iter().all(|&w| w == 0)));
        assert_eq!(b.next(0), None);
    }

    #[test]
    fn bitset_clear_stops_when_word_still_has_bits() {
        // Bids 100 and 101 share level 0 word 156_248 (bits 27 and 26).
        let mut b = HierBitset::new(SPAN);
        b.set(9_999_899);
        b.set(9_999_898);
        b.clear(9_999_899);
        assert!(!bit_at(&b, 0, 156_248, 27));
        assert!(bit_at(&b, 0, 156_248, 26));
        assert!(bit_at(&b, 1, 2_441, 24), "summary must stay set");
        assert!(bit_at(&b, 4, 0, 0), "top level must stay set");
        assert_eq!(b.next(0), Some(9_999_898));
        assert_eq!(b.next(9_999_899), None);
    }

    #[test]
    fn bitset_clear_in_separate_words_keeps_the_other_branch() {
        let mut b = HierBitset::new(SPAN);
        b.set(0);
        b.set(64); // word 1: summary bit 1 of the same level 1 word
        b.clear(0);
        assert!(!bit_at(&b, 1, 0, 0), "word 0 is empty now");
        assert!(bit_at(&b, 1, 0, 1));
        assert_eq!(b.next(0), Some(64));
    }

    #[test]
    fn bitset_clear_unset_bit_changes_nothing() {
        let mut b = HierBitset::new(SPAN);
        b.set(100);
        let snapshot = b.levels.clone();
        b.clear(101); // same word, not set
        b.clear(5_000_000); // empty word elsewhere
        assert_eq!(b.levels, snapshot);
    }

    #[test]
    fn bitset_can_set_again_after_clear() {
        let mut b = HierBitset::new(SPAN);
        b.set(12_345);
        b.clear(12_345);
        b.set(12_345);
        assert_eq!(b.next(0), Some(12_345));
    }

    #[test]
    fn bitset_next_at_set_bit_returns_it_and_just_past_skips_it() {
        let mut b = HierBitset::new(SPAN);
        b.set(200);
        assert_eq!(b.next(200), Some(200));
        assert_eq!(b.next(201), None);
        assert_eq!(b.next(199), Some(200));
    }

    #[test]
    fn bitset_next_at_word_boundaries() {
        let mut b = HierBitset::new(SPAN);
        b.set(63);
        b.set(64);
        assert_eq!(b.next(63), Some(63));
        assert_eq!(b.next(64), Some(64));
        b.clear(63);
        assert_eq!(b.next(0), Some(64), "skips the empty word 0");
        assert_eq!(b.next(65), None);
    }

    #[test]
    fn bitset_next_skips_many_empty_words() {
        let mut b = HierBitset::new(SPAN);
        b.set(3);
        b.set(19_000_000);
        assert_eq!(b.next(4), Some(19_000_000));
        assert_eq!(b.next(19_000_001), None);
    }

    #[test]
    fn bitset_next_from_past_the_end_is_none() {
        let mut b = HierBitset::new(1_000);
        b.set(999);
        assert_eq!(b.next(999), Some(999));
        assert_eq!(b.next(1_000), None);
        assert_eq!(b.next(usize::MAX >> 7), None);
    }

    #[test]
    fn bitset_matches_btreeset_under_random_ops() {
        use std::collections::BTreeSet;
        const N: usize = 20_000; // 3 levels: 313, 5 and 1 words
        let mut b = HierBitset::new(N);
        let mut model = BTreeSet::new();
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut rand = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let i = (rand() % N as u64) as usize;
            if rand() % 3 == 0 {
                b.clear(i);
                model.remove(&i);
            } else {
                b.set(i);
                model.insert(i);
            }
            let from = (rand() % (N as u64 + 10)) as usize;
            assert_eq!(b.next(from), model.range(from..).next().copied(), "from {from}");
        }
    }
}
