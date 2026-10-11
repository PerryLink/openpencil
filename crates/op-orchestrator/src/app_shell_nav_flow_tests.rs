use super::*;
use op_editor_core::EditorState;

fn brand() -> Value {
    json!({"type":"frame","id":"brand","name":"Brand","height":72,"children":[
        {"type":"text","id":"brand-text","content":"Admin Console"}
    ]})
}

fn nav() -> Value {
    json!({"type":"frame","id":"nav","name":"Nav Groups","layout":"vertical","height":"fit_content","gap":12,"children":[
        {"type":"frame","id":"users","name":"Users","height":44,"children":[{"type":"text","id":"users-text","content":"Users"}]},
        {"type":"frame","id":"settings","name":"Settings","height":44,"children":[{"type":"text","id":"settings-text","content":"Settings"}]}
    ]})
}

fn account() -> Value {
    json!({"type":"frame","id":"account","name":"User Profile Card","height":72,"children":[
        {"type":"text","id":"account-text","content":"Maya / Administrator"}
    ]})
}

fn sidebar(kids: Vec<Value>, justify: &str) -> Value {
    json!({"type":"frame","id":"sidebar","name":"Sidebar","width":260,"height":"fill_container","layout":"vertical","justifyContent":justify,"gap":16,"children":kids})
}

fn state(sidebar: Value) -> EditorState {
    EditorState::from_document(serde_json::from_value(json!({"version":"1.0.0","children":[
        {"type":"frame","id":"root","name":"Admin Console","width":1440,"height":900,"layout":"horizontal","children":[sidebar,
            {"type":"frame","id":"main","name":"Main Content","width":"fill_container","height":"fill_container","layout":"vertical","children":[
                {"type":"frame","id":"table","name":"Users Table","height":"fill_container"},
                {"type":"frame","id":"pagination","name":"Pagination Footer","height":48}
            ]}
        ]}
    ]})).unwrap())
}

fn bounds(state: &EditorState, id: &str) -> (f64, f64) {
    let scene = op_pen_loader::editor_state_to_active_page_layout_scene(state);
    let b = scene
        .active_page()
        .unwrap()
        .find(id)
        .unwrap_or_else(|| panic!("missing layout node {id}"))
        .bounds;
    (b.origin.y as f64, b.size.y as f64)
}

fn check_geometry(state: &EditorState) {
    let brand = bounds(state, "brand");
    let nav = bounds(state, "nav");
    assert!(
        (nav.0 - brand.0 - brand.1 - 16.0).abs() < 1.0,
        "brand={brand:?}, nav={nav:?}"
    );
    let account = bounds(state, "account");
    assert!((account.0 + account.1 - 900.0).abs() < 1.0, "{account:?}");
    let page_footer = bounds(state, "pagination");
    assert!((page_footer.0 + page_footer.1 - 900.0).abs() < 1.0);
}

#[test]
fn flat_distribution_and_alignment_variants_pin_only_the_footer() {
    for justify in ["space_between", "space_around", "center", "end", "start"] {
        let mut state = state(sidebar(vec![brand(), nav(), account()], justify));
        assert!(sink_structured_sidebar_footers(&mut state.doc.children[0]));
        check_geometry(&state);
        let once = state.doc.clone();
        assert!(!sink_structured_sidebar_footers(&mut state.doc.children[0]));
        assert_eq!(once, state.doc);
    }
}

#[test]
fn nested_full_height_top_group_does_not_push_navigation_down() {
    for justify in ["space_between", "center", "end"] {
        let top = json!({"type":"frame","id":"top","name":"Sidebar Top","layout":"vertical","height":"fill_container","justifyContent":justify,"gap":16,"children":[brand(),nav()]});
        let mut state = state(sidebar(vec![top, account()], "space_between"));
        assert!(bounds(&state, "nav").0 > 200.0);
        assert!(sink_structured_sidebar_footers(&mut state.doc.children[0]));
        check_geometry(&state);
    }
}

