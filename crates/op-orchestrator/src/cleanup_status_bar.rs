//! Status bar identification helpers shared by cleanup and tree-heuristics passes.

use jian_ops_schema::node::PenNode;
use op_editor_core::PenNodeExt;

/// The alias set both entry points match against. Kept in one place because
/// the two callers read from different representations (typed tree vs. raw
/// JSON) and a second copy of this list would drift the moment a new alias
/// is added on one side only.
fn matches_status_bar_identity(role: Option<&str>, id: &str, name: &str) -> bool {
    if role.is_some_and(|role| role.eq_ignore_ascii_case("status-bar")) {
        return true;
    }
    let hay = format!("{} {}", id.to_lowercase(), name.to_lowercase());
    hay.contains("status bar")
        || hay.contains("status-bar")
        || hay.contains("statusbar")
        || hay.contains("状态栏")
        || hay.contains("系统栏")
}

/// Explicit status-bar role or an English/Chinese name/id match identifies
/// status-bar chrome. The role path keeps generated/custom-named bars stable;
/// Chinese aliases cover direct local edits such as "顶部状态栏".
pub(crate) fn is_status_bar(node: &PenNode) -> bool {
    matches_status_bar_identity(
        node.base().role.as_deref(),
        node.id_str(),
        node.base().name.as_deref().unwrap_or(""),
    )
}

/// JSON variant for tree-heuristics passes, which run on `serde_json` values
/// before deserialization.
pub(crate) fn is_status_bar_from_json(node: &serde_json::Value) -> bool {
    let field = |key: &str| node.get(key).and_then(serde_json::Value::as_str);
    matches_status_bar_identity(
        field("role"),
        field("id").unwrap_or(""),
        field("name").unwrap_or(""),
    )
}

/// Keep scaffold chrome and remove a duplicate at the start of a full-width
/// whole-screen shell. Do not recurse into cards or nested phone mockups.
pub(super) fn remove_duplicate_status_bars(sink: &mut dyn crate::types::DocSink, root_id: &str) {
    use op_editor_core::{EditorCommand, NodeId};
    let ids = {
        let Some(root) = op_editor_core::walkers::find_node(
            sink.state().active_children(),
            &NodeId::new(root_id),
        ) else {
            return;
        };
        let Some(children) = root.children() else {
            return;
        };
        let mut ids: Vec<_> = children
            .iter()
            .filter(|node| is_status_bar(node))
            .skip(1)
            .map(|node| NodeId::new(node.id_str()))
            .collect();
        if root
            .width_px()
            .is_some_and(|w| (240.0..=480.0).contains(&w))
            && children.first().is_some_and(is_status_bar)
        {
            for shell in children.iter().filter(|node| !is_status_bar(node)) {
                let Ok(value) = serde_json::to_value(shell) else {
                    continue;
                };
                if value.get("layout").and_then(serde_json::Value::as_str) != Some("vertical")
                    || value.get("width").and_then(serde_json::Value::as_str)
                        != Some("fill_container")
                    || value.get("height").and_then(serde_json::Value::as_str)
                        != Some("fit_content")
                    || value.get("role").is_some()
                    || value.get("constraints").is_some()
                    || value.get("x").is_some()
                    || value.get("y").is_some()
                    || value.get("padding").is_some()
                {
                    continue;
                }
                if let Some(bar) = shell
                    .children()
                    .and_then(|nodes| nodes.first())
                    .filter(|node| {
                        is_status_bar(node)
                            && node.height_px().is_some_and(|h| (20.0..=96.0).contains(&h))
                    })
                {
                    ids.push(NodeId::new(bar.id_str()));
                }
            }
        }
        ids
    };
    for node_id in ids {
        sink.apply(EditorCommand::DeleteNode {
            node_id,
            page_id: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The two entry points must agree on every alias — that agreement is the
    /// reason the matcher is shared, so it is asserted rather than assumed.
    #[test]
    fn typed_and_json_predicates_agree_on_every_alias() {
        let cases = [
            (
                json!({"type":"frame","role":"status-bar","id":"n1","name":"Bar"}),
                true,
            ),
            (json!({"type":"frame","id":"n2","name":"Status Bar"}), true),
            (
                json!({"type":"frame","id":"status-bar-1","name":"Chrome"}),
                true,
            ),
            (json!({"type":"frame","id":"n3","name":"statusbar"}), true),
            (json!({"type":"frame","id":"n4","name":"顶部状态栏"}), true),
            (json!({"type":"frame","id":"n5","name":"系统栏"}), true),
            (json!({"type":"frame","id":"n6","name":"Header"}), false),
            (
                json!({"type":"frame","role":"bottom-tab-bar","id":"n7","name":"Nav"}),
                false,
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(
                is_status_bar_from_json(&value),
                expected,
                "json predicate disagreed on {value}"
            );
            let node: PenNode =
                serde_json::from_value(value.clone()).expect("fixture parses as a node");
            assert_eq!(
                is_status_bar(&node),
                expected,
                "typed predicate disagreed on {value}"
            );
        }
    }

    #[test]
    fn duplicate_status_at_whole_screen_shell_start_is_removed() {
        use crate::test_support::VecDocSink;
        use op_editor_core::{EditorCommand, NodeId};
        let mut sink = VecDocSink::new();
        let tree: PenNode = serde_json::from_value(json!({"type":"frame","id":"root","width":375,"height":812,"children":[
            {"type":"frame","id":"status","name":"Status Bar","height":62},
            {"type":"frame","id":"body","name":"Home","layout":"vertical","width":"fill_container","height":"fit_content","children":[
                {"type":"frame","id":"duplicate","name":"status-bar","height":44},
                {"type":"text","id":"content","content":"Keep this content"}
            ]},
            {"type":"frame","id":"mockup","name":"Phone mockup","width":200,"height":420,"layout":"vertical","children":[
                {"type":"frame","id":"mockup-status","name":"status-bar","height":24}
            ]}
        ]})).unwrap();
        sink.state.apply(EditorCommand::InsertSubtree {
            nodes: vec![tree],
            parent_id: NodeId::NONE,
            page_id: None,
        });
        let id = sink.state.active_children()[0].id_str().to_string();
        remove_duplicate_status_bars(&mut sink, &id);
        let root = &sink.state.active_children()[0];
        assert_eq!(root.children().unwrap().len(), 3);
        assert_eq!(root.children().unwrap()[1].children().unwrap().len(), 1);
        assert_eq!(root.children().unwrap()[2].children().unwrap().len(), 1);
    }
}
