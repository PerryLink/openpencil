//! Immediate-mode paint pass for the 制图台 Home surface.

#[path = "home_surface_paint_cards.rs"]
mod cards;

// Re-exported for the home surface tests, which assert the tag labels.
#[cfg(test)]
pub(super) use self::cards::card_tag_label;
use self::cards::paint_card;
use super::{home_enter, HomeEnterBlock, HomeLayout, HomePalette, HomeSurface, HOME_TOPBAR_H};
use crate::widgets::brand_icons::paint_figma_logo;
use crate::widgets::property_panel_text_input::paint_text_input_view;
use crate::widgets::{draw_icon, Icon, PaintCx};
use crate::{Color, Point2D, Rect, TextLayout, Theme};
use op_editor_core::{EditorUiState, HomeDevice, HomeFamily, HomeHit};

const SANS: &str = "system-ui";
const MONO: &str = "SF Mono";
const SERIF_CANDIDATES: [&str; 4] = [
    "Songti SC",
    "STSong",
    "Noto Serif CJK SC",
    "Source Han Serif SC",
];

fn text(
    cx: &mut PaintCx<'_>,
    content: &str,
    origin: Point2D,
    size: f32,
    color: Color,
    family: &str,
) {
    let layout = TextLayout::single_run(content, family, size, color.to_jian(), Point2D::ZERO);
    cx.backend.draw_text(&layout, origin);
}

