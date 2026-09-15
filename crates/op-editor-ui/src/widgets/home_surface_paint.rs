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
    // "专业模式" is the quiet label, "直接进画布 →" the action; the pair
    // is right-aligned to the page margin, and hover inks the action
    // with a hairline underneath (prototype `.pro:hover`).
    let pro_hover = surface.state.hover == Some(HomeHit::Professional);
    let quiet = "专业模式";
    let action = "直接进画布 →";
    let quiet_w = cx.backend.measure_text_family(quiet, 13.0, SANS);
    let action_w = cx.backend.measure_text_family(action, 13.0, SANS);
    let right = layout.professional.origin.x + layout.professional.size.x;
    let action_x = right - action_w;
    let quiet_x = action_x - 6.0 - quiet_w;
    let base_y = layout.professional.origin.y + 19.0;
    text(
        cx,
        quiet,
        Point2D::new(quiet_x, base_y),
        13.0,
        chrome_palette.ash,
        SANS,
    );
    text_weighted(
        cx,
        action,
        Point2D::new(action_x, base_y),
        13.0,
        if pro_hover {
            chrome_palette.ink
        } else {
            chrome_palette.graphite
        },
        SANS,
        500,
    );
    if pro_hover {
        cx.backend.stroke_line(
            Point2D::new(action_x, base_y + 5.0),
            Point2D::new(right, base_y + 5.0),
            chrome_palette.ink,
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
        700,
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
            headline_y + 43.0,
            underline_width,
            underline_fraction,
            palette.blue,
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
    if surface.state.connect_card_open {
        super::connect::paint_connect_card(surface, cx, &layout, palette);
    }
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
    serif_family(surface.ui)
}

/// The serif face the Home headlines and the connect-card title paint
/// in — first bundled/system CJK serif available, else the editor sans.
pub(super) fn serif_family(ui: &EditorUiState) -> &'static str {
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

/// The hand-drawn stroke under 做成什么: the prototype's SVG
/// `M2 9 C 40 3, 80 12, 120 7 S 180 4, 198 8` (viewBox 200×14, stretched to
/// 104 % of the word and hanging 8 px under it), flattened to a polyline
/// and drawn left→right up to `fraction` of its length.
fn paint_wavy_underline(
    cx: &mut PaintCx<'_>,
    x: f32,
    baseline_y: f32,
    width: f32,
    fraction: f32,
    color: Color,
) {
    const STEPS: usize = 18;
    let left = x - width * 0.02;
    let scale_x = width * 1.04 / 200.0;
    let top = baseline_y - 1.0;
    let map = |px: f32, py: f32| Point2D::new(left + px * scale_x, top + py);
    let cubic = |a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32), t: f32| {
        let u = 1.0 - t;
        (
            u * u * u * a.0 + 3.0 * u * u * t * b.0 + 3.0 * u * t * t * c.0 + t * t * t * d.0,
            u * u * u * a.1 + 3.0 * u * u * t * b.1 + 3.0 * u * t * t * c.1 + t * t * t * d.1,
        )
    };
    let mut points: Vec<Point2D> = Vec::with_capacity(STEPS * 2 + 1);
    for i in 0..=STEPS {
        let t = i as f32 / STEPS as f32;
        let (px, py) = cubic((2.0, 9.0), (40.0, 3.0), (80.0, 12.0), (120.0, 7.0), t);
        points.push(map(px, py));
    }
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        // `S 180 4, 198 8` reflects the previous control point about (120, 7).
        let (px, py) = cubic((120.0, 7.0), (160.0, 2.0), (180.0, 4.0), (198.0, 8.0), t);
        points.push(map(px, py));
    }
    let drawn = ((points.len() - 1) as f32 * fraction.clamp(0.0, 1.0)).round() as usize;
    for pair in points[..=drawn.min(points.len() - 1)].windows(2) {
        cx.backend.stroke_line(pair[0], pair[1], color, 2.4);
    }
}

fn paint_wordmark(surface: &HomeSurface<'_>, cx: &mut PaintCx<'_>, rect: Rect) {
    let mark = Rect::xywh(80.0, 22.0, 18.0, 18.0);
    cx.backend
        .stroke_round_rect(mark, 4.0, label_color(surface), 1.5);
    // The 8 px square rotated 45° from the prototype wordmark.
    let (cx0, cy0, r) = (89.0, 31.0, 5.6);
    cx.backend.fill_polygon(
        &[
            Point2D::new(cx0, cy0 - r),
            Point2D::new(cx0 + r, cy0),
            Point2D::new(cx0, cy0 + r),
            Point2D::new(cx0 - r, cy0),
        ],
        blue(surface),
    );
    draw_spaced_text(
        cx,
        "OpenPencil",
        Point2D::new(108.0, 36.0),
        14.0,
        label_color(surface),
        SANS,
        600,
        0.28,
    );
    let _ = rect;
}

