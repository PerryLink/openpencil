use super::*;

fn toolbar() -> Value {
    json!({"type":"frame","id":"filter","name":"Filter Bar","layout":"vertical","children":[
        {"type":"frame","id":"row","name":"Filter Controls Row","layout":"horizontal","gap":12,"children":[
            {"type":"select","id":"select","name":"Role Select","width":150,"height":40,
             "options":[{"value":"all","label":"All Roles"}],"fill":[{"type":"solid","color":"$--card"}]},
            {"type":"frame","id":"add","name":"Add User Button","width":"fit_content","height":40,
             "layout":"horizontal","gap":8,"padding":[0,16],"cornerRadius":6,
             "fill":[{"type":"solid","color":"$--primary"}],"children":[]},
            {"type":"icon_font","id":"plus","name":"Add User Icon","iconFontFamily":"lucide","iconFontName":"plus","width":16,"height":16},
            {"type":"text","id":"label","name":"Add User Label","content":"Add User","fill":[{"type":"solid","color":"#FFFFFF"}]},
            {"type":"frame","id":"export","name":"Export Icon Button","width":40,"height":40,"cornerRadius":6,
             "fill":[{"type":"solid","color":"$--card"}],"children":[]},
            {"type":"icon_font","id":"download","name":"Export Icon","iconFontFamily":"lucide","iconFontName":"download","width":18,"height":18}
        ]}
    ]})
}

#[test]
fn named_button_parts_are_adopted_before_contrast_and_stub_cleanup() {
    use crate::role_defaults::Theme;
    use crate::test_support::VecDocSink;
    use crate::types::DocSink;
    use op_editor_core::{EditorState, NodeId};

    let mut nodes: Vec<PenNode> = vec![serde_json::from_value(toolbar()).unwrap()];
    crate::role_infer::resolve_forest_roles(&mut nodes, 1440.0, Theme::Light);
    post_pass_forest(&mut nodes, 1440.0);
    crate::tree_heuristics::apply_tree_heuristics(
        &mut nodes,
        Some("#F7F9FC"),
        Theme::Light,
        Some("$--primary"),
    );
    let value = serde_json::to_value(&nodes[0]).unwrap();
    assert!(value.get("fill").is_none_or(|fill| fill == &json!([])));
    let row = &value["children"][0]["children"];
    assert_eq!(row.as_array().unwrap().len(), 3);
    assert_eq!(row[1]["id"], "add");
    assert_eq!(row[1]["children"][0]["id"], "plus");
    assert_eq!(row[1]["children"][1]["id"], "label");
    assert_eq!(row[2]["children"][0]["id"], "download");
    assert_eq!(row[1]["fill"][0]["color"], "$--primary");

    let doc = serde_json::from_value(json!({"version":"1.0.0","children":nodes})).unwrap();
    let mut sink = VecDocSink::new();
    sink.state = EditorState::from_document(doc);
    assert!(!crate::stub_repair::remove_empty_decorated_stubs(
        &mut sink, "filter"
    ));
    assert!(op_editor_core::walkers::find_node(
        sink.state().active_children(),
        &NodeId::new("add")
    )
    .is_some());

    post_pass_forest(&mut nodes, 1440.0);
    assert_eq!(
        serde_json::to_value(&nodes[0]).unwrap(),
        value,
        "second pass must be stable"
    );
}

#[test]
fn orphan_control_adoption_preserves_authored_boundaries() {
    for case in [
        "populated",
        "mismatched",
        "positioned",
        "leaf-positioned",
        "leaf-event",
        "unnamed",
        "vertical",
        "non-button",
    ] {
        let mut node = toolbar()["children"][0].clone();
        node["children"].as_array_mut().unwrap().truncate(4);
        node["children"][1]["role"] = json!("button");
        match case {
            "populated" => {
                node["children"][1]["children"] = json!([{"type":"text","content":"Existing"}])
            }
            "mismatched" => node["children"][2]["name"] = json!("Other Icon"),
            "positioned" => node["children"][1]["x"] = json!(0),
            "leaf-positioned" => node["children"][2]["x"] = json!(0),
            "leaf-event" => node["children"][2]["events"] = json!({"onClick":[]}),
            "unnamed" => node["children"][1]["name"] = json!("Button"),
            "vertical" => node["layout"] = json!("vertical"),
            "non-button" => node["children"][1]["role"] = json!("card"),
            _ => unreachable!(),
        }
        let before = node.clone();
        adopt_orphan_button_parts(&mut node);
        assert_eq!(node, before, "{case}");
    }
}

#[test]
#[ignore = "requires a retained document with restored empty control shells and QA paths"]
fn replay_retained_toolbar_parts() {
    use crate::role_defaults::Theme;
    use op_editor_core::PenNodeExt;

    let input = std::env::var("OPENPENCIL_QA_TOOLBAR_INPUT").unwrap();
    let output = std::env::var("OPENPENCIL_QA_TOOLBAR_OUTPUT").unwrap();
    let id = std::env::var("OPENPENCIL_QA_TOOLBAR_ID").unwrap();
    let source = std::fs::read_to_string(input).unwrap();
    let mut doc = op_pen_loader::payload::load_canonical(&source)
        .unwrap()
        .value;
    fn repair(nodes: &mut [PenNode], id: &str) -> bool {
        for node in nodes {
            if node.id_str() == id {
                let nodes = std::slice::from_mut(node);
                crate::role_infer::resolve_forest_roles(nodes, 1440.0, Theme::Light);
                post_pass_forest(nodes, 1440.0);
                crate::tree_heuristics::apply_tree_heuristics(
                    nodes,
                    Some("#F7F9FC"),
                    Theme::Light,
                    Some("$--primary"),
                );
                return true;
            }
            if node
                .children_mut()
                .is_some_and(|children| repair(children, id))
            {
                return true;
            }
        }
        false
    }
    assert!(repair(&mut doc.children, &id));
    std::fs::write(output, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
}
