use crate::error::{DbError, Result};
use crate::storage::page::{
    read_u16, read_u32, write_u16, write_u32, PageId, PageType, PAGE_SIZE, set_page_type,
};

/// Slotted heap page:
/// byte 0: PageType::Heap
/// 2-3: slot_count
/// 4-7: free_end (lowest byte used by records)
/// 8-11: next_page (0 = none)
/// 16+: slot directory of (offset u16, len u16)
/// records grow down from PAGE_SIZE.
const HDR: usize = 16;

pub fn init_heap_page(page: &mut [u8], next: Option<PageId>) {
    page.fill(0);
    set_page_type(page, PageType::Heap);
    write_u16(page, 2, 0);
    write_u32(page, 4, PAGE_SIZE as u32);
    write_u32(page, 8, next.map(|p| p.0).unwrap_or(0));
}

pub fn next_page(page: &[u8]) -> Option<PageId> {
    let id = read_u32(page, 8);
    if id == 0 {
        None
    } else {
        Some(PageId(id))
    }
}

pub fn set_next_page(page: &mut [u8], next: Option<PageId>) {
    write_u32(page, 8, next.map(|p| p.0).unwrap_or(0));
}

pub fn slot_count(page: &[u8]) -> u16 {
    read_u16(page, 2)
}

fn free_end(page: &[u8]) -> usize {
    read_u32(page, 4) as usize
}

fn dir_end(page: &[u8]) -> usize {
    HDR + slot_count(page) as usize * 4
}

pub fn try_insert(page: &mut [u8], record: &[u8]) -> Result<u16> {
    if record.len() > u16::MAX as usize {
        return Err(DbError::msg("record too large"));
    }
    let needed = record.len() + 4;
    let fe = free_end(page);
    let de = dir_end(page);
    if de + needed > fe {
        return Err(DbError::msg("page full"));
    }
    let new_fe = fe - record.len();
    page[new_fe..fe].copy_from_slice(record);
    let slot = slot_count(page);
    write_u16(page, HDR + slot as usize * 4, new_fe as u16);
    write_u16(page, HDR + slot as usize * 4 + 2, record.len() as u16);
    write_u16(page, 2, slot + 1);
    write_u32(page, 4, new_fe as u32);
    Ok(slot)
}

pub fn get_record(page: &[u8], slot: u16) -> Result<Vec<u8>> {
    if slot >= slot_count(page) {
        return Err(DbError::msg("invalid slot"));
    }
    let off = read_u16(page, HDR + slot as usize * 4) as usize;
    let len = read_u16(page, HDR + slot as usize * 4 + 2) as usize;
    if off + len > PAGE_SIZE {
        return Err(DbError::msg("corrupt slot"));
    }
    Ok(page[off..off + len].to_vec())
}

pub fn for_each_slot(page: &[u8], mut f: impl FnMut(u16, &[u8])) {
    let n = slot_count(page);
    for slot in 0..n {
        let off = read_u16(page, HDR + slot as usize * 4) as usize;
        let len = read_u16(page, HDR + slot as usize * 4 + 2) as usize;
        if len == 0 {
            continue;
        }
        if off + len <= PAGE_SIZE {
            f(slot, &page[off..off + len]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::page::empty_page;

    #[test]
    fn slotted_insert_get() {
        let mut p = empty_page();
        init_heap_page(&mut p, None);
        let s = try_insert(&mut p, b"hello").unwrap();
        assert_eq!(s, 0);
        assert_eq!(get_record(&p, 0).unwrap(), b"hello");
        let s2 = try_insert(&mut p, b"world").unwrap();
        assert_eq!(s2, 1);
        assert_eq!(get_record(&p, 1).unwrap(), b"world");
    }

    #[test]
    fn slotted_page_full() {
        let mut p = empty_page();
        init_heap_page(&mut p, None);
        let rec = vec![0xAAu8; 200];
        let mut n = 0;
        while try_insert(&mut p, &rec).is_ok() {
            n += 1;
        }
        assert!(n > 1);
        assert!(try_insert(&mut p, &rec).is_err());
        assert_eq!(get_record(&p, 0).unwrap().len(), 200);
    }
}
