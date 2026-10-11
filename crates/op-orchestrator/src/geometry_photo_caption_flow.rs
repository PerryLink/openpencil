//! Separate slightly overlapping, left-aligned captions in a phone photo.
use super::*;

pub(super) fn collect_fixes(
    v: &Value,
    rects: &HashMap<String, Rect>,
    ops: &mut Vec<EditorCommand>,
) {
    if let Some(board) =
        rect(v, rects).filter(|r| (240.0..=480.0).contains(&r.w) && (100.0..=480.0).contains(&r.h))
    {
        if layout_str(v) == Some("none")
            && plain_visible(v)
            && children(v)
                .iter()
                .any(|n| n.get("type").and_then(Value::as_str) == Some("image"))
        {
            let mut candidates: Vec<_> = children(v)
                .iter()
                .filter(|n| {
                    n.get("x").and_then(Value::as_f64).is_some()
                        && n.get("y").and_then(Value::as_f64).is_some()
                        && n.get("constraints").is_none()
                        && caption_only(n)
                        && rect(n, rects).is_some_and(|r| r.h > 0.0 && r.h <= 72.0)
                })
                .collect();
            candidates.sort_by(|a, b| num(a, "y").total_cmp(&num(b, "y")));
            for pair in candidates.windows(2) {
                let (a, b) = (rect(pair[0], rects).unwrap(), rect(pair[1], rects).unwrap());
                let overlap = a.y + a.h - b.y;
                // Shared coordinates can be an authored counter/animation.
                // Deep overlaps or different alignments need design judgment.
                if (a.x - b.x).abs() > 1.0
                    || b.y <= a.y + 1.0
                    || overlap <= 1.0
                    || overlap > a.h.min(b.h) * 0.5
                {
                    continue;
                }
                let shift = overlap + 8.0;
                if b.y + b.h + shift > board.y + board.h + 0.5 {
                    continue;
                }
                if let Some(id) = pair[1].get("id").and_then(Value::as_str) {
                    ops.push(EditorCommand::SetNodeLayoutProp {
                        node_id: NodeId::new(id),
                        property: "y".into(),
                        value: LayoutPropValue::Number(num(pair[1], "y") + shift),
                    });
                }
            }
        }
    }
    for child in children(v) {
        collect_fixes(child, rects, ops);
    }
}

fn rect<'a>(v: &Value, rects: &'a HashMap<String, Rect>) -> Option<&'a Rect> {
    rects.get(v.get("id")?.as_str()?)
}

fn plain_visible(v: &Value) -> bool {
    if v.get("visible").and_then(Value::as_bool) == Some(false)
        || v.get("enabled").and_then(Value::as_bool) == Some(false)
        || v.get("role").and_then(Value::as_str) == Some("button")
        || v.get("rotation")
            .and_then(Value::as_f64)
            .is_some_and(|r| r.abs() > 0.01)
        || v.get("opacity")
            .and_then(Value::as_f64)
            .is_some_and(|p| p < 0.99)
    {
        return false;
    }
    true
}

fn caption_only(v: &Value) -> bool {
    if !plain_visible(v) {
        return false;
    }
    match v.get("type").and_then(Value::as_str) {
        Some("text") => true,
        Some("frame" | "group") => !children(v).is_empty() && children(v).iter().all(caption_only),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn a_wrapped_heading_reserves_its_actual_line_height_above_the_price() {
        let v = json!({"type":"frame","id":"hero","layout":"none","children":[
            {"type":"text","id":"title","x":24,"y":208,"width":280,"content":"燕麦拿铁"},
            {"type":"frame","id":"price-row","x":24,"y":254,"children":[{"type":"text","content":"32元"}]},
            {"type":"image","id":"photo"}
        ]});
        let mut rects = HashMap::from([
            (
                "hero".into(),
                Rect {
                    x: 80.0,
                    y: 165.0,
                    w: 375.0,
                    h: 300.0,
                },
            ),
            (
                "title".into(),
                Rect {
                    x: 104.0,
                    y: 373.0,
                    w: 280.0,
                    h: 54.0,
                },
            ),
            (
                "price-row".into(),
                Rect {
                    x: 104.0,
                    y: 419.0,
                    w: 104.0,
                    h: 27.0,
                },
            ),
        ]);
        let mut ops = Vec::new();
        collect_fixes(&v, &rects, &mut ops);
        assert!(
            matches!(&ops[..],[EditorCommand::SetNodeLayoutProp{node_id,property,value:LayoutPropValue::Number(y)}]
            if node_id.as_str()=="price-row" && property=="y" && *y==270.0)
        );
        rects.get_mut("price-row").unwrap().y += 16.0;
        ops.clear();
        collect_fixes(&v, &rects, &mut ops);
        assert!(ops.is_empty());
        rects.get_mut("price-row").unwrap().y -= 16.0;
        rects.get_mut("hero").unwrap().h = 280.0;
        collect_fixes(&v, &rects, &mut ops);
        assert!(ops.is_empty(), "do not push copy outside a short viewport");
    }
}
