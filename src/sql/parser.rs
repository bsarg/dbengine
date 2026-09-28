use crate::catalog::ColType;
use crate::error::{DbError, Result};
use crate::sql::ast::{CmpOp, ColumnDef, Predicate, Statement, Value};
use crate::sql::token::{tokenize, Keyword, Spanned, Token};

pub fn parse(sql: &str) -> Result<Statement> {
    let tokens = tokenize(sql)?;
    let mut p = Parser { tokens, i: 0 };
    let stmt = p.statement()?;
    p.eat_semicolons();
    if p.i != p.tokens.len() {
        return Err(p.error("unexpected tokens after statement"));
    }
    Ok(stmt)
}

struct Parser {
    tokens: Vec<Spanned>,
    i: usize,
}

impl Parser {
    fn pos(&self) -> usize {
        self.tokens
            .get(self.i)
            .map(|t| t.pos)
            .or_else(|| self.tokens.last().map(|t| t.pos))
            .unwrap_or(0)
    }

    fn error(&self, msg: impl Into<String>) -> DbError {
        DbError::parse(self.pos(), msg)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.i).map(|s| &s.token)
    }

    fn bump(&mut self) -> Option<Token> {
        if self.i < self.tokens.len() {
            let t = self.tokens[self.i].token.clone();
            self.i += 1;
            Some(t)
        } else {
            None
        }
    }

    fn eat_semicolons(&mut self) {
        while matches!(self.peek(), Some(Token::Semicolon)) {
            self.i += 1;
        }
    }

    fn ident(&mut self) -> Result<String> {
        let pos = self.pos();
        match self.bump() {
            Some(Token::Ident(s)) => Ok(s),
            _ => Err(DbError::parse(pos, "expected identifier")),
        }
    }

    fn expect_kw(&mut self, kw: Keyword) -> Result<()> {
        let pos = self.pos();
        match self.bump() {
            Some(Token::Keyword(got)) if got == kw => Ok(()),
            Some(other) => Err(DbError::parse(
                pos,
                format!("expected {}, got {other:?}", kw.as_str()),
            )),
            None => Err(DbError::parse(
                pos,
                format!("expected {}, got EOF", kw.as_str()),
            )),
        }
    }

    fn expect(&mut self, want: Token) -> Result<()> {
        let pos = self.pos();
        match self.bump() {
            Some(t) if t == want => Ok(()),
            Some(t) => Err(DbError::parse(
                pos,
                format!("expected {want:?}, got {t:?}"),
            )),
            None => Err(DbError::parse(pos, format!("expected {want:?}, got EOF"))),
        }
    }

    fn statement(&mut self) -> Result<Statement> {
        match self.bump() {
            Some(Token::Keyword(Keyword::Create)) => self.create_table(),
            Some(Token::Keyword(Keyword::Insert)) => self.insert(),
            Some(Token::Keyword(Keyword::Select)) => self.select(),
            Some(other) => Err(self.error(format!("unknown statement {other:?}"))),
            None => Err(self.error("empty statement")),
        }
    }

    fn create_table(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Table)?;
        let name = self.ident()?;
        self.expect(Token::LParen)?;
        let mut columns = Vec::new();
        loop {
            let cname = self.ident()?;
            let ty = match self.bump() {
                Some(Token::Keyword(Keyword::Int)) => ColType::Int,
                Some(Token::Keyword(Keyword::Text)) => ColType::Text,
                _ => return Err(self.error("expected INT or TEXT")),
            };
            columns.push(ColumnDef { name: cname, ty });
            match self.peek() {
                Some(Token::Comma) => {
                    self.i += 1;
                }
                Some(Token::RParen) => {
                    self.i += 1;
                    break;
                }
                _ => return Err(self.error("expected ',' or ')' in CREATE TABLE")),
            }
        }
        if columns.is_empty() {
            return Err(self.error("CREATE TABLE needs at least one column"));
        }
        if !columns.iter().any(|c| c.ty == ColType::Int) {
            return Err(self.error(
                "CREATE TABLE needs at least one INT column (used as the index key)",
            ));
        }
        Ok(Statement::CreateTable { name, columns })
    }

    fn insert(&mut self) -> Result<Statement> {
        self.expect_kw(Keyword::Into)?;
        let table = self.ident()?;
        self.expect_kw(Keyword::Values)?;
        self.expect(Token::LParen)?;
        let mut values = Vec::new();
        loop {
            values.push(self.value()?);
            match self.peek() {
                Some(Token::Comma) => self.i += 1,
                Some(Token::RParen) => {
                    self.i += 1;
                    break;
                }
                _ => return Err(self.error("expected ',' or ')' in VALUES")),
            }
        }
        Ok(Statement::Insert { table, values })
    }

    fn select(&mut self) -> Result<Statement> {
        self.expect(Token::Star)?;
        self.expect_kw(Keyword::From)?;
        let table = self.ident()?;
        let where_clause = if matches!(self.peek(), Some(Token::Keyword(Keyword::Where))) {
            self.i += 1;
            let column = self.ident()?;
            let op = self.cmp_op()?;
            let value = self.value()?;
            Some(Predicate { column, op, value })
        } else {
            None
        };
        Ok(Statement::Select {
            table,
            where_clause,
        })
    }

    fn cmp_op(&mut self) -> Result<CmpOp> {
        match self.bump() {
            Some(Token::Eq) => Ok(CmpOp::Eq),
            Some(Token::Ne) => Ok(CmpOp::Ne),
            Some(Token::Lt) => Ok(CmpOp::Lt),
            Some(Token::Le) => Ok(CmpOp::Le),
            Some(Token::Gt) => Ok(CmpOp::Gt),
            Some(Token::Ge) => Ok(CmpOp::Ge),
            _ => Err(self.error("expected comparison operator")),
        }
    }

    fn value(&mut self) -> Result<Value> {
        match self.bump() {
            Some(Token::Number(n)) => Ok(Value::Int(n)),
            Some(Token::String(s)) => Ok(Value::Text(s)),
            _ => Err(self.error("expected literal")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::ast::{CmpOp, Statement, Value};

    #[test]
    fn parse_create_insert_select() {
        let s = parse("CREATE TABLE t (id INT, name TEXT)").unwrap();
        match s {
            Statement::CreateTable { name, columns } => {
                assert_eq!(name, "t");
                assert_eq!(columns.len(), 2);
                assert_eq!(columns[0].ty, ColType::Int);
                assert_eq!(columns[1].ty, ColType::Text);
            }
            _ => panic!("wrong stmt"),
        }
        let s = parse("INSERT INTO t VALUES (1, 'ada')").unwrap();
        match s {
            Statement::Insert { values, .. } => {
                assert_eq!(values, vec![Value::Int(1), Value::Text("ada".into())]);
            }
            _ => panic!("wrong stmt"),
        }
        let s = parse("SELECT * FROM t WHERE id = 1").unwrap();
        match s {
            Statement::Select { where_clause, .. } => {
                let p = where_clause.unwrap();
                assert_eq!(p.column, "id");
                assert_eq!(p.op, CmpOp::Eq);
                assert_eq!(p.value, Value::Int(1));
            }
            _ => panic!("wrong stmt"),
        }
    }

    #[test]
    fn parse_is_case_insensitive_and_allows_semicolon() {
        let a = parse("create table Users (id int, name text);").unwrap();
        let b = parse("CREATE TABLE Users (id INT, name TEXT)").unwrap();
        assert_eq!(a, b);
        let s = parse("select * from Users where name <> 'x'").unwrap();
        match s {
            Statement::Select {
                table,
                where_clause,
            } => {
                assert_eq!(table, "Users");
                assert_eq!(where_clause.unwrap().op, CmpOp::Ne);
            }
            _ => panic!("wrong stmt"),
        }
    }

    #[test]
    fn parse_select_without_where_and_range_ops() {
        assert!(matches!(
            parse("SELECT * FROM t").unwrap(),
            Statement::Select {
                where_clause: None,
                ..
            }
        ));
        for (sql, op) in [
            ("SELECT * FROM t WHERE id < 3", CmpOp::Lt),
            ("SELECT * FROM t WHERE id <= 3", CmpOp::Le),
            ("SELECT * FROM t WHERE id > 3", CmpOp::Gt),
            ("SELECT * FROM t WHERE id >= 3", CmpOp::Ge),
            ("SELECT * FROM t WHERE id != 3", CmpOp::Ne),
        ] {
            match parse(sql).unwrap() {
                Statement::Select { where_clause, .. } => {
                    assert_eq!(where_clause.unwrap().op, op);
                }
                _ => panic!("{sql}"),
            }
        }
    }

    #[test]
    fn parse_rejects_bad_sql() {
        assert!(parse("").is_err());
        assert!(parse("DROP TABLE t").is_err());
        assert!(parse("SELECT id FROM t").is_err());
        assert!(parse("CREATE TABLE t (name TEXT)").is_err());
        assert!(parse("INSERT INTO t VALUES (1) extra").is_err());
        let err = parse("SELECT * FROM").unwrap_err();
        assert!(matches!(err, DbError::Parse { .. }));
    }
}
