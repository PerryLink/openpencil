//! Family-card artwork for the 制图台 Home surface: the App card's
//! three-phone flow, the raster template previews for the others, and
//! the card chrome that frames them.

use super::{fade, text, text_weighted, HomePalette, HomeSurface, MONO, SANS};
use crate::widgets::canvas_viewport_image::{
    has_cached_image_bytes, note_pending_decode, required_raster_edge, store_remote_image_bytes,
};
use crate::widgets::PaintCx;
use crate::{ImageDrawMode, Point2D, Rect};
use op_editor_core::{HomeFamily, HomeHit, ThemeMode};

pub(super) fn paint_app_flow(
    surface: &HomeSurface<'_>,
    cx: &mut PaintCx<'_>,
    art: Rect,
    palette: HomePalette,
) {
    let hovered = surface.state.hover == Some(HomeHit::Card(HomeFamily::AppUi));
    let phone_w = 56.0;
    let phone_h = art.size.y.min(136.0) - 30.0;
    let y = art.origin.y + (art.size.y - phone_h) / 2.0;
    let start_x = art.origin.x + (art.size.x - phone_w * 3.0 - 48.0) / 2.0;
    for index in 0..3 {
        let x = start_x + index as f32 * (phone_w + 24.0);
        let nudged = if hovered && index == 1 {
            Rect::xywh(x + 3.0, y - 3.0, phone_w, phone_h)
        } else {
            Rect::xywh(x, y, phone_w, phone_h)
        };
        let phone = nudged;
        cx.backend.fill_round_rect(phone, 8.0, palette.sheet);
        cx.backend
            .stroke_round_rect(phone, 8.0, palette.graphite, 1.2);
        // Prototype `.ph`: an 8 px header bar in line, three 6 px rows in
        // paper-2 with 5 px gaps, and the action button pinned to the
        // bottom (blue on the middle screen).
        let (px, py) = (phone.origin.x, phone.origin.y);
        cx.backend.fill_round_rect(
            Rect::xywh(px + 6.0, py + 8.0, phone_w - 12.0, 8.0),
            2.0,
            palette.line,
        );
        for row in 0..3 {
            cx.backend.fill_round_rect(
                Rect::xywh(px + 6.0, py + 21.0 + row as f32 * 11.0, phone_w - 12.0, 6.0),
                2.0,
                palette.paper_2,
            );
        }
        cx.backend.fill_round_rect(
            Rect::xywh(px + 6.0, py + phone_h - 18.0, phone_w - 12.0, 10.0),
            2.0,
            if index == 1 {
                palette.blue
            } else {
                fade(palette.graphite, 0.35)
            },
        );
        if index < 2 {
            let arrow = if hovered {
                palette.blue
            } else {
                palette.graphite
            };
            let ax = x + phone_w + 4.0 + if hovered { 2.0 } else { 0.0 };
            let ay = y + phone_h / 2.0;
            cx.backend.stroke_line(
                Point2D::new(ax, ay),
                Point2D::new(ax + 16.0, ay),
                arrow,
                1.5,
            );
            cx.backend.stroke_line(
                Point2D::new(ax + 11.0, ay - 4.5),
                Point2D::new(ax + 16.0, ay),
                arrow,
                1.5,
            );
            cx.backend.stroke_line(
                Point2D::new(ax + 11.0, ay + 4.5),
                Point2D::new(ax + 16.0, ay),
                arrow,
                1.5,
            );
        }
    }
}

pub(super) fn paint_template_preview(
    cx: &mut PaintCx<'_>,
    rect: Rect,
    id: &str,
    palette: HomePalette,
    alpha: f32,
) {
    let Some(asset) = crate::widgets::scene_template_previews::scene_template_preview(id) else {
        return;
    };
    let Some(bytes) = asset.bytes else {
        op_editor_core::web_assets::request(asset.route);
        return;
    };
    if !has_cached_image_bytes(asset.image_id) {
        store_remote_image_bytes(asset.image_id, bytes.to_vec());
    }
    let max_edge = required_raster_edge(rect, cx.backend.dpi_scale());
    let sharp = cx.backend.image_decoded(asset.image_id, bytes, max_edge);
    if !sharp {
        note_pending_decode(asset.image_id, max_edge);
    }
    // The shared image draw carries no alpha, so a template preview only
    // shows once its card's entrance has settled; until then the faded
    // placeholder keeps the block reading as one unit (the same slot the
    // cold-start decode path paints).
    let resident = sharp || cx.backend.image_resident(asset.image_id);
    if resident && alpha >= 1.0 {
        cx.backend
            .draw_image_with_mode(rect, asset.image_id, bytes, ImageDrawMode::Fill);
    } else {
        cx.backend.fill_rect(rect, palette.paper_2);
    }
}

/// Corner tag on a family card's art; the App card names its three screens.
pub(crate) fn card_tag_label(family: HomeFamily) -> &'static str {
    if family == HomeFamily::AppUi {
        "示例 · 三屏"
    } else {
        "示例"
    }
}

