pub const PAGE_SIZE: usize = 4096;
pub const HEADER_PAGE: PageId = PageId(0);
pub const MAGIC: &[u8; 8] = b"DBENGv01";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u32);

impl PageId {
    pub fn as_u32(self) -> u32 {
        self.0
    }

    pub fn offset(self) -> u64 {
        self.0 as u64 * PAGE_SIZE as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rid {
    pub page: PageId,
    pub slot: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PageType {
    Header = 0,
    Heap = 1,
    BTreeLeaf = 2,
    BTreeInternal = 3,
}

impl PageType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(PageType::Header),
            1 => Some(PageType::Heap),
            2 => Some(PageType::BTreeLeaf),
            3 => Some(PageType::BTreeInternal),
            _ => None,
        }
    }
}

pub fn empty_page() -> Vec<u8> {
    vec![0u8; PAGE_SIZE]
}

pub fn read_u16(page: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(page[off..off + 2].try_into().unwrap())
}

pub fn write_u16(page: &mut [u8], off: usize, v: u16) {
    page[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn read_u32(page: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(page[off..off + 4].try_into().unwrap())
}

pub fn write_u32(page: &mut [u8], off: usize, v: u32) {
    page[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn read_i64(page: &[u8], off: usize) -> i64 {
    i64::from_le_bytes(page[off..off + 8].try_into().unwrap())
}

pub fn write_i64(page: &mut [u8], off: usize, v: i64) {
    page[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

pub fn set_page_type(page: &mut [u8], ty: PageType) {
    page[0] = ty as u8;
}

pub fn get_page_type(page: &[u8]) -> Option<PageType> {
    PageType::from_u8(page[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_id_offset() {
        assert_eq!(PageId(0).offset(), 0);
        assert_eq!(PageId(2).offset(), (2 * PAGE_SIZE) as u64);
    }

    #[test]
    fn u32_roundtrip() {
        let mut p = empty_page();
        write_u32(&mut p, 8, 42);
        assert_eq!(read_u32(&p, 8), 42);
    }
}
