use std::collections::HashMap;

use crate::error::{DbError, Result};
use crate::eviction::EvictionPolicy;
use crate::storage::disk::FileManager;
use crate::storage::page::{empty_page, HEADER_PAGE, PAGE_SIZE, PageId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameId(pub usize);

struct Frame {
    page_id: Option<PageId>,
    data: Vec<u8>,
    pin_count: u32,
    dirty: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PoolStats {
    pub hits: u64,
    pub misses: u64,
}

pub struct BufferPool<P: EvictionPolicy> {
    file: FileManager,
    frames: Vec<Frame>,
    page_table: HashMap<PageId, usize>,
    policy: P,
    stats: PoolStats,
}

impl<P: EvictionPolicy> BufferPool<P> {
    pub fn open(path: impl AsRef<std::path::Path>, capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(DbError::msg("buffer pool capacity must be > 0"));
        }
        let file = FileManager::open_or_create(path)?;
        let frames = (0..capacity)
            .map(|_| Frame {
                page_id: None,
                data: empty_page(),
                pin_count: 0,
                dirty: false,
            })
            .collect();
        Ok(BufferPool {
            file,
            frames,
            page_table: HashMap::new(),
            policy: P::new(capacity),
            stats: PoolStats::default(),
        })
    }

    pub fn stats(&self) -> PoolStats {
        self.stats
    }

    pub fn reset_stats(&mut self) {
        self.stats = PoolStats::default();
    }

    pub fn page_count(&self) -> u32 {
        self.file.page_count()
    }

    pub fn pin(&mut self, id: PageId) -> Result<FrameId> {
        if let Some(&frame) = self.page_table.get(&id) {
            self.stats.hits += 1;
            self.frames[frame].pin_count += 1;
            self.policy.on_access(frame);
            return Ok(FrameId(frame));
        }
        self.stats.misses += 1;
        let frame = self.victim_frame()?;
        if let Some(old) = self.frames[frame].page_id {
            if self.frames[frame].dirty {
                self.file.write_page(old, &self.frames[frame].data)?;
            }
            self.page_table.remove(&old);
            self.policy.on_remove(frame);
        }
        self.file.read_page(id, &mut self.frames[frame].data)?;
        self.frames[frame].page_id = Some(id);
        self.frames[frame].pin_count = 1;
        self.frames[frame].dirty = false;
        self.page_table.insert(id, frame);
        self.policy.on_insert(frame);
        Ok(FrameId(frame))
    }

    pub fn unpin(&mut self, frame: FrameId, dirty: bool) {
        let f = &mut self.frames[frame.0];
        if dirty {
            f.dirty = true;
        }
        if f.pin_count > 0 {
            f.pin_count -= 1;
        }
    }

    pub fn data(&self, frame: FrameId) -> &[u8] {
        &self.frames[frame.0].data
    }

    pub fn data_mut(&mut self, frame: FrameId) -> &mut [u8] {
        &mut self.frames[frame.0].data
    }

    /// Copy a page out (B+Tree / heap helpers). Still counted as a pin hit/miss.
    pub fn load_page(&mut self, id: PageId) -> Result<Vec<u8>> {
        let f = self.pin(id)?;
        let data = self.data(f).to_vec();
        self.unpin(f, false);
        Ok(data)
    }

    pub fn store_page(&mut self, id: PageId, data: &[u8]) -> Result<()> {
        if data.len() != PAGE_SIZE {
            return Err(DbError::msg("store_page: wrong size"));
        }
        let f = self.pin(id)?;
        self.data_mut(f).copy_from_slice(data);
        self.unpin(f, true);
        Ok(())
    }

    pub fn allocate_page(&mut self) -> Result<PageId> {
        // Persist a dirty header first so FileManager's page-count rewrite
        // cannot replace the on-disk catalog with a stale header.
        self.flush_header_if_dirty()?;
        let id = self.file.allocate_page()?;
        self.sync_page_count_header()?;
        Ok(id)
    }

    fn flush_header_if_dirty(&mut self) -> Result<()> {
        if let Some(&frame) = self.page_table.get(&HEADER_PAGE) {
            if self.frames[frame].dirty {
                self.file
                    .write_page(HEADER_PAGE, &self.frames[frame].data)?;
                self.frames[frame].dirty = false;
            }
        }
        Ok(())
    }

    fn sync_page_count_header(&mut self) -> Result<()> {
        let count = self.file.page_count();
        let f = self.pin(HEADER_PAGE)?;
        let buf = self.data_mut(f);
        crate::storage::page::write_u32(buf, 8, count);
        buf[0..8].copy_from_slice(crate::storage::page::MAGIC);
        self.unpin(f, true);
        Ok(())
    }

    pub fn flush_all(&mut self) -> Result<()> {
        let _ = self.sync_page_count_header();
        for frame in &mut self.frames {
            if frame.dirty {
                if let Some(id) = frame.page_id {
                    self.file.write_page(id, &frame.data)?;
                    frame.dirty = false;
                }
            }
        }
        self.file.flush()?;
        Ok(())
    }

    fn victim_frame(&mut self) -> Result<usize> {
        if let Some(i) = self.frames.iter().position(|f| f.page_id.is_none()) {
            return Ok(i);
        }
        let pins: Vec<u32> = self.frames.iter().map(|f| f.pin_count).collect();
        self.policy
            .evict(&pins)
            .ok_or_else(|| DbError::msg("buffer pool full: all pages pinned"))
    }
}

impl<P: EvictionPolicy> Drop for BufferPool<P> {
    fn drop(&mut self) {
        let _ = self.flush_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eviction::LruPolicy;
    use crate::storage::page::{read_u32, write_u32};
    use crate::test_util::temp_db_path;

    #[test]
    fn pool_hit_miss_counts() {
        let path = temp_db_path("pool-hits");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 2).unwrap();
        let a = pool.allocate_page().unwrap();
        let b = pool.allocate_page().unwrap();
        pool.reset_stats();
        let fa = pool.pin(a).unwrap();
        pool.unpin(fa, false);
        let fa2 = pool.pin(a).unwrap();
        pool.unpin(fa2, false);
        let s = pool.stats();
        assert_eq!(s.hits, 1);
        assert_eq!(s.misses, 1);
        let _ = b;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn evict_flushes_dirty() {
        let path = temp_db_path("evict-flush");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 2).unwrap();
        let a = pool.allocate_page().unwrap();
        let b = pool.allocate_page().unwrap();
        let c = pool.allocate_page().unwrap();
        let fa = pool.pin(a).unwrap();
        write_u32(pool.data_mut(fa), 32, 123456);
        pool.unpin(fa, true);
        // Fill the other slot, then force eviction of a by pinning c (capacity 2:
        // header + pages compete). Use a larger pool's replacement: pin b and c
        // after unpinning a so a is the LRU dirty victim.
        let fb = pool.pin(b).unwrap();
        pool.unpin(fb, false);
        let fc = pool.pin(c).unwrap();
        pool.unpin(fc, false);
        pool.flush_all().unwrap();
        drop(pool);

        let mut pool = BufferPool::<LruPolicy>::open(&path, 2).unwrap();
        let fa = pool.pin(a).unwrap();
        assert_eq!(read_u32(pool.data(fa), 32), 123456);
        pool.unpin(fa, false);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pinned_pages_not_evicted() {
        let path = temp_db_path("pin-full");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 2).unwrap();
        let a = pool.allocate_page().unwrap();
        let b = pool.allocate_page().unwrap();
        let c = pool.allocate_page().unwrap();
        // Pool capacity 2: pin two data pages (header may already occupy a frame).
        // Use capacity 3 so we can pin two data pages without header eviction issues.
        drop(pool);
        let mut pool = BufferPool::<LruPolicy>::open(&path, 3).unwrap();
        let fa = pool.pin(a).unwrap();
        let fb = pool.pin(b).unwrap();
        let fc = pool.pin(HEADER_PAGE).unwrap();
        let err = pool.pin(c).unwrap_err();
        assert!(err.to_string().contains("all pages pinned"));
        pool.unpin(fa, false);
        pool.unpin(fb, false);
        pool.unpin(fc, false);
        let _ = pool.pin(c).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn capacity_two_evicts_third_page() {
        let path = temp_db_path("pool-evict-third");
        let mut pool = BufferPool::<LruPolicy>::open(&path, 2).unwrap();
        let a = pool.allocate_page().unwrap();
        let b = pool.allocate_page().unwrap();
        let c = pool.allocate_page().unwrap();
        pool.flush_all().unwrap();
        pool.reset_stats();
        let fa = pool.pin(a).unwrap();
        pool.unpin(fa, false);
        let fb = pool.pin(b).unwrap();
        pool.unpin(fb, false);
        let fc = pool.pin(c).unwrap();
        pool.unpin(fc, false);
        let s = pool.stats();
        assert!(s.misses >= 3);
        // a should have been evicted; pinning it again is a miss
        pool.reset_stats();
        let fa = pool.pin(a).unwrap();
        pool.unpin(fa, false);
        assert_eq!(pool.stats().misses, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clock_and_lfu_pools_pin_pages() {
        use crate::eviction::{ClockPolicy, LfuPolicy};
        for path_label in ["pool-clock", "pool-lfu"] {
            let path = temp_db_path(path_label);
            if path_label.ends_with("clock") {
                let mut pool = BufferPool::<ClockPolicy>::open(&path, 4).unwrap();
                let id = pool.allocate_page().unwrap();
                let f = pool.pin(id).unwrap();
                write_u32(pool.data_mut(f), 40, 7);
                pool.unpin(f, true);
                pool.flush_all().unwrap();
                drop(pool);
                let mut pool = BufferPool::<ClockPolicy>::open(&path, 4).unwrap();
                let f = pool.pin(id).unwrap();
                assert_eq!(read_u32(pool.data(f), 40), 7);
                pool.unpin(f, false);
            } else {
                let mut pool = BufferPool::<LfuPolicy>::open(&path, 4).unwrap();
                let id = pool.allocate_page().unwrap();
                let f = pool.pin(id).unwrap();
                write_u32(pool.data_mut(f), 40, 8);
                pool.unpin(f, true);
                pool.flush_all().unwrap();
                drop(pool);
                let mut pool = BufferPool::<LfuPolicy>::open(&path, 4).unwrap();
                let f = pool.pin(id).unwrap();
                assert_eq!(read_u32(pool.data(f), 40), 8);
                pool.unpin(f, false);
            }
            let _ = std::fs::remove_file(&path);
        }
    }
}
