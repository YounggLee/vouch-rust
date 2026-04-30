use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn from_env() -> Self {
        let dir =
            std::env::var("VOUCH_CACHE_DIR").unwrap_or_else(|_| "fixtures/responses".to_string());
        Self {
            dir: PathBuf::from(dir),
        }
    }

    fn key(&self, stage: &str, payload: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(payload.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        format!("{}.{}.json", stage, &hash[..16])
    }

    pub fn load(&self, stage: &str, payload: &str) -> Option<serde_json::Value> {
        let path = self.dir.join(self.key(stage, payload));
        if let Ok(content) = fs::read_to_string(&path) {
            return serde_json::from_str(&content).ok();
        }
        let fallback = self.dir.join(format!("{}.json", stage));
        if let Ok(content) = fs::read_to_string(&fallback) {
            return serde_json::from_str(&content).ok();
        }
        None
    }

    pub fn save(&self, stage: &str, payload: &str, result: &serde_json::Value) {
        fs::create_dir_all(&self.dir).ok();
        let path = self.dir.join(self.key(stage, payload));
        let json = serde_json::to_string_pretty(result).unwrap();
        fs::write(path, json).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_then_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let data = serde_json::json!({"a": 1, "b": [2, 3]});
        cache.save("stage_x", "payload-1", &data);
        let loaded = cache.load("stage_x", "payload-1");
        assert_eq!(loaded, Some(data));
    }

    #[test]
    fn load_miss_returns_none() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        assert_eq!(cache.load("stage_x", "missing"), None);
    }

    #[test]
    fn fallback_to_stage_json() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        std::fs::write(dir.path().join("stage_x.json"), r#"[{"id": "fallback"}]"#).unwrap();
        let loaded = cache.load("stage_x", "any-payload");
        assert_eq!(loaded, Some(serde_json::json!([{"id": "fallback"}])));
    }

    #[test]
    fn hashed_key_takes_precedence_over_fallback() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        cache.save("stage_x", "payload-A", &serde_json::json!({"hashed": true}));
        std::fs::write(dir.path().join("stage_x.json"), r#"{"hashed": false}"#).unwrap();
        let loaded = cache.load("stage_x", "payload-A");
        assert_eq!(loaded, Some(serde_json::json!({"hashed": true})));
    }
}
