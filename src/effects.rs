use anyhow::{bail, Context, Result};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

pub trait Effects {
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;
    fn read_file_string(&self, path: &Path) -> Result<String>;
    fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()>;
    fn path_exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn remove_dir_all(&self, path: &Path) -> Result<()>;
    fn read_dir_names(&self, path: &Path) -> Result<Vec<String>>;
    fn set_permissions(&self, path: &Path, mode: u32) -> Result<()>;
    fn copy_file(&self, from: &Path, to: &Path) -> Result<()>;
    fn println(&self, msg: &str);
    fn eprintln(&self, msg: &str);
    fn pick_from_list(&self, prompt: &str, items: &[String]) -> Result<usize>;
    fn prompt_text(&self, prompt: &str) -> Result<String>;
    fn prompt_optional(&self, prompt: &str) -> Result<Option<String>>;
    fn force_retype(&self, prompt: &str, expected: &str) -> Result<()>;
    fn check_ssh_prereqs(&self) -> Result<()>;
    fn ssh_keygen_generate(&self, key_path: &Path, comment: &str) -> Result<()>;
    fn ssh_keygen_fingerprint(&self, pub_path: &Path) -> Result<String>;
    fn ssh_add(&self, key_path: &Path) -> Result<()>;
    fn ssh_add_list(&self) -> Result<String>;
    fn current_date_string(&self) -> String;
    fn home_dir(&self) -> Result<String>;
    fn shell_env(&self) -> Result<String>;
    fn current_exe_dir(&self) -> Result<PathBuf>;
}

// --- RealEffects ---
pub struct RealEffects;

