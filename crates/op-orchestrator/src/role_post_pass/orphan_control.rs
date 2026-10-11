//! Recover an empty button followed by its clearly named, misparented parts
//! before contrast repair or empty-stub cleanup can destroy the control.

use super::*;

fn part_stem(node: &Value) -> Option<String> {
    let name = node.get("name")?.as_str()?.to_lowercase();
    let mut words: Vec<_> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let original_len = words.len();
    while words.last().is_some_and(|word| {
        matches!(
            *word,
            "button" | "btn" | "icon" | "label" | "text" | "glyph"
        )
    }) {
        words.pop();
    }
    (!words.is_empty() && words.len() < original_len).then(|| words.join(" "))
}

fn positioned(node: &Value) -> bool {
    node.get("x").is_some() || node.get("y").is_some()
}

pub(super) fn adopt_orphan_button_parts(node: &mut Value) {
    if node.get("layout").and_then(Value::as_str) != Some("horizontal") {
        return;
    }
    let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) else {
        return;
    };
    let mut index = 0;
    while index < children.len() {
        let button = &children[index];
        let eligible = button.get("type").and_then(Value::as_str) == Some("frame")
            && matches!(role_of(button), Some("button" | "icon-button"))
            && !positioned(button)
            && button
                .get("children")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty);
        if let Some(stem) = eligible.then(|| part_stem(button)).flatten() {
            let count = children[index + 1..]
                .iter()
                .take_while(|part| {
                    matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("text" | "icon_font" | "path")
                    ) && !positioned(part)
                        && part.get("events").is_none()
                        && part_stem(part).as_deref() == Some(stem.as_str())
                })
                .count();
            // A compact control has at most an icon, label and trailing icon.
            // Larger runs are ambiguous; preserve them for validation.
            if (1..=3).contains(&count) {
                let parts: Vec<_> = children.drain(index + 1..index + 1 + count).collect();
                children[index]["children"] = Value::Array(parts);
            }
        }
        index += 1;
    }
}
