use super::*;
use serde_json::json;

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
}

/// astra-new2 w3 rep-1: Sidebar(260) > section wrapper(fill) > rail(112,
/// right border) > nav items.
fn shell(rail_stroke: bool, rail_h: f64) -> (Value, HashMap<String, Rect>) {
    let mut rail = json!({"type":"frame","id":"rail","name":"影院导航轨","width":112,
        "height":"fill_container","layout":"vertical","children":[
            {"type":"text","id":"item","content":"总览"}]});
    if rail_stroke {
        rail["stroke"] =
            json!({"thickness":{"right":1},"fill":[{"type":"solid","color":"#3A2A30"}]});
    }
    let root = json!({"type":"frame","id":"root","layout":"horizontal","children":[
        {"type":"frame","id":"sb","name":"Sidebar","width":260,"height":"fill_container",
         "layout":"vertical","fill":[{"type":"solid","color":"#160B10"}],"children":[
            {"type":"frame","id":"wrap","name":"影院侧边导航","width":"fill_container",
             "height":"fill_container","layout":"vertical","children":[rail]}]},
        {"type":"frame","id":"main","name":"Main Content","width":"fill_container"}]});
    let rects = HashMap::from([
        ("root".to_string(), rect(0.0, 0.0, 1440.0, 900.0)),
        ("sb".to_string(), rect(0.0, 0.0, 260.0, 900.0)),
        ("wrap".to_string(), rect(0.0, 0.0, 260.0, 900.0)),
        ("rail".to_string(), rect(0.0, 0.0, 112.0, rail_h)),
        ("item".to_string(), rect(16.0, 80.0, 80.0, 20.0)),
        ("main".to_string(), rect(260.0, 0.0, 1180.0, 900.0)),
    ]);
    (root, rects)
}

fn fixes(root: &Value, rects: &HashMap<String, Rect>) -> Vec<String> {
    let mut cmds = Vec::new();
    collect_sidebar_rail_fixes(root, rects, &mut cmds);
    cmds.iter()
        .map(|cmd| match cmd {
            EditorCommand::UpdateNode { node_id, width, .. } => {
                format!("{} width {}", node_id.as_str(), width.unwrap())
            }
            EditorCommand::SetNodeLayoutProp {
                node_id,
                property,
                value: LayoutPropValue::Keyword(k),
            } => format!("{} {property} {k}", node_id.as_str()),
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

#[test]
fn the_scaffold_column_shrinks_to_the_rail_the_model_drew() {
    let (root, rects) = shell(true, 900.0);
    assert_eq!(fixes(&root, &rects), ["sb width 112"]);
}

#[test]
fn unpainted_short_or_full_width_content_keeps_the_column() {
    // Text-only navigation: whitespace beside the labels is the design.
    let (root, rects) = shell(false, 900.0);
    assert!(fixes(&root, &rects).is_empty());
    // A rail authored shorter than the column is a card, not the edge.
    let (mut root, rects) = shell(true, 300.0);
    root["children"][0]["children"][0]["children"][0]["height"] = json!(300);
    assert!(fixes(&root, &rects).is_empty());
    // A rail that nearly fills the column leaves nothing to reclaim.
    let (mut root, mut rects) = shell(true, 900.0);
    root["children"][0]["children"][0]["children"][0]["width"] = json!(230);
    rects.get_mut("rail").unwrap().w = 230.0;
    assert!(fixes(&root, &rects).is_empty());
    // A painted card beside the rail means the column holds more than the rail.
    let (mut root, mut rects) = shell(true, 900.0);
    root["children"][0]["children"][0]["children"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"frame","id":"promo","width":120,
            "fill":[{"type":"solid","color":"#2A1A20"}]}));
    rects.insert("promo".to_string(), rect(120.0, 600.0, 120.0, 100.0));
    assert!(fixes(&root, &rects).is_empty());
}

/// astra-new2 w3 rep-2: the rail asked to fill the column but its wrapper
/// hugged, so it collapsed to 495px; the wrapper stretches first.
#[test]
fn a_collapsed_full_height_rail_first_gets_its_wrapper_stretched() {
    let (mut root, rects) = shell(true, 495.0);
    root["children"][0]["children"][0]["height"] = json!("fit_content");
    assert_eq!(fixes(&root, &rects), ["wrap height fill_container"]);
}
