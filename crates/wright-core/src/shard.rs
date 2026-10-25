//! `--shard i/n`: run one contiguous slice of a fixed, ordered list of jobs
//! (bench scenarios, sweep cases), so N headless containers each run one
//! slice and a merge step puts the results back together (docs/aci.md).
//!
//! Precedent: Playwright's `--shard=current/total`
//! (`packages/playwright/src/runner/testGroups.ts::filterForShard`). The
//! index is 1-based; with equal weights each shard gets
//! `floor(total_jobs / shards)` jobs, the remainder goes one each to the
//! first shards, and shard `i` takes `[from, to)` where `from` is the sum
//! of the sizes before it. Every job lands in exactly one shard, a shard
//! past the job count is empty, and the slices concatenated in shard order
//! are the original list. Deviation, stated: no per-group weights (every
//! scenario is one job).

use std::ops::Range;
use std::str::FromStr;

/// One slice of `total` (1-based `current`, as Playwright and Jest write it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shard {
    pub current: usize,
    pub total: usize,
}

impl Shard {
    /// The whole list as one shard.
    pub const ALL: Shard = Shard { current: 1, total: 1 };

    /// The index range of this shard in a list of `len` jobs.
    pub fn range(&self, len: usize) -> Range<usize> {
        let base = len / self.total;
        let remainder = len % self.total;
        let size = |i: usize| base + usize::from(i < remainder);
        let from: usize = (0..self.current - 1).map(size).sum();
        from..from + size(self.current - 1)
    }

    /// This shard's jobs, in their original order.
    pub fn select<T>(&self, items: Vec<T>) -> Vec<T> {
        let r = self.range(items.len());
        items.into_iter().skip(r.start).take(r.len()).collect()
    }

    /// `shard-2-of-4`: a file-name tag.
    pub fn tag(&self) -> String {
        format!("shard-{}-of-{}", self.current, self.total)
    }
}

impl FromStr for Shard {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (a, b) = s.split_once('/').ok_or_else(|| anyhow::anyhow!("--shard wants i/n (e.g. 2/4), got `{s}`"))?;
        let current: usize = a.trim().parse().map_err(|_| anyhow::anyhow!("--shard `{s}`: `{a}` is not a number"))?;
        let total: usize = b.trim().parse().map_err(|_| anyhow::anyhow!("--shard `{s}`: `{b}` is not a number"))?;
        anyhow::ensure!(total >= 1, "--shard `{s}`: n must be at least 1");
        anyhow::ensure!((1..=total).contains(&current), "--shard `{s}`: i must be between 1 and {total} (1-based, as Playwright's --shard)");
        Ok(Shard { current, total })
    }
}

impl std::fmt::Display for Shard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.current, self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rejects() {
        assert_eq!("2/4".parse::<Shard>().unwrap(), Shard { current: 2, total: 4 });
        for bad in ["0/4", "5/4", "1/0", "2", "a/3", "1/b", ""] {
            assert!(bad.parse::<Shard>().is_err(), "{bad} should not parse");
        }
        assert_eq!(Shard { current: 3, total: 7 }.to_string(), "3/7");
        assert_eq!(Shard { current: 3, total: 7 }.tag(), "shard-3-of-7");
    }

    /// Every job in exactly one shard, in order, sizes differing by at most
    /// one, the larger ones first (Playwright's remainder rule).
    #[test]
    fn partitions_every_list() {
        for len in 0..30 {
            for total in 1..12 {
                let items: Vec<usize> = (0..len).collect();
                let mut joined = vec![];
                let mut sizes = vec![];
                for current in 1..=total {
                    let s = Shard { current, total }.select(items.clone());
                    sizes.push(s.len());
                    joined.extend(s);
                }
                assert_eq!(joined, items, "len {len} total {total}");
                assert!(sizes.windows(2).all(|w| w[0] >= w[1] && w[0] - w[1] <= 1), "{sizes:?}");
            }
        }
    }

    #[test]
    fn eleven_scenarios_in_four() {
        let r: Vec<_> = (1..=4).map(|c| Shard { current: c, total: 4 }.range(11)).collect();
        assert_eq!(r, vec![0..3, 3..6, 6..9, 9..11]);
        assert_eq!(Shard::ALL.range(11), 0..11);
        assert_eq!(Shard { current: 5, total: 5 }.range(3), 3..3);
    }
}
