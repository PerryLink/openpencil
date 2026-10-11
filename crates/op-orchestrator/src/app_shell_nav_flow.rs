//! Sidebar free space belongs before the account footer, never before menus.

use super::*;

fn children(v: &Value) -> &[Value] {
    v.get("children")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn has_word(v: &Value, words: &[&str]) -> bool {
    ident_text(v)
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| words.contains(&word))
}

fn navigation(v: &Value) -> bool {
    has_word(v, &["nav", "navigation", "menu", "link"])
        || matches!(
            v.get("role").and_then(Value::as_str),
            Some("nav" | "navigation" | "menu" | "menu-item" | "link")
        )
        || ident_text(v).contains("菜单")
        || children(v).iter().any(navigation)
}

fn footer(v: &Value) -> bool {
    // "Users", "Admin" and "Settings" are ordinary menu entries. Searching
    // their substrings anywhere in a subtree previously misclassified menus.
    if navigation(v) {
        return false;
    }
    if has_word(v, &["users", "settings", "admin"])
        && !has_word(v, &["profile", "avatar", "footer"])
    {
        return false;
    }
    if has_word(
        v,
        &["profile", "avatar", "footer", "account", "member", "bottom"],
    ) || ["账户", "账号", "个人资料", "页脚"]
        .iter()
        .any(|s| ident_text(v).contains(s))
    {
        return true;
    }
    if let Some(text) = v.get("content").and_then(Value::as_str) {
        let text = text.to_lowercase();
        if [
            "owner", "sign out", "log out", "upgrade", "pro plan", "go pro",
        ]
        .iter()
        .any(|s| text.contains(s))
        {
            return true;
        }
    }
    children(v).iter().any(footer)
}

fn spacer(v: &Value) -> bool {
    // Only layout-only spacers may move. Preserve decorated/content-bearing
    // nodes even if a model happened to put "spacer" in their name.
    ident_text(v).contains("spacer")
        && v.get("type").and_then(Value::as_str) == Some("frame")
        && children(v).is_empty()
        && ["fill", "stroke", "effects", "events"].iter().all(|key| {
            v.get(key)
                .is_none_or(|value| value.is_null() || value.as_array().is_some_and(Vec::is_empty))
        })
}

fn set(v: &mut Value, key: &str, value: Value) -> bool {
    if v.get(key) == Some(&value) {
        return false;
    }
    v.as_object_mut().unwrap().insert(key.into(), value);
    true
}

fn start(v: &mut Value) -> bool {
    if matches!(
        v.get("justifyContent").and_then(Value::as_str),
        Some("center" | "end" | "space_between" | "space_around" | "space_evenly")
    ) {
        return set(v, "justifyContent", json!("start"));
    }
    false
}

/// Top/body columns may stretch or scroll, but their contents start at the top.
/// Do not change horizontal item alignment, brand artwork or account cards.
fn top_flow(v: &mut Value) -> bool {
    if layout_str(v) != Some("vertical") || footer(v) || has_word(v, &["brand", "logo"]) {
        return false;
    }
    let mut changed = start(v);
    if let Some(kids) = v.get_mut("children").and_then(Value::as_array_mut) {
        for child in kids {
            if spacer(child) {
                changed |= set(child, "height", json!(0.0));
            } else {
                changed |= top_flow(child);
            }
        }
    }
    changed
}

pub(super) fn normalize_sidebar(v: &mut Value) -> bool {
    if layout_str(v) != Some("vertical") {
        return false;
    }
    let kids = children(v);
    if kids.len() == 1 {
        // The actual surface is often behind one or more unnamed wrappers.
        return normalize_sidebar(&mut v["children"][0]);
    }
    if kids.len() < 2 {
        return false;
    }
    let has_footer = kids.last().is_some_and(footer) && !footer(&kids[0]);
    if !has_footer {
        return top_flow(v);
    }
    let two_groups = kids.len() == 2 && !spacer(&kids[0]);
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("sidebar")
        .to_owned();
    let mut changed = set(v, "height", json!("fill_container"));
    if two_groups {
        changed |= set(v, "justifyContent", json!("space_between"));
        changed |= top_flow(&mut v["children"][0]);
        return changed;
    }

    changed |= start(v);
    let kids = v["children"].as_array_mut().unwrap();
    let footer_index = kids.len() - 1;
    let spacer_index = kids[..footer_index].iter().rposition(spacer);
    if let Some(index) = spacer_index {
        if index != footer_index - 1 {
            let empty = kids.remove(index);
            kids.insert(footer_index - 1, empty);
            changed = true;
        }
    } else {
        // Derive a unique ID within this subtree; preserve all existing IDs.
        let mut spacer_id = format!("{id}-footer-spacer");
        let serialized = serde_json::to_string(kids).unwrap_or_default();
        while serialized.contains(&format!("\"{spacer_id}\"")) {
            spacer_id.push('-');
        }
        kids.insert(
            footer_index,
            json!({
                "type":"frame", "id":spacer_id, "name":"Sidebar Spacer",
                "width":"fill_container", "height":"fill_container", "children":[]
            }),
        );
        changed = true;
    }
    let spacer_index = kids.len() - 2;
    changed |= set(&mut kids[spacer_index], "height", json!("fill_container"));
    for child in &mut kids[..spacer_index] {
        if spacer(child) {
            changed |= set(child, "height", json!(0.0));
        } else {
            changed |= top_flow(child);
        }
    }
    changed
}

pub(super) fn repair_tree(v: &mut Value) -> bool {
    if is_sidebar_named(&ident_text(v)) && layout_str(v) == Some("vertical") {
        return normalize_sidebar(v);
    }
    let mut changed = false;
    if let Some(kids) = v.get_mut("children").and_then(Value::as_array_mut) {
        for child in kids {
            changed |= repair_tree(child);
        }
    }
    changed
}

pub(crate) fn sidebar_navigation_diagnostic(v: &Value) -> Option<String> {
    if !is_sidebar_named(&ident_text(v)) || children(v).len() < 2 {
        return None;
    }
    let mut fixed = v.clone();
    normalize_sidebar(&mut fixed).then(|| format!(
        "{}: desktop-sidebar-nav-flow — keep brand and menus top-aligned; distribute remaining height only before the account footer",
        v.get("id").and_then(Value::as_str).unwrap_or("sidebar")
    ))
}

#[cfg(test)]
#[path = "app_shell_nav_flow_tests.rs"]
mod tests;
