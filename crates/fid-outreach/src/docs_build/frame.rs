//! HTML page frame and stylesheet for `docs-build` (spec node `OUTR.DOCS.FRAME.NAV`).
//!
//! Every page of an edition is wrapped in one static frame:
//! - skip link (`#main-content`) and `<html lang="en">` for accessibility;
//! - `<header>` with the project placeholder name (until D-002);
//! - release banner showing the release tag (REQ-OUTR.DOCS.FRAME.NAV-002);
//! - `<!-- edition-notice -->` directly below the banner (REQ-OUTR.DOCS.FRAME.NAV-003);
//! - `<nav>` generated from `nav.toml` with relative links ending in a file name;
//! - `<main id="main-content">` holding the page body;
//! - `<footer>` with the release tag and home link.
//!
//! No JavaScript is emitted here (search files are added by `OUTR.DOCS.SEARCH`).
//! All stylesheet colours resolve through CSS custom properties (`var(--...)`), with no literal
//! colours and no off-origin URLs.

use super::markdown::{escape_html, relative_href};
use super::{NavItem, NavTree};

/// Placeholder project name in the frame header until D-002 chooses the name.
pub const PROJECT_NAME: &str = "Metrology Maven Docs";
/// Exact notice placeholder directly below the release banner (REQ-OUTR.DOCS.FRAME.NAV-003).
pub const EDITION_NOTICE_PLACEHOLDER: &str = "<!-- edition-notice -->";
/// Stylesheet path relative to the edition root.
pub const FRAME_CSS_PATH: &str = "frame.css";

/// Static stylesheet written once per edition at `<out>/<release>/frame.css`.
/// Uses custom properties only (no literal colours, no remote URLs) so it passes `qa a11y`
/// and `asset-check` out of the box.
pub const FRAME_CSS: &str = "\
body{margin:0;background:var(--surface-base);color:var(--text-body);font:16px/1.6 system-ui,sans-serif}
a{color:var(--text-body)}
.skip-link{position:absolute;left:-9999px;top:0;padding:0.5rem 1rem;background:var(--surface-base);color:var(--text-body)}
.skip-link:focus{left:0.5rem;top:0.5rem;z-index:10}
.site-header{padding:0.75rem 1.25rem;border-bottom:1px solid var(--text-muted)}
.site-brand{font-weight:700;text-decoration:none}
.release-banner{padding:0.5rem 1.25rem;border-bottom:1px solid var(--text-muted);font-size:0.95rem}
.page-layout{display:grid;grid-template-columns:16rem minmax(0,1fr);gap:1.5rem;max-width:76rem;margin:0 auto;padding:1.25rem}
.site-nav{border-right:1px solid var(--text-muted);padding-right:1rem}
.nav-section{margin-bottom:1rem}
.nav-section-title{margin:0 0 0.35rem;font-weight:700;font-size:0.95rem}
.nav-list{list-style:none;margin:0;padding-left:0}
.nav-list .nav-list{padding-left:1rem;margin-top:0.25rem}
.nav-item{margin:0.2rem 0}
.nav-item a[aria-current=\"page\"]{font-weight:700}
main{min-width:0}
pre{overflow-x:auto;padding:0.75rem;border:1px solid var(--text-muted)}
code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:0.92em}
table{border-collapse:collapse;margin:1rem 0;width:100%}
th,td{border:1px solid var(--text-muted);padding:0.4rem 0.6rem;text-align:left}
.align-left{text-align:left}
.align-center{text-align:center}
.align-right{text-align:right}
blockquote{margin:1rem 0;padding:0.25rem 1rem;border-left:3px solid var(--text-muted)}
.site-footer{margin-top:2rem;padding:1rem 1.25rem;border-top:1px solid var(--text-muted);font-size:0.9rem;color:var(--text-body)}
@media (max-width:48rem){.page-layout{grid-template-columns:1fr}.site-nav{border-right:0;border-bottom:1px solid var(--text-muted);padding-right:0;padding-bottom:1rem}}
";

