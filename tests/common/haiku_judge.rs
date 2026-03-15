/// Hash-cached judge for semantic output validation.
///
/// When asked to judge `(question, output)`:
/// 1. Hash `SHA256(question + "\n---\n" + output)` → hex key
/// 2. Look up in `tests/haiku_cache.json`
/// 3. Cache hit  → return immediately (no subprocess)
/// 4. Cache miss → call `claude -p <prompt> --model claude-haiku-4-5-20251001`,
///                 parse PASS/FAIL, store in cache, return
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

pub struct HaikuJudge {
    cache: HashMap<String, bool>,
    cache_path: PathBuf,
}

impl HaikuJudge {
    pub fn load() -> Self {
        let cache_path = cache_file_path();
        let cache = if cache_path.exists() {
            let text = std::fs::read_to_string(&cache_path).unwrap_or_default();
            serde_json::from_str::<HashMap<String, bool>>(&text).unwrap_or_default()
        } else {
            HashMap::new()
        };
        HaikuJudge { cache, cache_path }
    }

    /// Judge whether `output` satisfies `question`. Returns true = PASS.
    pub fn judge(&mut self, question: &str, output: &str) -> bool {
        let key = content_hash(question, output);

        if let Some(&cached) = self.cache.get(&key) {
            return cached;
        }

        let result = call_claude_cli(question, output);
        self.cache.insert(key, result);
        self.save();
        result
    }

    fn save(&self) {
        // Re-read the on-disk cache and merge so parallel test runs don't
        // clobber each other's newly written entries.
        let mut on_disk: HashMap<String, bool> = if self.cache_path.exists() {
            let text = std::fs::read_to_string(&self.cache_path).unwrap_or_default();
            serde_json::from_str(&text).unwrap_or_default()
        } else {
            HashMap::new()
        };
        on_disk.extend(self.cache.iter().map(|(k, v)| (k.clone(), *v)));
        let text = serde_json::to_string_pretty(&on_disk).expect("serialize cache");
        std::fs::write(&self.cache_path, text).expect("write haiku cache");
    }
}

fn content_hash(question: &str, output: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(question.as_bytes());
    h.update(b"\n---\n");
    h.update(output.as_bytes());
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

fn cache_file_path() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("tests/haiku_cache.json")
}

fn call_claude_cli(question: &str, output: &str) -> bool {
    if std::env::var("HAIKU_OFFLINE").is_ok() {
        panic!(
            "Haiku cache miss — run haiku tests locally with `claude` on PATH \
             to populate tests/haiku_cache.json, then commit it.\n\
             (Tried to call claude for question: {})",
            question
        );
    }

    let prompt = format!(
        "You are a test oracle for a CLI tool called `key` (SSH key manager).\n\
         Answer with exactly one word: PASS or FAIL — nothing else.\n\n\
         Question: {}\n\n\
         CLI output:\n---\n{}\n---",
        question, output
    );

    let result = Command::new("claude")
        .args(["--model", "claude-haiku-4-5-20251001", "-p", &prompt])
        .output()
        .unwrap_or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                panic!(
                    "Haiku cache miss — run haiku tests locally with `claude` on PATH \
                     to populate tests/haiku_cache.json, then commit it.\n\
                     (Tried to call claude for question: {})",
                    question
                );
            }
            panic!("Failed to run claude: {}", e);
        });

    let text = String::from_utf8_lossy(&result.stdout)
        .trim()
        .to_uppercase();
    eprintln!("[haiku_judge] response: {:?}", text);
    text.contains("PASS")
}
