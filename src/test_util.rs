use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn temp_db_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dbengine-{}-{}-{}.db",
        label,
        std::process::id(),
        nanos
    ))
}
