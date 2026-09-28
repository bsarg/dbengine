use dbengine::eviction::LruPolicy;
use dbengine::index::BPlusTree;
use dbengine::storage::buffer::BufferPool;
use dbengine::storage::page::{PageId, Rid};
use dbengine::test_util::temp_db_path;

fn shuffle(keys: &mut [i64]) {
    let mut state: u64 = 0xa5a5_5a5a_1234_fedc;
    for i in (1..keys.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (state as usize) % (i + 1);
        keys.swap(i, j);
    }
}

#[test]
fn btree_random_and_range() {
    let path = temp_db_path("bt-int");
    let mut pool = BufferPool::<LruPolicy>::open(&path, 64).unwrap();
    let mut t = BPlusTree::create(&mut pool).unwrap();
    let mut keys: Vec<i64> = (0..80).collect();
    shuffle(&mut keys);
    for (i, k) in keys.iter().enumerate() {
        t.insert(
            &mut pool,
            *k,
            Rid {
                page: PageId(1),
                slot: i as u16,
            },
        )
        .unwrap();
    }
    for k in 0..80 {
        assert!(!t.get(&mut pool, k).unwrap().is_empty(), "missing {k}");
    }
    let r = t.range(&mut pool, 10, true, 20, true).unwrap();
    assert_eq!(r.len(), 11);
    assert!(t.height(&mut pool).unwrap() >= 2);
    let _ = std::fs::remove_file(&path);
}
