//! astra rerun 1027, florist App: complete deliveries the gate under-counted.

use super::*;
use crate::test_support::VecDocSink;
use serde_json::{json, Value};

fn task(id: &str, label: &str, elements: &str) -> Subtask {
    serde_json::from_value(json!({
        "id": id, "label": label, "elements": elements,
        "region": {"width": 375, "height": 300}
    }))
    .unwrap()
}

fn verdict(task: &Subtask, section: Value) -> Option<CompletenessFailure> {
    let document = serde_json::from_value(json!({"version": "1.0", "children": [section]}))
        .expect("synthetic section");
    let sink = VecDocSink {
        state: EditorState::from_document(document),
        applied: Vec::new(),
        batch_depth: 0,
    };
    let outcome = SubtaskOutcome {
        id: task.id.clone(),
        headline: None,
        node_count: 12,
        inserted_root_ids: vec!["section".into()],
        error: None,
        subtask: Some(task.clone()),
    };
    incomplete_attempt(&sink, task, &outcome)
}

fn plan_card(id: &str, price: &str, extra: Vec<Value>) -> Value {
    let mut children = vec![
        json!({"type":"text","id":format!("{id}-name"),"content":id}),
        json!({"type":"text","id":format!("{id}-price"),"content":price}),
    ];
    children.extend(extra);
    json!({"type":"frame","id":id,"layout":"none","width":103,"height":96,"children":children})
}

const PLANS: &str = "「订阅方案」标题；三张等宽横排小卡，分别单周 ¥69、双周 ¥129、包月 ¥239；当前方案选择双周，双周卡使用鼠尾草绿边框、浅绿底及「当前方案」";

#[test]
fn the_highlighted_current_plan_counts_with_its_siblings() {
    let task = task("home-plans", "订阅方案", PLANS);
    let section = |marked: Value| {
        json!({"type":"frame","id":"section","layout":"vertical","children":[
            {"type":"text","id":"title","content":"订阅方案"},
            {"type":"frame","id":"row","layout":"horizontal","children":[
                plan_card("weekly", "¥69", vec![]), marked, plan_card("monthly", "¥239", vec![])]}]})
    };
    // rep-2: a marker text and a tint layer; rep-1: the marker alone.
    let tinted = plan_card(
        "biweekly",
        "¥129",
        vec![
            json!({"type":"text","id":"current","content":"当前方案"}),
            json!({"type":"rectangle","id":"tint","width":103,"height":96}),
        ],
    );
    assert_eq!(verdict(&task, section(tinted)), None);
    let marked = plan_card(
        "biweekly",
        "¥ 129",
        vec![json!({"type":"text","id":"current","content":"当前方案"})],
    );
    assert_eq!(verdict(&task, section(marked)), None);
    // A genuinely missing card still fails.
    let short = json!({"type":"frame","id":"section","layout":"vertical","children":[
        {"type":"frame","id":"row","layout":"horizontal","children":[
            plan_card("weekly", "¥69", vec![]), plan_card("monthly", "¥239", vec![])]}]});
    assert_eq!(verdict(&task, short).map(|f| f.delivered), Some(2));
    // Three cards that do not each show their own promised price prove nothing.
    let unpriced = json!({"type":"frame","id":"section","layout":"vertical","children":[
        {"type":"frame","id":"row","layout":"horizontal","children":[
            plan_card("weekly", "¥69", vec![]),
            plan_card("biweekly", "¥69", vec![json!({"type":"text","id":"current","content":"当前方案"})]),
            plan_card("monthly", "¥239", vec![])]}]});
    assert_eq!(verdict(&task, unpriced).map(|f| f.delivered), Some(2));
}

fn bouquet(id: &str) -> Value {
    json!({"type":"frame","id":id,"layout":"vertical","width":"fill_container","children":[
        {"type":"image","id":format!("{id}-photo"),"src":"","width":"fill_container","height":150},
        {"type":"frame","id":format!("{id}-meta"),"layout":"vertical","children":[
            {"type":"text","id":format!("{id}-name"),"content":"晚霞来信"},
            {"type":"text","id":format!("{id}-date"),"content":"10 月 2 日"}]}]})
}

#[test]
fn a_grid_that_names_only_its_columns_counts_cells_across_rows() {
    // rep-1: "两列四张真实花束照片网格", delivered as two rows of two.
    let task = task(
        "past-bouquets",
        "过往花束",
        "「过往花束」标题，两列四张真实花束照片网格，每张只含照片、共享实体中的名称与日期",
    );
    let grid = |rows: Vec<Vec<&str>>| {
        let rows: Vec<Value> = rows
            .into_iter()
            .enumerate()
            .map(|(i, cells)| {
                json!({"type":"frame","id":format!("row-{i}"),"layout":"horizontal",
                    "children": cells.into_iter().map(bouquet).collect::<Vec<_>>()})
            })
            .collect();
        json!({"type":"frame","id":"section","layout":"vertical","children":[
            {"type":"text","id":"title","content":"过往花束"},
            {"type":"frame","id":"grid","layout":"vertical","children":rows}]})
    };
    assert_eq!(
        verdict(&task, grid(vec![vec!["a", "b"], vec!["c", "d"]])),
        None
    );
    assert_eq!(
        verdict(&task, grid(vec![vec!["a", "b"]])).map(|f| f.delivered),
        Some(2)
    );
    assert_eq!(
        grid::columns_only_shape("两列四张照片网格", 4),
        Some((2, 2))
    );
    assert_eq!(grid::columns_only_shape("两列四张照片", 4), None);
    assert_eq!(grid::columns_only_shape("三列五张照片网格", 5), None);
}
