use std::path::Path;

use crate::catalog::{Catalog, Column, TableMeta};
use crate::error::{DbError, Result};
use crate::eviction::EvictionPolicy;
use crate::exec::row::{decode_row, encode_row, matches_pred};
use crate::index::BPlusTree;
use crate::sql::ast::{CmpOp, Statement, Value};
use crate::sql::parse;
use crate::storage::buffer::BufferPool;
use crate::storage::heap::HeapFile;
use crate::storage::page::{HEADER_PAGE, Rid};

#[derive(Clone, Debug)]
pub enum QueryResult {
    Ok,
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
}

impl QueryResult {
    pub fn format(&self) -> String {
        match self {
            QueryResult::Ok => "ok".to_string(),
            QueryResult::Rows { columns, rows } => {
                let mut s = columns.join(" | ");
                s.push('\n');
                for row in rows {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|v| match v {
                            Value::Int(n) => n.to_string(),
                            Value::Text(t) => t.clone(),
                        })
                        .collect();
                    s.push_str(&cells.join(" | "));
                    s.push('\n');
                }
                s.push_str(&format!("({} rows)", rows.len()));
                s
            }
        }
    }
}

pub struct Database<P: EvictionPolicy> {
    pub pool: BufferPool<P>,
    catalog: Catalog,
}

impl<P: EvictionPolicy> Database<P> {
    pub fn open(path: impl AsRef<Path>, capacity: usize) -> Result<Self> {
        let mut pool = BufferPool::open(path, capacity)?;
        let page0 = pool.load_page(HEADER_PAGE)?;
        let catalog = Catalog::load(&page0)?;
        Ok(Database { pool, catalog })
    }

    pub fn execute(&mut self, sql: &str) -> Result<QueryResult> {
        match parse(sql)? {
            Statement::CreateTable { name, columns } => {
                if self.catalog.tables.iter().any(|t| t.name.eq_ignore_ascii_case(&name)) {
                    return Err(DbError::msg(format!("table '{name}' already exists")));
                }
                let heap = HeapFile::create(&mut self.pool)?;
                let tree = BPlusTree::create(&mut self.pool)?;
                self.catalog.tables.push(TableMeta {
                    name,
                    columns: columns
                        .into_iter()
                        .map(|c| Column {
                            name: c.name,
                            ty: c.ty,
                        })
                        .collect(),
                    heap_first: heap.first,
                    heap_last: heap.last,
                    btree_root: tree.root,
                });
                self.save_catalog()?;
                Ok(QueryResult::Ok)
            }
            Statement::Insert { table, values } => {
                self.insert_row(&table, values)?;
                Ok(QueryResult::Ok)
            }
            Statement::Select {
                table,
                where_clause,
            } => self.select_rows(&table, where_clause),
        }
    }

    fn insert_row(&mut self, table: &str, values: Vec<Value>) -> Result<()> {
        let meta = self.catalog.get(table)?.clone();
        if values.len() != meta.columns.len() {
            return Err(DbError::msg("INSERT value count does not match columns"));
        }
        for (c, v) in meta.columns.iter().zip(values.iter()) {
            match (c.ty, v) {
                (crate::catalog::ColType::Int, Value::Int(_)) => {}
                (crate::catalog::ColType::Text, Value::Text(_)) => {}
                _ => {
                    return Err(DbError::msg(format!(
                        "type mismatch for column '{}'",
                        c.name
                    )))
                }
            }
        }
        let idx = meta
            .index_col()
            .ok_or_else(|| DbError::msg("table has no INT column to index"))?;
        let key = values[idx]
            .as_int()
            .ok_or_else(|| DbError::msg("index key is not INT"))?;
        let bytes = encode_row(&values)?;
        let mut heap = meta.heap();
        let rid = heap.insert(&mut self.pool, &bytes)?;
        let mut tree = BPlusTree {
            root: meta.btree_root,
        };
        tree.insert(&mut self.pool, key, rid)?;
        let t = self.catalog.get_mut(table)?;
        t.heap_last = heap.last;
        t.btree_root = tree.root;
        self.save_catalog()?;
        Ok(())
    }

    fn select_rows(
        &mut self,
        table: &str,
        where_clause: Option<crate::sql::ast::Predicate>,
    ) -> Result<QueryResult> {
        let meta = self.catalog.get(table)?.clone();
        let columns: Vec<String> = meta.columns.iter().map(|c| c.name.clone()).collect();
        let rids = self.plan_rids(&meta, where_clause.as_ref())?;
        let heap = meta.heap();
        let mut rows = Vec::new();
        for rid in rids {
            let bytes = heap.get(&mut self.pool, rid)?;
            let row = decode_row(&meta.columns, &bytes)?;
            if let Some(pred) = where_clause.as_ref() {
                if !matches_pred(&row, &meta.columns, pred)? {
                    continue;
                }
            }
            rows.push(row);
        }
        Ok(QueryResult::Rows { columns, rows })
    }

    fn plan_rids(
        &mut self,
        meta: &TableMeta,
        pred: Option<&crate::sql::ast::Predicate>,
    ) -> Result<Vec<Rid>> {
        let heap = meta.heap();
        let Some(pred) = pred else {
            return Ok(heap.scan(&mut self.pool)?.into_iter().map(|(r, _)| r).collect());
        };
        let idx_col = meta.index_col();
        let pred_col = meta
            .columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(&pred.column));
        let use_index = matches!((idx_col, pred_col, &pred.value), (Some(i), Some(j), Value::Int(_)) if i == j);
        if !use_index {
            return Ok(heap.scan(&mut self.pool)?.into_iter().map(|(r, _)| r).collect());
        }
        let key = pred.value.as_int().unwrap();
        let tree = BPlusTree {
            root: meta.btree_root,
        };
        match pred.op {
            CmpOp::Eq => tree.get(&mut self.pool, key),
            CmpOp::Lt => tree.range(&mut self.pool, i64::MIN, true, key, false),
            CmpOp::Le => tree.range(&mut self.pool, i64::MIN, true, key, true),
            CmpOp::Gt => tree.range(&mut self.pool, key, false, i64::MAX, true),
            CmpOp::Ge => tree.range(&mut self.pool, key, true, i64::MAX, true),
            CmpOp::Ne => Ok(heap.scan(&mut self.pool)?.into_iter().map(|(r, _)| r).collect()),
        }
    }

    fn save_catalog(&mut self) -> Result<()> {
        let f = self.pool.pin(HEADER_PAGE)?;
        {
            let buf = self.pool.data_mut(f);
            self.catalog.store(buf)?;
        }
        self.pool.unpin(f, true);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.save_catalog()?;
        self.pool.flush_all()
    }
}