#[test]
fn misplaced_spacer_moves_after_menu_without_losing_content_or_ids() {
    let empty = json!({"type":"frame","id":"spacer","name":"Spacer","height":"fill_container","children":[]});
    let mut state = state(sidebar(vec![brand(), empty, nav(), account()], "start"));
    assert!(bounds(&state, "nav").0 > 200.0);
    assert!(sink_structured_sidebar_footers(&mut state.doc.children[0]));
    let v = serde_json::to_value(&state.doc).unwrap();
    let kids = &v["children"][0]["children"][0]["children"];
    assert_eq!(kids[2]["id"], "spacer");
    assert_eq!(kids[1]["id"], "nav");
    check_geometry(&state);
}

#[test]
fn healthy_two_group_sidebar_and_unrelated_creative_page_are_unchanged() {
    let top =
        json!({"type":"frame","id":"top","layout":"vertical","gap":16,"children":[brand(),nav()]});
    let mut healthy = sidebar(vec![top, account()], "space_between");
    assert!(!repair_tree(&mut healthy));
    assert!(sidebar_navigation_diagnostic(&healthy).is_none());
    let mut creative = sidebar(vec![brand(), nav(), account()], "center");
    creative["id"] = json!("poster");
    creative["name"] = json!("Creative Poster");
    assert!(!repair_tree(&mut creative));
}

#[test]
fn users_settings_and_admin_menu_are_not_account_footers() {
    for label in ["Users", "Settings", "Admin", "Account Settings"] {
        let last = json!({"type":"frame","id":"menu-item","name":label,"children":[{"type":"text","id":"label","content":label}]});
        let mut v = sidebar(vec![brand(), nav(), last], "start");
        assert!(!normalize_sidebar(&mut v), "{label}");
        assert_eq!(children(&v).len(), 3);
    }
    // Even an unnamed top group containing a Users item must not look like a footer.
    let mut menus = nav();
    menus["name"] = json!("Top Group");
    menus["id"] = json!("top");
    assert!(!footer(&menus));
}

#[test]
fn brand_alignment_and_decorated_spacers_are_preserved() {
    let mut logo = brand();
    logo["layout"] = json!("vertical");
    logo["justifyContent"] = json!("center");
    let decorated = json!({"type":"frame","id":"promotion-spacer","name":"Spacer Promo","height":100,"fill":"#ff0000","children":[]});
    let mut v = sidebar(
        vec![logo.clone(), nav(), decorated.clone(), account()],
        "space_between",
    );
    assert!(normalize_sidebar(&mut v));
    assert_eq!(v["children"][0], logo);
    assert_eq!(v["children"][2], decorated);
    assert_eq!(v["children"][3]["name"], "Sidebar Spacer");
}

#[test]
fn nested_spacer_collapses_once_and_preserves_scroll_viewport() {
    let top = json!({"type":"frame","id":"top","layout":"vertical","height":"fill_container","role":"scroll-area","clipContent":true,"gap":16,"children":[
        brand(), {"type":"frame","id":"empty","name":"Spacer","height":"fill_container","children":[]}, nav()
    ]});
    let mut state = state(sidebar(vec![top, account()], "space_between"));
    assert!(sink_structured_sidebar_footers(&mut state.doc.children[0]));
    assert!(bounds(&state, "nav").0 < 140.0);
    let v = serde_json::to_value(&state.doc).unwrap();
    let top = &v["children"][0]["children"][0]["children"][0];
    assert_eq!(top["height"], "fill_container");
    assert_eq!(top["role"], "scroll-area");
    assert_eq!(top["clipContent"], true);
    assert!(!sink_structured_sidebar_footers(&mut state.doc.children[0]));
}

