//! The reference side of R2's comparisons (`openspec/changes/r2-front-end/design.md`).
//!
//! `refdump tokens FILE` — the Rust lexer's tokens, one per line, `Kind start end payload`, then
//! its diagnostics, `error CODE start end message`.
//! `refdump tree FILE` — the Rust parser's tree, `{:#?}`, then every diagnostic of the parse.
//!
//! Every payload and message is written with [`esc`], byte by byte, so the Nazm side produces
//! the same bytes without a Unicode table.

use nazm_syntax::{FPiece, TokenKind as T};

fn esc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if !(0x20..0x7f).contains(&b) || b == b'\\' || b == b'|' {
            out.push_str(&format!("\\x{b:02x}"));
        } else {
            out.push(char::from(b));
        }
    }
    out
}

fn kind(k: &T) -> (String, String) {
    let name = format!("{k:?}");
    let name = name.split('(').next().unwrap_or("").to_owned();
    let payload = match k {
        T::Int(v) => v.to_string(),
        T::IntTooLarge(d) | T::Float(d) => d.clone(),
        T::Str(s) | T::Ident(s) => esc(s),
        T::FStr(pieces) => pieces
            .iter()
            .map(|p| match p {
                FPiece::Text(t) => format!("T:{}", esc(t)),
                FPiece::Hole {
                    text,
                    at,
                    spec,
                    span,
                } => format!(
                    "H:{at}:{}:{}:{}:{}:{}:{}",
                    match spec.align {
                        Some(nazm_syntax::Align::Left) => "<",
                        Some(nazm_syntax::Align::Right) => ">",
                        None => "-",
                    },
                    spec.width.map_or("-".to_owned(), |w| w.to_string()),
                    spec.precision.map_or("-".to_owned(), |w| w.to_string()),
                    span.start,
                    span.end,
                    esc(text)
                ),
            })
            .collect::<Vec<_>>()
            .join("|"),
        _ => String::new(),
    };
    (name, payload)
}

fn diag_line(d: &nazm_diag::Diagnostic) -> String {
    format!(
        "error {} {} {} {}",
        d.code.as_str(),
        d.primary.span.start,
        d.primary.span.end,
        esc(&d.message)
    )
}

/// `{:#?}` with every `Span { file: FileId(0), start: S, end: E }` written `S..E` (design D3):
/// seven lines a span become one, and nothing else changes. A span of another file is left
/// whole, so it would show as a difference.
fn compact_spans(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.ends_with("Span {") && i + 6 < lines.len() {
            let t = |k: usize| lines[i + k].trim();
            let (s, e) = (t(4), t(5));
            if t(1) == "file: FileId(" && t(2) == "0," && t(3) == ")," && t(6).starts_with('}') {
                if let (Some(s), Some(e)) = (
                    s.strip_prefix("start: ").and_then(|v| v.strip_suffix(',')),
                    e.strip_prefix("end: ").and_then(|v| v.strip_suffix(',')),
                ) {
                    out.push_str(&line[..line.len() - "Span {".len()]);
                    out.push_str(&format!("{s}..{e}{}\n", &t(6)[1..]));
                    i += 7;
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(mode), Some(path)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: refdump tokens|tree FILE");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(path).expect("read the source");
    let out = match mode.as_str() {
        "tokens" => {
            let mut out = String::new();
            let (tokens, errors) = nazm_syntax::lex(&text);
            for t in &tokens {
                if t.kind == T::Eof {
                    continue;
                }
                let (name, payload) = kind(&t.kind);
                out.push_str(&format!(
                    "{name} {} {} {payload}\n",
                    t.span.start, t.span.end
                ));
            }
            for e in &errors {
                out.push_str(&diag_line(e));
                out.push('\n');
            }
            out
        }
        "tree" => std::thread::Builder::new()
            .stack_size(nazm_syntax::PARSE_STACK_BYTES + (64 << 20))
            .spawn(move || {
                let (program, errors) = nazm_syntax::parse_source(&text);
                let mut s = compact_spans(&format!("{program:#?}\n"));
                for e in &errors {
                    s.push_str(&diag_line(e));
                    s.push('\n');
                }
                s
            })
            .expect("spawn")
            .join()
            .expect("the parse"),
        _ => {
            eprintln!("unknown mode {mode}");
            std::process::exit(2);
        }
    };
    print!("{out}");
}
