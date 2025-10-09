use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use reqwest::Url;

#[derive(Clone)]
pub struct MirrorPool {
    urls: Arc<Vec<Url>>,
    cursor: Arc<AtomicUsize>,
}

impl MirrorPool {
    pub fn new(urls: Vec<Url>) -> Self {
        assert!(!urls.is_empty(), "at least one URL required");
        Self {
            cursor: Arc::new(AtomicUsize::new(0)),
            urls: Arc::new(urls),
        }
    }

    pub fn next(&self) -> Url {
        let idx = self.cursor.fetch_add(1, Ordering::Relaxed);
        let urls = self.urls.as_ref();
        urls[idx % urls.len()].clone()
    }

    pub fn primary(&self) -> Url {
        self.urls[0].clone()
    }

    pub fn all(&self) -> Vec<Url> {
        self.urls.as_ref().clone()
    }
}
