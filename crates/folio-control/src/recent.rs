//! Recently opened files (`recent.json` in the config folder), newest first.

use std::path::{Path, PathBuf};

const MAX: usize = 30;

fn file(config: &Path) -> PathBuf {
    config.join("recent.json")
}

pub fn list(config: &Path) -> Vec<PathBuf> {
    std::fs::read(file(config)).ok().and_then(|b| serde_json::from_slice::<Vec<PathBuf>>(&b).ok()).unwrap_or_default()
}

pub fn add(config: &Path, path: &Path) {
    let mut l = list(config);
    l.retain(|p| p != path);
    l.insert(0, path.to_path_buf());
    l.truncate(MAX);
    save(config, &l);
}

pub fn remove(config: &Path, path: &Path) {
    let mut l = list(config);
    l.retain(|p| p != path);
    save(config, &l);
}

fn save(config: &Path, l: &[PathBuf]) {
    if let Ok(json) = serde_json::to_vec_pretty(l) {
        let _ = std::fs::create_dir_all(config);
        let tmp = config.join("recent.json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, file(config));
        }
    }
}
