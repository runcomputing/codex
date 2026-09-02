//! Rewrite LaTeX math into readable Unicode text before Markdown parsing.
//!
//! Models routinely answer with `\[ ... \]`, `$$ ... $$`, or `\( ... \)`. Markdown has no math, so
//! the TUI used to print the raw LaTeX source. Converting to Unicode keeps the meaning visible in
//! every terminal, including ones without image or text-sizing support.

use std::borrow::Cow;

/// Rewrite closed math spans in `source`.
///
/// Unclosed spans are left untouched so a streaming prefix keeps its source text until the closing
/// delimiter arrives, the same way an open code fence stays code.
pub(super) fn rewrite_math(source: &str) -> Cow<'_, str> {
    if !source.contains("\\[") && !source.contains("$$") && !source.contains("\\(") {
        return Cow::Borrowed(source);
    }

    let mut out = String::with_capacity(source.len());
    let mut fence: Option<(char, usize)> = None;
    let mut lines = source.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        if let Some((marker, len)) = fence {
            out.push_str(line);
            if closing_fence(trimmed, marker, len) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = opening_fence(trimmed) {
            fence = Some(open);
            out.push_str(line);
            continue;
        }
        if let Some(delimiter) = display_open(trimmed) {
            let mut block = Vec::new();
            let mut closed = false;
            let mut rest = lines.clone();
            for next in rest.by_ref() {
                let next_body = next.trim_end_matches(['\n', '\r']);
                if next_body.trim() == delimiter.close {
                    closed = true;
                    break;
                }
                block.push(next_body.to_string());
            }
            if closed {
                lines = rest;
                let trailing_newline = line.ends_with('\n');
                push_display_block(&mut out, &block, trailing_newline);
                continue;
            }
        }
        out.push_str(&rewrite_inline(body));
        out.push_str(&line[body.len()..]);
    }
    Cow::Owned(out)
}

struct DisplayDelimiter {
    close: &'static str,
}

fn display_open(trimmed: &str) -> Option<DisplayDelimiter> {
    match trimmed {
        "\\[" => Some(DisplayDelimiter { close: "\\]" }),
        "$$" => Some(DisplayDelimiter { close: "$$" }),
        _ => None,
    }
}

fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = trimmed.chars().take_while(|c| *c == marker).count();
    (len >= 3).then_some((marker, len))
}

fn closing_fence(trimmed: &str, marker: char, open_len: usize) -> bool {
    opening_fence(trimmed).is_some_and(|(close_marker, close_len)| {
        close_marker == marker && close_len >= open_len && trimmed.len() == close_len
    })
}

