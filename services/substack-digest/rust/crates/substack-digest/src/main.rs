use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
};
use substack_digest::{Article, Backend, Config, browser::Renderer, source::Api, upload::Cloud};

struct Service {
    cookies: PathBuf,
    renderer: Option<Renderer>,
    cloud: Cloud,
}
impl Backend for Service {
    fn collect(
        &mut self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
        delivered: &BTreeMap<String, DateTime<Utc>>,
    ) -> Result<Vec<Article>> {
        let api = Api::new(&self.cookies)?;
        let renderer = Renderer::new(&api.cookies)?;
        let mut articles = vec![];
        for (publication, subscription) in api.subscriptions()? {
            for post in api.posts(&publication, since, until)? {
                if delivered.contains_key(&post.id.to_string())
                    || post.kind.as_deref().is_some_and(|t| t != "newsletter")
                {
                    continue;
                }
                let body = renderer.article(&post, &subscription)?;
                articles.push(Article {
                    post,
                    publication: publication.name.clone(),
                    body,
                });
            }
        }
        self.renderer = Some(renderer);
        Ok(articles)
    }
    fn render(
        &mut self,
        articles: &[Article],
        config: &Config,
        title: &str,
        path: &Path,
    ) -> Result<()> {
        self.renderer
            .as_ref()
            .context("Browser was not initialized")?
            .pdf(articles, config, title, path)
    }
    fn upload(&mut self, path: &Path, folder: &str) -> Result<()> {
        self.cloud.deliver(path, folder)
    }
}
fn run() -> Result<()> {
    let mut config = PathBuf::from("/config/config.json");
    let mut data = PathBuf::from("/data");
    let mut cookies = PathBuf::from("/run/secrets/substack-cookies.json");
    let mut token = PathBuf::from("/run/secrets/remarkable-token");
    let mut smoke = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "substack-digest [--config FILE] [--data DIR] [--cookies FILE] [--token FILE] [--smoke-test PDF]"
            );
            return Ok(());
        }
        let value = PathBuf::from(args.next().context("Option requires a path")?);
        match arg.as_str() {
            "--config" => config = value,
            "--data" => data = value,
            "--cookies" => cookies = value,
            "--token" => token = value,
            "--smoke-test" => smoke = Some(value),
            _ => bail!("Unknown command-line option"),
        }
    }
    let config: Config =
        serde_json::from_reader(File::open(config).context("Cannot open digest config")?)
            .context("Invalid digest config")?;
    config.validate()?;
    if let Some(output) = smoke {
        return substack_digest::browser::smoke_test(&config, &output);
    }
    let _lock = substack_digest::lock_data(&data)?;
    let cloud = Cloud::new(&token, &data)?;
    substack_digest::run(
        &config,
        &data,
        &mut Service {
            cookies,
            renderer: None,
            cloud,
        },
        Utc::now(),
    )?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        // Only the top-level contextual error; never remote response bodies,
        // cookie values, command environments or authentication payloads.
        eprintln!("Digest failed: {error}; checkpoint preserved.");
        std::process::exit(1);
    }
}