impl Effects for RealEffects {
    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path).with_context(|| format!("Reading {}", path.display()))
    }

    fn read_file_string(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))
    }

    fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()> {
        std::fs::write(path, contents).with_context(|| format!("Writing {}", path.display()))
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path).with_context(|| format!("Creating dir {}", path.display()))
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::remove_dir_all(path).with_context(|| format!("Removing {}", path.display()))
    }

    fn read_dir_names(&self, path: &Path) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let entries =
            std::fs::read_dir(path).with_context(|| format!("Reading dir {}", path.display()))?;
        for entry in entries {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                .with_context(|| format!("Setting permissions on {}", path.display()))?;
        }
        Ok(())
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Result<()> {
        std::fs::copy(from, to)
            .with_context(|| format!("Copying {} to {}", from.display(), to.display()))?;
        Ok(())
    }

    fn println(&self, msg: &str) {
        println!("{}", msg);
    }

    fn eprintln(&self, msg: &str) {
        eprintln!("{}", msg);
    }

    fn pick_from_list(&self, prompt: &str, items: &[String]) -> Result<usize> {
        if items.is_empty() {
            bail!("No items to pick from");
        }
        use dialoguer::{theme::ColorfulTheme, Select};
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .items(items)
            .default(0)
            .interact()?;
        Ok(selection)
    }

    fn prompt_text(&self, prompt: &str) -> Result<String> {
        use std::io::{BufRead, Write};
        print!("{}: ", prompt);
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        Ok(line.trim().to_string())
    }

    fn prompt_optional(&self, prompt: &str) -> Result<Option<String>> {
        use std::io::{BufRead, Write};
        print!("{} (leave empty to skip): ", prompt);
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed))
        }
    }

    fn force_retype(&self, prompt: &str, expected: &str) -> Result<()> {
        use std::io::{BufRead, Write};
        println!("{}", prompt);
        loop {
            print!("Type '{}' to confirm: ", expected);
            std::io::stdout().flush()?;
            let mut line = String::new();
            let n = std::io::stdin().lock().read_line(&mut line)?;
            if n == 0 {
                bail!("Aborted: EOF before confirmation was typed");
            }
            if line.trim() == expected {
                return Ok(());
            }
            println!("Input did not match. Try again (Ctrl-C to abort).");
        }
    }

    fn check_ssh_prereqs(&self) -> Result<()> {
        use std::process::Command;
        for tool in &["ssh-keygen", "ssh-add"] {
            let status = Command::new("which")
                .arg(tool)
                .output()
                .with_context(|| format!("Checking for {}", tool))?;
            if !status.status.success() {
                bail!(
                    "Required tool '{}' not found on PATH. Please install OpenSSH.",
                    tool
                );
            }
        }
        Ok(())
    }

    fn ssh_keygen_generate(&self, key_path: &Path, comment: &str) -> Result<()> {
        use std::process::Command;
        let status = Command::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-f",
                key_path.to_str().expect("key path is valid UTF-8"),
                "-C",
                comment,
            ])
            .status()
            .context("Running ssh-keygen")?;

        if !status.success() {
            bail!("ssh-keygen exited with status {}", status);
        }

        self.set_permissions(key_path, 0o600)?;
        Ok(())
    }

    fn ssh_keygen_fingerprint(&self, pub_path: &Path) -> Result<String> {
        use std::process::Command;
        let output = Command::new("ssh-keygen")
            .args([
                "-l",
                "-E",
                "sha256",
                "-f",
                pub_path.to_str().expect("pub path is valid UTF-8"),
            ])
            .output()
            .context("Running ssh-keygen -l")?;

        if !output.status.success() {
            bail!("ssh-keygen -l failed for {}", pub_path.display());
        }

        let line = String::from_utf8_lossy(&output.stdout);
        let fp = line
            .split_whitespace()
            .nth(1)
            .map(|s| s.to_string())
            .unwrap_or_default();
        Ok(fp)
    }

    fn ssh_add(&self, key_path: &Path) -> Result<()> {
        use std::process::Command;
        let status = Command::new("ssh-add")
            .arg(key_path)
            .status()
            .context("Running ssh-add")?;

        if !status.success() {
            bail!("ssh-add exited with status {}", status);
        }
        Ok(())
    }

    fn ssh_add_list(&self) -> Result<String> {
        use std::process::Command;
        let output = Command::new("ssh-add")
            .arg("-l")
            .output()
            .context("Running ssh-add -l")?;

        if output.status.code() == Some(1) {
            return Ok(String::new());
        }
        if !output.status.success() {
            bail!("ssh-add -l exited with status {}", output.status);
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn current_date_string(&self) -> String {
        use chrono::Local;
        let now = Local::now();
        now.format("%Y-%m-%d_%H-%M_UTC%z").to_string()
    }

    fn home_dir(&self) -> Result<String> {
        Ok(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
    }

    fn shell_env(&self) -> Result<String> {
        Ok(std::env::var("SHELL").unwrap_or_default())
    }

    fn current_exe_dir(&self) -> Result<PathBuf> {
        let exe = std::env::current_exe().context("Failed to determine executable path")?;
        exe.parent()
            .context("Executable has no parent directory")
            .map(|p| p.to_path_buf())
    }
}

// --- CannedEffects ---
pub struct CannedEffects {
    fs: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
    dirs: RefCell<BTreeSet<PathBuf>>,
    output: RefCell<Vec<String>>,
    err_output: RefCell<Vec<String>>,
    prompt_answers: RefCell<VecDeque<String>>,
    pick_answers: RefCell<VecDeque<usize>>,
    date: String,
    home: String,
    shell: String,
    exe_dir: PathBuf,
    agent_keys: String,
}

impl CannedEffects {
    pub fn new() -> Self {
        CannedEffects {
            fs: RefCell::new(BTreeMap::new()),
            dirs: RefCell::new(BTreeSet::new()),
            output: RefCell::new(Vec::new()),
            err_output: RefCell::new(Vec::new()),
            prompt_answers: RefCell::new(VecDeque::new()),
            pick_answers: RefCell::new(VecDeque::new()),
            date: "1970-01-01_00-00_UTC+0000".to_string(),
            home: "/fake/home".to_string(),
            shell: "/bin/zsh".to_string(),
            exe_dir: PathBuf::from("/fake/bin"),
            agent_keys: String::new(),
        }
    }

    pub fn with_date(mut self, date: &str) -> Self {
        self.date = date.to_string();
        self
    }

    pub fn with_home(mut self, home: &str) -> Self {
        self.home = home.to_string();
        self
    }

    pub fn with_shell(mut self, shell: &str) -> Self {
        self.shell = shell.to_string();
        self
    }

    pub fn with_exe_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.exe_dir = dir.into();
        self
    }

    pub fn with_prompt_answers(mut self, answers: Vec<String>) -> Self {
        self.prompt_answers = RefCell::new(VecDeque::from(answers));
        self
    }

    pub fn with_pick_answers(mut self, answers: Vec<usize>) -> Self {
        self.pick_answers = RefCell::new(VecDeque::from(answers));
        self
    }

    pub fn with_agent_keys(mut self, keys: &str) -> Self {
        self.agent_keys = keys.to_string();
        self
    }

    pub fn output(&self) -> String {
        let lines = self.output.borrow();
        if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n") + "\n"
        }
    }

    pub fn output_lines(&self) -> Vec<String> {
        self.output.borrow().clone()
    }

    pub fn err_output(&self) -> String {
        let lines = self.err_output.borrow();
        if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n") + "\n"
        }
    }

    pub fn clear_output(&self) {
        self.output.borrow_mut().clear();
    }
}