/// What sits in front of a sheet entry's label.
#[derive(Clone, Copy)]
enum RefIcon {
    Lucide(Icon),
    Figma,
    None,
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
    // Home owns its focus treatment. At rest the sheet is a piece of
    // paper on the table (soft shadow, hairline border); it only takes
    // the blue ring once the user engages it — a draft in progress or
    // the pointer on it — never on a cold launch.
    let engaged = !surface.state.draft.is_empty()
        || matches!(
            surface.state.hover,
            Some(HomeHit::Sheet) | Some(HomeHit::Send)
        );
    cx.backend.fill_drop_shadow(
        Rect::xywh(
            sheet.origin.x + 8.0,
            sheet.origin.y + 12.0,
            sheet.size.x - 16.0,
            sheet.size.y - 8.0,
        ),
        14.0,
        16.0,
        fade(palette.ink, 0.14),
    );
    if engaged {
        cx.backend.fill_drop_shadow(
            Rect::xywh(
                sheet.origin.x - 3.0,
                sheet.origin.y - 3.0,
                sheet.size.x + 6.0,
                sheet.size.y + 6.0,
            ),
            16.0,
            4.0,
            fade(palette.blue, 0.10),
        );
    }
    cx.backend.fill_round_rect(sheet, 14.0, palette.sheet);
    cx.backend.stroke_round_rect(
        sheet,
        14.0,
        if engaged {
            fade(palette.blue, 0.55)
        } else {
            palette.line
        },
        1.0,
    );
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
        (
            screenshot,
            HomeHit::Attachment,
            "截图",
            RefIcon::Lucide(Icon::Image),
        ),
        (
            reference_link,
            HomeHit::ReferenceLink,
            "参考链接",
            RefIcon::Lucide(Icon::ArrowUpRight),
        ),
        // The Figma row carries the brand mark, not a lucide glyph.
        (figma, HomeHit::Figma, "Figma", RefIcon::Figma),
        // The example is plain blue text, as in the prototype.
        (example, HomeHit::TryExample, "试试这个示例", RefIcon::None),
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
        let label_x = match icon {
            RefIcon::Lucide(icon) => {
                draw_icon(cx.backend, icon, icon_origin, 14.0, color, 1.25);
                rect.origin.x + 19.0
            }
            RefIcon::Figma => {
                paint_figma_logo(cx.backend, icon_origin, 14.0, color);
                rect.origin.x + 19.0
            }
            RefIcon::None => rect.origin.x + 4.0,
        };
        text(
            cx,
            label,
            Point2D::new(label_x, rect.origin.y + 19.0),
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
    let send_fill = if surface.state.draft.trim().is_empty() || !surface.usable_agent {
        // An empty draft AND the no-agent state share the same quiet
        // fill — the connect card replaces the launch either way.
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
    let send_hint = shifted(layout.send_hint, rise);
    text(
        cx,
        "⏎ 发送",
        Point2D::new(send_hint.origin.x, send_hint.origin.y + 17.0),
        12.0,
        palette.ash,
        MONO,
    );
    super::model::paint_model_chip(surface, cx, shifted(layout.model_chip, rise), palette);
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
    // Prototype `.foot`: 13 px, 14 px gaps, the empty recent list in ash and
    // the two actions in graphite — no separators.
    let y = layout.footer.origin.y + 16.0;
    let mut x = layout.footer.origin.x;
    for (label, color) in [
        ("最近项目（空）", palette.ash),
        ("新建空白画布", palette.graphite),
        ("打开文件", palette.graphite),
    ] {
        text(cx, label, Point2D::new(x, y), 13.0, color, SANS);
        x += cx.backend.measure_text_family(label, 13.0, SANS) + 14.0;
    }
}

#[cfg(test)]
mod tests {
    use super::{serif_family, EditorUiState};
    use std::sync::Arc;

    #[test]
    fn headline_serif_resolution_follows_the_authored_candidate_order() {
        let ui = EditorUiState {
            system_font_families: Arc::new(vec!["Source Han Serif SC".into(), "Songti SC".into()]),
            ..EditorUiState::default()
        };
        assert_eq!(serif_family(&ui), "Songti SC");
    }

    #[test]
    fn headline_serif_resolution_falls_back_to_sans_when_unavailable() {
        let ui = EditorUiState {
            system_font_families: Arc::new(vec!["PingFang SC".into()]),
            bundled_font_families: Arc::new(vec!["Inter".into()]),
            ..EditorUiState::default()
        };
        assert_eq!(serif_family(&ui), "system-ui");
    }
}
