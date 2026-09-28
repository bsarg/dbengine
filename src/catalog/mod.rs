use crate::error::{DbError, Result};
use crate::storage::heap::HeapFile;
use crate::storage::page::{read_u16, read_u32, PageId, PAGE_SIZE};

pub const CATALOG_OFFSET: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColType {
    Int,
    Text,
}

impl ColType {
    fn to_u8(self) -> u8 {
        match self {
            ColType::Int => 0,
            ColType::Text => 1,
        }
    }

    fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(ColType::Int),
            1 => Ok(ColType::Text),
            _ => Err(DbError::msg("unknown column type")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Column {
    pub name: String,
    pub ty: ColType,
}

#[derive(Clone, Debug)]
pub struct TableMeta {
    pub name: String,
    pub columns: Vec<Column>,
    pub heap_first: PageId,
    pub heap_last: PageId,
    pub btree_root: PageId,
}

impl TableMeta {
    pub fn heap(&self) -> HeapFile {
        HeapFile {
            first: self.heap_first,
            last: self.heap_last,
        }
    }

    pub fn index_col(&self) -> Option<usize> {
        self.columns.iter().position(|c| c.ty == ColType::Int)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub tables: Vec<TableMeta>,
}

impl Catalog {
    pub fn load(page0: &[u8]) -> Result<Self> {
        let mut off = CATALOG_OFFSET;
        let n = read_u16(page0, off) as usize;
        off += 2;
        let mut tables = Vec::with_capacity(n);
        for _ in 0..n {
            let name = read_string(page0, &mut off)?;
            let n_cols = read_u16(page0, off) as usize;
            off += 2;
            let mut columns = Vec::with_capacity(n_cols);
            for _ in 0..n_cols {
                let cname = read_string(page0, &mut off)?;
                let ty = ColType::from_u8(page0[off])?;
                off += 1;
                columns.push(Column { name: cname, ty });
            }
            let heap_first = PageId(read_u32(page0, off));
            off += 4;
            let heap_last = PageId(read_u32(page0, off));
            off += 4;
            let btree_root = PageId(read_u32(page0, off));
            off += 4;
            tables.push(TableMeta {
                name,
                columns,
                heap_first,
                heap_last,
                btree_root,
            });
        }
        Ok(Catalog { tables })
    }

    pub fn store(&self, page0: &mut [u8]) -> Result<()> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.tables.len() as u16).to_le_bytes());
        for t in &self.tables {
            write_string(&mut buf, &t.name)?;
            buf.extend_from_slice(&(t.columns.len() as u16).to_le_bytes());
            for c in &t.columns {
                write_string(&mut buf, &c.name)?;
                buf.push(c.ty.to_u8());
            }
            buf.extend_from_slice(&t.heap_first.0.to_le_bytes());
            buf.extend_from_slice(&t.heap_last.0.to_le_bytes());
            buf.extend_from_slice(&t.btree_root.0.to_le_bytes());
        }
        if CATALOG_OFFSET + buf.len() > PAGE_SIZE {
            return Err(DbError::msg("catalog does not fit on header page"));
        }
        page0[CATALOG_OFFSET..CATALOG_OFFSET + buf.len()].copy_from_slice(&buf);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<&TableMeta> {
        self.tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| DbError::msg(format!("unknown table '{name}'")))
    }

    pub fn get_mut(&mut self, name: &str) -> Result<&mut TableMeta> {
        self.tables
            .iter_mut()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| DbError::msg(format!("unknown table '{name}'")))
    }
}

fn read_string(page: &[u8], off: &mut usize) -> Result<String> {
    if *off + 2 > page.len() {
        return Err(DbError::msg("truncated catalog string"));
    }
    let len = read_u16(page, *off) as usize;
    *off += 2;
    if *off + len > page.len() {
        return Err(DbError::msg("truncated catalog string"));
    }
    let s = std::str::from_utf8(&page[*off..*off + len])
        .map_err(|_| DbError::msg("catalog name is not utf-8"))?
        .to_string();
    *off += len;
    Ok(s)
}

fn write_string(buf: &mut Vec<u8>, s: &str) -> Result<()> {
    if s.len() > u16::MAX as usize {
        return Err(DbError::msg("name too long"));
    }
    buf.extend_from_slice(&(s.len() as u16).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
    Ok(())
}
