use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{DbError, Result};
use crate::storage::page::{empty_page, read_u32, write_u32, HEADER_PAGE, MAGIC, PAGE_SIZE, PageId};

/// Owns the database file. Page 0 is the header (magic + page_count + catalog).
pub struct FileManager {
    file: File,
    page_count: u32,
}

impl FileManager {
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let exists = path.exists() && path.metadata()?.len() > 0;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        if exists {
            let mut fm = FileManager {
                file,
                page_count: 1,
            };
            let mut header = empty_page();
            fm.read_page(HEADER_PAGE, &mut header)?;
            if &header[0..8] != MAGIC {
                return Err(DbError::msg("invalid database file magic"));
            }
            fm.page_count = read_u32(&header, 8);
            if fm.page_count == 0 {
                return Err(DbError::msg("corrupt header: page_count is 0"));
            }
            Ok(fm)
        } else {
            let mut fm = FileManager {
                file,
                page_count: 1,
            };
            let mut header = empty_page();
            header[0..8].copy_from_slice(MAGIC);
            write_u32(&mut header, 8, 1);
            fm.write_page(HEADER_PAGE, &header)?;
            fm.file.flush()?;
            Ok(fm)
        }
    }

    pub fn page_count(&self) -> u32 {
        self.page_count
    }

    pub fn read_page(&mut self, id: PageId, buf: &mut [u8]) -> Result<()> {
        if buf.len() != PAGE_SIZE {
            return Err(DbError::msg("page buffer must be PAGE_SIZE"));
        }
        if id.0 >= self.page_count {
            return Err(DbError::msg(format!(
                "read past end: page {} >= count {}",
                id.0, self.page_count
            )));
        }
        self.file.seek(SeekFrom::Start(id.offset()))?;
        self.file.read_exact(buf)?;
        Ok(())
    }

    pub fn write_page(&mut self, id: PageId, buf: &[u8]) -> Result<()> {
        if buf.len() != PAGE_SIZE {
            return Err(DbError::msg("page buffer must be PAGE_SIZE"));
        }
        if id.0 >= self.page_count {
            return Err(DbError::msg(format!(
                "write past end: page {} >= count {}",
                id.0, self.page_count
            )));
        }
        self.file.seek(SeekFrom::Start(id.offset()))?;
        self.file.write_all(buf)?;
        Ok(())
    }

    /// Allocate a new zeroed page at the end of the file and persist `page_count`
    /// in the header so a reopen can read the new page.
    pub fn allocate_page(&mut self) -> Result<PageId> {
        let id = PageId(self.page_count);
        self.page_count += 1;
        self.file
            .set_len(self.page_count as u64 * PAGE_SIZE as u64)?;
        let zeros = empty_page();
        self.write_page(id, &zeros)?;
        self.persist_page_count()?;
        Ok(id)
    }

    pub fn persist_page_count(&mut self) -> Result<()> {
        let mut header = empty_page();
        self.read_page(HEADER_PAGE, &mut header)?;
        self.sync_header_page_count(&mut header);
        self.write_page(HEADER_PAGE, &header)?;
        Ok(())
    }

    pub fn sync_header_page_count(&mut self, header: &mut [u8]) {
        write_u32(header, 8, self.page_count);
        header[0..8].copy_from_slice(MAGIC);
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::temp_db_path;

    #[test]
    fn file_roundtrip() {
        let path = temp_db_path("file-roundtrip");
        let mut fm = FileManager::open_or_create(&path).unwrap();
        let id = fm.allocate_page().unwrap();
        let mut page = empty_page();
        page[100] = 0xAB;
        write_u32(&mut page, 16, 0xDEAD_BEEF);
        fm.write_page(id, &page).unwrap();
        fm.flush().unwrap();
        drop(fm);

        let mut fm = FileManager::open_or_create(&path).unwrap();
        let mut back = empty_page();
        fm.read_page(id, &mut back).unwrap();
        assert_eq!(back[100], 0xAB);
        assert_eq!(read_u32(&back, 16), 0xDEAD_BEEF);
        let _ = std::fs::remove_file(&path);
    }
}
