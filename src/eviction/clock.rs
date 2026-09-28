use super::EvictionPolicy;

pub struct ClockPolicy {
    ref_bit: Vec<bool>,
    occupied: Vec<bool>,
    hand: usize,
}

impl EvictionPolicy for ClockPolicy {
    fn new(capacity: usize) -> Self {
        ClockPolicy {
            ref_bit: vec![false; capacity],
            occupied: vec![false; capacity],
            hand: 0,
        }
    }

    fn on_insert(&mut self, frame: usize) {
        self.occupied[frame] = true;
        self.ref_bit[frame] = true;
    }

    fn on_access(&mut self, frame: usize) {
        self.ref_bit[frame] = true;
    }

    fn on_remove(&mut self, frame: usize) {
        self.occupied[frame] = false;
        self.ref_bit[frame] = false;
    }

    fn evict(&mut self, pinned: &[u32]) -> Option<usize> {
        let n = self.ref_bit.len();
        if n == 0 {
            return None;
        }
        // Two passes is enough: first clears ref bits, second finds a victim.
        for _ in 0..(n * 2) {
            let f = self.hand;
            self.hand = (self.hand + 1) % n;
            if !self.occupied[f] {
                continue;
            }
            if pinned.get(f).copied().unwrap_or(0) > 0 {
                continue;
            }
            if self.ref_bit[f] {
                self.ref_bit[f] = false;
                continue;
            }
            return Some(f);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_second_chance() {
        let mut p = ClockPolicy::new(2);
        p.on_insert(0);
        p.on_insert(1);
        p.ref_bit[0] = true;
        p.ref_bit[1] = false;
        p.hand = 0;
        let pins = [0, 0];
        // Frame 0 has a second chance (ref=1); frame 1 is evicted.
        assert_eq!(p.evict(&pins), Some(1));
        assert!(!p.ref_bit[0]);
    }

    #[test]
    fn clock_skips_pinned() {
        let mut p = ClockPolicy::new(2);
        p.on_insert(0);
        p.on_insert(1);
        p.ref_bit[0] = false;
        p.ref_bit[1] = false;
        p.hand = 0;
        let pins = [1, 0];
        assert_eq!(p.evict(&pins), Some(1));
    }
}
