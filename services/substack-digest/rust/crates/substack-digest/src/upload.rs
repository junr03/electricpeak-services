use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "visibleName")]
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

fn already_uploaded(entries: &[Entry], name: &str) -> Result<bool> {
    let matches: Vec<_> = entries.iter().filter(|e| e.name == name).collect();
    if matches.is_empty() {
        return Ok(false);
    }
    ensure!(
        matches.len() == 1 && matches[0].kind == "DocumentType",
        "Ambiguous existing digest; refusing to replace documents"
    );
    Ok(true)
}

pub struct Cloud {
    token: String,
    cache: PathBuf,
}
impl Cloud {
    /// # Errors
    /// Fails if the device token is unreadable or empty.
    pub fn new(token: &Path, data: &Path) -> Result<Self> {
        let token = fs::read_to_string(token)
            .context("Cannot read reMarkable token")?
            .trim()
            .to_owned();
        ensure!(!token.is_empty(), "Empty reMarkable token");
        Ok(Self {
            token,
            cache: data.join("rmapi"),
        })
    }
    fn command(&self, args: &[&str]) -> Result<serde_json::Value> {
        // Token is passed only to this child, never process-wide or via argv.
        // systemd RuntimeMaxSec bounds the whole run, including a stuck CLI.
        let output = Command::new("rmapi-js")
            .args(args)
            .args(["--json", "--refresh"])
            .env("RMAPI_DEVICE_TOKEN", &self.token)
            .env("RMAPI_CONFIG_DIR", &self.cache)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .context("Could not start reMarkable client")?;
        ensure!(
            output.status.success(),
            "reMarkable {} failed (status {}); checkpoint preserved",
            args[0],
            output.status
        );
        if output.stdout.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(&output.stdout).context("Invalid reMarkable response")
    }
    /// # Errors
    /// Fails on CLI errors or ambiguous cloud names. Never overwrites files.
    pub fn deliver(&self, pdf: &Path, folder: &str) -> Result<()> {
        self.command(&["mkdir", folder, "--parents"])?;
        let entries: Vec<Entry> = serde_json::from_value(self.command(&["ls", folder])?)
            .context("Invalid reMarkable listing")?;
        let name = pdf
            .file_stem()
            .and_then(|n| n.to_str())
            .context("Invalid PDF filename")?;
        // Previous upload may have succeeded before a crash. Do not overwrite
        // the file: it may already contain the user's handwritten annotations.
        if !already_uploaded(&entries, name)? {
            self.command(&["put", pdf.to_str().context("Invalid PDF path")?, folder])?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_overwrite_existing_or_ambiguous_documents() {
        let entry = || Entry {
            name: "daily".into(),
            kind: "DocumentType".into(),
        };
        assert!(already_uploaded(&[entry()], "daily").unwrap());
        assert!(!already_uploaded(&[entry()], "new").unwrap());
        assert!(already_uploaded(&[entry(), entry()], "daily").is_err());
        assert!(
            already_uploaded(
                &[Entry {
                    name: "daily".into(),
                    kind: "CollectionType".into()
                }],
                "daily"
            )
            .is_err()
        );
    }
}