/// Paint one family card. `rect` is the FINAL layout rect already moved by
/// the entrance rise; `palette`/`alpha` carry the block's fade. The art
/// content lags the card by up to 2 px — the "翘起" settle.
pub(super) fn paint_card(
    surface: &HomeSurface<'_>,
    cx: &mut PaintCx<'_>,
    rect: Rect,
    family: HomeFamily,
    palette: HomePalette,
    alpha: f32,
) {
    let hover = surface.state.hover == Some(HomeHit::Card(family));
    let pressed = surface.state.pressed == Some(HomeHit::Card(family));
    let paint_rect = if hover {
        Rect::xywh(rect.origin.x, rect.origin.y - 6.0, rect.size.x, rect.size.y)
    } else {
        rect
    };
    // The drop shadow only exists once the card has faded in (the shared
    // shadow fill has no alpha channel of its own to fade through).
    if surface.ui.effective_theme_mode() == ThemeMode::Light && alpha > 0.95 {
        // Prototype `--shadow`: `0 12px 28px -12px rgba(27,26,23,.18)` — a
        // soft pool under the card, never a halo around it; hover deepens
        // it (`0 22px 40px -16px … .28`).
        let (drop, spread, tint) = if hover {
            (18.0, 20.0, 0.24)
        } else {
            (12.0, 16.0, 0.16)
        };
        cx.backend.fill_drop_shadow(
            Rect::xywh(
                paint_rect.origin.x + 10.0,
                paint_rect.origin.y + drop,
                paint_rect.size.x - 20.0,
                paint_rect.size.y - 8.0,
            ),
            14.0,
            spread,
            fade(palette.ink, tint),
        );
    }
    cx.backend.fill_round_rect(paint_rect, 14.0, palette.sheet);
    if hover || pressed {
        cx.backend.fill_round_rect(
            paint_rect,
            14.0,
            fade(palette.blue, if pressed { 0.18 } else { 0.08 }),
        );
    }
    let art = Rect::xywh(
        paint_rect.origin.x,
        paint_rect.origin.y,
        paint_rect.size.x,
        (paint_rect.size.y.min(200.0) - 64.0).max(40.0),
    );
    // The art shares the card's rounded top corners; the previews below
    // are clipped to the same shape so nothing pokes past the border.
    cx.backend.save();
    cx.backend.clip_round_rect(paint_rect, 14.0);
    cx.backend.fill_rect(art, palette.paper_2);
    let art_settle = (1.0 - alpha) * 2.0;
    let art_content = Rect::xywh(
        art.origin.x,
        art.origin.y + art_settle,
        art.size.x,
        art.size.y,
    );
    match family {
        HomeFamily::AppUi => paint_app_flow(surface, cx, art_content, palette),
        HomeFamily::KnowledgeCards => {
            paint_template_preview(cx, art_content, "knowledge-carousel", palette, alpha)
        }
        HomeFamily::ScreenshotTutorial => {
            paint_template_preview(cx, art_content, "screenshot-tutorial", palette, alpha)
        }
        HomeFamily::EventPoster => {
            paint_template_preview(cx, art_content, "music-fest-poster-card", palette, alpha)
        }
    }
    cx.backend.restore();
    // The tag is the art's topmost layer (prototype `.card .tag`): it
    // paints after the family art so the App card's first phone cannot
    // cover it.
    let tag_label = card_tag_label(family);
    let label_w = cx.backend.measure_text_family(tag_label, 11.0, MONO);
    let tag_w = (label_w + 16.0).max(44.0);
    let tag = Rect::xywh(art.origin.x + 12.0, art.origin.y + 12.0, tag_w, 22.0);
    cx.backend
        .fill_round_rect(tag, 7.0, fade(palette.sheet, 0.86));
    text(
        cx,
        tag_label,
        Point2D::new(tag.origin.x + 8.0, tag.origin.y + 15.0),
        11.0,
        palette.graphite,
        MONO,
    );
    // The border goes on last so the art never paints over it.
    cx.backend.stroke_round_rect(
        paint_rect,
        14.0,
        if surface.state.bound == Some(family) {
            palette.blue
        } else {
            palette.line
        },
        1.0,
    );
    let title_y = paint_rect.origin.y + paint_rect.size.y - 43.0;
    text_weighted(
        cx,
        family.label(),
        Point2D::new(paint_rect.origin.x + 14.0, title_y),
        15.0,
        palette.ink,
        SANS,
        650,
    );
    let desc = match family {
        HomeFamily::AppUi => "一句话或一张截图，到可编辑的高保真界面",
        HomeFamily::KnowledgeCards => "把这段文字做成一套今天能发的图",
        HomeFamily::ScreenshotTutorial => "把几张截图串成一篇步骤图",
        HomeFamily::EventPoster => "做一组完整一致的活动视觉",
    };
    text(
        cx,
        desc,
        Point2D::new(paint_rect.origin.x + 14.0, title_y + 22.0),
        12.5,
        palette.graphite,
        SANS,
    );
}