impl Effects for CannedEffects {
    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.fs
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("File not found: {}", path.display()))
    }

    fn read_file_string(&self, path: &Path) -> Result<String> {
        let bytes = self.read_file(path)?;
        Ok(String::from_utf8(bytes)?)
    }

    fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()> {
        self.fs
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_vec());
        // Also ensure parent directories are in dirs
        if let Some(parent) = path.parent() {
            self.dirs.borrow_mut().insert(parent.to_path_buf());
        }
        Ok(())
    }

    fn path_exists(&self, path: &Path) -> bool {
        self.fs.borrow().contains_key(path) || self.dirs.borrow().contains(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.borrow().contains(path)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        let mut dirs = self.dirs.borrow_mut();
        let mut current = path.to_path_buf();
        loop {
            dirs.insert(current.clone());
            if current.parent().is_none() {
                break;
            }
            current = current.parent().unwrap().to_path_buf();
        }
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        let mut fs = self.fs.borrow_mut();
        let mut dirs = self.dirs.borrow_mut();
        fs.retain(|k, _| !k.starts_with(path));
        dirs.retain(|k| !k.starts_with(path));
        Ok(())
    }

    fn read_dir_names(&self, path: &Path) -> Result<Vec<String>> {
        let fs = self.fs.borrow();
        let dirs = self.dirs.borrow();
        let mut names = BTreeSet::new();

        for file_path in fs.keys() {
            if file_path.parent() == Some(path) {
                if let Some(name) = file_path.file_name().and_then(|n| n.to_str()) {
                    names.insert(name.to_string());
                }
            }
        }

        for dir_path in dirs.iter() {
            if dir_path.parent() == Some(path) {
                if let Some(name) = dir_path.file_name().and_then(|n| n.to_str()) {
                    names.insert(name.to_string());
                }
            }
        }

        Ok(names.into_iter().collect())
    }

    fn set_permissions(&self, _path: &Path, _mode: u32) -> Result<()> {
        Ok(())
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Result<()> {
        let content = self.read_file(from)?;
        self.write_file(to, &content)?;
        Ok(())
    }

    fn println(&self, msg: &str) {
        self.output.borrow_mut().push(msg.to_string());
    }

    fn eprintln(&self, msg: &str) {
        self.err_output.borrow_mut().push(msg.to_string());
    }

    fn pick_from_list(&self, _prompt: &str, _items: &[String]) -> Result<usize> {
        Ok(self.pick_answers.borrow_mut().pop_front().unwrap_or(0))
    }

    fn prompt_text(&self, _prompt: &str) -> Result<String> {
        Ok(self
            .prompt_answers
            .borrow_mut()
            .pop_front()
            .unwrap_or_default())
    }

    fn prompt_optional(&self, _prompt: &str) -> Result<Option<String>> {
        let answer = self
            .prompt_answers
            .borrow_mut()
            .pop_front()
            .unwrap_or_default();
        if answer.is_empty() {
            Ok(None)
        } else {
            Ok(Some(answer))
        }
    }

    fn force_retype(&self, _prompt: &str, expected: &str) -> Result<()> {
        let answer = self
            .prompt_answers
            .borrow_mut()
            .pop_front()
            .unwrap_or_default();
        if answer == expected {
            Ok(())
        } else {
            bail!("Aborted: retype confirmation did not match")
        }
    }

    fn check_ssh_prereqs(&self) -> Result<()> {
        Ok(())
    }

    fn ssh_keygen_generate(&self, key_path: &Path, comment: &str) -> Result<()> {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(key_path.to_string_lossy().as_bytes());
        h.update(comment.as_bytes());
        let hash = h.finalize();
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();

        let priv_content = format!("fake-private-key-{}", hex);
        let pub_content = format!("ssh-ed25519 AAAA{} {}\n", &hex[..16], comment);

        self.write_file(key_path, priv_content.as_bytes())?;
        self.write_file(&key_path.with_extension("pub"), pub_content.as_bytes())?;
        Ok(())
    }

    fn ssh_keygen_fingerprint(&self, pub_path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        let content = self.read_file(pub_path)?;
        let mut h = Sha256::new();
        h.update(&content);
        let hash = h.finalize();
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(format!("SHA256:{}", &hex[..43]))
    }

    fn ssh_add(&self, _key_path: &Path) -> Result<()> {
        Ok(())
    }

    fn ssh_add_list(&self) -> Result<String> {
        Ok(self.agent_keys.clone())
    }

    fn current_date_string(&self) -> String {
        self.date.clone()
    }

    fn home_dir(&self) -> Result<String> {
        Ok(self.home.clone())
    }

    fn shell_env(&self) -> Result<String> {
        Ok(self.shell.clone())
    }

    fn current_exe_dir(&self) -> Result<PathBuf> {
        Ok(self.exe_dir.clone())
    }
}
