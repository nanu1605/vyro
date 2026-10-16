//! Exports of a report: JSON, Markdown and HTML here, PDF in `pdf`. All of them take the text
//! from the same helpers ([`fmt_citation`], [`counts`], [`status_line`]), so every format shows
//! the values the app shows.

use super::{fmt_ago, fmt_citation, CheckId, CheckStatus, Citation, DiagCheck, DiagReport, Field};
use crate::units::{temps_in, TempUnit};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportOptions {
    pub unit: TempUnit,
    /// The report's time as the app shows it, e.g. "15 Oct 2026, 10:24".
    pub when: String,
    /// Serial numbers to include, by device; empty unless the owner asked.
    pub serials: Vec<Field>,
}

pub const TITLE: &str = "Vyro diagnostics";

/// "Generated 15 Oct 2026, 10:24 · checks v1 · Vyro 0.4.0 · Vyro service 0.4.0".
pub fn status_line(r: &DiagReport, o: &ExportOptions) -> String {
    format!(
        "Generated {} · checks v{} · Vyro {} · {}",
        o.when, r.version, r.app_version, r.engine
    )
}

/// "2 attention · 8 pass · 2 info · 1 unavailable".
pub fn counts(r: &DiagReport) -> String {
    [
        CheckStatus::Attention,
        CheckStatus::Pass,
        CheckStatus::Info,
        CheckStatus::Unavailable,
    ]
    .iter()
    .map(|s| format!("{} {}", r.count(*s), s.label().to_lowercase()))
    .collect::<Vec<_>>()
    .join(" · ")
}

/// The check's name, e.g. "Fans and pump".
pub fn name(c: &DiagCheck) -> &'static str {
    CheckId::label(c.id)
}

/// "7-day history" or "Health log, 3 d before".
pub fn source_text(c: &Citation) -> String {
    match c.ago {
        Some(a) => format!("{}, {}", c.source.label(), fmt_ago(a)),
        None => c.source.label().to_owned(),
    }
}

/// Text from the engine (summaries, advice) in the chosen unit.
pub fn text(s: &str, o: &ExportOptions) -> String {
    temps_in(s, o.unit)
}

pub const FOOTNOTE: &str =
    "Measured values come from Vyro's own readings and history; Info entries are never faults. \
                            Serial numbers are left out unless asked for.";

