//! Model-access chrome on the Home sheet: the model chip on the bottom
//! row, the label it derives from the chat selection, and the geometry
//! of the Home-anchored chat model picker that opens above the chip.

use super::{HomeLayout, HomePalette, HomeSurface};
use crate::widgets::ai_chat_model_picker::picker_view_height;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};
use op_editor_core::{EditorState, ModelEntry};

/// Chip height — matches the sheet's reference row band.
pub const MODEL_CHIP_H: f32 = 28.0;
/// Leading zone: padding + the ⚡ glyph + its gap to the label.
pub const MODEL_CHIP_PREFIX_W: f32 = 26.0;
/// Trailing zone: padding + the chevron-down.
pub const MODEL_CHIP_CHEVRON_W: f32 = 22.0;
/// Widest the chip may grow before the label clips inside the pill.
/// Chosen so the pill never collides with the "试试这个示例" row that
/// anchors the sheet's bottom-left on the narrowest supported sheet.
pub const MODEL_CHIP_MAX_W: f32 = 180.0;

/// Width of the fixed "⏎ 发送" hint text slot left of the send button.
pub const SEND_HINT_W: f32 = 50.0;
/// Gap between the hint's right edge and the send circle.
pub const HINT_SEND_GAP: f32 = 8.0;
/// Gap between the model chip's right edge and the hint.
pub const CHIP_HINT_GAP: f32 = 12.0;

/// Width of the Home-anchored model picker card.
pub const HOME_MODEL_PICKER_W: f32 = 300.0;
/// The card floats this far above the chip.
pub const HOME_MODEL_PICKER_GAP: f32 = 8.0;
/// The trailing "接入更多模型…" action row hangs below the card.
pub const CONNECT_MORE_ROW_H: f32 = 34.0;
pub const CONNECT_MORE_ROW_GAP: f32 = 6.0;

/// The label the Home model chip shows. Reuses the exact derivation the
/// chat panel's bottom-left model pill paints (`selected_model_entry`'s
/// display name); when no agent can answer it becomes the localized
/// connect hint instead.
pub fn model_chip_label(state: &EditorState) -> String {
    let entry = state
        .has_usable_chat_agent()
        .then(|| state.chat.selected_model_entry())
        .flatten();
    match entry {
        Some(entry) => entry.display_name.clone(),
        None => op_i18n::translate(state.editor_ui.locale, "home.connect.chipEmpty").to_string(),
    }
}

/// Natural pill width for `label` (prefix + measured label + chevron),
/// clamped so an extreme model name cannot swallow the sheet row.
pub fn model_chip_width(label: &str) -> f32 {
    let label_w = crate::widgets::ai_chat_panel::footer_label_width(label, 13.0);
    (MODEL_CHIP_PREFIX_W + label_w + MODEL_CHIP_CHEVRON_W).min(MODEL_CHIP_MAX_W)
}

/// The picker card anchored above the sheet's model chip plus the
/// trailing connect-more row under it. `None` when the chip is not laid
/// out (zero-width) or the viewport cannot hold the card.
pub fn home_model_picker_rects(
    layout: &HomeLayout,
    viewport_w: f32,
    models: &[ModelEntry],
    search: &str,
) -> Option<(Rect, Rect)> {
    let chip = layout.model_chip;
    if chip.size.x <= 0.0 {
        return None;
    }
    let height = picker_view_height(models, search);
    let bottom = chip.origin.y - HOME_MODEL_PICKER_GAP;
    let top = (bottom - height).max(8.0);
    let x = (chip.origin.x + chip.size.x / 2.0 - HOME_MODEL_PICKER_W / 2.0)
        .clamp(8.0, (viewport_w - HOME_MODEL_PICKER_W - 8.0).max(8.0));
    let card = Rect::xywh(x, top, HOME_MODEL_PICKER_W, height);
    let connect_row = Rect::xywh(
        x,
        card.origin.y + card.size.y + CONNECT_MORE_ROW_GAP,
        HOME_MODEL_PICKER_W,
        CONNECT_MORE_ROW_H,
    );
    Some((card, connect_row))
}

/// Paint the model chip pill. The empty state (no usable agent) drops
/// the chevron and paints the label in blue — the chip is then a call
/// to action, not a picker.
pub(super) fn paint_model_chip(
    surface: &HomeSurface<'_>,
    cx: &mut PaintCx<'_>,
    rect: Rect,
    palette: HomePalette,
) {
    let usable = surface.usable_agent;
    let label = surface.chip_label.as_str();
    let hovered = surface.state.hover == Some(op_editor_core::HomeHit::ModelChip);
    let pressed = surface.state.pressed == Some(op_editor_core::HomeHit::ModelChip);
    cx.backend
        .fill_round_rect(rect, MODEL_CHIP_H / 2.0, palette.sheet);
    if hovered || pressed {
        cx.backend.fill_round_rect(
            rect,
            MODEL_CHIP_H / 2.0,
            if pressed {
                fade(palette.blue, 0.20)
            } else {
                fade(palette.graphite, 0.10)
            },
        );
    }
    cx.backend
        .stroke_round_rect(rect, MODEL_CHIP_H / 2.0, palette.line, 1.0);
    draw_icon(
        cx.backend,
        Icon::Zap,
        Point2D::new(rect.origin.x + 10.0, rect.origin.y + 7.0),
        13.0,
        palette.graphite,
        1.4,
    );
    let label_color = if usable { palette.ink } else { palette.blue };
    cx.backend.save();
    cx.backend.clip_rect(rect);
    let layout = TextLayout::single_run(
        label,
        "system-ui",
        13.0,
        label_color.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &layout,
        Point2D::new(rect.origin.x + MODEL_CHIP_PREFIX_W, rect.origin.y + 19.0),
    );
    if usable {
        draw_icon(
            cx.backend,
            Icon::ChevronDown,
            Point2D::new(rect.origin.x + rect.size.x - 16.0, rect.origin.y + 8.0),
            12.0,
            palette.graphite,
            1.4,
        );
    }
    cx.backend.restore();
}

/// Multiply a colour's alpha by `factor` (composes with baked alpha).
fn fade(color: Color, factor: f32) -> Color {
    Color {
        a: color.a * factor,
        ..color
    }
}

/// Paint the picker's trailing "接入更多模型…" row — the card-styled
/// action under the dropdown that routes to the Agents settings tab.
pub fn paint_connect_more_row(
    cx: &mut PaintCx<'_>,
    theme: &crate::theme::Theme,
    rect: Rect,
    label: &str,
    hovered: bool,
) {
    cx.backend.fill_round_rect(rect, 10.0, theme.card);
    if hovered {
        cx.backend.fill_round_rect(rect, 10.0, theme.muted);
    }
    cx.backend.stroke_round_rect(rect, 10.0, theme.border, 1.0);
    draw_icon(
        cx.backend,
        Icon::Plus,
        Point2D::new(rect.origin.x + 10.0, rect.origin.y + 10.0),
        13.0,
        theme.muted_foreground,
        1.4,
    );
    let text = TextLayout::single_run(
        label,
        "system-ui",
        12.0,
        theme.muted_foreground.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &text,
        Point2D::new(rect.origin.x + 30.0, rect.origin.y + 22.0),
    );
}