fn text_weighted(
    cx: &mut PaintCx<'_>,
    content: &str,
    origin: Point2D,
    size: f32,
    color: Color,
    family: &str,
    weight: u16,
) {
    let layout = TextLayout::single_run(content, family, size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    cx.backend.draw_text(&layout, origin);
}

#[allow(clippy::too_many_arguments)]
fn draw_spaced_text(
    cx: &mut PaintCx<'_>,
    content: &str,
    origin: Point2D,
    size: f32,
    color: Color,
    family: &str,
    weight: u16,
    spacing: f32,
) {
    let mut x = origin.x;
    for character in content.chars() {
        let glyph = character.to_string();
        text_weighted(
            cx,
            &glyph,
            Point2D::new(x, origin.y),
            size,
            color,
            family,
            weight,
        );
        x += cx.backend.measure_text_family(&glyph, size, family) + spacing;
    }
}

fn spaced_width(cx: &mut PaintCx<'_>, content: &str, size: f32, family: &str, spacing: f32) -> f32 {
    content
        .chars()
        .map(|character| {
            cx.backend
                .measure_text_family(&character.to_string(), size, family)
                + spacing
        })
        .sum::<f32>()
        - spacing
}

fn label_color(surface: &HomeSurface<'_>) -> Color {
    home_palette(surface).ink
}

fn home_palette(surface: &HomeSurface<'_>) -> HomePalette {
    HomePalette::for_mode(surface.ui.effective_theme_mode())
}

/// Multiply a colour's alpha by `factor` — relative, so an entrance fade
/// composes with the baked alpha (absolute `Color::with_alpha` would
/// drop the fade).
fn fade(color: Color, factor: f32) -> Color {
    Color {
        a: color.a * factor,
        ..color
    }
}

/// Every palette token at `factor` of its alpha: one entrance block's
/// whole colour set fades through a single value.
fn faded(palette: HomePalette, factor: f32) -> HomePalette {
    let faded = |color| fade(color, factor);
    HomePalette {
        paper: faded(palette.paper),
        paper_2: faded(palette.paper_2),
        sheet: faded(palette.sheet),
        ink: faded(palette.ink),
        graphite: faded(palette.graphite),
        ash: faded(palette.ash),
        line: faded(palette.line),
        blue: faded(palette.blue),
        blue_2: faded(palette.blue_2),
        blue_soft: faded(palette.blue_soft),
        dots: faded(palette.dots),
        margin_rule: faded(palette.margin_rule),
    }
}

/// A rect painted `dy` px below its final position (an entrance rise).
fn shifted(rect: Rect, dy: f32) -> Rect {
    Rect::xywh(rect.origin.x, rect.origin.y + dy, rect.size.x, rect.size.y)
}

fn blue(surface: &HomeSurface<'_>) -> Color {
    home_palette(surface).blue
}

fn home_input_theme(palette: HomePalette) -> Theme {
    let mut theme = Theme::light();
    theme.background = palette.sheet;
    theme.foreground = palette.ink;
    theme.card = palette.sheet;
    theme.card_foreground = palette.ink;
    theme.primary = palette.blue;
    theme.primary_foreground = palette.sheet;
    theme.muted = palette.paper_2;
    theme.muted_foreground = palette.ash;
    theme.border = palette.line;
    theme.input = palette.line;
    theme.ring = palette.blue;
    theme.accent = palette.blue_soft;
    theme.accent_foreground = palette.blue_2;
    theme
}

fn paint_button(
    surface: &HomeSurface<'_>,
    cx: &mut PaintCx<'_>,
    rect: Rect,
    hit: HomeHit,
    active: bool,
    text_label: &str,
    palette: HomePalette,
) {
    let hovered = surface.state.hover == Some(hit);
    let pressed = surface.state.pressed == Some(hit);
    let fill = if active { palette.ink } else { palette.sheet };
    let fg = if active { palette.paper } else { palette.ink };
    cx.backend.fill_round_rect(rect, rect.size.y / 2.0, fill);
    if hovered || pressed {
        cx.backend.fill_round_rect(
            rect,
            rect.size.y / 2.0,
            if pressed {
                fade(palette.blue, 0.20)
            } else {
                fade(palette.graphite, 0.10)
            },
        );
    }
    cx.backend.stroke_round_rect(
        rect,
        rect.size.y / 2.0,
        if active { palette.ink } else { palette.line },
        1.0,
    );
    let width = rect.size.x;
    let approx = cx.backend.measure_text_family(text_label, 14.0, SANS);
    text(
        cx,
        text_label,
        Point2D::new(
            rect.origin.x + (width - approx) / 2.0,
            rect.origin.y + rect.size.y / 2.0 + 5.0,
        ),
        14.0,
        fg,
        SANS,
    );
}

pub(super) fn paint_home(surface: &HomeSurface<'_>, cx: &mut PaintCx<'_>, rect: Rect) {
    let layout = surface.layout(rect.size.x, rect.size.y);
    let palette = home_palette(surface);
    let bg = palette.paper;
    cx.backend.fill_rect(rect, bg);
    // Drafting ground: static dot grid and blue margin rule.
    let dots = palette.dots;
    let mut x = 12.0;
    while x < rect.size.x {
        let mut y = 12.0;
        while y < rect.size.y {
            cx.backend
                .fill_oval(Rect::xywh(x - 0.75, y - 0.75, 1.5, 1.5), dots);
            y += 24.0;
        }
        x += 24.0;
    }
    cx.backend.stroke_line(
        Point2D::new(56.0, 0.0),
        Point2D::new(56.0, rect.size.y),
        palette.margin_rule,
        1.0,
    );

    paint_wordmark(surface, cx, rect);
    // Footer + 专业模式 fade in together over the last window, fade only.
    let shown_at = surface.state.shown_at_ms;
    let enter = |block, index| home_enter(block, index, shown_at, surface.now_ms);
    let (_, chrome_alpha) = enter(HomeEnterBlock::Footer, 0);
    let chrome_palette = faded(palette, chrome_alpha);
    text(
        cx,
        "专业模式 · 直接进画布 →",
        Point2D::new(
            layout.professional.origin.x,
            layout.professional.origin.y + 19.0,
        ),
        13.0,
        chrome_palette.graphite,
        SANS,
    );
    if surface.state.hover == Some(HomeHit::Professional) {
        cx.backend.stroke_line(
            Point2D::new(
                layout.professional.origin.x,
                layout.professional.origin.y + 25.0,
            ),
            Point2D::new(
                layout.professional.origin.x + layout.professional.size.x,
                layout.professional.origin.y + 25.0,
            ),
            chrome_palette.blue,
            1.0,
        );
    }
    cx.backend.save();
    cx.backend.clip_rect(Rect::xywh(
        0.0,
        HOME_TOPBAR_H,
        rect.size.x,
        (layout.footer.origin.y - HOME_TOPBAR_H).max(0.0),
    ));
    let headline_family = headline_family(surface);
    let headline_text = "你想做成什么？";
    let spacing = 52.0 * 0.01;
    let headline_width = spaced_width(cx, headline_text, 52.0, headline_family, spacing);
    let (headline_dy, headline_alpha) = enter(HomeEnterBlock::Headline, 0);
    let headline_y = layout.headline.origin.y + headline_dy;
    draw_spaced_text(
        cx,
        headline_text,
        Point2D::new(
            layout.headline.origin.x + (layout.headline.size.x - headline_width) / 2.0,
            headline_y + 43.0,
        ),
        52.0,
        faded(palette, headline_alpha).ink,
        headline_family,
        600,
        spacing,
    );
    let underline_start = layout.headline.origin.x
        + (layout.headline.size.x - headline_width) / 2.0
        + cx.backend
            .measure_text_family("你想", 52.0, headline_family)
        + spacing * 2.0;
    let underline_width = spaced_width(cx, "做成什么", 52.0, headline_family, spacing);
    // The underline draws on after the headline has settled: only the
    // first fraction of the wavy path paints, in that same fraction's
    // alpha.
    let (_, underline_fraction) = enter(HomeEnterBlock::Underline, 0);
    if underline_fraction > 0.0 {
        paint_wavy_underline(
            cx,
            underline_start,
            headline_y + 50.0,
            underline_width * underline_fraction,
            faded(palette, underline_fraction).blue,
        );
    }
    let (subtitle_dy, subtitle_alpha) = enter(HomeEnterBlock::Subtitle, 0);
    text(
        cx,
        "先说要做成的东西，再放你的文字或截图。画布还在，随时进。",
        Point2D::new(
            layout.subtitle.origin.x + 44.0,
            layout.subtitle.origin.y + subtitle_dy + 16.0,
        ),
        15.0,
        faded(palette, subtitle_alpha).graphite,
        SANS,
    );

    let (sheet_dy, sheet_alpha) = enter(HomeEnterBlock::Sheet, 0);
    paint_sheet(surface, cx, layout, sheet_dy, faded(palette, sheet_alpha));
    for (index, family) in HomeFamily::ALL.into_iter().enumerate() {
        let (chip_dy, chip_alpha) = enter(HomeEnterBlock::Chip, index);
        paint_button(
            surface,
            cx,
            shifted(layout.chips[index], chip_dy),
            HomeHit::Chip(family),
            surface.state.bound == Some(family),
            family.label(),
            faded(palette, chip_alpha),
        );
    }
    // The expected row never moves on entrance.
    paint_expected(surface, cx, layout);
    for (index, family) in HomeFamily::ALL.into_iter().enumerate() {
        let (card_dy, card_alpha) = enter(HomeEnterBlock::Card, index);
        paint_card(
            surface,
            cx,
            shifted(layout.cards[index], card_dy),
            family,
            faded(palette, card_alpha),
            card_alpha,
        );
    }
    cx.backend.restore();
    paint_footer(cx, layout, chrome_palette);
}

fn headline_family(surface: &HomeSurface<'_>) -> &'static str {
    resolve_headline_family(surface.ui)
}

fn resolve_headline_family(ui: &EditorUiState) -> &'static str {
    for candidate in SERIF_CANDIDATES {
        if ui
            .system_font_families
            .iter()
            .chain(ui.bundled_font_families.iter())
            .any(|family| family.eq_ignore_ascii_case(candidate))
        {
            return candidate;
        }
    }
    SANS
}

fn paint_wavy_underline(cx: &mut PaintCx<'_>, x: f32, y: f32, width: f32, color: Color) {
    let segment = width / 6.0;
    let points = [
        Point2D::new(x, y),
        Point2D::new(x + segment, y - 1.0),
        Point2D::new(x + segment * 2.0, y + 0.5),
        Point2D::new(x + segment * 3.0, y - 0.5),
        Point2D::new(x + segment * 4.0, y + 0.8),
        Point2D::new(x + segment * 5.0, y - 0.4),
        Point2D::new(x + width, y),
    ];
    for pair in points.windows(2) {
        cx.backend.stroke_line(pair[0], pair[1], color, 2.2);
    }
}

fn paint_wordmark(surface: &HomeSurface<'_>, cx: &mut PaintCx<'_>, rect: Rect) {
    let mark = Rect::xywh(80.0, 22.0, 18.0, 18.0);
    cx.backend
        .stroke_round_rect(mark, 4.0, label_color(surface), 1.5);
    cx.backend
        .fill_round_rect(Rect::xywh(84.0, 26.0, 8.0, 8.0), 1.0, blue(surface));
    text(
        cx,
        "OpenPencil",
        Point2D::new(108.0, 36.0),
        14.0,
        label_color(surface),
        SANS,
    );
    let _ = rect;
}

fn paint_sheet(
    surface: &HomeSurface<'_>,
    cx: &mut PaintCx<'_>,
    layout: HomeLayout,
    rise: f32,
    palette: HomePalette,
) {
    // The whole drafting sheet (input, refs, send) enters as one block:
    // every rect paints `rise` px below its final position.
    let sheet = shifted(layout.sheet, rise);
    let sheet_text = shifted(layout.sheet_text, rise);
    let screenshot = shifted(layout.screenshot, rise);
    let reference_link = shifted(layout.reference_link, rise);
    let figma = shifted(layout.figma, rise);
    let example = shifted(layout.example, rise);
    let send = shifted(layout.send, rise);
    // Home owns its focus treatment. The editor's normal blue ring must not
    // leak into the drafting-table surface.
    cx.backend.fill_drop_shadow(
        Rect::xywh(
            sheet.origin.x - 3.0,
            sheet.origin.y - 3.0,
            sheet.size.x + 6.0,
            sheet.size.y + 6.0,
        ),
        14.0,
        6.0,
        fade(palette.blue_soft, 0.55),
    );
    cx.backend.fill_round_rect(sheet, 14.0, palette.sheet);
    cx.backend
        .stroke_round_rect(sheet, 14.0, fade(palette.blue, 0.55), 1.0);
    for tick in 1..28 {
        let x = sheet.origin.x + tick as f32 * 22.0;
        cx.backend.stroke_line(
            Point2D::new(x, sheet.origin.y),
            Point2D::new(x, sheet.origin.y + 9.0),
            fade(palette.line, 0.45),
            1.0,
        );
    }
    let placeholder = surface
        .state
        .bound
        .unwrap_or(HomeFamily::AppUi)
        .placeholder();
    let input_theme = home_input_theme(palette);
    paint_text_input_view(
        cx,
        &input_theme,
        &surface.state.input,
        sheet_text,
        16.0,
        0.0,
        sheet_text.origin.y + 25.0,
        surface.now_ms,
        if surface.state.draft.is_empty() {
            placeholder
        } else {
            ""
        },
        surface.state.visible,
    );
    let refs = [
        (screenshot, HomeHit::Attachment, "截图", Some(Icon::Image)),
        (
            reference_link,
            HomeHit::ReferenceLink,
            "参考链接",
            Some(Icon::ArrowUpRight),
        ),
        // The Figma row carries the brand mark, not a lucide glyph.
        (figma, HomeHit::Figma, "Figma", None),
        (
            example,
            HomeHit::TryExample,
            "试试这个示例",
            Some(Icon::Sparkles),
        ),
    ];
    for (rect, hit, label, icon) in refs {
        let disabled = matches!(hit, HomeHit::ReferenceLink | HomeHit::Figma);
        let hovered = surface.state.hover == Some(hit);
        // The example entry is the sheet's one accent (prototype `.refs .ex`).
        let color = match hit {
            HomeHit::TryExample if hovered => palette.blue_2,
            HomeHit::TryExample => palette.blue,
            _ if disabled => fade(palette.graphite, 0.65),
            _ => palette.graphite,
        };
        if hovered && !disabled {
            cx.backend.fill_round_rect(rect, 8.0, palette.paper_2);
        }
        let icon_origin = Point2D::new(rect.origin.x, rect.origin.y + 6.0);
        match icon {
            Some(icon) => draw_icon(cx.backend, icon, icon_origin, 14.0, color, 1.25),
            None => paint_figma_logo(cx.backend, icon_origin, 14.0, color),
        }
        text(
            cx,
            label,
            Point2D::new(rect.origin.x + 19.0, rect.origin.y + 19.0),
            13.0,
            color,
            SANS,
        );
        if disabled && hovered {
            let tooltip = Rect::xywh(rect.origin.x, rect.origin.y - 28.0, 68.0, 22.0);
            cx.backend.fill_round_rect(tooltip, 7.0, palette.ink);
            text(
                cx,
                "即将支持",
                Point2D::new(tooltip.origin.x + 10.0, tooltip.origin.y + 15.0),
                11.0,
                palette.paper,
                SANS,
            );
        }
    }
    let send_fill = if surface.state.draft.trim().is_empty() {
        fade(palette.ink, 0.28)
    } else {
        palette.blue
    };
    cx.backend.fill_oval(send, send_fill);
    if surface.state.pressed == Some(HomeHit::Send) {
        cx.backend.stroke_oval(send, fade(palette.ink, 0.30), 2.0);
    }
    draw_icon(
        cx.backend,
        Icon::ArrowUp,
        Point2D::new(send.origin.x + 11.0, send.origin.y + 11.0),
        18.0,
        palette.paper,
        1.8,
    );
    text(
        cx,
        "⏎ 发送",
        Point2D::new(send.origin.x - 58.0, send.origin.y + 25.0),
        12.0,
        palette.ash,
        MONO,
    );
}

fn paint_expected(surface: &HomeSurface<'_>, cx: &mut PaintCx<'_>, layout: HomeLayout) {
    let palette = home_palette(surface);
    let Some(family) = surface.state.bound else {
        return;
    };
    text(
        cx,
        "预期产物：",
        Point2D::new(layout.expected.origin.x, layout.expected.origin.y + 20.0),
        13.0,
        palette.graphite,
        SANS,
    );
    let mut x = layout.expected.origin.x + 72.0;
    for output in family.expected_outputs() {
        let w = cx.backend.measure_text_family(output, 12.0, SANS) + 20.0;
        let pill = Rect::xywh(x, layout.expected.origin.y + 1.0, w, 24.0);
        cx.backend.fill_round_rect(pill, 12.0, palette.blue_soft);
        text(
            cx,
            output,
            Point2D::new(x + 10.0, layout.expected.origin.y + 18.0),
            12.0,
            palette.blue_2,
            SANS,
        );
        x += w + 6.0;
    }
    if family == HomeFamily::AppUi {
        text(
            cx,
            "给哪种设备？",
            Point2D::new(
                layout.expected.origin.x + 374.0,
                layout.expected.origin.y + 18.0,
            ),
            13.0,
            palette.graphite,
            SANS,
        );
        let active = if surface.state.device == HomeDevice::Mobile {
            layout.device_mobile
        } else {
            layout.device_desktop
        };
        let inactive = if surface.state.device == HomeDevice::Mobile {
            layout.device_desktop
        } else {
            layout.device_mobile
        };
        cx.backend.fill_round_rect(inactive, 13.0, palette.sheet);
        cx.backend
            .stroke_round_rect(inactive, 13.0, palette.line, 1.0);
        cx.backend.fill_round_rect(active, 13.0, palette.ink);
        text(
            cx,
            "手机",
            Point2D::new(
                layout.device_mobile.origin.x + 9.0,
                layout.device_mobile.origin.y + 18.0,
            ),
            12.0,
            if surface.state.device == HomeDevice::Mobile {
                palette.paper
            } else {
                palette.graphite
            },
            SANS,
        );
        text(
            cx,
            "桌面",
            Point2D::new(
                layout.device_desktop.origin.x + 9.0,
                layout.device_desktop.origin.y + 18.0,
            ),
            12.0,
            if surface.state.device == HomeDevice::Desktop {
                palette.paper
            } else {
                palette.graphite
            },
            SANS,
        );
    }
}

fn paint_footer(cx: &mut PaintCx<'_>, layout: HomeLayout, palette: HomePalette) {
    text(
        cx,
        "最近项目（空）",
        Point2D::new(layout.footer.origin.x, layout.footer.origin.y + 16.0),
        13.0,
        palette.graphite,
        SANS,
    );
    text(
        cx,
        "·",
        Point2D::new(layout.footer.origin.x + 88.0, layout.footer.origin.y + 16.0),
        13.0,
        palette.graphite,
        SANS,
    );
    text(
        cx,
        "新建空白画布",
        Point2D::new(
            layout.footer.origin.x + 104.0,
            layout.footer.origin.y + 16.0,
        ),
        13.0,
        palette.ink,
        SANS,
    );
    text(
        cx,
        "·  打开文件",
        Point2D::new(
            layout.footer.origin.x + 206.0,
            layout.footer.origin.y + 16.0,
        ),
        13.0,
        palette.ink,
        SANS,
    );
}

#[cfg(test)]
mod tests {
    use super::{resolve_headline_family, EditorUiState};
    use std::sync::Arc;

    #[test]
    fn headline_serif_resolution_follows_the_authored_candidate_order() {
        let ui = EditorUiState {
            system_font_families: Arc::new(vec!["Source Han Serif SC".into(), "Songti SC".into()]),
            ..EditorUiState::default()
        };
        assert_eq!(resolve_headline_family(&ui), "Songti SC");
    }

    #[test]
    fn headline_serif_resolution_falls_back_to_sans_when_unavailable() {
        let ui = EditorUiState {
            system_font_families: Arc::new(vec!["PingFang SC".into()]),
            bundled_font_families: Arc::new(vec!["Inter".into()]),
            ..EditorUiState::default()
        };
        assert_eq!(resolve_headline_family(&ui), "system-ui");
    }
}
