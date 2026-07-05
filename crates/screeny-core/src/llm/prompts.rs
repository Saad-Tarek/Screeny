//! The analysis prompt and a lenient parser for model output. Small vision
//! models frequently wrap JSON in prose or code fences — never trust them.

pub const DEFAULT_PROMPT: &str = "You are analyzing a screenshot from the user's own computer. \
Respond with ONLY a JSON object, no other text:\n\
{\"ocr\": \"<transcribe all legible on-screen text, preserving line structure>\", \
\"description\": \"<2-3 sentences: which application/activity is shown and what the user is doing>\"}";

/// Extracted (ocr, description) from raw model output.
pub fn parse_response(raw: &str) -> (String, String) {
    let cleaned = strip_think_blocks(raw);
    if let Some((ocr, description)) = try_parse_json(&cleaned) {
        return (ocr, description);
    }
    // Fallback: model ignored the format; keep everything as description.
    (String::new(), cleaned.trim().to_string())
}

/// Reasoning models often prefix their answer with `<think>…</think>`.
/// Remove those blocks so only the final answer is parsed.
fn strip_think_blocks(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => {
                // Unterminated block: everything after is reasoning; drop it.
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn try_parse_json(raw: &str) -> Option<(String, String)> {
    // The JSON may be fenced (```json ... ```) or surrounded by prose; find
    // the outermost braces and try progressively.
    let candidates = [raw.trim(), strip_fences(raw), outer_braces(raw)?];
    for candidate in candidates {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate) {
            let ocr = value.get("ocr").and_then(|v| v.as_str()).unwrap_or("");
            let description = value
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !ocr.is_empty() || !description.is_empty() {
                return Some((ocr.to_string(), description.to_string()));
            }
        }
    }
    None
}

fn strip_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed)
}

fn outer_braces(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then(|| &raw[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json() {
        let (ocr, desc) = parse_response(r#"{"ocr": "hello world", "description": "A terminal."}"#);
        assert_eq!(ocr, "hello world");
        assert_eq!(desc, "A terminal.");
    }

    #[test]
    fn parses_fenced_json() {
        let raw = "```json\n{\"ocr\": \"text\", \"description\": \"An editor.\"}\n```";
        let (ocr, desc) = parse_response(raw);
        assert_eq!(ocr, "text");
        assert_eq!(desc, "An editor.");
    }

    #[test]
    fn parses_json_embedded_in_prose() {
        let raw = "Sure! Here is the analysis:\n{\"ocr\": \"abc\", \"description\": \"A browser.\"} Hope that helps.";
        let (ocr, desc) = parse_response(raw);
        assert_eq!(ocr, "abc");
        assert_eq!(desc, "A browser.");
    }

    #[test]
    fn think_blocks_are_stripped_before_parsing() {
        let raw = "<think>The user wants JSON. Let me look at the image…</think>\n{\"ocr\": \"ls -la\", \"description\": \"A terminal.\"}";
        let (ocr, desc) = parse_response(raw);
        assert_eq!(ocr, "ls -la");
        assert_eq!(desc, "A terminal.");

        // Unterminated think block (model ran out of tokens mid-reasoning)
        let (ocr2, desc2) = parse_response("<think>hmm this looks like");
        assert_eq!(ocr2, "");
        assert_eq!(desc2, "");
    }

    #[test]
    fn garbage_falls_back_to_description() {
        let (ocr, desc) = parse_response("The screen shows a code editor with a Rust file open.");
        assert_eq!(ocr, "");
        assert!(desc.contains("code editor"));
    }
}
