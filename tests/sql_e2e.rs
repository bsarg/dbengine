use dbengine::sql::ast::Value;
use dbengine::test_util::temp_db_path;
use dbengine::{Database, LruPolicy, QueryResult};

fn rows_of(result: QueryResult) -> Vec<Vec<Value>> {
    match result {
        QueryResult::Rows { rows, .. } => rows,
        QueryResult::Ok => panic!("expected rows"),
    }
}

#[test]
fn sql_e2e_create_insert_select_reopen() {
    let path = temp_db_path("sql-e2e");
    {
        let mut db = Database::<LruPolicy>::open(&path, 32).unwrap();
        db.execute("CREATE TABLE t (id INT, name TEXT)").unwrap();
        db.execute("INSERT INTO t VALUES (1, 'ada')").unwrap();
        db.execute("INSERT INTO t VALUES (2, 'grace')").unwrap();
        db.execute("INSERT INTO t VALUES (3, 'alan')").unwrap();
        let all = rows_of(db.execute("SELECT * FROM t").unwrap());
        assert_eq!(all.len(), 3);
        let one = rows_of(db.execute("SELECT * FROM t WHERE id = 2").unwrap());
        assert_eq!(
            one,
            vec![vec![Value::Int(2), Value::Text("grace".into())]]
        );
        let range = rows_of(db.execute("SELECT * FROM t WHERE id >= 2").unwrap());
        assert_eq!(range.len(), 2);
        let by_name = rows_of(db.execute("SELECT * FROM t WHERE name = 'ada'").unwrap());
        assert_eq!(by_name.len(), 1);
        assert!(db.execute("INSERT INTO t VALUES ('nope', 1)").is_err());
        assert!(db.execute("SELECT * FROM missing").is_err());
        db.flush().unwrap();
    }
    let mut db = Database::<LruPolicy>::open(&path, 32).unwrap();
    let one = rows_of(db.execute("SELECT * FROM t WHERE id = 1").unwrap());
    assert_eq!(one, vec![vec![Value::Int(1), Value::Text("ada".into())]]);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn sql_e2e_many_rows_reopen() {
    let path = temp_db_path("sql-many");
    let n = 80i64;
    {
        let mut db = Database::<LruPolicy>::open(&path, 16).unwrap();
        db.execute("CREATE TABLE t (id INT, note TEXT)").unwrap();
        for i in 0..n {
            let note = format!("row-{i}-{}", "x".repeat(80));
            db.execute(&format!("INSERT INTO t VALUES ({i}, '{note}')"))
                .unwrap();
        }
        assert_eq!(rows_of(db.execute("SELECT * FROM t").unwrap()).len(), n as usize);
        assert_eq!(
            rows_of(db.execute("SELECT * FROM t WHERE id = 50").unwrap()).len(),
            1
        );
        assert_eq!(
            rows_of(db.execute("SELECT * FROM t WHERE id < 10").unwrap()).len(),
            10
        );
        db.flush().unwrap();
    }
    let mut db = Database::<LruPolicy>::open(&path, 8).unwrap();
    assert_eq!(rows_of(db.execute("SELECT * FROM t").unwrap()).len(), n as usize);
    let row = rows_of(db.execute("SELECT * FROM t WHERE id = 79").unwrap());
    assert_eq!(row[0][0], Value::Int(79));
    let _ = std::fs::remove_file(&path);
}