#[test]
fn diagnostics_detect_the_gap_even_when_sidebar_and_footer_heights_are_correct() {
    let mut v = sidebar(vec![brand(), nav(), account()], "space_between");
    assert!(sidebar_navigation_diagnostic(&v)
        .unwrap()
        .contains("desktop-sidebar-nav-flow"));
    normalize_sidebar(&mut v);
    assert!(sidebar_navigation_diagnostic(&v).is_none());
}

#[test]
fn complete_cleanup_keeps_navigation_at_the_top() {
    let mut state = state(sidebar(vec![brand(), nav(), account()], "space_between"));
    let root = state.active_children()[0].clone();
    let mut plan = crate::loop_finalize::synthesize_plan(std::slice::from_ref(&root), 1440.0);
    plan.root_frame.height = 900.0;
    let mut sink = crate::loop_finalize::StateDocSink { state: &mut state };
    crate::cleanup::run_cleanup_passes(&mut sink, &plan, &["root"]);
    fn named(v: &Value, name: &str) -> Option<String> {
        if v.get("name").and_then(Value::as_str) == Some(name) {
            return v["id"].as_str().map(str::to_owned);
        }
        children(v).iter().find_map(|child| named(child, name))
    }
    let value = serde_json::to_value(&sink.state.doc).unwrap();
    let brand = bounds(sink.state, &named(&value, "Brand").unwrap());
    let nav = bounds(sink.state, &named(&value, "Nav Groups").unwrap());
    let account = bounds(sink.state, &named(&value, "User Profile Card").unwrap());
    let gap = nav.0 - brand.0 - brand.1;
    assert!((0.0..=48.0).contains(&gap), "brand={brand:?}, nav={nav:?}");
    assert!((account.0 + account.1 - 900.0).abs() < 1.0, "{account:?}");
}

#[test]
#[ignore = "requires a retained .op and output directory"]
fn replay_saved_sidebar_flow() {
    fn content(v: &Value, out: &mut std::collections::BTreeMap<String, Value>) {
        if let Some(id) = v.get("id").and_then(Value::as_str) {
            let mut own = v.clone();
            let map = own.as_object_mut().unwrap();
            for key in ["children", "height", "justifyContent"] {
                map.remove(key);
            }
            out.insert(id.to_owned(), own);
        }
        for c in children(v) {
            content(c, out);
        }
    }
    fn rects(state: &EditorState) -> Value {
        let scene = op_pen_loader::editor_state_to_active_page_layout_scene(state);
        let page = scene.active_page().unwrap();
        let mut nodes = std::collections::BTreeMap::new();
        content(&serde_json::to_value(&state.doc).unwrap(), &mut nodes);
        let mut out = serde_json::Map::new();
        for (id, node) in nodes {
            let b = page.find(&id).unwrap().bounds;
            out.insert(
                id,
                json!({"name":node["name"],"y_height":[b.origin.y,b.size.y]}),
            );
        }
        Value::Object(out)
    }
    let input = std::env::var("OPENPENCIL_REPLAY_INPUT").unwrap();
    let dir = std::path::PathBuf::from(std::env::var("OPENPENCIL_REPLAY_DIR").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let doc = serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    let mut state = EditorState::from_document(doc);
    let before_rects = rects(&state);
    let mut before = std::collections::BTreeMap::new();
    content(&serde_json::to_value(&state.doc).unwrap(), &mut before);
    assert!(sink_structured_sidebar_footers(&mut state.doc.children[0]));
    let mut after = std::collections::BTreeMap::new();
    content(&serde_json::to_value(&state.doc).unwrap(), &mut after);
    for (id, content) in &before {
        assert_eq!(after.get(id), Some(content), "{id}");
    }
    assert!(!sink_structured_sidebar_footers(&mut state.doc.children[0]));
    let after_rects = rects(&state);
    std::fs::write(
        dir.join("after.op"),
        serde_json::to_vec_pretty(&state.doc).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("layout-receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "input":input,"before":before_rects,"after":after_rects,
            "original_content_ids_and_style_preserved":true,"extra_model_calls":0
        }))
        .unwrap(),
    )
    .unwrap();
}
