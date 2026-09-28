use crate::catalog::{ColType, Column};
use crate::error::{DbError, Result};
use crate::sql::ast::Value;

pub fn encode_row(values: &[Value]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    for v in values {
        match v {
            Value::Int(n) => buf.extend_from_slice(&n.to_le_bytes()),
            Value::Text(s) => {
                if s.len() > u16::MAX as usize {
                    return Err(DbError::msg("TEXT value too long"));
                }
                buf.extend_from_slice(&(s.len() as u16).to_le_bytes());
                buf.extend_from_slice(s.as_bytes());
            }
        }
    }
    Ok(buf)
}

pub fn decode_row(columns: &[Column], mut bytes: &[u8]) -> Result<Vec<Value>> {
    let mut out = Vec::with_capacity(columns.len());
    for col in columns {
        match col.ty {
            ColType::Int => {
                if bytes.len() < 8 {
                    return Err(DbError::msg("truncated INT"));
                }
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes[..8]);
                out.push(Value::Int(i64::from_le_bytes(arr)));
                bytes = &bytes[8..];
            }
            ColType::Text => {
                if bytes.len() < 2 {
                    return Err(DbError::msg("truncated TEXT"));
                }
                let len = u16::from_le_bytes(bytes[..2].try_into().unwrap()) as usize;
                bytes = &bytes[2..];
                if bytes.len() < len {
                    return Err(DbError::msg("truncated TEXT body"));
                }
                let s = std::str::from_utf8(&bytes[..len])
                    .map_err(|_| DbError::msg("TEXT is not utf-8"))?
                    .to_string();
                out.push(Value::Text(s));
                bytes = &bytes[len..];
            }
        }
    }
    Ok(out)
}

pub fn matches_pred(row: &[Value], columns: &[Column], pred: &crate::sql::ast::Predicate) -> Result<bool> {
    let idx = columns
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(&pred.column))
        .ok_or_else(|| DbError::msg(format!("unknown column '{}'", pred.column)))?;
    let left = &row[idx];
    Ok(match (left, &pred.value) {
        (Value::Int(a), Value::Int(b)) => pred.op.eval_int(*a, *b),
        (Value::Text(a), Value::Text(b)) => pred.op.eval_text(a, b),
        _ => false,
    })
}
