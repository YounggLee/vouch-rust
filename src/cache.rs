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
        let dir = std::env::var("VOUCH_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| PathBuf::from(h).join(".cache/vouch/responses"))
                    .unwrap_or_else(|_| PathBuf::from("fixtures/responses"))
            });
        Self { dir }
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
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn from_env_defaults_to_home_cache() {
        let _g = env_guard();
        let dir = TempDir::new().unwrap();
        let orig_home = std::env::var("HOME").ok();
        std::env::remove_var("VOUCH_CACHE_DIR");
        std::env::set_var("HOME", dir.path());
        let cache = Cache::from_env();
        cache.save("stage", "p", &serde_json::json!(1));
        assert!(dir.path().join(".cache/vouch/responses").exists());
        match orig_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn from_env_respects_override() {
        let _g = env_guard();
        let dir = TempDir::new().unwrap();
        std::env::set_var("VOUCH_CACHE_DIR", dir.path());
        let cache = Cache::from_env();
        cache.save("stage", "p", &serde_json::json!(1));
        std::env::remove_var("VOUCH_CACHE_DIR");
        assert!(dir.path().read_dir().unwrap().count() > 0);
    }

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
