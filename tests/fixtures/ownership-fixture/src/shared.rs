//! Types with shared ownership via Arc.

use std::sync::Arc;

pub struct SharedCache {
    inner: Arc<Vec<u8>>,
}

pub struct NodeRef {
    node: Arc<dyn std::fmt::Debug>,
}
