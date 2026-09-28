use crate::error::{DbError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    Create,
    Table,
    Insert,
    Into,
    Values,
    Select,
    From,
    Where,
    Int,
    Text,
}

impl Keyword {
    pub fn from_ident(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("CREATE") {
            Some(Keyword::Create)
        } else if s.eq_ignore_ascii_case("TABLE") {
            Some(Keyword::Table)
        } else if s.eq_ignore_ascii_case("INSERT") {
            Some(Keyword::Insert)
        } else if s.eq_ignore_ascii_case("INTO") {
            Some(Keyword::Into)
        } else if s.eq_ignore_ascii_case("VALUES") {
            Some(Keyword::Values)
        } else if s.eq_ignore_ascii_case("SELECT") {
            Some(Keyword::Select)
        } else if s.eq_ignore_ascii_case("FROM") {
            Some(Keyword::From)
        } else if s.eq_ignore_ascii_case("WHERE") {
            Some(Keyword::Where)
        } else if s.eq_ignore_ascii_case("INT") {
            Some(Keyword::Int)
        } else if s.eq_ignore_ascii_case("TEXT") {
            Some(Keyword::Text)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Keyword::Create => "CREATE",
            Keyword::Table => "TABLE",
            Keyword::Insert => "INSERT",
            Keyword::Into => "INTO",
            Keyword::Values => "VALUES",
            Keyword::Select => "SELECT",
            Keyword::From => "FROM",
            Keyword::Where => "WHERE",
            Keyword::Int => "INT",
            Keyword::Text => "TEXT",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Keyword(Keyword),
    Ident(String),
    Number(i64),
    String(String),
    Star,
    Comma,
    LParen,
    RParen,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Semicolon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spanned {
    pub token: Token,
    pub pos: usize,
}

pub fn tokenize(input: &str) -> Result<Vec<Spanned>> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let pos = i;
        match c {
            '*' => {
                tokens.push(Spanned {
                    token: Token::Star,
                    pos,
                });
                i += 1;
            }
            ',' => {
                tokens.push(Spanned {
                    token: Token::Comma,
                    pos,
                });
                i += 1;
            }
            '(' => {
                tokens.push(Spanned {
                    token: Token::LParen,
                    pos,
                });
                i += 1;
            }
            ')' => {
                tokens.push(Spanned {
                    token: Token::RParen,
                    pos,
                });
                i += 1;
            }
            ';' => {
                tokens.push(Spanned {
                    token: Token::Semicolon,
                    pos,
                });
                i += 1;
            }
            '=' => {
                tokens.push(Spanned {
                    token: Token::Eq,
                    pos,
                });
                i += 1;
            }
            '<' => {
                let token = if i + 1 < chars.len() && chars[i + 1] == '=' {
                    i += 2;
                    Token::Le
                } else if i + 1 < chars.len() && chars[i + 1] == '>' {
                    i += 2;
                    Token::Ne
                } else {
                    i += 1;
                    Token::Lt
                };
                tokens.push(Spanned { token, pos });
            }
            '>' => {
                let token = if i + 1 < chars.len() && chars[i + 1] == '=' {
                    i += 2;
                    Token::Ge
                } else {
                    i += 1;
                    Token::Gt
                };
                tokens.push(Spanned { token, pos });
            }
            '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::Ne,
                    pos,
                });
                i += 2;
            }
            '\'' => {
                i += 1;
                let mut s = String::new();
                loop {
                    if i >= chars.len() {
                        return Err(DbError::parse(pos, "unterminated string"));
                    }
                    if chars[i] == '\'' {
                        if i + 1 < chars.len() && chars[i + 1] == '\'' {
                            s.push('\'');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    s.push(chars[i]);
                    i += 1;
                }
                tokens.push(Spanned {
                    token: Token::String(s),
                    pos,
                });
            }
            _ if c.is_ascii_digit()
                || (c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) =>
            {
                let start = i;
                if c == '-' {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let n: i64 = chars[start..i]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .map_err(|_| DbError::parse(pos, "invalid number"))?;
                tokens.push(Spanned {
                    token: Token::Number(n),
                    pos,
                });
            }
            _ if is_ident_start(c) => {
                let start = i;
                i += 1;
                while i < chars.len() && is_ident_cont(chars[i]) {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let token = match Keyword::from_ident(&s) {
                    Some(kw) => Token::Keyword(kw),
                    None => Token::Ident(s),
                };
                tokens.push(Spanned { token, pos });
            }
            _ => {
                return Err(DbError::parse(
                    pos,
                    format!("unexpected character '{c}'"),
                ));
            }
        }
    }
    Ok(tokens)
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_cont(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(sql: &str) -> Vec<Token> {
        tokenize(sql)
            .unwrap()
            .into_iter()
            .map(|s| s.token)
            .collect()
    }

    #[test]
    fn tokenize_select() {
        let t = kinds("SELECT * FROM t WHERE id >= 2");
        assert_eq!(t[0], Token::Keyword(Keyword::Select));
        assert_eq!(t[1], Token::Star);
        assert_eq!(t[2], Token::Keyword(Keyword::From));
        assert_eq!(t[3], Token::Ident("t".into()));
        assert_eq!(t[4], Token::Keyword(Keyword::Where));
        assert_eq!(t[5], Token::Ident("id".into()));
        assert_eq!(t[6], Token::Ge);
        assert_eq!(t[7], Token::Number(2));
    }

    #[test]
    fn tokenize_ops_and_string_escape() {
        let t = kinds("a <> b != c <= d 'it''s'");
        assert_eq!(t[1], Token::Ne);
        assert_eq!(t[3], Token::Ne);
        assert_eq!(t[5], Token::Le);
        assert_eq!(t[7], Token::String("it's".into()));
    }

    #[test]
    fn tokenize_negative_and_error() {
        assert_eq!(kinds("-12")[0], Token::Number(-12));
        let err = tokenize("id @ 1").unwrap_err();
        match err {
            DbError::Parse { pos, msg } => {
                assert_eq!(pos, 3);
                assert!(msg.contains('@'));
            }
            other => panic!("expected parse error, got {other}"),
        }
        assert!(tokenize("'nope").is_err());
    }
}
