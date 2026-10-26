//! A scaffold sidebar wider than the rail the model drew inside it.
//!
//! The two-column dashboard shell pre-builds a 260px `Sidebar` column. On the
//! astra-new2 cinema console the model drew its own 112px navigation rail in
//! it — a full-height frame with a right border — so a 148px empty strip sat
//! between the rail's border and the content in both drafts. The rail's
//! border is the authored edge of the sidebar; the column is shrunk to it and
//! the fill-width content column takes the strip back. In the second draft
//! the rail (a 104px card meant to run the full 900px) sat in a hugging
//! wrapper and collapsed to its content, so the wrapper is first stretched to
//! the column's height.

use super::*;
use crate::scaffold::SIDEBAR_COLUMN_NAME;

/// Narrower gaps are ordinary breathing room beside a rail.
const MIN_EMPTY_STRIP: f64 = 48.0;
/// The rail must run (almost) the column's full height to be its edge.
const MIN_HEIGHT_SHARE: f64 = 0.8;

fn paints_surface(v: &Value) -> bool {
    let filled = v
        .get("fill")
        .and_then(Value::as_array)
        .is_some_and(|fills| !fills.is_empty());
    filled || v.get("stroke").is_some_and(|stroke| !stroke.is_null())
}

fn is_hidden(v: &Value) -> bool {
    v.get("visible").and_then(Value::as_bool) == Some(false)
}

/// What the column holds: one or more rails drawn from its top-left corner,
/// seen through transparent wrappers, and nothing else visible.
struct Rails<'a> {
    right_edge: f64,
    /// Transparent wrappers above a full-height rail that collapsed to its
    /// content: the rail asks to fill the column, a hugging wrapper says no.
    collapsed_wrappers: Vec<&'a str>,
}

fn find_rails<'a>(
    column: &'a Value,
    column_rect: &Rect,
    rects: &HashMap<String, Rect>,
) -> Option<Rails<'a>> {
    let mut rails = Rails {
        right_edge: f64::MIN,
        collapsed_wrappers: Vec::new(),
    };
    let mut stack: Vec<(&Value, Vec<&str>)> =
        children(column).iter().map(|c| (c, Vec::new())).collect();
    while let Some((node, wrappers)) = stack.pop() {
        if is_hidden(node) {
            continue;
        }
        let Some(id) = node.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(rect) = rects.get(id) else {
            continue;
        };
        let is_rail = node.get("type").and_then(Value::as_str) == Some("frame")
            && fixed_width(node).is_some()
            && paints_surface(node)
            && (rect.x - column_rect.x).abs() <= 1.0
            && (rect.y - column_rect.y).abs() <= 1.0;
        if is_rail {
            rails.right_edge = rails.right_edge.max(rect.x + rect.w);
            let short = rect.h < column_rect.h * MIN_HEIGHT_SHARE;
            if short {
                if node.get("height").and_then(Value::as_str) != Some("fill_container") {
                    // A rail sized shorter on purpose is a card, not the edge.
                    return None;
                }
                rails.collapsed_wrappers.extend(wrappers);
            }
            continue;
        }
        // A transparent wrapper (the subtask's own section frame) is looked
        // through; anything painted or fixed outside a rail ends the search.
        if paints_surface(node) || fixed_width(node).is_some() {
            return None;
        }
        let hugs = node.get("height").and_then(Value::as_str) != Some("fill_container");
        for child in children(node) {
            let mut path = wrappers.clone();
            if hugs {
                path.push(id);
            }
            stack.push((child, path));
        }
    }
    (rails.right_edge > column_rect.x).then_some(rails)
}

pub(super) fn collect_sidebar_rail_fixes(
    v: &Value,
    rects: &HashMap<String, Rect>,
    cmds: &mut Vec<EditorCommand>,
) {
    if layout_str(v) == Some("horizontal") {
        for column in children(v) {
            if column.get("name").and_then(Value::as_str) != Some(SIDEBAR_COLUMN_NAME) {
                continue;
            }
            let (Some(id), Some(width)) = (
                column.get("id").and_then(Value::as_str),
                fixed_width(column),
            ) else {
                continue;
            };
            let Some(column_rect) = rects.get(id) else {
                continue;
            };
            let Some(rails) = find_rails(column, column_rect, rects) else {
                continue;
            };
            if !rails.collapsed_wrappers.is_empty() {
                // Let the rail reach the floor first; the next round sees
                // its full height and narrows the column.
                for wrapper in rails.collapsed_wrappers {
                    cmds.push(EditorCommand::SetNodeLayoutProp {
                        node_id: NodeId::new(wrapper.to_string()),
                        property: "height".to_string(),
                        value: LayoutPropValue::Keyword("fill_container".to_string()),
                    });
                }
                continue;
            }
            let rail_width = (rails.right_edge - column_rect.x).ceil();
            if rail_width > 0.0 && width - rail_width >= MIN_EMPTY_STRIP {
                cmds.push(EditorCommand::UpdateNode {
                    node_id: NodeId::new(id.to_string()),
                    x: None,
                    y: None,
                    width: Some(rail_width as i32),
                    height: None,
                    name: None,
                    fill_hex: None,
                    page_id: None,
                });
            }
        }
    }
    for c in children(v) {
        collect_sidebar_rail_fixes(c, rects, cmds);
    }
}

#[cfg(test)]
#[path = "geometry_sidebar_rail_tests.rs"]
mod tests;
