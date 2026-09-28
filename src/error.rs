use std::fmt;
use std::io;

#[derive(Debug)]
pub enum DbError {
    Io(io::Error),
    Msg(String),
    Parse { pos: usize, msg: String },
}

impl DbError {
    pub fn msg(m: impl Into<String>) -> Self {
        DbError::Msg(m.into())
    }

    pub fn parse(pos: usize, m: impl Into<String>) -> Self {
        DbError::Parse {
            pos,
            msg: m.into(),
        }
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Io(e) => write!(f, "I/O error: {e}"),
            DbError::Msg(m) => write!(f, "{m}"),
            DbError::Parse { pos, msg } => write!(f, "parse error at {pos}: {msg}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<io::Error> for DbError {
    fn from(e: io::Error) -> Self {
        DbError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, DbError>;
