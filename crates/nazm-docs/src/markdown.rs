//! The Markdown structure this corpus uses, and nothing more: ATX headings, fenced code,
//! HTML comments, explicit anchors and inline links.
//!
//! Not a CommonMark engine. The canonical documents use one heading form — `#` to
//! `######` at the start of a line — and fenced code with backticks or tildes; that is what
//! is recognised. The two heading forms they do not use, setext underlines and HTML
//! `<h1>`–`<h6>`, are **refused** rather than guessed at, so a document that starts using
//! one fails the corpus instead of being silently mis-sectioned.

use crate::Problem;

/// One ATX heading, where it starts, and the anchor written under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Heading {
    pub(crate) level: u8,
    /// The heading's text exactly, without its `#`s or a closing sequence.
    pub(crate) title: String,
    /// The byte its line starts at.
    pub(crate) start: usize,
    /// Its line, counted from zero.
    pub(crate) line: usize,
    /// The explicit anchor on the line directly below it, `<!-- id: NAME -->`.
    pub(crate) anchor: Option<String>,
}

/// The anchor syntax, before and after its name.
const ANCHOR_OPEN: &str = "<!-- id:";
const ANCHOR_CLOSE: &str = "-->";

/// Every heading of `text`, in source order, or every structural problem in it.
pub(crate) fn headings(document: &str, text: &str) -> Result<Vec<Heading>, Vec<Problem>> {
    let mut out: Vec<Heading> = Vec::new();
    let mut problems = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut comment = false;
    let mut start = 0usize;
    // The previous line, when it could be a paragraph a setext underline would turn into a
    // heading.
    let mut paragraph = false;
    let mut previous_heading = false;

    for (line, raw) in text.split_inclusive('\n').enumerate() {
        let here = start;
        start += raw.len();
        let l = raw.trim_end_matches(['\n', '\r']);
        let trimmed = l.trim_start();
        let was_heading = std::mem::replace(&mut previous_heading, false);
        let was_paragraph = std::mem::replace(&mut paragraph, false);

        if let Some((c, n)) = fence {
            if closes(trimmed, c, n) {
                fence = None;
            }
            continue;
        }
        if comment {
            if l.contains("-->") {
                comment = false;
            }
            continue;
        }
        if let Some(open) = opens(trimmed) {
            fence = Some(open);
            continue;
        }

        if trimmed.starts_with(ANCHOR_OPEN) || trimmed.starts_with("<!--id:") {
            match (was_heading, anchor_name(trimmed)) {
                (true, Some(name)) => {
                    if let Some(h) = out.last_mut() {
                        h.anchor = Some(name);
                    }
                }
                (true, None) => problems.push(Problem::at("malformed_anchor", document, line, l)),
                (false, _) => problems.push(Problem::at("misplaced_anchor", document, line, l)),
            }
            continue;
        }
        if let Some(i) = l.find("<!--") {
            if !l.get(i..).is_some_and(|rest| rest.contains("-->")) {
                comment = true;
                continue;
            }
        }

        if let Some((level, title)) = atx(l) {
            out.push(Heading {
                level,
                title,
                start: here,
                line,
                anchor: None,
            });
            previous_heading = true;
            continue;
        }
        if html_heading(trimmed) || (was_paragraph && underline(l)) {
            problems.push(Problem::at("unsupported_heading", document, line, l));
            continue;
        }
        paragraph = could_be_paragraph(l);
    }
    if fence.is_some() {
        problems.push(Problem::of("unclosed_fence", document));
    }
    if comment {
        problems.push(Problem::of("unclosed_comment", document));
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// A fence opener: three or more backticks or tildes, at any indentation.
fn opens(trimmed: &str) -> Option<(char, usize)> {
    let c = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = trimmed.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// Whether `trimmed` closes a fence of `n` `c`s: at least as many, and nothing after them.
fn closes(trimmed: &str, c: char, n: usize) -> bool {
    let run = trimmed.chars().take_while(|x| *x == c).count();
    run >= n && trimmed.chars().skip(run).all(char::is_whitespace)
}

/// An ATX heading: up to three spaces, one to six `#`, then a space or the end.
fn atx(l: &str) -> Option<(u8, String)> {
    let indent = l.len() - l.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = l.get(indent..)?;
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = rest.get(hashes..)?;
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let mut title = after.trim();
    // An optional closing sequence: `#`s preceded by a space.
    let closing = title.trim_end_matches('#');
    if closing.len() < title.len() && (closing.is_empty() || closing.ends_with(' ')) {
        title = closing.trim_end();
    }
    Some((u8::try_from(hashes).ok()?, title.to_owned()))
}

/// `<!-- id: NAME -->`, exactly, with a name of lower-case letters, digits and hyphens
/// that starts with a letter.
fn anchor_name(trimmed: &str) -> Option<String> {
    let inner = trimmed
        .trim_end()
        .strip_prefix(ANCHOR_OPEN)?
        .strip_suffix(ANCHOR_CLOSE)?;
    let name = inner.strip_prefix(' ')?.strip_suffix(' ')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    (first.is_ascii_lowercase()
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.ends_with('-'))
    .then(|| name.to_owned())
}

fn html_heading(trimmed: &str) -> bool {
    let lower = trimmed.get(..4).map(str::to_ascii_lowercase);
    lower.is_some_and(|p| {
        let b = p.as_bytes();
        b.first() == Some(&b'<')
            && b.get(1) == Some(&b'h')
            && b.get(2).is_some_and(|d| (b'1'..=b'6').contains(d))
            && b.get(3).is_some_and(|c| *c == b'>' || *c == b' ')
    })
}

/// A setext underline: `=`s, or three or more `-`s, and nothing else.
fn underline(l: &str) -> bool {
    let t = l.trim();
    (!t.is_empty() && t.chars().all(|c| c == '=')) || (t.len() >= 3 && t.chars().all(|c| c == '-'))
}

/// A line a setext underline below it would make a heading: text that is not a list item,
/// a table row, a quotation, markup or a heading.
fn could_be_paragraph(l: &str) -> bool {
    let t = l.trim_start();
    if t.is_empty() || l.len() - t.len() > 3 {
        return false;
    }
    let listed = t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("+ ")
        || t.split_once(['.', ')'])
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    !(listed || t.starts_with('|') || t.starts_with('>') || t.starts_with('<'))
}

/// Every inline link target in `body` — `[text](target)` outside code — in order.
pub(crate) fn links(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for raw in body.lines() {
        let trimmed = raw.trim_start();
        if let Some((c, n)) = fence {
            if closes(trimmed, c, n) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = opens(trimmed) {
            fence = Some(open);
            continue;
        }
        let prose = without_code(raw);
        let mut rest = prose.as_str();
        while let Some(i) = rest.find("](") {
            let after = rest.get(i + 2..).unwrap_or_default();
            let target: String = after
                .chars()
                .take_while(|c| *c != ')' && !c.is_whitespace())
                .collect();
            if !target.is_empty()
                && after
                    .get(target.len()..)
                    .is_some_and(|r| r.starts_with(')'))
            {
                out.push(target.clone());
            }
            rest = after;
        }
    }
    out
}

/// `line` with every inline code span removed: a run of backticks up to the next run of
/// the same length.
fn without_code(line: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '`' {
            let n = chars.iter().skip(i).take_while(|x| **x == '`').count();
            let mut j = i + n;
            let mut closed = None;
            while j < chars.len() {
                if chars.get(j) == Some(&'`') {
                    let m = chars.iter().skip(j).take_while(|x| **x == '`').count();
                    if m == n {
                        closed = Some(j + m);
                        break;
                    }
                    j += m;
                } else {
                    j += 1;
                }
            }
            match closed {
                Some(end) => i = end,
                None => {
                    out.extend(chars.iter().skip(i).take(n));
                    i += n;
                }
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The anchor GitHub gives a heading, which is what a `#fragment` link names: lower case,
/// letters, digits, `-` and `_` kept, spaces made `-`, everything else dropped.
pub(crate) fn github_slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        } else if c == ' ' {
            out.push('-');
        }
    }
    out
}

/// A heading's text as an identity segment: its letters and digits, lower-cased, each run
/// of anything else one `-`. Locale-independent: `char::to_lowercase` is Unicode's.
pub(crate) fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.extend(c.to_lowercase());
        } else {
            gap = true;
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_in_a_fence_is_code_and_not_a_section() {
        let text =
            "# T\n\n```text\n# fake heading\n## another\n```\n\n~~~~\n# tilde\n~~~~\n## Real\n";
        let h = headings("d", text).unwrap();
        let titles: Vec<&str> = h.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, ["T", "Real"]);
    }

    #[test]
    fn a_longer_fence_is_closed_only_by_one_as_long() {
        let text = "# T\n````\n```\n# inside\n```\n````\n## After\n";
        let h = headings("d", text).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].title, "After");
    }

    #[test]
    fn setext_and_html_headings_are_refused_not_guessed() {
        for text in [
            "# T\n\nTitle\n=====\n",
            "# T\n\nTitle\n---\n",
            "# T\n<h2>x</h2>\n",
        ] {
            let e = headings("d", text).unwrap_err();
            assert_eq!(e[0].kind, "unsupported_heading", "{text}");
        }
        // A rule after a blank line, and a table's separator, are not headings.
        assert!(headings("d", "# T\n\ntext\n\n---\n\n| a |\n| --- |\n").is_ok());
    }

    #[test]
    fn an_anchor_belongs_to_the_heading_directly_above_it() {
        let h = headings("d", "# T\n## Heading\n<!-- id: stable-name -->\ntext\n").unwrap();
        assert_eq!(h[1].anchor.as_deref(), Some("stable-name"));
        for bad in [
            "<!-- id: Bad -->",
            "<!-- id:x -->",
            "<!-- id: 1x -->",
            "<!-- id: x- -->",
        ] {
            let e = headings("d", &format!("# T\n## H\n{bad}\n")).unwrap_err();
            assert_eq!(e[0].kind, "malformed_anchor", "{bad}");
        }
        let e = headings("d", "# T\n## H\n\n<!-- id: late -->\n").unwrap_err();
        assert_eq!(e[0].kind, "misplaced_anchor");
    }

    #[test]
    fn links_in_code_are_not_links() {
        let body = "See [spec](spec.md#return) and `f[T](a: T)`.\n```\n[x](nope.md)\n```\n[g](https://x.org/a)";
        assert_eq!(links(body), ["spec.md#return", "https://x.org/a"]);
    }

    #[test]
    fn slugs_are_unicode_and_locale_free() {
        assert_eq!(
            slug("`Vec[T]` — a mutable handle"),
            "vec-t-a-mutable-handle"
        );
        assert_eq!(slug("Ünïcödé — ÄÖ"), "ünïcödé-äö");
        assert_eq!(github_slug("`return`"), "return");
        assert_eq!(
            github_slug("What compatibility means"),
            "what-compatibility-means"
        );
    }
}
