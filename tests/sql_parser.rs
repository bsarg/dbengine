use dbengine::catalog::ColType;
use dbengine::sql::ast::{CmpOp, Statement, Value};
use dbengine::sql::parse;
use dbengine::sql::token::{tokenize, Keyword, Token};

#[test]
fn golden_create_insert_select() {
    match parse("CREATE TABLE t (id INT, name TEXT)").unwrap() {
        Statement::CreateTable { name, columns } => {
            assert_eq!(name, "t");
            assert_eq!(columns[0].name, "id");
            assert_eq!(columns[0].ty, ColType::Int);
            assert_eq!(columns[1].name, "name");
            assert_eq!(columns[1].ty, ColType::Text);
        }
        other => panic!("{other:?}"),
    }

    match parse("INSERT INTO t VALUES (-4, 'it''s')").unwrap() {
        Statement::Insert { table, values } => {
            assert_eq!(table, "t");
            assert_eq!(
                values,
                vec![Value::Int(-4), Value::Text("it's".into())]
            );
        }
        other => panic!("{other:?}"),
    }

    match parse("SELECT * FROM t WHERE name >= 'a';").unwrap() {
        Statement::Select {
            table,
            where_clause,
        } => {
            assert_eq!(table, "t");
            let p = where_clause.unwrap();
            assert_eq!(p.column, "name");
            assert_eq!(p.op, CmpOp::Ge);
            assert_eq!(p.value, Value::Text("a".into()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn tokenizer_keywords_are_not_idents() {
    let tokens: Vec<Token> = tokenize("CREATE TABLE t (id INT)")
        .unwrap()
        .into_iter()
        .map(|s| s.token)
        .collect();
    assert_eq!(tokens[0], Token::Keyword(Keyword::Create));
    assert_eq!(tokens[1], Token::Keyword(Keyword::Table));
    assert_eq!(tokens[2], Token::Ident("t".into()));
}
