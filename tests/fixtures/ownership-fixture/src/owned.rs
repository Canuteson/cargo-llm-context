//! Types that own heap-allocated data.
//!
//! ## Ownership
//! All types in this module are sole owners of their data. Cloning is a deep copy.

use std::collections::HashMap;

pub struct Buffer {
    pub data: Vec<u8>,
    pub label: String,
}

pub struct Registry {
    entries: HashMap<String, Vec<u8>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    pub fn insert(&mut self, key: String, value: Vec<u8>) {
        self.entries.insert(key, value);
    }
}
