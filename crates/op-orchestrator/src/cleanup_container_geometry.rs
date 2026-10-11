//! Container geometry repairs: expand absolute containers to their children
//! and collapse nested horizontal padding (plus the frame/padding readers
//! those passes share).

use super::*;

pub(super) fn expand_absolute_container_to_children(sink: &mut dyn DocSink, root_id: &str) {
    let repairs: Vec<AbsoluteContainerRepair> = {
        let Some(root) = find_root(sink.state(), root_id) else {
            return;
        };
        let mut repairs = Vec::new();
        collect_absolute_container_repairs(root, &mut repairs);
        repairs
    };

    for repair in repairs {
        sink.apply(EditorCommand::UpdateNode {
            node_id: repair.node_id,
            x: None,
            y: None,
            width: None,
            height: Some(repair.height.ceil() as i32),
            name: None,
            fill_hex: None,
            page_id: None,
        });
    }
}

#[derive(Debug, Clone)]
pub(super) struct AbsoluteContainerRepair {
    node_id: NodeId,
    height: f64,
}

pub(super) fn collect_absolute_container_repairs(
    node: &PenNode,
    repairs: &mut Vec<AbsoluteContainerRepair>,
) {
    if let Some(repair) = absolute_container_repair(node) {
        repairs.push(repair);
    }
    if let Some(children) = node.children() {
        for child in children {
            collect_absolute_container_repairs(child, repairs);
        }
    }
}

pub(super) fn absolute_container_repair(node: &PenNode) -> Option<AbsoluteContainerRepair> {
    let props = frame_container_props(node)?;
    if props.layout.as_ref() != Some(&LayoutMode::None)
        || !matches!(
            props.height.as_ref(),
            Some(SizingBehavior::Keyword(SizingKeyword::FitContent))
        )
    {
        return None;
    }

    let children = node.children()?;
    if !children
        .iter()
        .any(|child| matches!(child, PenNode::Image(_)))
    {
        return None;
    }

    let height = children
        .iter()
        .filter_map(|child| {
            // Captions and scrims share the absolute composition. Using only
            // the image's height clips lower captions, then the overflow pass
            // moves them upward into badges that were correctly separated.
            child
                .height_px()
                .map(|height| child.base().y.unwrap_or(0.0) + height)
        })
        .max_by(f64::total_cmp)?;
    (height > 0.0).then(|| AbsoluteContainerRepair {
        node_id: NodeId::new(node.id_str().to_string()),
        height,
    })
}

/// Collapse `section > transparent fill-width wrapper` double horizontal
/// padding while preserving all vertical padding.
pub(super) fn collapse_nested_horizontal_padding(sink: &mut dyn DocSink, root_id: &str) {
    let repairs: Vec<NestedHorizontalPaddingRepair> = {
        let Some(root) = find_root(sink.state(), root_id) else {
            return;
        };
        let mut repairs = Vec::new();
        collect_nested_horizontal_padding_repairs(root, &mut repairs);
        repairs
    };

    for repair in repairs {
        sink.apply(EditorCommand::SetNodeLayoutProp {
            node_id: repair.wrapper_id,
            property: "padding".to_string(),
            value: LayoutPropValue::NumberArray(vec![
                repair.wrapper_padding[0],
                0.0,
                repair.wrapper_padding[2],
                0.0,
            ]),
        });
    }
}

#[derive(Debug, Clone)]
pub(super) struct NestedHorizontalPaddingRepair {
    wrapper_id: NodeId,
    wrapper_padding: [f64; 4],
}

pub(super) fn collect_nested_horizontal_padding_repairs(
    node: &PenNode,
    repairs: &mut Vec<NestedHorizontalPaddingRepair>,
) {
    let collapsed_wrapper_id = nested_horizontal_padding_repair(node).map(|repair| {
        let id = repair.wrapper_id.to_string();
        repairs.push(repair);
        id
    });

    if let Some(children) = node.children() {
        for child in children {
            if collapsed_wrapper_id.as_deref() == Some(child.id_str()) {
                continue;
            }
            collect_nested_horizontal_padding_repairs(child, repairs);
        }
    }
}

pub(super) fn nested_horizontal_padding_repair(
    node: &PenNode,
) -> Option<NestedHorizontalPaddingRepair> {
    equal_positive_horizontal_frame_padding(node)?;
    let children = node.children()?;
    let [wrapper] = children.as_slice() else {
        return None;
    };
    if !is_transparent_fill_width_wrapper_frame(wrapper) {
        return None;
    }
    let wrapper_padding = equal_positive_horizontal_frame_padding(wrapper)?;
    Some(NestedHorizontalPaddingRepair {
        wrapper_id: NodeId::new(wrapper.id_str().to_string()),
        wrapper_padding,
    })
}

pub(super) fn equal_positive_horizontal_frame_padding(node: &PenNode) -> Option<[f64; 4]> {
    let padding = frame_container_props(node)?.padding.as_ref()?;
    let sides = padding_sides(padding);
    if sides[1] > 0.0 && (sides[1] - sides[3]).abs() <= f64::EPSILON {
        Some(sides)
    } else {
        None
    }
}

pub(super) fn is_transparent_fill_width_wrapper_frame(node: &PenNode) -> bool {
    let Some(props) = frame_container_props(node) else {
        return false;
    };
    width_is_fill_container(props)
        && props.fill.as_ref().map(Vec::is_empty).unwrap_or(true)
        && props.stroke.is_none()
        && props.effects.as_ref().map(Vec::is_empty).unwrap_or(true)
        && props.clip_content != Some(true)
}

pub(super) fn frame_container_props(node: &PenNode) -> Option<&ContainerProps> {
    match node {
        PenNode::Frame(n) => Some(&n.container),
        _ => None,
    }
}

pub(super) fn width_is_fill_container(props: &ContainerProps) -> bool {
    matches!(
        props.width.as_ref(),
        Some(SizingBehavior::Keyword(SizingKeyword::FillContainer))
    )
}

pub(super) fn padding_sides(padding: &Padding) -> [f64; 4] {
    match padding {
        Padding::Uniform(v) => [*v, *v, *v, *v],
        Padding::XY([y, x]) => [*y, *x, *y, *x],
        Padding::LtrB(values) => *values,
        Padding::Expression(_) => [0.0, 0.0, 0.0, 0.0],
    }
}
