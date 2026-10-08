//! Incremental Substack digest orchestration. State remains compatible with the
//! initial JSON format; retries never replace an existing cloud document.
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub mod browser;
pub mod source;
pub mod upload;

#[derive(Clone, Deserialize)]
pub struct Config {
    pub folder: String,
    pub timezone: chrono_tz::Tz,
    pub first_run_hours: i64,
    pub overlap_hours: i64,
    pub page_width_mm: f64,
    pub page_height_mm: f64,
    pub font_size_pt: f64,
    pub line_height: f64,
    pub margin_mm: f64,
    pub annotation_margin_mm: f64,
    pub local_retention_days: i64,
}
impl Config {
    /// # Errors
    /// Rejects invalid collection windows, cloud paths and page dimensions.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.folder.starts_with('/') && self.folder.len() > 1,
            "Cloud folder must be an absolute path"
        );
        ensure!(
            (1..=8760).contains(&self.first_run_hours) && (0..=8760).contains(&self.overlap_hours),
            "Invalid collection window"
        );
        ensure!(
            (1..=3650).contains(&self.local_retention_days),
            "Invalid PDF retention"
        );
        ensure!(
            [
                self.page_width_mm,
                self.page_height_mm,
                self.font_size_pt,
                self.line_height,
                self.margin_mm,
                self.annotation_margin_mm
            ]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0),
            "Invalid page layout"
        );
        ensure!(
            self.page_width_mm > self.margin_mm + self.annotation_margin_mm + 20.0
                && self.page_height_mm > self.margin_mm * 2.0 + 20.0,
            "Margins leave no reading area"
        );
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Post {
    pub id: u64,
    pub publication_id: u64,
    pub post_date: DateTime<Utc>,
    pub title: String,
    pub canonical_url: String,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}
#[derive(Clone)]
pub struct Article {
    pub post: Post,
    pub publication: String,
    pub body: String,
}
#[derive(Default, Deserialize, Serialize)]
pub struct State {
    pub cutoff: Option<DateTime<Utc>>,
    #[serde(default)]
    pub initial: bool,
    pub delivered: BTreeMap<String, DateTime<Utc>>,
}
#[derive(Deserialize, Serialize)]
struct Pending {
    cutoff: DateTime<Utc>,
    ids: BTreeMap<String, DateTime<Utc>>,
    pdf: Option<String>,
    folder: String,
}

pub trait Backend {
    /// # Errors
    /// Fails when any required publication or article cannot be retrieved.
    fn collect(
        &mut self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
        delivered: &BTreeMap<String, DateTime<Utc>>,
    ) -> Result<Vec<Article>>;
    /// # Errors
    /// Fails rather than publishing an incomplete PDF.
    fn render(
        &mut self,
        articles: &[Article],
        config: &Config,
        title: &str,
        path: &Path,
    ) -> Result<()>;
    /// # Errors
    /// Fails if delivery cannot be confirmed without overwriting documents.
    fn upload(&mut self, path: &Path, folder: &str) -> Result<()>;
}

/// # Errors
/// Fails if serialization or an atomic, durable filesystem write fails.
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("State path has no parent")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temporary, value)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

