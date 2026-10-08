use crate::Post;
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use reqwest::{blocking::Client, cookie::Jar};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Path,
    sync::Arc,
    thread,
    time::Duration,
};
use url::Url;

#[derive(Clone, Deserialize)]
pub struct Publication {
    pub id: u64,
    pub name: String,
    pub subdomain: String,
}
#[derive(Clone, Deserialize)]
pub struct Subscription {
    pub publication_id: u64,
    pub membership_state: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}
impl Subscription {
    #[must_use]
    pub fn is_free(&self) -> bool {
        self.membership_state.as_deref() == Some("free_subscriber")
            || self.kind.as_deref() == Some("free")
    }
}
#[derive(Deserialize)]
pub struct Subscriptions {
    pub subscriptions: Vec<Subscription>,
    pub publications: Vec<Publication>,
}

/// # Errors
/// Rejects empty responses and missing subscription metadata.
pub fn subscribed(payload: Subscriptions) -> Result<Vec<(Publication, Subscription)>> {
    ensure!(
        !payload.subscriptions.is_empty(),
        "Empty subscription response; refusing checkpoint"
    );
    let pubs: BTreeMap<_, _> = payload
        .publications
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let subs: BTreeMap<_, _> = payload
        .subscriptions
        .into_iter()
        .map(|s| (s.publication_id, s))
        .collect();
    subs.into_iter()
        .map(|(id, sub)| {
            Ok((
                pubs.get(&id)
                    .context("Missing subscribed publication metadata")?
                    .clone(),
                sub,
            ))
        })
        .collect()
}

/// # Errors
/// Rejects pagination failures or posts belonging to another publication.
pub fn archive(
    mut fetch: impl FnMut(usize) -> Result<Vec<Post>>,
    publication: u64,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<Vec<Post>> {
    let mut offset = 0;
    let mut seen = BTreeSet::new();
    let mut posts = Vec::new();
    loop {
        let batch = fetch(offset)?;
        if batch.is_empty() {
            break;
        }
        let all_old = batch.iter().all(|p| p.post_date < since);
        offset += batch.len();
        let mut fresh = false;
        for post in batch {
            ensure!(
                post.publication_id == publication,
                "Archive returned another publication's post"
            );
            if seen.insert(post.id) {
                fresh = true;
                if since <= post.post_date && post.post_date <= until {
                    posts.push(post);
                }
            }
        }
        ensure!(fresh, "Archive pagination stopped making progress");
        if all_old {
            break;
        }
    }
    Ok(posts)
}

pub struct Api {
    client: Client,
    pub cookies: Vec<Value>,
}
impl Api {
    /// # Errors
    /// Rejects unreadable/malformed credentials or HTTP client setup failures.
    pub fn new(path: &Path) -> Result<Self> {
        let raw: Value =
            serde_json::from_reader(File::open(path).context("Cannot open Substack cookies")?)
                .context("Invalid Substack cookie JSON")?;
        let cookies = raw
            .get("cookies")
            .unwrap_or(&raw)
            .as_array()
            .context("Expected browser cookies or storage state")?
            .clone();
        ensure!(!cookies.is_empty(), "No Substack cookies supplied");
        let jar = Arc::new(Jar::default());
        for c in &cookies {
            let text = |key| {
                c.get(key)
                    .and_then(Value::as_str)
                    .context("Invalid browser cookie")
            };
            let name = text("name")?;
            let value = text("value")?;
            let domain = text("domain")?;
            let path = text("path")?;
            ensure!(
                ![name, value, domain, path]
                    .iter()
                    .any(|s| s.contains(['\r', '\n', ';'])),
                "Invalid cookie field"
            );
            let origin = Url::parse(&format!("https://{}/", domain.trim_start_matches('.')))
                .context("Invalid cookie domain")?;
            let mut cookie = format!("{name}={value}; Domain={domain}; Path={path}");
            if c.get("secure").and_then(Value::as_bool) == Some(true) {
                cookie.push_str("; Secure");
            }
            jar.add_cookie_str(&cookie, &origin);
        }
        let client = Client::builder()
            .cookie_provider(jar)
            .timeout(Duration::from_secs(60))
            .user_agent("Mozilla/5.0 SubstackDigest/1.0")
            .build()?;
        Ok(Self { client, cookies })
    }
    fn get<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        for attempt in 0..4 {
            let response = self
                .client
                .get(url)
                .query(params)
                .send()
                .context("Substack API transport failed")?;
            let status = response.status();
            if status.as_u16() == 429 || status.is_server_error() {
                thread::sleep(Duration::from_secs(5 * (attempt + 1)));
                continue;
            }
            ensure!(
                status.is_success(),
                "Substack request failed (HTTP {}); refresh cookies if expired",
                status.as_u16()
            );
            return response
                .json()
                .context("Unexpected Substack response schema");
        }
        bail!("Substack request failed after retries")
    }
    /// # Errors
    /// Rejects HTTP, authentication and subscription-schema failures.
    pub fn subscriptions(&self) -> Result<Vec<(Publication, Subscription)>> {
        subscribed(self.get(
            "https://substack.com/api/v1/subscriptions",
            &[("tvOnly", "false".into())],
        )?)
    }
    /// # Errors
    /// Fails if the archive cannot be completely paginated through the cutoff.
    pub fn posts(
        &self,
        publication: &Publication,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<Post>> {
        ensure!(
            !publication.subdomain.is_empty()
                && publication
                    .subdomain
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Invalid publication subdomain"
        );
        let url = format!(
            "https://{}.substack.com/api/v1/archive",
            publication.subdomain
        );
        archive(
            |offset| {
                self.get(
                    &url,
                    &[
                        ("sort", "new".into()),
                        ("limit", "20".into()),
                        ("offset", offset.to_string()),
                    ],
                )
            },
            publication.id,
            since,
            until,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn suggestions_in_metadata_are_not_subscriptions() {
        let payload = json!({"subscriptions":[{"publication_id":1}], "publications":[{"id":1,"name":"Subscribed","subdomain":"one"},{"id":99,"name":"Suggested","subdomain":"other"}]});
        let pubs = subscribed(serde_json::from_value(payload).unwrap()).unwrap();
        assert_eq!(pubs.len(), 1);
        assert_eq!(pubs[0].0.id, 1);
        assert!(
            subscribed(Subscriptions {
                subscriptions: vec![],
                publications: vec![]
            })
            .is_err()
        );
    }
    fn post(id: u64, date: DateTime<Utc>) -> Post {
        Post {
            id,
            publication_id: 1,
            post_date: date,
            title: "test".into(),
            canonical_url: "https://example.substack.com/p/a".into(),
            audience: None,
            kind: None,
        }
    }
    #[test]
    fn pagination_handles_more_than_one_page_and_overlap() {
        let now = Utc::now();
        let mut offsets = vec![];
        let posts = archive(
            |offset| {
                offsets.push(offset);
                Ok(match offset {
                    0 => (0..20).map(|i| post(i, now)).collect(),
                    20 => (19..39).map(|i| post(i, now)).collect(),
                    _ => vec![post(40, now - chrono::Duration::days(5))],
                })
            },
            1,
            now - chrono::Duration::days(1),
            now,
        )
        .unwrap();
        assert_eq!(posts.len(), 39);
        assert_eq!(offsets, vec![0, 20, 40]);
    }
    #[test]
    fn stuck_pagination_and_wrong_publication_fail() {
        let now = Utc::now();
        assert!(archive(|_| Ok(vec![post(1, now)]), 1, now, now).is_err());
        assert!(archive(|_| Ok(vec![post(1, now)]), 2, now, now).is_err());
    }
}
