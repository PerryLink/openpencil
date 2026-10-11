//! Sidebar footer sinking (both the inline and already-structured paths)
//! and the column restructure into a sidebar/content row.

use super::*;

/// Shared navigation-flow repair also handles sidebars created by reshape.
pub(super) fn sink_sidebar_footer(sidebar: &mut Value) {
    super::nav_flow::normalize_sidebar(sidebar);
}

/// Normalize footer placement and top-aligned navigation in generated sidebars.
pub(crate) fn sink_structured_sidebar_footers(root: &mut PenNode) -> bool {
    let Ok(mut v) = serde_json::to_value(&*root) else {
        return false;
    };
    if !super::nav_flow::repair_tree(&mut v) {
        return false;
    }
    match serde_json::from_value::<PenNode>(v) {
        Ok(new_node) => {
            *root = new_node;
            true
        }
        Err(_) => false,
    }
}

/// Recursively retarget any descendant whose width was sized to (about) the OLD
/// full root width down to `fill_container`, so full-width sections/dividers
/// fill their new narrower column instead of overflowing it (taffy never
/// shrinks the cross axis). Targeted: only ~full-width Number widths change.
pub(super) fn fill_full_width_descendants(v: &mut Value, old_root_w: f64) {
    if let Some(w) = num(v, "width") {
        if w >= 0.9 * old_root_w {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("width".into(), json!("fill_container"));
            }
        }
    }
    if let Some(kids) = v.get_mut("children").and_then(Value::as_array_mut) {
        for c in kids.iter_mut() {
            fill_full_width_descendants(c, old_root_w);
        }
    }
}

/// Detection already passed: split the wrapper's children into `[sidebar |
/// content-column]` and flip the wrapper to a horizontal app-shell. Returns
/// `false` only if the tree shape changed under us (defensive).
pub(super) fn restructure(v: &mut Value) -> bool {
    let root_w = match num(v, "width") {
        Some(w) => w,
        None => return false,
    };
    let content_gap = num(v, "gap").filter(|g| *g > 0.0).unwrap_or(24.0);
    let content_id = format!(
        "{}-content",
        v.get("id").and_then(Value::as_str).unwrap_or("root")
    );

    let Some(obj) = v.as_object_mut() else {
        return false;
    };
    let Some(kids) = obj.get_mut("children").and_then(Value::as_array_mut) else {
        return false;
    };
    if kids.len() < 3 {
        return false;
    }
    let mut sidebar = kids.remove(0);
    let mut content_sections: Vec<Value> = std::mem::take(kids);

    // Sidebar → fixed narrow vertical column that stretches to the row height.
    // `height: fill_container` is the LOAD-BEARING cross-axis stretch (jian maps
    // `alignItems: stretch` to FlexStart, so the wrapper's alignItems alone
    // would NOT stretch it). Clip + width-retarget keep its old full-width
    // descendants from bleeding over the content column.
    if let Some(s) = sidebar.as_object_mut() {
        s.insert("width".into(), json!(SIDEBAR_WIDTH));
        s.insert("height".into(), json!("fill_container"));
        if s.get("layout").and_then(Value::as_str) != Some("vertical") {
            s.insert("layout".into(), json!("vertical"));
        }
        s.insert("clipContent".into(), json!(true));
    }
    fill_full_width_descendants(&mut sidebar, root_w);
    sink_sidebar_footer(&mut sidebar);

    // Content sections → fill the new column instead of the old full root width.
    for section in content_sections.iter_mut() {
        fill_full_width_descendants(section, root_w);
    }

    let content = json!({
        "type": "frame",
        "id": content_id,
        "name": "Main Content",
        "width": "fill_container",
        "height": "fit_content",
        "layout": "vertical",
        "gap": content_gap,
        // Outer page gutter (Pencil's app-shell content carries padding:[32,40]
        // so sections don't run edge-to-edge into the viewport). Without it the
        // new fill_container column lets the stat cards touch the right edge.
        "padding": CONTENT_PADDING,
        "children": content_sections,
    });

    obj.insert("children".into(), json!([sidebar, content]));
    obj.insert("layout".into(), json!("horizontal"));
    obj.insert("gap".into(), json!(0));
    obj.insert("alignItems".into(), json!("stretch"));
    // The old height was the SUM of the vertical stack (incl. the sidebar); the
    // horizontal shell only needs the taller column. Track content instead of
    // leaving ~600px of dead canvas below.
    obj.insert("height".into(), json!("fit_content"));
    true
}

// ── Evict mis-parented content sections from the sidebar column ──
//
// A dashboard's LEFT RAIL holds navigation only. But the two-column routing
// (`run.rs` → `dashboard_columns::is_sidebar_subtask`) matches the bare
// `nav`/`menu` substrings, so a weak model that describes a "Client Directory"
// content section with a "filter menu" — or emits it as a second forest root of
// the sidebar subtask — strands a full data TABLE inside the 260px `clipContent`
// rail, where it overflows and paints over the nav. This whole-root pass
// relocates such sections into the sibling `Main Content` column.
//
// Detection is STRUCTURAL (a real multi-column data table) or a content-only
// NAME token ("table"/"directory"/"data grid"). It deliberately does NOT reuse
// `section_has_dashboard_signal`, whose broad "analytics"/"metric" tokens also
// match sidebar MENU-ITEM labels (this very bug's rail carries an "Analytics"
// nav item — flagging the whole rail as content would evict the navigation
// itself). Runs in `run_cleanup_passes`. Self-contained; only MOVES nodes.
