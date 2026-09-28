use super::EvictionPolicy;

pub struct LfuPolicy {
    freq: Vec<u64>,
    occupied: Vec<bool>,
}

impl EvictionPolicy for LfuPolicy {
    fn new(capacity: usize) -> Self {
        LfuPolicy {
            freq: vec![0; capacity],
            occupied: vec![false; capacity],
        }
    }

    fn on_insert(&mut self, frame: usize) {
        self.occupied[frame] = true;
        self.freq[frame] = 1;
    }

    fn on_access(&mut self, frame: usize) {
        self.freq[frame] = self.freq[frame].saturating_add(1);
    }

    fn on_remove(&mut self, frame: usize) {
        self.occupied[frame] = false;
        self.freq[frame] = 0;
    }

    fn evict(&mut self, pinned: &[u32]) -> Option<usize> {
        let mut best: Option<(u64, usize)> = None;
        for (f, &occupied) in self.occupied.iter().enumerate() {
            if !occupied {
                continue;
            }
            if pinned.get(f).copied().unwrap_or(0) > 0 {
                continue;
            }
            let freq = self.freq[f];
            match best {
                None => best = Some((freq, f)),
                Some((bf, bi)) if freq < bf || (freq == bf && f < bi) => {
                    best = Some((freq, f));
                }
                _ => {}
            }
        }
        best.map(|(_, f)| f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfu_frequency() {
        let mut p = LfuPolicy::new(3);
        p.on_insert(0);
        p.on_insert(1);
        p.on_insert(2);
        p.on_access(0);
        p.on_access(0);
        p.on_access(1);
        // freq: 0=3, 1=2, 2=1
        let pins = [0, 0, 0];
        assert_eq!(p.evict(&pins), Some(2));
        p.on_remove(2);
        p.on_insert(2);
        // 2 reset to 1, still lowest (tie with none; 1 is 2, 0 is 3)
        assert_eq!(p.evict(&pins), Some(2));
    }

    #[test]
    fn lfu_tie_breaks_by_frame_id() {
        let mut p = LfuPolicy::new(2);
        p.on_insert(0);
        p.on_insert(1);
        let pins = [0, 0];
        assert_eq!(p.evict(&pins), Some(0));
    }
}