pub fn json(r: &DiagReport, o: &ExportOptions) -> String {
    let mut v = serde_json::to_value(r).unwrap_or(serde_json::Value::Null);
    if let serde_json::Value::Object(map) = &mut v {
        if !o.serials.is_empty() {
            if let Ok(s) = serde_json::to_value(&o.serials) {
                map.insert("serials".into(), s);
            }
        }
    }
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn markdown(r: &DiagReport, o: &ExportOptions) -> String {
    let mut out = format!(
        "# {TITLE}\n\n{}\n\n**{}**\n\n## This PC\n\n| | |\n|---|---|\n",
        status_line(r, o),
        counts(r)
    );
    for f in r.pc.iter().chain(&o.serials) {
        out.push_str(&format!(
            "| {} | {} |\n",
            md_cell(&f.label),
            md_cell(&f.value)
        ));
    }
    out.push_str("\n## Checks\n");
    for c in &r.checks {
        out.push_str(&format!(
            "\n### {} — {}\n\n{}\n",
            name(c),
            c.status.label(),
            text(&c.summary, o)
        ));
        if let Some(a) = &c.advice {
            let head = if c.status == CheckStatus::Unavailable {
                "Why"
            } else {
                "What to do"
            };
            out.push_str(&format!("\n**{head}:** {}\n", text(a, o)));
        }
        if !c.citations.is_empty() {
            out.push_str("\n| Reading | Value | From |\n|---|---|---|\n");
            for x in &c.citations {
                out.push_str(&format!(
                    "| {} | {} | {} |\n",
                    md_cell(&x.label),
                    md_cell(&fmt_citation(x, o.unit)),
                    md_cell(&source_text(x))
                ));
            }
        }
    }
    out.push_str(&format!("\n---\n\n{FOOTNOTE}\n"));
    out
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

const CSS: &str = "body{font:14px/1.45 'Segoe UI',system-ui,sans-serif;color:#1d232b;background:#fff;max-width:56rem;margin:2rem auto;padding:0 1rem}\
h1{font-size:1.5rem;margin:0}h2{font-size:1.1rem;margin:1.6rem 0 .5rem}h3{font-size:1rem;margin:0}\
.meta{color:#56606c;margin:.3rem 0 0}.counts{font-weight:600;margin:.6rem 0 0}\
table{border-collapse:collapse;width:100%;margin:.4rem 0 0}th,td{text-align:left;padding:.25rem .5rem .25rem 0;border-bottom:1px solid #e3e7ec;vertical-align:top}\
th{color:#56606c;font-weight:600;font-size:.85rem}td.v{white-space:nowrap;font-variant-numeric:tabular-nums}\
.check{border:1px solid #d9dee5;border-left-width:4px;border-radius:6px;padding:.7rem .9rem;margin:.7rem 0;break-inside:avoid}\
.check header{display:flex;gap:.6rem;align-items:baseline;flex-wrap:wrap}.pill{font-size:.75rem;font-weight:700;padding:0 .45rem;border-radius:99px;border:1px solid}\
.pass{border-left-color:#1f8a4c}.pass .pill{color:#1f8a4c}.attention{border-left-color:#b45309}.attention .pill{color:#b45309}\
.info{border-left-color:#1d4ed8}.info .pill{color:#1d4ed8}.unavailable{border-left-color:#8a94a1}.unavailable .pill{color:#56606c}\
p{margin:.35rem 0}.advice{color:#3b444f}footer{color:#56606c;font-size:.85rem;margin-top:2rem}";

pub fn html(r: &DiagReport, o: &ExportOptions) -> String {
    let mut out = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{TITLE}</title><style>{CSS}</style></head><body>\n<h1>{TITLE}</h1>\n<p class=\"meta\">{}</p>\n<p class=\"counts\">{}</p>\n\
         <h2>This PC</h2>\n<table><tbody>\n",
        esc(&status_line(r, o)),
        esc(&counts(r))
    );
    for f in r.pc.iter().chain(&o.serials) {
        out.push_str(&format!(
            "<tr><th scope=\"row\">{}</th><td>{}</td></tr>\n",
            esc(&f.label),
            esc(&f.value)
        ));
    }
    out.push_str("</tbody></table>\n<h2>Checks</h2>\n");
    for c in &r.checks {
        let cls = c.status.label().to_lowercase();
        out.push_str(&format!(
            "<section class=\"check {cls}\"><header><h3>{}</h3><span class=\"pill\">{}</span></header>\n<p>{}</p>\n",
            esc(name(c)),
            esc(c.status.label()),
            esc(&text(&c.summary, o))
        ));
        if let Some(a) = &c.advice {
            let head = if c.status == CheckStatus::Unavailable {
                "Why"
            } else {
                "What to do"
            };
            out.push_str(&format!(
                "<p class=\"advice\"><b>{head}:</b> {}</p>\n",
                esc(&text(a, o))
            ));
        }
        if !c.citations.is_empty() {
            out.push_str("<table><thead><tr><th>Reading</th><th>Value</th><th>From</th></tr></thead><tbody>\n");
            for x in &c.citations {
                out.push_str(&format!(
                    "<tr><td>{}</td><td class=\"v\">{}</td><td>{}</td></tr>\n",
                    esc(&x.label),
                    esc(&fmt_citation(x, o.unit)),
                    esc(&source_text(x))
                ));
            }
            out.push_str("</tbody></table>\n");
        }
        out.push_str("</section>\n");
    }
    out.push_str(&format!(
        "<footer>{}</footer>\n</body></html>\n",
        esc(FOOTNOTE)
    ));
    out
}
