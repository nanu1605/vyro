//! A small PDF writer for the diagnostics report: A4 pages of text in the standard Helvetica
//! fonts (WinAnsi encoding, nothing embedded), wrapped with Helvetica's own widths, with plain
//! rules between sections. Content streams are left uncompressed, so the text stays searchable
//! and testable.

use super::export::{counts, name, source_text, status_line, text, ExportOptions, FOOTNOTE, TITLE};
use super::{fmt_citation, CheckStatus, DiagReport};

const W: f32 = 595.0;
const H: f32 = 842.0;
const MARGIN: f32 = 50.0;
const BOTTOM: f32 = 60.0;
const BODY: f32 = 10.0;

/// Helvetica widths (1/1000 em) for 32–126, from its AFM.
const WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32–47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0–9
    278, 278, 584, 584, 584, 556, 1015, // :–@
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667,
    611, 722, 667, 944, 667, 667, 611, // A–Z
    278, 278, 278, 469, 556, 333, // [–`
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500,
    278, 556, 500, 722, 500, 500, 500, // a–z
    334, 260, 334, 584, // {–~
];

/// A character as WinAnsi bytes (a few become ASCII pairs); `?` when it has none.
fn win_ansi(c: char, out: &mut Vec<u8>) {
    let b: &[u8] = match c {
        ' '..='~' => {
            out.push(c as u8);
            return;
        }
        '\u{a0}'..='\u{ff}' => {
            out.push(c as u32 as u8);
            return;
        }
        '€' => &[0x80],
        '…' => &[0x85],
        '•' => &[0x95],
        '–' => &[0x96],
        '—' => &[0x97],
        '‘' => &[0x91],
        '’' => &[0x92],
        '“' => &[0x93],
        '”' => &[0x94],
        '™' => &[0x99],
        '−' => b"-",
        '→' => b"->",
        '≥' => b">=",
        '≤' => b"<=",
        _ => b"?",
    };
    out.extend_from_slice(b);
}

fn encode(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len());
    for c in s.chars() {
        win_ansi(c, &mut v);
    }
    v
}

/// Width of `s` in points at `size`.
fn width(s: &str, size: f32, bold: bool) -> f32 {
    let units: u32 = encode(s)
        .iter()
        .map(|b| match b {
            32..=126 => u32::from(WIDTHS[usize::from(b - 32)]),
            0xB0 => 400,
            0xB7 => 278,
            0xB1 | 0xD7 | 0xF7 => 584,
            0x97 => 1000,
            _ => 556,
        })
        .sum();
    let w = units as f32 * size / 1000.0;
    if bold {
        w * 1.08
    } else {
        w
    }
}

