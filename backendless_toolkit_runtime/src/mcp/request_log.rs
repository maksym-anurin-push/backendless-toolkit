//! Append-only NDJSON log for MCP server handler traffic.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// File logger for every request/notification that hits the MCP service layer.
pub struct RequestLog {
    file: Mutex<File>,
    path: PathBuf,
}

impl RequestLog {
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            file: Mutex::new(file),
            path,
        })
    }

    /// Default path: `$BACKENDLESS_TOOLKIT_MCP_REQUEST_LOG`, else
    /// `<workspace>/.var/mcp-server-handler.log` when the binary lives under `target/*`,
    /// else `./.var/mcp-server-handler.log`.
    pub fn default_path() -> PathBuf {
        if let Ok(path) = std::env::var("BACKENDLESS_TOOLKIT_MCP_REQUEST_LOG") {
            return PathBuf::from(path);
        }

        if let Ok(exe) = std::env::current_exe() {
            // .../target/<profile>/backendless-toolkit-mcp -> workspace root
            if let (Some(profile_dir), Some(file_name)) = (exe.parent(), exe.file_name()) {
                let _ = file_name;
                if let Some(target_dir) = profile_dir.parent() {
                    if target_dir.file_name().and_then(|s| s.to_str()) == Some("target") {
                        if let Some(workspace) = target_dir.parent() {
                            return workspace.join(".var/mcp-server-handler.log");
                        }
                    }
                }
            }
        }

        PathBuf::from(".var/mcp-server-handler.log")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write_entry(&self, entry: &serde_json::Value) {
        let Ok(mut file) = self.file.lock() else {
            return;
        };
        let Ok(line) = serde_json::to_string(entry) else {
            return;
        };
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}
