use crate::error::Result;
use crate::eviction::EvictionPolicy;
use crate::index::node::{Node, MAX_KEYS};
use crate::storage::buffer::BufferPool;
use crate::storage::page::{PageId, Rid};

pub struct BPlusTree {
    pub root: PageId,
}

impl BPlusTree {
    pub fn create<P: EvictionPolicy>(pool: &mut BufferPool<P>) -> Result<Self> {
        let id = pool.allocate_page()?;
        let node = Node::leaf();
        pool.store_page(id, &node.encode()?)?;
        Ok(BPlusTree { root: id })
    }

    pub fn insert<P: EvictionPolicy>(
        &mut self,
        pool: &mut BufferPool<P>,
        key: i64,
        rid: Rid,
    ) -> Result<()> {
        let path = self.path_to_leaf(pool, key, true)?;
        let leaf_id = *path.last().unwrap();
        let mut leaf = Node::decode(&pool.load_page(leaf_id)?)?;
        leaf.insert_leaf(key, rid);
        if leaf.keys.len() <= MAX_KEYS {
            pool.store_page(leaf_id, &leaf.encode()?)?;
            return Ok(());
        }
        let (right, sep) = leaf.split_leaf();
        let right_id = pool.allocate_page()?;
        leaf.next_leaf = Some(right_id);
        pool.store_page(leaf_id, &leaf.encode()?)?;
        pool.store_page(right_id, &right.encode()?)?;
        self.insert_parent(pool, &path, sep, right_id)
    }

    pub fn get<P: EvictionPolicy>(
        &self,
        pool: &mut BufferPool<P>,
        key: i64,
    ) -> Result<Vec<Rid>> {
        let leaf_id = self.find_leaf(pool, key)?;
        let mut out = Vec::new();
        let mut cur = Some(leaf_id);
        while let Some(id) = cur {
            let node = Node::decode(&pool.load_page(id)?)?;
            let mut saw_greater = false;
            for (i, k) in node.keys.iter().enumerate() {
                if *k == key {
                    out.push(node.rids[i]);
                } else if *k > key {
                    saw_greater = true;
                    break;
                }
            }
            if saw_greater {
                break;
            }
            cur = node.next_leaf;
        }
        Ok(out)
    }

    pub fn range<P: EvictionPolicy>(
        &self,
        pool: &mut BufferPool<P>,
        low: i64,
        low_incl: bool,
        high: i64,
        high_incl: bool,
    ) -> Result<Vec<Rid>> {
        let mut out = Vec::new();
        let mut cur = Some(self.find_leaf(pool, low)?);
        while let Some(id) = cur {
            let node = Node::decode(&pool.load_page(id)?)?;
            for (i, k) in node.keys.iter().enumerate() {
                let ge_low = if low_incl { *k >= low } else { *k > low };
                let le_high = if high_incl { *k <= high } else { *k < high };
                if ge_low && le_high {
                    out.push(node.rids[i]);
                }
                if *k > high || (!high_incl && *k == high) {
                    return Ok(out);
                }
            }
            cur = node.next_leaf;
        }
        Ok(out)
    }

