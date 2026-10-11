//! Compose proven scrims over the photo samples in the renderer's paint order.
use super::*;

pub(super) fn composite_samples(
    samples: &[[u8; 3]],
    layers: &[PenNode],
    page: &jian_scene::layout_scene::ScenePage,
    region: [f64; 4],
    variables: &op_design_lint::node_util::Variables,
    theme: &op_design_lint::node_util::Theme,
) -> Option<Vec<String>> {
    if samples.len() != 25 {
        return None;
    }
    let mut colors: Vec<_> = samples
        .iter()
        .map(|p| rgb_hex([p[0], p[1], p[2], 255]))
        .collect();
    for node in layers.iter().rev().filter(|node| is_node_visible(node)) {
        let Some(scene) = page.find(node.id_str()) else {
            continue;
        };
        if scene.hidden {
            continue;
        }
        let b = scene.bounds;
        let owner = ResolvedRect {
            x: b.origin.x as f64,
            y: b.origin.y as f64,
            w: b.size.x as f64,
            h: b.size.y as f64,
        };
        if owner.x > region[0]
            || owner.y > region[1]
            || owner.x + owner.w < region[0] + region[2]
            || owner.y + owner.h < region[1] + region[3]
        {
            continue;
        }
        let fill = resolve_fill_kind(node_fills(node), variables, theme);
        if matches!(fill, ResolvedFill::Transparent) {
            continue;
        }
        // Unknown paint, transformed layers, fill opacity and group opacity
        // need rendered evidence; do not declare their contrast sufficient.
        let value = serde_json::to_value(node).ok()?;
        let uncertain_opacity = value
            .get("fill")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .any(|fill| {
                fill.get("opacity")
                    .and_then(serde_json::Value::as_f64)
                    .is_some_and(|opacity| opacity != 1.0)
            });
        if opacity(node) < 0.99
            || node.base().rotation.is_some_and(|r| r.abs() > 0.01)
            || !node_children_of(node).is_empty()
            || uncertain_opacity
            || !matches!(
                value.get("type").and_then(serde_json::Value::as_str),
                Some("rectangle" | "frame")
            )
            || scene.opacity < 0.99
            || scene.composite_opacity < 0.99
            || scene.blend_mode != Default::default()
            || scene.flip_x
            || scene.flip_y
            || scene.corner_radius > 0.0
            || scene
                .corner_radii
                .is_some_and(|r| r.iter().any(|r| *r > 0.0))
            || !scene.effects.is_empty()
            || value
                .get("fill")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|fills| fills.len() > 1)
        {
            return None;
        }
        for (index, color) in colors.iter_mut().enumerate() {
            let under = parse_color_rgba(color)?;
            *color = match &fill {
                ResolvedFill::Solid(rgba) => composite_over(*rgba, under),
                ResolvedFill::Gradient(gradient) => {
                    let mut gradient = gradient.clone();
                    gradient.flatten_over(under);
                    // The raster helper samples a 5x5 grid, in row-major order.
                    let point = ResolvedRect {
                        x: region[0] + region[2] * ((index % 5) as f64 + 0.5) / 5.0 - 0.005,
                        y: region[1] + region[3] * ((index / 5) as f64 + 0.5) / 5.0 - 0.005,
                        w: 0.01,
                        h: 0.01,
                    };
                    super::super::gradient::sample_at_text(&gradient, owner, point)?
                }
                _ => return None,
            };
        }
    }
    Some(colors)
}