/// Emit the converted block as its own paragraph, one source row per line.
fn push_display_block(out: &mut String, block: &[String], trailing_newline: bool) {
    let rows = block
        .iter()
        .flat_map(|line| {
            latex_to_unicode(line)
                .split("\n")
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with("\n\n") && !out.ends_with('\n') {
        out.push('\n');
    }
    for (index, row) in rows.iter().enumerate() {
        out.push_str(row);
        if index + 1 < rows.len() {
            // Two trailing spaces keep the rows on separate rendered lines.
            out.push_str("  \n");
        }
    }
    if trailing_newline {
        out.push('\n');
    }
}

/// Convert `\( ... \)` and `$ ... $` spans inside one line.
fn rewrite_inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    let mut in_code = false;
    while !rest.is_empty() {
        if in_code {
            match rest.find('`') {
                Some(index) => {
                    out.push_str(&rest[..=index]);
                    rest = &rest[index + 1..];
                    in_code = false;
                }
                None => {
                    out.push_str(rest);
                    break;
                }
            }
            continue;
        }
        let next = ["`", "\\(", "$"]
            .into_iter()
            .filter_map(|token| rest.find(token).map(|index| (index, token)))
            .min();
        let Some((index, token)) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        match token {
            "`" => {
                out.push('`');
                rest = &rest[1..];
                in_code = true;
            }
            "\\(" => match rest.find("\\)") {
                Some(end) => {
                    out.push_str(&latex_to_unicode(&rest[2..end]));
                    rest = &rest[end + 2..];
                }
                None => {
                    out.push_str(rest);
                    break;
                }
            },
            _ => match rest[1..].find('$') {
                Some(end) if end > 0 => {
                    out.push_str(&latex_to_unicode(&rest[1..=end]));
                    rest = &rest[end + 2..];
                }
                _ => {
                    out.push('$');
                    rest = &rest[1..];
                }
            },
        }
    }
    out
}

const COMMANDS: &[(&str, &str)] = &[
    ("\\times", "×"),
    ("\\cdot", "·"),
    ("\\div", "÷"),
    ("\\pm", "±"),
    ("\\mp", "∓"),
    ("\\leq", "≤"),
    ("\\le", "≤"),
    ("\\geq", "≥"),
    ("\\ge", "≥"),
    ("\\neq", "≠"),
    ("\\ne", "≠"),
    ("\\approx", "≈"),
    ("\\equiv", "≡"),
    ("\\propto", "∝"),
    ("\\infty", "∞"),
    ("\\sum", "∑"),
    ("\\prod", "∏"),
    ("\\int", "∫"),
    ("\\partial", "∂"),
    ("\\nabla", "∇"),
    ("\\rightarrow", "→"),
    ("\\to", "→"),
    ("\\leftarrow", "←"),
    ("\\Rightarrow", "⇒"),
    ("\\Leftarrow", "⇐"),
    ("\\in", "∈"),
    ("\\notin", "∉"),
    ("\\subset", "⊂"),
    ("\\cup", "∪"),
    ("\\cap", "∩"),
    ("\\forall", "∀"),
    ("\\exists", "∃"),
    ("\\alpha", "α"),
    ("\\beta", "β"),
    ("\\gamma", "γ"),
    ("\\delta", "δ"),
    ("\\epsilon", "ε"),
    ("\\varepsilon", "ε"),
    ("\\zeta", "ζ"),
    ("\\eta", "η"),
    ("\\theta", "θ"),
    ("\\lambda", "λ"),
    ("\\mu", "μ"),
    ("\\pi", "π"),
    ("\\rho", "ρ"),
    ("\\sigma", "σ"),
    ("\\tau", "τ"),
    ("\\phi", "φ"),
    ("\\chi", "χ"),
    ("\\psi", "ψ"),
    ("\\omega", "ω"),
    ("\\Delta", "Δ"),
    ("\\Gamma", "Γ"),
    ("\\Lambda", "Λ"),
    ("\\Omega", "Ω"),
    ("\\Phi", "Φ"),
    ("\\Pi", "Π"),
    ("\\Sigma", "Σ"),
    ("\\Theta", "Θ"),
    ("\\ldots", "…"),
    ("\\dots", "…"),
    ("\\quad", " "),
    ("\\qquad", "  "),
    ("\\,", " "),
    ("\\;", " "),
    ("\\!", ""),
    ("\\left", ""),
    ("\\right", ""),
    ("\\displaystyle", ""),
    ("\\limits", ""),
];

/// Commands whose single braced argument is kept verbatim.
const UNWRAPPED: &[&str] = &[
    "\\text",
    "\\textrm",
    "\\mathrm",
    "\\mathit",
    "\\mathbf",
    "\\operatorname",
];

/// Convert one LaTeX fragment to Unicode text.
pub(super) fn latex_to_unicode(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("\\\\") {
            out.push('\n');
            rest = tail;
            continue;
        }
        if rest.starts_with('\\') {
            if let Some(tail) = environment_command_tail(rest, "\\begin") {
                rest = tail;
                continue;
            }
            if let Some(tail) = environment_command_tail(rest, "\\end") {
                rest = tail;
                continue;
            }
            if let Some((name, tail)) = unwrapped_command(rest) {
                out.push_str(&latex_to_unicode(&name));
                rest = tail;
                continue;
            }
            if let Some(tail) = rest.strip_prefix("\\frac") {
                let (numerator, tail) = take_group(tail);
                let (denominator, tail) = take_group(tail);
                out.push_str(&format!(
                    "{} / {}",
                    parenthesize(&latex_to_unicode(&numerator)),
                    parenthesize(&latex_to_unicode(&denominator)),
                ));
                rest = tail;
                continue;
            }
            if let Some(tail) = rest.strip_prefix("\\sqrt") {
                let (radicand, tail) = take_group(tail);
                out.push_str(&format!("√{}", parenthesize(&latex_to_unicode(&radicand))));
                rest = tail;
                continue;
            }
            if let Some((replacement, tail)) = command(rest) {
                out.push_str(replacement);
                rest = tail;
                continue;
            }
            // Unknown command: drop the backslash and keep the name readable.
            out.push_str(&rest[1..2]);
            rest = &rest[2..];
            continue;
        }
        let mut characters = rest.chars();
        let character = characters.next().unwrap_or_default();
        let tail = characters.as_str();
        match character {
            '{' | '}' | '&' => rest = tail,
            '^' | '_' => {
                let (script, remainder) = take_group(tail);
                let script = latex_to_unicode(&script);
                match script_characters(&script, character == '^') {
                    Some(converted) => out.push_str(&converted),
                    None => {
                        out.push(character);
                        out.push_str(&parenthesize(&script));
                    }
                }
                rest = remainder;
            }
            _ => {
                out.push(character);
                rest = tail;
            }
        }
    }
    collapse_spaces(&out)
}

