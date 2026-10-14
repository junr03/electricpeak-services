use crate::{Article, Config, Post, source::Subscription};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use headless_chrome::{
    Browser, LaunchOptions, Tab, protocol::cdp::Network::CookieParam, types::PrintToPdfOptions,
};
use serde::Deserialize;
use serde_json::Value;
use std::{fs, path::Path, sync::Arc, thread, time::Duration};

pub struct Renderer {
    browser: Browser,
    tab: Arc<Tab>,
}
impl Renderer {
    /// # Errors
    /// Fails if Chromium cannot launch or cookie import is invalid.
    pub fn new(cookies: &[Value]) -> Result<Self> {
        let browser = Browser::new(LaunchOptions {
            path: Some("/usr/local/bin/chromium".into()),
            sandbox: false, // Container drops all capabilities and isolates writes.
            ignore_certificate_errors: false,
            idle_browser_timeout: Duration::from_secs(300),
            ..LaunchOptions::default()
        })
        .context("Cannot launch Chromium")?;
        let tab = browser.new_tab()?;
        tab.set_default_timeout(Duration::from_secs(60));
        let cookies: Vec<CookieParam> = cookies
            .iter()
            .map(|c| {
                let mut cookie = c.clone();
                // Playwright uses -1 for a session cookie; CDP expects no expiry.
                if cookie
                    .get("expires")
                    .and_then(Value::as_f64)
                    .is_some_and(|v| v < 0.0)
                {
                    cookie
                        .as_object_mut()
                        .context("Invalid cookie")?
                        .remove("expires");
                }
                serde_json::from_value(cookie).context("Invalid Chromium cookie")
            })
            .collect::<Result<_>>()?;
        tab.set_cookies(cookies)
            .context("Could not load browser session")?;
        Ok(Self { browser, tab })
    }
    /// # Errors
    /// Fails for missing content, inaccessible paid content or navigation errors.
    pub fn article(&self, post: &Post, subscription: &Subscription) -> Result<String> {
        ensure!(
            url::Url::parse(&post.canonical_url)?.scheme() == "https",
            "Expected an HTTPS article"
        );
        self.tab
            .navigate_to(&post.canonical_url)
            .context("Article navigation failed")?
            .wait_until_navigated()
            .context("Article navigation timed out")?;
        self.tab
            .wait_for_element(".available-content .body.markup, .body.markup")
            .context("Article body missing; refresh session if expired")?;
        thread::sleep(Duration::from_millis(1500));
        extract(&self.tab, post, subscription)
    }
    /// # Errors
    /// Fails for missing images, browser errors or output write failures.
    pub fn pdf(
        &self,
        articles: &[Article],
        config: &Config,
        title: &str,
        path: &Path,
    ) -> Result<()> {
        // Fresh incognito context has no account cookies. CSP blocks scripts,
        // external frames, and all network traffic except approved image hosts.
        let context = self.browser.new_context()?;
        let page = context.new_tab()?;
        page.set_default_timeout(Duration::from_secs(60));
        let html = document(articles, config, title);
        page.navigate_to(&format!("data:text/html;base64,{}", STANDARD.encode(html)))?
            .wait_until_navigated()?;
        let loaded = page.evaluate(r"Promise.race([
            Promise.all([...document.images].map(i => i.complete ? Promise.resolve() : new Promise(r => { i.onload=r; i.onerror=r; }))).then(() => [...document.images].every(i => i.naturalWidth > 0)),
            new Promise(r => setTimeout(() => r(false), 60000))
        ])", true)?.value;
        ensure!(
            loaded == Some(Value::Bool(true)),
            "Article image could not be rendered; refusing incomplete PDF"
        );
        let bytes = page.print_to_pdf(Some(PrintToPdfOptions {
            prefer_css_page_size: Some(true), print_background: Some(true),
            display_header_footer: Some(true), header_template: Some("<span></span>".into()),
            footer_template: Some("<div style=\"font-size:7pt;width:100%;text-align:center\"><span class=\"pageNumber\"></span> / <span class=\"totalPages\"></span></div>".into()),
            ..PrintToPdfOptions::default()
        }))?;
        fs::write(path, bytes)?;
        page.close(true)?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct Extracted {
    #[serde(default)]
    missing: bool,
    #[serde(default)]
    locked: bool,
    body: Option<String>,
}
fn extract(tab: &Tab, post: &Post, subscription: &Subscription) -> Result<String> {
    let result = tab
        .evaluate(include_str!("extract.js"), false)?
        .value
        .context("Article extraction failed")?;
    let result: Extracted =
        serde_json::from_str(result.as_str().context("Invalid extraction response")?)?;
    ensure!(!result.missing, "Article body missing");
    if result.locked && subscription.is_free() && post.audience.as_deref() == Some("only_paid") {
        return Ok(
            "<p><em>Paid subscription required. Full article was not downloaded.</em></p>".into(),
        );
    }
    ensure!(
        !result.locked,
        "Article {} is locked; refresh the publication's login cookies",
        post.id
    );
    let body = result.body.context("Empty article body")?;
    ensure!(!body.trim().is_empty(), "Empty article body");
    Ok(body)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn document(articles: &[Article], c: &Config, title: &str) -> String {
    use std::fmt::Write;
    let mut contents = String::new();
    let mut sections = String::new();
    for (index, article) in articles.iter().enumerate() {
        let name = escape(&article.post.title);
        write!(contents, "<p><a href=\"#article-{index}\">{name}</a></p>").unwrap();
        write!(sections, "<article id=\"article-{index}\"><h1>{name}</h1><p class=\"meta\">{} · {}</p>{}<p class=\"meta\"><a href=\"{}\">Original article</a></p></article>", escape(&article.publication), article.post.post_date.format("%Y-%m-%d"), article.body, escape(&article.post.canonical_url)).unwrap();
    }
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src https://substackcdn.com https://*.substackcdn.com https://substack-post-media.s3.amazonaws.com; style-src 'unsafe-inline'; script-src 'none'">
<title>{title}</title><style>
@page {{ size: {width}mm {height}mm; margin: {margin}mm; margin-right: {annotation}mm; }}
body {{ font: {font}pt/{line} Georgia,serif; color: #111; }}
h1 {{ font-size: 18pt; line-height: 1.15; }} h2 {{ font-size: 15pt; }}
h1,h2,h3 {{ break-after: avoid; }} p {{ orphans: 3; widows: 3; }}
article {{ break-before: page; }} img {{ max-width: 100%; max-height: 120mm; object-fit: contain; }}
a {{ color: #222; overflow-wrap: anywhere; }} pre {{ white-space: pre-wrap; font-size: 9pt; }}
pre,td {{ overflow-wrap: anywhere; }} table {{ width: 100%; table-layout: fixed; font-size: 9pt; }}
blockquote {{ margin: 0; padding-left: 3mm; border-left: 1px solid #888; }}
.meta {{ font: 9pt/1.4 sans-serif; }}
</style></head><body><h1>{title}</h1>{contents}{sections}</body></html>"#,
        title = escape(title),
        width = c.page_width_mm,
        height = c.page_height_mm,
        margin = c.margin_mm,
        annotation = c.annotation_margin_mm,
        font = c.font_size_pt,
        line = c.line_height
    )
}

/// Offline browser acceptance check; never opens account credentials.
/// # Errors
/// Fails if browser extraction, paywall checks or PDF rendering regress.
pub fn smoke_test(config: &Config, output: &Path) -> Result<()> {
    let renderer = Renderer::new(&[])?;
    let post = Post {
        id: 1,
        publication_id: 1,
        title: "A morning on the small screen".into(),
        post_date: "2026-10-07T10:00:00Z".parse()?,
        canonical_url: "https://example.substack.com/p/sample".into(),
        audience: Some("only_paid".into()),
        kind: None,
    };
    let mut subscription = Subscription {
        publication_id: 1,
        membership_state: None,
        kind: Some("paid".into()),
    };
    let paragraph = "<p>A good reading layout leaves room to think. This sample uses a comfortable serif face, short lines, and space in the right margin for handwritten notes. Highlight a sentence or write a question beside it.</p>";
    let body = format!(
        "<h2>Space for annotations</h2>{}<blockquote>Keep the text readable without zooming.</blockquote><h2>A second section</h2>{}",
        paragraph.repeat(9),
        paragraph.repeat(7)
    );
    let page =
        format!("<div class=\"available-content\"><div class=\"body markup\">{body}</div></div>");
    renderer
        .tab
        .navigate_to(&format!("data:text/html;base64,{}", STANDARD.encode(page)))?
        .wait_until_navigated()?;
    let body = extract(&renderer.tab, &post, &subscription)?;
    ensure!(
        body.contains("Space for annotations"),
        "Fixture extraction failed"
    );
    renderer
        .tab
        .navigate_to(&format!(
            "data:text/html;base64,{}",
            STANDARD.encode(
                "<div class=\"body markup\">Preview</div><div class=\"paywall\">Subscribe</div>"
            )
        ))?
        .wait_until_navigated()?;
    ensure!(
        extract(&renderer.tab, &post, &subscription).is_err(),
        "Paywall preview was silently accepted"
    );
    subscription.kind = Some("free".into());
    ensure!(
        extract(&renderer.tab, &post, &subscription)?.contains("Paid subscription required"),
        "Missing paid notice"
    );
    let articles = [Article {
        post,
        publication: "Sample publication".into(),
        body,
    }];
    renderer.pdf(&articles, config, "Substack · October 7", output)?;
    println!("Browser extraction, paywall and PDF smoke test passed");
    Ok(())
}
