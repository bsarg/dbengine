use crate::error::{DbError, Result};
use crate::storage::page::{
    empty_page, read_i64, read_u16, read_u32, write_i64, write_u16, write_u32, PageId, PageType,
    Rid, PAGE_SIZE, set_page_type,
};

/// Small fanout so splits show up in tests without tens of thousands of rows.
pub const MAX_KEYS: usize = 16;

#[derive(Clone, Debug)]
pub struct Node {
    pub is_leaf: bool,
    pub keys: Vec<i64>,
    pub children: Vec<PageId>,
    pub rids: Vec<Rid>,
    pub next_leaf: Option<PageId>,
}

impl Node {
    pub fn leaf() -> Self {
        Node {
            is_leaf: true,
            keys: Vec::new(),
            children: Vec::new(),
            rids: Vec::new(),
            next_leaf: None,
        }
    }

    pub fn internal(keys: Vec<i64>, children: Vec<PageId>) -> Self {
        Node {
            is_leaf: false,
            keys,
            children,
            rids: Vec::new(),
            next_leaf: None,
        }
    }

    pub fn decode(page: &[u8]) -> Result<Self> {
        let ty = page[0];
        let is_leaf = match PageType::from_u8(ty) {
            Some(PageType::BTreeLeaf) => true,
            Some(PageType::BTreeInternal) => false,
            _ => return Err(DbError::msg("not a B+Tree page")),
        };
        let key_count = read_u16(page, 2) as usize;
        if key_count > MAX_KEYS {
            return Err(DbError::msg("corrupt B+Tree node: too many keys"));
        }
        let next = read_u32(page, 4);
        let next_leaf = if is_leaf && next != 0 {
            Some(PageId(next))
        } else {
            None
        };
        let mut keys = Vec::with_capacity(key_count);
        let mut off = 16;
        for _ in 0..key_count {
            keys.push(read_i64(page, off));
            off += 8;
        }
        if is_leaf {
            let mut rids = Vec::with_capacity(key_count);
            for _ in 0..key_count {
                let page_id = read_u32(page, off);
                let slot = read_u16(page, off + 4);
                off += 8;
                rids.push(Rid {
                    page: PageId(page_id),
                    slot,
                });
            }
            Ok(Node {
                is_leaf: true,
                keys,
                children: Vec::new(),
                rids,
                next_leaf,
            })
        } else {
            let child_n = key_count + 1;
            let mut children = Vec::with_capacity(child_n);
            for _ in 0..child_n {
                children.push(PageId(read_u32(page, off)));
                off += 4;
            }
            Ok(Node {
                is_leaf: false,
                keys,
                children,
                rids: Vec::new(),
                next_leaf: None,
            })
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.keys.len() > MAX_KEYS {
            return Err(DbError::msg("B+Tree node overflow on encode"));
        }
        let mut page = empty_page();
        if self.is_leaf {
            set_page_type(&mut page, PageType::BTreeLeaf);
            if self.keys.len() != self.rids.len() {
                return Err(DbError::msg("leaf key/rid mismatch"));
            }
        } else {
            set_page_type(&mut page, PageType::BTreeInternal);
            if self.children.len() != self.keys.len() + 1 {
                return Err(DbError::msg("internal child/key mismatch"));
            }
        }
        write_u16(&mut page, 2, self.keys.len() as u16);
        write_u32(
            &mut page,
            4,
            self.next_leaf.map(|p| p.0).unwrap_or(0),
        );
        let mut off = 16;
        for k in &self.keys {
            write_i64(&mut page, off, *k);
            off += 8;
        }
        if self.is_leaf {
            for rid in &self.rids {
                write_u32(&mut page, off, rid.page.0);
                write_u16(&mut page, off + 4, rid.slot);
                off += 8;
            }
        } else {
            for c in &self.children {
                write_u32(&mut page, off, c.0);
                off += 4;
            }
        }
        if off > PAGE_SIZE {
            return Err(DbError::msg("B+Tree node does not fit in a page"));
        }
        Ok(page)
    }

    /// Child to follow on insert: equal keys go right so new duplicates land
    /// on the rightmost leaf that already holds `key`.
    pub fn find_child(&self, key: i64) -> PageId {
        self.child_at(key, true)
    }

    /// Child to follow on lookup: equal keys go left so we start at the
    /// leftmost leaf that may contain `key`.
    pub fn find_child_leftmost(&self, key: i64) -> PageId {
        self.child_at(key, false)
    }

    fn child_at(&self, key: i64, equal_goes_right: bool) -> PageId {
        let mut i = 0;
        while i < self.keys.len() {
            let go_right = if equal_goes_right {
                key >= self.keys[i]
            } else {
                key > self.keys[i]
            };
            if go_right {
                i += 1;
            } else {
                break;
            }
        }
        self.children[i]
    }

    pub fn insert_leaf(&mut self, key: i64, rid: Rid) {
        let mut i = 0;
        while i < self.keys.len() && self.keys[i] <= key {
            i += 1;
        }
        self.keys.insert(i, key);
        self.rids.insert(i, rid);
    }

    pub fn insert_internal(&mut self, sep: i64, right: PageId) {
        let mut i = 0;
        while i < self.keys.len() && self.keys[i] <= sep {
            i += 1;
        }
        self.keys.insert(i, sep);
        self.children.insert(i + 1, right);
    }

    pub fn split_leaf(&mut self) -> (Node, i64) {
        let mid = self.keys.len() / 2;
        let right = Node {
            is_leaf: true,
            keys: self.keys.split_off(mid),
            children: Vec::new(),
            rids: self.rids.split_off(mid),
            next_leaf: self.next_leaf,
        };
        let sep = right.keys[0];
        (right, sep)
    }

    pub fn split_internal(&mut self) -> (Node, i64) {
        let mid = self.keys.len() / 2;
        let sep = self.keys[mid];
        let right_keys = self.keys.split_off(mid + 1);
        self.keys.pop(); // remove sep from left
        let right_children = self.children.split_off(mid + 1);
        let right = Node::internal(right_keys, right_children);
        (right, sep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_encode_decode() {
        let mut n = Node::leaf();
        n.insert_leaf(
            7,
            Rid {
                page: PageId(3),
                slot: 1,
            },
        );
        let bytes = n.encode().unwrap();
        let back = Node::decode(&bytes).unwrap();
        assert!(back.is_leaf);
        assert_eq!(back.keys, vec![7]);
        assert_eq!(back.rids[0].page, PageId(3));
    }

    #[test]
    fn internal_encode_decode_and_split() {
        let n = Node::internal(
            vec![10, 20],
            vec![PageId(1), PageId(2), PageId(3)],
        );
        let bytes = n.encode().unwrap();
        let back = Node::decode(&bytes).unwrap();
        assert!(!back.is_leaf);
        assert_eq!(back.keys, vec![10, 20]);
        assert_eq!(back.children, vec![PageId(1), PageId(2), PageId(3)]);
        assert_eq!(back.find_child(10), PageId(2));
        assert_eq!(back.find_child_leftmost(10), PageId(1));

        let mut leaf = Node::leaf();
        for k in 0..4 {
            leaf.insert_leaf(
                k,
                Rid {
                    page: PageId(1),
                    slot: k as u16,
                },
            );
        }
        let (right, sep) = leaf.split_leaf();
        assert_eq!(sep, 2);
        assert_eq!(leaf.keys, vec![0, 1]);
        assert_eq!(right.keys, vec![2, 3]);
    }
}
