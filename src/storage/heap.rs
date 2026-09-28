use crate::error::{DbError, Result};
use crate::eviction::EvictionPolicy;
use crate::storage::buffer::BufferPool;
use crate::storage::page::{PageId, Rid};
use crate::storage::slotted;

pub struct HeapFile {
    pub first: PageId,
    pub last: PageId,
}

impl HeapFile {
    pub fn create<P: EvictionPolicy>(pool: &mut BufferPool<P>) -> Result<Self> {
        let id = pool.allocate_page()?;
        let mut page = pool.load_page(id)?;
        slotted::init_heap_page(&mut page, None);
        pool.store_page(id, &page)?;
        Ok(HeapFile {
            first: id,
            last: id,
        })
    }

    pub fn insert<P: EvictionPolicy>(
        &mut self,
        pool: &mut BufferPool<P>,
        record: &[u8],
    ) -> Result<Rid> {
        let mut last = pool.load_page(self.last)?;
        match slotted::try_insert(&mut last, record) {
            Ok(slot) => {
                pool.store_page(self.last, &last)?;
                Ok(Rid {
                    page: self.last,
                    slot,
                })
            }
            Err(_) => {
                let new_id = pool.allocate_page()?;
                let mut new_page = pool.load_page(new_id)?;
                slotted::init_heap_page(&mut new_page, None);
                let slot = slotted::try_insert(&mut new_page, record)
                    .map_err(|_| DbError::msg("record larger than a page"))?;
                slotted::set_next_page(&mut last, Some(new_id));
                pool.store_page(self.last, &last)?;
                pool.store_page(new_id, &new_page)?;
                self.last = new_id;
                Ok(Rid {
                    page: new_id,
                    slot,
                })
            }
        }
    }

    pub fn get<P: EvictionPolicy>(&self, pool: &mut BufferPool<P>, rid: Rid) -> Result<Vec<u8>> {
        let page = pool.load_page(rid.page)?;
        slotted::get_record(&page, rid.slot)
    }

    pub fn scan<P: EvictionPolicy>(
        &self,
        pool: &mut BufferPool<P>,
    ) -> Result<Vec<(Rid, Vec<u8>)>> {
        let mut out = Vec::new();
        let mut cur = Some(self.first);
        while let Some(id) = cur {
            let page = pool.load_page(id)?;
            slotted::for_each_slot(&page, |slot, rec| {
                out.push((
                    Rid { page: id, slot },
                    rec.to_vec(),
                ));
            });
            cur = slotted::next_page(&page);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eviction::LruPolicy;
    use crate::test_util::temp_db_path;

    #[test]
    fn heap_insert_get_scan() {
        let path = temp_db_path("heap-scan");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 8).unwrap();
        let mut heap = HeapFile::create(&mut pool).unwrap();
        let r0 = heap.insert(&mut pool, b"alpha").unwrap();
        let r1 = heap.insert(&mut pool, b"beta").unwrap();
        assert_eq!(heap.get(&mut pool, r0).unwrap(), b"alpha");
        assert_eq!(heap.get(&mut pool, r1).unwrap(), b"beta");
        let rows = heap.scan(&mut pool).unwrap();
        assert_eq!(rows.len(), 2);
        pool.flush_all().unwrap();
        drop(pool);

        let mut pool = BufferPool::<LruPolicy>::open(&path, 8).unwrap();
        let heap = HeapFile {
            first: heap.first,
            last: heap.last,
        };
        assert_eq!(heap.get(&mut pool, r0).unwrap(), b"alpha");
        assert_eq!(heap.scan(&mut pool).unwrap().len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn heap_spans_multiple_pages() {
        let path = temp_db_path("heap-multipage");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 8).unwrap();
        let mut heap = HeapFile::create(&mut pool).unwrap();
        let rec = vec![b'x'; 800];
        let mut rids = Vec::new();
        for _ in 0..20 {
            rids.push(heap.insert(&mut pool, &rec).unwrap());
        }
        assert_ne!(heap.first, heap.last);
        assert_eq!(heap.scan(&mut pool).unwrap().len(), 20);
        assert_eq!(heap.get(&mut pool, rids[0]).unwrap(), rec);
        assert_eq!(heap.get(&mut pool, rids[19]).unwrap(), rec);
        pool.flush_all().unwrap();
        drop(pool);

        let mut pool = BufferPool::<LruPolicy>::open(&path, 4).unwrap();
        let heap = HeapFile {
            first: heap.first,
            last: heap.last,
        };
        assert_eq!(heap.scan(&mut pool).unwrap().len(), 20);
        let _ = std::fs::remove_file(&path);
    }
}