/// Splits `s` into lines no wider than `max`.
fn wrap(s: &str, size: f32, bold: bool, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let trial = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if width(&trial, size, bold) <= max || line.is_empty() {
            line = trial;
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// A PDF literal string, escaped.
fn lit(s: &str) -> Vec<u8> {
    let mut v = vec![b'('];
    for b in encode(s) {
        if matches!(b, b'(' | b')' | b'\\') {
            v.push(b'\\');
        }
        v.push(b);
    }
    v.push(b')');
    v
}

#[derive(Clone, Copy)]
struct Rgb(f32, f32, f32);

const INK: Rgb = Rgb(0.11, 0.14, 0.17);
const MUTED: Rgb = Rgb(0.34, 0.38, 0.42);
const RULE: Rgb = Rgb(0.85, 0.87, 0.9);

fn status_color(s: CheckStatus) -> Rgb {
    match s {
        CheckStatus::Pass => Rgb(0.12, 0.54, 0.3),
        CheckStatus::Attention => Rgb(0.71, 0.33, 0.04),
        CheckStatus::Info => Rgb(0.11, 0.3, 0.85),
        CheckStatus::Unavailable => MUTED,
    }
}

struct Doc {
    pages: Vec<Vec<u8>>,
    cur: Vec<u8>,
    y: f32,
}

impl Doc {
    fn new() -> Self {
        Self {
            pages: Vec::new(),
            cur: Vec::new(),
            y: H - MARGIN,
        }
    }

    fn new_page(&mut self) {
        self.pages.push(std::mem::take(&mut self.cur));
        self.y = H - MARGIN;
    }

    /// Makes room for `h` points, starting a page when needed.
    fn room(&mut self, h: f32) {
        if self.y - h < BOTTOM {
            self.new_page();
        }
    }

    fn put(&mut self, x: f32, size: f32, bold: bool, color: Rgb, s: &str) {
        let font = if bold { "F2" } else { "F1" };
        self.cur.extend_from_slice(
            format!(
                "BT /{font} {size} Tf {:.3} {:.3} {:.3} rg {x:.2} {:.2} Td ",
                color.0, color.1, color.2, self.y
            )
            .as_bytes(),
        );
        self.cur.extend_from_slice(&lit(s));
        self.cur.extend_from_slice(b" Tj ET\n");
    }

    /// Wrapped text from `x` to the right margin; moves down.
    fn para(&mut self, x: f32, size: f32, bold: bool, color: Rgb, s: &str) {
        let lead = size * 1.35;
        for line in wrap(s, size, bold, W - MARGIN - x) {
            self.room(lead);
            self.y -= lead;
            self.put(x, size, bold, color, &line);
        }
    }

    fn rule(&mut self) {
        self.room(10.0);
        self.y -= 6.0;
        self.cur.extend_from_slice(
            format!(
                "{:.3} {:.3} {:.3} RG 0.6 w {MARGIN} {y:.2} m {:.2} {y:.2} l S\n",
                RULE.0,
                RULE.1,
                RULE.2,
                W - MARGIN,
                y = self.y
            )
            .as_bytes(),
        );
        self.y -= 4.0;
    }

    fn gap(&mut self, h: f32) {
        self.y -= h;
    }

    /// Rows of three columns (reading, value, from), each wrapped in its column.
    fn table(&mut self, rows: &[[String; 3]], header: bool) {
        let cols = [MARGIN + 8.0, MARGIN + 270.0, MARGIN + 370.0];
        let widths = [255.0, 95.0, W - MARGIN - cols[2]];
        let size = BODY - 0.5;
        let lead = size * 1.3;
        let mut all: Vec<(&[String; 3], bool)> = Vec::new();
        let head = [
            String::from("Reading"),
            String::from("Value"),
            String::from("From"),
        ];
        if header {
            all.push((&head, true));
        }
        all.extend(rows.iter().map(|r| (r, false)));
        for (row, is_head) in all {
            let cells: Vec<Vec<String>> = (0..3)
                .map(|i| wrap(&row[i], size, is_head, widths[i]))
                .collect();
            let n = cells.iter().map(Vec::len).max().unwrap_or(1);
            self.room(lead * n as f32 + 2.0);
            let top = self.y;
            for (i, lines) in cells.iter().enumerate() {
                self.y = top;
                for l in lines {
                    self.y -= lead;
                    let color = if is_head || i == 2 { MUTED } else { INK };
                    self.put(cols[i], size, is_head || i == 1, color, l);
                }
            }
            self.y = top - lead * n as f32 - 2.0;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if !self.cur.is_empty() || self.pages.is_empty() {
            self.pages.push(std::mem::take(&mut self.cur));
        }
        let n = self.pages.len();
        for (i, p) in self.pages.iter_mut().enumerate() {
            let label = format!("{TITLE} · page {} of {n}", i + 1);
            p.extend_from_slice(
                format!(
                    "BT /F1 8 Tf {:.3} {:.3} {:.3} rg {MARGIN} 30 Td ",
                    MUTED.0, MUTED.1, MUTED.2
                )
                .as_bytes(),
            );
            p.extend_from_slice(&lit(&label));
            p.extend_from_slice(b" Tj ET\n");
        }
        assemble(&self.pages)
    }
}

/// Writes the objects, the cross-reference table and the trailer.
fn assemble(pages: &[Vec<u8>]) -> Vec<u8> {
    // 1 catalog, 2 pages, 3 Helvetica, 4 Helvetica-Bold, 5 info, then a page and its
    // contents for each page.
    let mut objs: Vec<Vec<u8>> = Vec::new();
    let page_id = |i: usize| 6 + 2 * i;
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", page_id(i)))
        .collect();
    objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objs.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        )
        .into_bytes(),
    );
    objs.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    objs.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    let mut info = b"<< /Producer (Vyro) /Title ".to_vec();
    info.extend_from_slice(&lit(TITLE));
    info.extend_from_slice(b" >>");
    objs.push(info);
    for (i, content) in pages.iter().enumerate() {
        objs.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                page_id(i) + 1
            )
            .into_bytes(),
        );
        let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        s.extend_from_slice(content);
        s.extend_from_slice(b"endstream");
        objs.push(s);
    }
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objs.len());
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

