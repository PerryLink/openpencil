//! A bounded modal list for every staged Home reference image.

use super::{copy::SANS, fade, HomeLayout, HomeSurface, StudioPalette};
use crate::widgets::{
    icons::{draw_icon, Icon},
    text_metrics, PaintCx,
};
use crate::{Color, Point2D, Rect, TextLayout};
use op_editor_core::HomeHit;

struct ManagerLayout {
    card: Rect,
    close: Rect,
    rows: Vec<Rect>,
    remove: Vec<Rect>,
}

fn layout(surface: &HomeSurface<'_>, viewport: Rect) -> ManagerLayout {
    let width = (viewport.size.x - 24.0).clamp(0.0, 480.0);
    let height = 64.0 + surface.attachment_names.len() as f32 * 52.0;
    let card = Rect::xywh(
        viewport.origin.x + (viewport.size.x - width) / 2.0,
        viewport.origin.y + ((viewport.size.y - height) / 2.0).max(8.0),
        width,
        height,
    );
    let close = Rect::xywh(
        card.origin.x + width - 52.0,
        card.origin.y + 4.0,
        44.0,
        44.0,
    );
    let rows: Vec<_> = (0..surface.attachment_names.len())
        .map(|index| {
            Rect::xywh(
                card.origin.x + 12.0,
                card.origin.y + 56.0 + index as f32 * 52.0,
                width - 24.0,
                44.0,
            )
        })
        .collect();
    // Translated labels stay readable, while every remove target is at
    // least 44 px high on touch screens.
    let remove_w = (super::copy::estimate_text_w(
        op_i18n::translate(surface.ui.locale, "home.references.remove"),
        12.0,
    ) + 20.0)
        .max(64.0)
        .min(width * 0.42);
    let remove = rows
        .iter()
        .map(|row| {
            Rect::xywh(
                row.origin.x + row.size.x - remove_w,
                row.origin.y,
                remove_w,
                row.size.y,
            )
        })
        .collect();
    ManagerLayout {
        card,
        close,
        rows,
        remove,
    }
}

pub(super) fn focus_rect(surface: &HomeSurface<'_>, viewport: Rect, hit: HomeHit) -> Option<Rect> {
    if !surface.state.references_open {
        return None;
    }
    let layout = layout(surface, viewport);
    match hit {
        HomeHit::ReferencesClose => Some(layout.close),
        HomeHit::ReferenceRemove(index) => layout.remove.get(index).copied(),
        _ => None,
    }
}

pub(super) fn hit(surface: &HomeSurface<'_>, viewport: Rect, point: Point2D) -> HomeHit {
    let layout = layout(surface, viewport);
    if !layout.card.contains(point) || layout.close.contains(point) {
        return HomeHit::ReferencesClose;
    }
    layout
        .remove
        .iter()
        .position(|rect| rect.contains(point))
        .map(HomeHit::ReferenceRemove)
        .unwrap_or(HomeHit::References)
}

pub(super) fn paint(surface: &HomeSurface<'_>, cx: &mut PaintCx<'_>, home: &HomeLayout) {
    let palette = StudioPalette::for_mode(surface.ui.effective_theme_mode());
    let layout = layout(surface, home.viewport);
    cx.backend.fill_rect(home.viewport, fade(Color::BLACK, 0.3));
    cx.backend
        .fill_round_rect(layout.card, 14.0, palette.raised);
    cx.backend
        .stroke_round_rect(layout.card, 14.0, palette.raised_line, 1.0);
    let title_rect = Rect::xywh(
        layout.card.origin.x + 20.0,
        layout.card.origin.y + 8.0,
        layout.card.size.x - 84.0,
        36.0,
    );
    let title = format!(
        "{} ({})",
        op_i18n::translate(surface.ui.locale, "home.references.title"),
        surface.attachment_names.len()
    );
    label(cx, &title, title_rect, 16.0, palette.ink);
    if surface.state.hover == Some(HomeHit::ReferencesClose) {
        cx.backend
            .fill_round_rect(layout.close, 8.0, palette.button_hover);
    }
    draw_icon(
        cx.backend,
        Icon::Close,
        Point2D::new(layout.close.origin.x + 14.0, layout.close.origin.y + 14.0),
        16.0,
        palette.sub,
        1.5,
    );
    for (index, ((row, remove), name)) in layout
        .rows
        .iter()
        .zip(&layout.remove)
        .zip(&surface.attachment_names)
        .enumerate()
    {
        cx.backend.fill_round_rect(*row, 8.0, palette.chip_bg);
        draw_icon(
            cx.backend,
            Icon::Image,
            Point2D::new(row.origin.x + 10.0, row.origin.y + 14.0),
            16.0,
            palette.sub,
            1.5,
        );
        let text = Rect::xywh(
            row.origin.x + 36.0,
            row.origin.y,
            (remove.origin.x - row.origin.x - 44.0).max(0.0),
            row.size.y,
        );
        label(cx, name, text, 12.0, palette.ink);
        if surface.state.hover == Some(HomeHit::ReferenceRemove(index)) {
            cx.backend
                .fill_round_rect(*remove, 8.0, palette.button_hover);
        }
        let text = Rect::xywh(
            remove.origin.x + 10.0,
            remove.origin.y,
            remove.size.x - 20.0,
            remove.size.y,
        );
        label(
            cx,
            op_i18n::translate(surface.ui.locale, "home.references.remove"),
            text,
            12.0,
            palette.link,
        );
    }
    super::focus::paint_home_focus_ring(surface, cx, home, palette);
}

fn label(cx: &mut PaintCx<'_>, text: &str, rect: Rect, font: f32, ink: Color) {
    let text = text_metrics::fit_chrome(cx.backend, text, rect.size.x, font);
    cx.backend.draw_text(
        &TextLayout::single_run(&text, SANS, font, ink.to_jian(), Point2D::ZERO),
        Point2D::new(
            rect.origin.x,
            jian_widgets::centered_text_baseline_y(rect, font),
        ),
    );
}
