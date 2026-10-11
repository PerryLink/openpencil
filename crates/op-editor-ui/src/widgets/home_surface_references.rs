//! One bounded material row below the Home composer's editable text.

use super::{brand_chip, copy::SANS, fade, HomeSurface, StudioPalette};
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::{text_metrics, PaintCx};
use crate::{Point2D, Rect};

const ROW_H: f32 = 26.0;
const ROW_GAP: f32 = 6.0;
const TEXT_RESERVE: f32 = ROW_H + ROW_GAP;
const CHIP_GAP: f32 = 6.0;
const FONT: f32 = 11.0;
const ICON_LABEL_PAD: f32 = 38.0;

pub(crate) fn input_text_rect(surface: &HomeSurface<'_>, input_box: Rect) -> Rect {
    let has_references = !surface.attachment_names.is_empty() || surface.state.brand.chip_visible();
    // Leave two complete text lines above a phone's material row.
    let pad_y = match (surface.ui.compact_layout(), has_references) {
        (true, true) => 5.0,
        (true, false) => 7.0,
        (false, _) => 9.0,
    };
    let reserved = if has_references { TEXT_RESERVE } else { 0.0 };
    Rect::xywh(
        input_box.origin.x + 1.0,
        input_box.origin.y + pad_y,
        (input_box.size.x - 2.0).max(0.0),
        (input_box.size.y - 2.0 * pad_y - reserved).max(0.0),
    )
}

pub(crate) fn text_clip_rect(surface: &HomeSurface<'_>, input_box: Rect) -> Rect {
    input_text_rect(surface, input_box)
}

fn attachment_bounds(surface: &HomeSurface<'_>, input_box: Rect) -> Rect {
    let x = input_box.origin.x + 10.0;
    let right = brand_chip::chip_rects(surface, input_box)
        .map(|(chip, _)| chip.origin.x - CHIP_GAP)
        .unwrap_or(input_box.origin.x + input_box.size.x - 10.0);
    Rect::xywh(
        x,
        input_box.origin.y + input_box.size.y - TEXT_RESERVE,
        (right - x).max(0.0),
        ROW_H,
    )
}

#[derive(Debug)]
struct AttachmentChip {
    rect: Rect,
    label: String,
    image: bool,
}

fn chip_label(name: &str) -> String {
    let stem = match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    };
    if stem.chars().count() <= 10 {
        return stem.to_string();
    }
    let mut cut: String = stem.chars().take(9).collect();
    cut.push('…');
    cut
}

/// Keep the row on one line; a count represents references that do not fit.
fn attachment_chips(
    names: &[String],
    bounds: Rect,
    mut measure: impl FnMut(&str) -> f32,
) -> Vec<AttachmentChip> {
    if names.is_empty() || bounds.size.x <= 0.0 {
        return Vec::new();
    }
    let mut visible = names.len().min(3);
    let (overflow, overflow_w, each_w) = loop {
        let overflow = names.len() - visible;
        let overflow_w = if overflow > 0 {
            measure(&format!("+{overflow}")) + 16.0
        } else {
            0.0
        };
        let gaps = (visible.saturating_sub(1) + usize::from(overflow > 0)) as f32 * CHIP_GAP;
        let each_w = (bounds.size.x - gaps - overflow_w).max(0.0) / visible as f32;
        if visible == 1 || each_w >= 78.0 {
            break (overflow, overflow_w, each_w);
        }
        visible -= 1;
    };
    if visible == 1 && each_w < 78.0 {
        // A brand may share a narrow phone row. Keep every reference counted.
        // An image icon plus "… +N" conveys less than the total count here.
        return vec![AttachmentChip {
            rect: Rect::xywh(bounds.origin.x, bounds.origin.y, bounds.size.x, ROW_H),
            label: names.len().to_string(),
            image: true,
        }];
    }
    let mut chips = Vec::new();
    let mut x = bounds.origin.x;
    for name in names.iter().take(visible) {
        let label = chip_label(name);
        let width = (measure(&label) + ICON_LABEL_PAD).min(each_w);
        let label = crate::util::ellipsize_to_width(&label, width - ICON_LABEL_PAD, &mut measure);
        chips.push(AttachmentChip {
            rect: Rect::xywh(x, bounds.origin.y, width, ROW_H),
            label,
            image: true,
        });
        x += width + CHIP_GAP;
    }
    if overflow > 0 {
        chips.push(AttachmentChip {
            rect: Rect::xywh(x, bounds.origin.y, overflow_w, ROW_H),
            label: format!("+{overflow}"),
            image: false,
        });
    }
    chips
}

pub(crate) fn paint(
    surface: &HomeSurface<'_>,
    cx: &mut PaintCx<'_>,
    input_box: Rect,
    palette: StudioPalette,
) {
    let bounds = attachment_bounds(surface, input_box);
    let chips = attachment_chips(&surface.attachment_names, bounds, |label| {
        text_metrics::measure_chrome(cx.backend, label, FONT)
    });
    cx.backend.save();
    cx.backend.clip_rect(bounds);
    for chip in chips {
        cx.backend.fill_round_rect(chip.rect, 6.0, palette.chip_bg);
        cx.backend
            .stroke_round_rect(chip.rect, 6.0, palette.chip_line, 1.0);
        let label_x = if chip.image {
            draw_icon(
                cx.backend,
                Icon::Image,
                Point2D::new(chip.rect.origin.x + 7.0, chip.rect.origin.y + 6.0),
                14.0,
                fade(palette.blue, 0.85),
                1.5,
            );
            chip.rect.origin.x + 30.0
        } else {
            chip.rect.origin.x + 8.0
        };
        let text = crate::TextLayout::single_run(
            &chip.label,
            SANS,
            FONT,
            fade(palette.ink, 0.75).to_jian(),
            Point2D::ZERO,
        );
        cx.backend.draw_text(
            &text,
            Point2D::new(
                label_x,
                jian_widgets::centered_text_baseline_y(chip.rect, FONT),
            ),
        );
    }
    cx.backend.restore();
    brand_chip::paint_brand_chip(surface, cx, input_box, palette);
}

#[cfg(test)]
#[path = "home_surface_references_tests.rs"]
mod tests;