fn environment_command_tail<'a>(rest: &'a str, command: &str) -> Option<&'a str> {
    let tail = rest.strip_prefix(command)?;
    if !tail.starts_with('{') {
        return None;
    }
    let (_, remainder) = take_group(tail);
    Some(remainder)
}

fn command(rest: &str) -> Option<(&'static str, &str)> {
    COMMANDS
        .iter()
        .find(|(name, _)| {
            rest.starts_with(name)
                && !rest[name.len()..]
                    .starts_with(|c: char| c.is_ascii_alphabetic() && name.len() > 2)
        })
        .map(|(name, replacement)| (*replacement, &rest[name.len()..]))
}

fn unwrapped_command(rest: &str) -> Option<(String, &str)> {
    let name = UNWRAPPED.iter().find(|name| {
        rest.starts_with(**name)
            && !rest[name.len()..].starts_with(|c: char| c.is_ascii_alphabetic())
    })?;
    let (argument, tail) = take_group(&rest[name.len()..]);
    Some((argument, tail))
}

/// Split a leading `{...}` group, or a single character when the argument is unbraced.
fn take_group(rest: &str) -> (String, &str) {
    let Some(body) = rest.strip_prefix('{') else {
        let mut characters = rest.chars();
        return match characters.next() {
            Some(character) => (character.to_string(), characters.as_str()),
            None => (String::new(), rest),
        };
    };
    let mut depth = 1usize;
    for (index, character) in body.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return (body[..index].to_string(), &body[index + 1..]);
                }
            }
            _ => {}
        }
    }
    (body.to_string(), "")
}

fn parenthesize(text: &str) -> String {
    let simple = text
        .chars()
        .all(|character| character.is_alphanumeric() || character == '.');
    if simple || text.is_empty() {
        text.to_string()
    } else {
        format!("({text})")
    }
}

fn script_characters(text: &str, superscript: bool) -> Option<String> {
    text.chars()
        .map(|character| {
            let mapped = if superscript {
                match character {
                    '0' => '⁰',
                    '1' => '¹',
                    '2' => '²',
                    '3' => '³',
                    '4' => '⁴',
                    '5' => '⁵',
                    '6' => '⁶',
                    '7' => '⁷',
                    '8' => '⁸',
                    '9' => '⁹',
                    '+' => '⁺',
                    '-' => '⁻',
                    'n' => 'ⁿ',
                    'i' => 'ⁱ',
                    _ => return None,
                }
            } else {
                match character {
                    '0' => '₀',
                    '1' => '₁',
                    '2' => '₂',
                    '3' => '₃',
                    '4' => '₄',
                    '5' => '₅',
                    '6' => '₆',
                    '7' => '₇',
                    '8' => '₈',
                    '9' => '₉',
                    '+' => '₊',
                    '-' => '₋',
                    'i' => 'ᵢ',
                    'j' => 'ⱼ',
                    'n' => 'ₙ',
                    'x' => 'ₓ',
                    _ => return None,
                }
            };
            Some(mapped)
        })
        .collect()
}

fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_space = false;
    for character in text.chars() {
        let space = character == ' ';
        if space && previous_space {
            continue;
        }
        previous_space = space;
        out.push(character);
    }
    out.trim().to_string()
}

#[cfg(test)]
#[path = "math_tests.rs"]
mod tests;