/// # Errors
/// Fails on filesystem errors or when another process owns the lock.
pub fn lock_data(data: &Path) -> Result<File> {
    fs::create_dir_all(data)?;
    fs::set_permissions(data, fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(data.join("lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("Another digest run holds the state lock")?;
    Ok(lock)
}

fn pending_pdf(data: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        Path::new(name).file_name().and_then(|s| s.to_str()) == Some(name)
            && Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")),
        "Invalid outbox PDF filename"
    );
    Ok(data.join(name))
}

/// Run one transaction while the caller holds `lock_data`.
/// # Errors
/// Collection, rendering, upload or persistence failures preserve the last
/// successful checkpoint and retain an already-created upload outbox.
pub fn run(
    config: &Config,
    data: &Path,
    backend: &mut impl Backend,
    now: DateTime<Utc>,
) -> Result<usize> {
    config.validate()?;
    let state_path = data.join("state.json");
    let pending_path = data.join("pending.json");
    let mut state: State = if state_path.exists() {
        serde_json::from_reader(File::open(&state_path)?).context("Cannot read checkpoint")?
    } else {
        let state = State {
            cutoff: Some(now - Duration::hours(config.first_run_hours)),
            initial: true,
            ..State::default()
        };
        // Freeze the initial boundary before doing network work.
        atomic_json(&state_path, &state)?;
        state
    };
    let pending: Pending = if pending_path.exists() {
        serde_json::from_reader(File::open(&pending_path)?).context("Cannot read outbox")?
    } else {
        let mut since = state.cutoff.context("Checkpoint has no cutoff")?;
        if !state.initial {
            since -= Duration::hours(config.overlap_hours);
        }
        let mut articles = backend.collect(since, now, &state.delivered)?;
        articles.sort_by_key(|a| (a.post.post_date, a.post.id));
        let mut seen = BTreeSet::new();
        articles.retain(|a| {
            seen.insert(a.post.id) && !state.delivered.contains_key(&a.post.id.to_string())
        });
        let ids: BTreeMap<_, _> = articles
            .iter()
            .map(|a| (a.post.id.to_string(), a.post.post_date))
            .collect();
        let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&(now, &ids))?));
        let title = format!(
            "Substack {} {}",
            now.with_timezone(&config.timezone).format("%Y-%m-%d"),
            &hash[..12]
        );
        let pdf = if articles.is_empty() {
            None
        } else {
            let name = format!("{title}.pdf");
            let temporary = data.join(format!("{title}.tmp.pdf"));
            backend.render(&articles, config, &title, &temporary)?;
            File::open(&temporary)?.sync_all()?;
            fs::rename(temporary, data.join(&name))?;
            Some(name)
        };
        let pending = Pending {
            cutoff: now,
            ids,
            pdf,
            folder: config.folder.clone(),
        };
        // Durable outbox BEFORE upload. A retry reuses exactly this PDF.
        atomic_json(&pending_path, &pending)?;
        pending
    };
    if let Some(name) = &pending.pdf {
        backend.upload(&pending_pdf(data, name)?, &pending.folder)?;
    }
    let count = pending.ids.len();
    state.cutoff = Some(pending.cutoff);
    state.initial = false;
    state.delivered.extend(pending.ids);
    atomic_json(&state_path, &state)?;
    fs::remove_file(&pending_path)?;
    File::open(data)?.sync_all()?;
    let expiry = now - Duration::days(config.local_retention_days);
    for entry in fs::read_dir(data)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "pdf")
            && DateTime::<Utc>::from(fs::metadata(&path)?.modified()?) < expiry
        {
            fs::remove_file(path)?;
        }
    }
    println!(
        "Digest complete: {count} articles; cutoff {}",
        pending.cutoff
    );
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    struct Fake {
        articles: Vec<Article>,
        windows: Vec<DateTime<Utc>>,
        fail_collect: bool,
        fail_upload: bool,
        uploads: usize,
        renders: usize,
    }
    impl Backend for Fake {
        fn collect(
            &mut self,
            since: DateTime<Utc>,
            _: DateTime<Utc>,
            _: &BTreeMap<String, DateTime<Utc>>,
        ) -> Result<Vec<Article>> {
            self.windows.push(since);
            if self.fail_collect {
                bail!("source unavailable");
            }
            Ok(self.articles.clone())
        }
        fn render(&mut self, _: &[Article], _: &Config, _: &str, path: &Path) -> Result<()> {
            self.renders += 1;
            fs::write(path, b"fixture PDF")?;
            Ok(())
        }
        fn upload(&mut self, _: &Path, _: &str) -> Result<()> {
            if self.fail_upload {
                bail!("cloud unavailable");
            }
            self.uploads += 1;
            Ok(())
        }
    }
    fn config() -> Config {
        serde_json::from_str(include_str!(
            "../../../../fixtures/config.json"
        ))
        .unwrap()
    }
    fn now() -> DateTime<Utc> {
        "2026-10-07T11:00:00Z".parse().unwrap()
    }
    fn fake() -> Fake {
        Fake {
            articles: vec![Article {
                post: Post {
                    id: 1,
                    publication_id: 1,
                    post_date: now(),
                    title: "Article".into(),
                    canonical_url: "https://example.substack.com/p/article".into(),
                    audience: None,
                    kind: None,
                },
                publication: "Example".into(),
                body: "<p>Full article</p>".into(),
            }],
            windows: vec![],
            fail_collect: false,
            fail_upload: false,
            uploads: 0,
            renders: 0,
        }
    }
    #[test]
    fn failed_upload_reuses_exact_outbox() {
        let dir = tempfile::tempdir().unwrap();
        let mut backend = fake();
        backend.fail_upload = true;
        assert!(run(&config(), dir.path(), &mut backend, now()).is_err());
        let state: State =
            serde_json::from_reader(File::open(dir.path().join("state.json")).unwrap()).unwrap();
        assert_eq!(state.cutoff, Some(now() - Duration::hours(24)));
        backend.fail_upload = false;
        run(
            &config(),
            dir.path(),
            &mut backend,
            now() + Duration::days(1),
        )
        .unwrap();
        assert_eq!(backend.windows.len(), 1);
        assert_eq!(backend.renders, 1);
        assert_eq!(backend.uploads, 1);
        let state: State =
            serde_json::from_reader(File::open(dir.path().join("state.json")).unwrap()).unwrap();
        assert_eq!(state.cutoff, Some(now()));
        assert!(state.delivered.contains_key("1"));
        assert!(!dir.path().join("pending.json").exists());
    }
    #[test]
    fn first_failure_freezes_boundary_and_empty_run_does_not_upload() {
        let dir = tempfile::tempdir().unwrap();
        let mut backend = fake();
        backend.fail_collect = true;
        assert!(run(&config(), dir.path(), &mut backend, now()).is_err());
        backend.fail_collect = false;
        backend.articles.clear();
        run(
            &config(),
            dir.path(),
            &mut backend,
            now() + Duration::days(3),
        )
        .unwrap();
        assert_eq!(backend.windows, vec![now() - Duration::hours(24); 2]);
        assert_eq!(backend.uploads, 0);
        assert_eq!(backend.renders, 0);
        run(
            &config(),
            dir.path(),
            &mut backend,
            now() + Duration::days(4),
        )
        .unwrap();
        assert_eq!(backend.windows[2], now() + Duration::days(1));
    }
    #[test]
    fn delivered_articles_are_not_republished() {
        let dir = tempfile::tempdir().unwrap();
        let mut backend = fake();
        run(&config(), dir.path(), &mut backend, now()).unwrap();
        run(
            &config(),
            dir.path(),
            &mut backend,
            now() + Duration::days(1),
        )
        .unwrap();
        assert_eq!(backend.uploads, 1);
    }
    #[test]
    fn reads_original_python_state_and_outbox_without_recollection() {
        let dir = tempfile::tempdir().unwrap();
        let mut backend = fake();
        backend.fail_collect = true;
        fs::write(
            dir.path().join("state.json"),
            r#"{"cutoff":"2026-10-06T11:00:00+00:00","delivered":{}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("daily.pdf"), b"existing PDF").unwrap();
        fs::write(dir.path().join("pending.json"), r#"{"cutoff":"2026-10-07T11:00:00+00:00","ids":{"1":"2026-10-07T10:00:00Z"},"pdf":"daily.pdf","folder":"/Substack Daily"}"#).unwrap();
        run(&config(), dir.path(), &mut backend, now()).unwrap();
        assert_eq!(backend.uploads, 1);
        assert_eq!(backend.renders, 0);
    }
    #[test]
    fn concurrent_runs_cannot_share_state() {
        let dir = tempfile::tempdir().unwrap();
        let _lock = lock_data(dir.path()).unwrap();
        assert!(lock_data(dir.path()).is_err());
    }
}