/// Inputs to render one framed HTML page.
pub struct FrameParams<'a> {
    /// Edition release tag (e.g. `v0.2.0`).
    pub release: &'a str,
    /// Page title (un-escaped text).
    pub title: &'a str,
    /// Output path of this page relative to `<out>/<release>/` (e.g. `book/units.html`).
    pub current_page: &'a str,
    /// Output path of the edition entry page relative to `<out>/<release>/` (e.g. `book/index.html`).
    pub home_page: &'a str,
    /// Parsed and validated navigation tree.
    pub nav: &'a NavTree,
    /// Inner HTML content (rendered Markdown or raw reference HTML fragment).
    pub body_html: &'a str,
}

/// Renders one complete HTML page wrapped in the edition frame.
pub fn render_page(p: &FrameParams<'_>) -> String {
    let css_href = relative_href(p.current_page, FRAME_CSS_PATH);
    let home_href = relative_href(p.current_page, p.home_page);
    let esc_release = escape_html(p.release);
    let esc_title = escape_html(p.title);
    let esc_project = escape_html(PROJECT_NAME);

    let mut out = String::with_capacity(p.body_html.len() + 1024);
    out.push_str("<!doctype html>\n");
    out.push_str("<html lang=\"en\">\n");
    out.push_str("<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!("<title>{esc_title} — {esc_project} {esc_release}</title>\n"));
    out.push_str(&format!("<link rel=\"stylesheet\" href=\"{}\">\n", escape_html(&css_href)));
    out.push_str("</head>\n");
    out.push_str("<body>\n");
    out.push_str("<a class=\"skip-link\" href=\"#main-content\">Skip to content</a>\n");
    out.push_str("<header class=\"site-header\">\n");
    out.push_str(&format!("<a class=\"site-brand\" href=\"{}\">{esc_project}</a>\n", escape_html(&home_href)));
    out.push_str("</header>\n");
    out.push_str(&format!(
        "<div class=\"release-banner\" role=\"region\" aria-label=\"Release\">Documentation for release <strong class=\"release-tag\">{esc_release}</strong></div>\n"
    ));
    out.push_str(EDITION_NOTICE_PLACEHOLDER);
    out.push('\n');
    out.push_str("<div class=\"page-layout\">\n");
    render_nav(p.nav, p.current_page, &mut out);
    out.push_str("<main id=\"main-content\">\n");
    out.push_str(p.body_html);
    if !p.body_html.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("</main>\n");
    out.push_str("</div>\n");
    out.push_str("<footer class=\"site-footer\">\n");
    out.push_str(&format!("<p>{esc_project} · {esc_release} · <a href=\"{}\">Home</a></p>\n", escape_html(&home_href)));
    out.push_str("</footer>\n");
    out.push_str("</body>\n");
    out.push_str("</html>\n");
    out
}

fn render_nav(nav: &NavTree, current_page: &str, out: &mut String) {
    out.push_str("<nav class=\"site-nav\" aria-label=\"Documentation\">\n");
    for section in &nav.sections {
        out.push_str("<section class=\"nav-section\">\n");
        out.push_str(&format!("<p class=\"nav-section-title\">{}</p>\n", escape_html(&section.title)));
        render_nav_items(&section.items, current_page, out);
        out.push_str("</section>\n");
    }
    out.push_str("</nav>\n");
}

fn render_nav_items(items: &[NavItem], current_page: &str, out: &mut String) {
    if items.is_empty() {
        return;
    }
    out.push_str("<ul class=\"nav-list\">\n");
    for item in items {
        let href = relative_href(current_page, &item.output_path);
        let current_attr = if item.output_path == current_page { " aria-current=\"page\"" } else { "" };
        out.push_str(&format!(
            "<li class=\"nav-item\"><a href=\"{}\"{current_attr}>{}</a>",
            escape_html(&href),
            escape_html(&item.title)
        ));
        if !item.children.is_empty() {
            out.push('\n');
            render_nav_items(&item.children, current_page, out);
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
}