pub fn pdf(r: &DiagReport, o: &ExportOptions) -> Vec<u8> {
    let mut d = Doc::new();
    d.para(MARGIN, 18.0, true, INK, TITLE);
    d.gap(2.0);
    d.para(MARGIN, BODY - 1.0, false, MUTED, &status_line(r, o));
    d.para(MARGIN, BODY + 1.0, true, INK, &counts(r));
    d.gap(6.0);
    d.para(MARGIN, 12.5, true, INK, "This PC");
    for f in r.pc.iter().chain(&o.serials) {
        d.para(
            MARGIN + 8.0,
            BODY,
            false,
            INK,
            &format!("{}: {}", f.label, f.value),
        );
    }
    d.gap(6.0);
    d.para(MARGIN, 12.5, true, INK, "Checks");
    for c in &r.checks {
        d.rule();
        d.room(40.0);
        d.para(
            MARGIN,
            11.5,
            true,
            INK,
            &format!("{} — {}", name(c), c.status.label()),
        );
        // The status again in its colour, small, under the name.
        d.para(
            MARGIN,
            BODY - 1.5,
            true,
            status_color(c.status),
            c.status.label().to_uppercase().as_str(),
        );
        d.para(MARGIN, BODY, false, INK, &text(&c.summary, o));
        if let Some(a) = &c.advice {
            let head = if c.status == CheckStatus::Unavailable {
                "Why"
            } else {
                "What to do"
            };
            d.para(
                MARGIN,
                BODY,
                false,
                MUTED,
                &format!("{head}: {}", text(a, o)),
            );
        }
        if !c.citations.is_empty() {
            d.gap(2.0);
            let rows: Vec<[String; 3]> = c
                .citations
                .iter()
                .map(|x| [x.label.clone(), fmt_citation(x, o.unit), source_text(x)])
                .collect();
            d.table(&rows, true);
        }
    }
    d.rule();
    d.para(MARGIN, BODY - 1.0, false, MUTED, FOOTNOTE);
    d.finish()
}

#[cfg(test)]
pub(crate) fn text_runs(pdf: &[u8]) -> Vec<String> {
    // The literal strings shown with Tj, decoded from WinAnsi (ASCII and Latin-1 only).
    let s: String = pdf.iter().map(|b| char::from(*b)).collect();
    let mut out = Vec::new();
    let mut rest = s.as_str();
    while let Some(end) = rest.find(") Tj") {
        let head = &rest[..end];
        if let Some(start) = head.rfind("Td (") {
            out.push(
                head[start + 4..]
                    .replace("\\(", "(")
                    .replace("\\)", ")")
                    .replace("\\\\", "\\"),
            );
        }
        rest = &rest[end + 4..];
    }
    out
}
