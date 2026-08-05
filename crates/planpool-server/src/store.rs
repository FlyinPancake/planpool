use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Serialize, Deserialize, Clone)]
pub struct Meta {
    pub id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub size: u64,
}

impl Meta {
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }
}

/// Flat-directory storage: each plan is `{id}.html` plus a `{id}.json` sidecar
/// holding its expiry. A plan only exists if both files are present and readable.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Store { dir }
    }

    pub async fn init(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir).await
    }

    fn html_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.html"))
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub async fn put(&self, html: &[u8], ttl_secs: u64) -> io::Result<Meta> {
        let id = new_id();
        let now = unix_now();
        let meta = Meta {
            id: id.clone(),
            created_at: now,
            expires_at: now.saturating_add(ttl_secs),
            size: html.len() as u64,
        };
        // Meta first: the html file is the last piece to appear, so a plan is
        // never readable before its expiry is on disk.
        fs::write(self.meta_path(&id), serde_json::to_vec(&meta)?).await?;
        fs::write(self.html_path(&id), html).await?;
        Ok(meta)
    }

    /// Returns the html path for a live plan, deleting it eagerly if expired.
    pub async fn get(&self, id: &str) -> io::Result<Option<(PathBuf, Meta)>> {
        if !valid_id(id) {
            return Ok(None);
        }
        let raw = match fs::read(self.meta_path(id)).await {
            Ok(raw) => raw,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let meta: Meta = match serde_json::from_slice(&raw) {
            Ok(meta) => meta,
            Err(_) => return Ok(None),
        };
        if meta.is_expired(unix_now()) {
            self.remove_files(id).await?;
            return Ok(None);
        }
        Ok(Some((self.html_path(id), meta)))
    }

    /// Returns whether the plan existed.
    pub async fn delete(&self, id: &str) -> io::Result<bool> {
        if !valid_id(id) {
            return Ok(false);
        }
        let existed = fs::try_exists(self.meta_path(id)).await?;
        self.remove_files(id).await?;
        Ok(existed)
    }

    async fn remove_files(&self, id: &str) -> io::Result<()> {
        remove_if_exists(&self.html_path(id)).await?;
        remove_if_exists(&self.meta_path(id)).await
    }

    /// Deletes every expired plan; returns how many were removed.
    pub async fn sweep(&self) -> io::Result<usize> {
        let now = unix_now();
        let mut removed = 0;
        let mut entries = fs::read_dir(&self.dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !valid_id(id) {
                continue;
            }
            let expired = match fs::read(&path).await {
                Ok(raw) => match serde_json::from_slice::<Meta>(&raw) {
                    Ok(meta) => meta.is_expired(now),
                    // Unparseable sidecar: reclaim it rather than keep it forever.
                    Err(_) => true,
                },
                Err(_) => continue,
            };
            if expired {
                self.remove_files(id).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

async fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// 128-bit random ID as 32 hex chars — unguessable, so the URL acts as the
/// view capability. Also the only shape `get`/`delete` will touch on disk,
/// which rules out path traversal.
fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_valid_and_unique() {
        let a = new_id();
        let b = new_id();
        assert!(valid_id(&a));
        assert!(valid_id(&b));
        assert_ne!(a, b);
    }

    #[test]
    fn rejects_traversal_shaped_ids() {
        assert!(!valid_id(""));
        assert!(!valid_id("../../etc/passwd"));
        assert!(!valid_id("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
        assert!(!valid_id("0123456789abcdef0123456789abcde")); // 31 chars
    }

    #[tokio::test]
    async fn put_get_delete_roundtrip() {
        let dir = std::env::temp_dir().join(format!("planpool-test-{}", new_id()));
        let store = Store::new(dir.clone());
        store.init().await.unwrap();

        let meta = store.put(b"<h1>plan</h1>", 60).await.unwrap();
        let (path, _) = store
            .get(&meta.id)
            .await
            .unwrap()
            .expect("plan should exist");
        assert_eq!(fs::read(&path).await.unwrap(), b"<h1>plan</h1>");

        assert!(store.delete(&meta.id).await.unwrap());
        assert!(store.get(&meta.id).await.unwrap().is_none());
        assert!(!store.delete(&meta.id).await.unwrap());

        fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn expired_plans_are_gone_and_swept() {
        let dir = std::env::temp_dir().join(format!("planpool-test-{}", new_id()));
        let store = Store::new(dir.clone());
        store.init().await.unwrap();

        let live = store.put(b"live", 60).await.unwrap();
        let dead = store.put(b"dead", 0).await.unwrap();

        assert!(store.get(&dead.id).await.unwrap().is_none());
        assert_eq!(store.sweep().await.unwrap(), 0); // get() already reclaimed it

        let dead2 = store.put(b"dead2", 0).await.unwrap();
        assert_eq!(store.sweep().await.unwrap(), 1);
        assert!(store.get(&dead2.id).await.unwrap().is_none());
        assert!(store.get(&live.id).await.unwrap().is_some());

        fs::remove_dir_all(&dir).await.unwrap();
    }
}