    /// Delete the first matching key. Leaves may underflow (no merge).
    pub fn delete<P: EvictionPolicy>(&mut self, pool: &mut BufferPool<P>, key: i64) -> Result<bool> {
        let leaf_id = self.find_leaf(pool, key)?;
        let mut node = Node::decode(&pool.load_page(leaf_id)?)?;
        if let Some(i) = node.keys.iter().position(|k| *k == key) {
            node.keys.remove(i);
            node.rids.remove(i);
            pool.store_page(leaf_id, &node.encode()?)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn height<P: EvictionPolicy>(&self, pool: &mut BufferPool<P>) -> Result<usize> {
        let mut h = 1;
        let mut id = self.root;
        loop {
            let n = Node::decode(&pool.load_page(id)?)?;
            if n.is_leaf {
                return Ok(h);
            }
            id = n.children[0];
            h += 1;
        }
    }

    fn path_to_leaf<P: EvictionPolicy>(
        &self,
        pool: &mut BufferPool<P>,
        key: i64,
        equal_goes_right: bool,
    ) -> Result<Vec<PageId>> {
        let mut path = vec![self.root];
        loop {
            let node = Node::decode(&pool.load_page(*path.last().unwrap())?)?;
            if node.is_leaf {
                return Ok(path);
            }
            let child = if equal_goes_right {
                node.find_child(key)
            } else {
                node.find_child_leftmost(key)
            };
            path.push(child);
        }
    }

    fn find_leaf<P: EvictionPolicy>(
        &self,
        pool: &mut BufferPool<P>,
        key: i64,
    ) -> Result<PageId> {
        Ok(*self.path_to_leaf(pool, key, false)?.last().unwrap())
    }

    fn insert_parent<P: EvictionPolicy>(
        &mut self,
        pool: &mut BufferPool<P>,
        path: &[PageId],
        sep: i64,
        right_id: PageId,
    ) -> Result<()> {
        if path.len() == 1 {
            let new_root = pool.allocate_page()?;
            let node = Node::internal(vec![sep], vec![path[0], right_id]);
            pool.store_page(new_root, &node.encode()?)?;
            self.root = new_root;
            return Ok(());
        }
        let parent_id = path[path.len() - 2];
        let mut parent = Node::decode(&pool.load_page(parent_id)?)?;
        parent.insert_internal(sep, right_id);
        if parent.keys.len() <= MAX_KEYS {
            pool.store_page(parent_id, &parent.encode()?)?;
            return Ok(());
        }
        let (right, up_sep) = parent.split_internal();
        let new_id = pool.allocate_page()?;
        pool.store_page(parent_id, &parent.encode()?)?;
        pool.store_page(new_id, &right.encode()?)?;
        self.insert_parent(pool, &path[..path.len() - 1], up_sep, new_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eviction::LruPolicy;
    use crate::test_util::temp_db_path;

    fn rid(n: u16) -> Rid {
        Rid {
            page: PageId(1),
            slot: n,
        }
    }

    #[test]
    fn sequential_insert_get() {
        let path = temp_db_path("bt-seq");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 32).unwrap();
        let mut t = BPlusTree::create(&mut pool).unwrap();
        for i in 1..=40 {
            t.insert(&mut pool, i, rid(i as u16)).unwrap();
        }
        assert!(t.height(&mut pool).unwrap() >= 2);
        for i in 1..=40 {
            let got = t.get(&mut pool, i).unwrap();
            assert_eq!(got.len(), 1);
            assert_eq!(got[0].slot, i as u16);
        }
        let range = t.range(&mut pool, 10, true, 20, true).unwrap();
        assert_eq!(range.len(), 11);
        t.delete(&mut pool, 15).unwrap();
        assert!(t.get(&mut pool, 15).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_reopen() {
        let path = temp_db_path("bt-persist");
        let root;
        {
            let mut pool = BufferPool::<LruPolicy>::open(&path, 32).unwrap();
            let mut t = BPlusTree::create(&mut pool).unwrap();
            for i in 0..25 {
                t.insert(&mut pool, i, rid(i as u16)).unwrap();
            }
            root = t.root;
            pool.flush_all().unwrap();
        }
        let mut pool = BufferPool::<LruPolicy>::open(&path, 32).unwrap();
        let t = BPlusTree { root };
        assert_eq!(t.get(&mut pool, 7).unwrap()[0].slot, 7);
        let _ = std::fs::remove_file(&path);
    }

    fn shuffle(keys: &mut [i64]) {
        let mut state: u64 = 0x1234_5678_9abc_def0;
        for i in (1..keys.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (state as usize) % (i + 1);
            keys.swap(i, j);
        }
    }

    #[test]
    fn random_insert_get_and_duplicates() {
        let path = temp_db_path("bt-rand");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 64).unwrap();
        let mut t = BPlusTree::create(&mut pool).unwrap();
        let mut keys: Vec<i64> = (0..80).collect();
        shuffle(&mut keys);
        for k in &keys {
            t.insert(&mut pool, *k, rid(*k as u16)).unwrap();
        }
        assert!(t.height(&mut pool).unwrap() >= 2);
        for k in 0..80 {
            let got = t.get(&mut pool, k).unwrap();
            assert_eq!(got.len(), 1, "missing {k}");
            assert_eq!(got[0].slot, k as u16);
        }
        t.insert(&mut pool, 42, rid(999)).unwrap();
        assert_eq!(t.get(&mut pool, 42).unwrap().len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reverse_insert_and_exclusive_range() {
        let path = temp_db_path("bt-rev");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 32).unwrap();
        let mut t = BPlusTree::create(&mut pool).unwrap();
        for i in (1..=40).rev() {
            t.insert(&mut pool, i, rid(i as u16)).unwrap();
        }
        let open = t.range(&mut pool, 10, false, 20, false).unwrap();
        assert_eq!(open.len(), 9);
        assert!(t.delete(&mut pool, 1).unwrap());
        assert!(!t.delete(&mut pool, 1).unwrap());
        let _ = std::fs::remove_file(&path);
    }
}
