use super::EvictionPolicy;

struct LruNode {
    prev: Option<usize>,
    next: Option<usize>,
    in_list: bool,
}

/// Doubly-linked list over frame indices: head = MRU, tail = LRU.
pub struct LruPolicy {
    nodes: Vec<LruNode>,
    head: Option<usize>,
    tail: Option<usize>,
}

impl LruPolicy {
    fn unlink(&mut self, frame: usize) {
        if !self.nodes[frame].in_list {
            return;
        }
        let prev = self.nodes[frame].prev;
        let next = self.nodes[frame].next;
        if let Some(p) = prev {
            self.nodes[p].next = next;
        } else {
            self.head = next;
        }
        if let Some(n) = next {
            self.nodes[n].prev = prev;
        } else {
            self.tail = prev;
        }
        self.nodes[frame].prev = None;
        self.nodes[frame].next = None;
        self.nodes[frame].in_list = false;
    }

    fn push_head(&mut self, frame: usize) {
        self.nodes[frame].in_list = true;
        self.nodes[frame].prev = None;
        self.nodes[frame].next = self.head;
        if let Some(h) = self.head {
            self.nodes[h].prev = Some(frame);
        } else {
            self.tail = Some(frame);
        }
        self.head = Some(frame);
    }
}

impl EvictionPolicy for LruPolicy {
    fn new(capacity: usize) -> Self {
        let nodes = (0..capacity)
            .map(|_| LruNode {
                prev: None,
                next: None,
                in_list: false,
            })
            .collect();
        LruPolicy {
            nodes,
            head: None,
            tail: None,
        }
    }

    fn on_insert(&mut self, frame: usize) {
        self.unlink(frame);
        self.push_head(frame);
    }

    fn on_access(&mut self, frame: usize) {
        self.unlink(frame);
        self.push_head(frame);
    }

    fn on_remove(&mut self, frame: usize) {
        self.unlink(frame);
    }

    fn evict(&mut self, pinned: &[u32]) -> Option<usize> {
        let mut cur = self.tail;
        while let Some(f) = cur {
            if pinned.get(f).copied().unwrap_or(0) == 0 {
                return Some(f);
            }
            cur = self.nodes[f].prev;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_recency() {
        let mut p = LruPolicy::new(3);
        p.on_insert(0);
        p.on_insert(1);
        p.on_insert(2);
        // MRU=2, then 1, LRU=0
        p.on_access(0);
        // MRU=0, then 2, LRU=1
        let pins = [0, 0, 0];
        assert_eq!(p.evict(&pins), Some(1));
        p.on_remove(1);
        p.on_insert(1);
        // MRU=1, then 0, LRU=2
        assert_eq!(p.evict(&pins), Some(2));
    }

    #[test]
    fn lru_skips_pinned() {
        let mut p = LruPolicy::new(2);
        p.on_insert(0);
        p.on_insert(1);
        let pins = [1, 1];
        assert_eq!(p.evict(&pins), None);
        let pins = [1, 0];
        assert_eq!(p.evict(&pins), Some(1));
    }
}
