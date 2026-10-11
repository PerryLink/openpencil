//! Replay image landing on retained OP output without another model call.
use super::*;

fn retained_content(state: &EditorState) -> serde_json::Value {
    fn walk(v: &serde_json::Value, out: &mut Vec<serde_json::Value>) {
        if matches!(v["type"].as_str(), Some("text" | "image")) {
            out.push(json!({"id":v["id"],"type":v["type"],"content":v["content"],
                "src":v["src"],"fontFamily":v["fontFamily"],"fontSize":v["fontSize"],
                "fontWeight":v["fontWeight"],"lineHeight":v["lineHeight"]}));
        }
        for child in v["children"].as_array().into_iter().flatten() {
            walk(child, out);
        }
    }
    let value = serde_json::to_value(&state.doc).unwrap();
    let mut out = Vec::new();
    walk(&value, &mut out);
    json!(out)
}

#[test]
#[ignore = "requires OPENPENCIL_QA_PHOTO_INPUT, OUTPUT and explicit managed backing IDs"]
fn replay_retained_photo_captions() {
    let input = std::env::var("OPENPENCIL_QA_PHOTO_INPUT").unwrap();
    let source = std::fs::read_to_string(&input).unwrap();
    let loaded = op_pen_loader::payload::load_canonical(&source).unwrap();
    let mut state = EditorState::from_document(loaded.value);
    let before_content = retained_content(&state);
    let root = state.active_children()[0].id_str().to_owned();
    let before_board = serde_json::to_value(&state.active_children()[0]).unwrap();
    let before_issues = crate::geometry_validation::geometry_diagnostics(&state);
    // Only explicitly identified panels from the old callback are removed,
    // on this QA copy, so the corrected production callback sees the photo.
    let ids = std::env::var("OPENPENCIL_QA_PHOTO_BACKINGS").unwrap();
    for id in ids.split(',').filter(|id| !id.is_empty()) {
        let node =
            op_editor_core::walkers::find_node(state.active_children(), &NodeId::new(id)).unwrap();
        assert_eq!(node.base().name.as_deref(), Some("Text contrast backing"));
        assert!(state.apply(EditorCommand::DeleteNode {
            node_id: NodeId::new(id),
            page_id: None
        }));
    }
    let mut sink = crate::loop_finalize::StateDocSink { state: &mut state };
    let geometry_rounds = crate::geometry_validation::geometry_validate_and_fix(&mut sink, &root);
    let contrast_repairs = repair(&mut sink, &root);
    let after_issues = crate::geometry_validation::geometry_diagnostics(sink.state());
    assert!(after_issues.is_empty(), "{after_issues:?}");
    assert_eq!(
        retained_content(sink.state()),
        before_content,
        "retain copy, fonts and photos"
    );
    let after_board = serde_json::to_value(&sink.state().active_children()[0]).unwrap();
    assert_eq!(before_board["width"], after_board["width"]);
    assert_eq!(before_board["height"], after_board["height"]);
    let settled = sink.state().doc.clone();
    assert_eq!(
        crate::geometry_validation::geometry_validate_and_fix(&mut sink, &root),
        0
    );
    assert_eq!(repair(&mut sink, &root), 0);
    assert_eq!(settled, sink.state().doc, "second callback must settle");
    let output = std::path::PathBuf::from(std::env::var("OPENPENCIL_QA_PHOTO_OUTPUT").unwrap());
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(&output).unwrap();
    jian_ops_schema::image_table::write_document_with_extension(
        &mut file,
        &sink.state().doc,
        &jian_ops_schema::image_thumbs::capture_snapshot(),
        "editorMeta",
        &op_pen_loader::EditorMeta::from_state(sink.state()),
    )
    .unwrap();
    assert_eq!(source, std::fs::read_to_string(input).unwrap());
    std::fs::write(
        output.with_extension("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "issues_before":before_issues,"issues_after":after_issues,
            "removed_old_managed_backings":ids,"geometry_rounds":geometry_rounds,
            "photo_contrast_repairs":contrast_repairs,"content_fonts_images_preserved":true,
            "root_dimensions_preserved":true,"idempotent":true,"extra_model_calls":0,
            "source_document_mutated":false
        }))
        .unwrap(),
    )
    .unwrap();
}
