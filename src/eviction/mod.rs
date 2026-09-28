pub mod clock;
pub mod lfu;
pub mod lru;

pub use clock::ClockPolicy;
pub use lfu::LfuPolicy;
pub use lru::LruPolicy;

/// Buffer-pool eviction policy. Static dispatch via `BufferPool<P>`.
///
/// `pinned[frame]` is the pin count for that frame. A frame with pin count > 0
/// must never be chosen as a victim.
pub trait EvictionPolicy {
    fn new(capacity: usize) -> Self;
    fn on_insert(&mut self, frame: usize);
    fn on_access(&mut self, frame: usize);
    fn on_remove(&mut self, frame: usize);
    /// Victim among unpinned frames; `None` if all pinned.
    fn evict(&mut self, pinned: &[u32]) -> Option<usize>;
}
