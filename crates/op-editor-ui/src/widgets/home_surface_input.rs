//! Home composer text geometry shared by paint, pointer input and IME.

use super::{HomeEnterBlock, HomeSurface};
use crate::{Point2D, Rect, RenderBackend};
use jian_core::text_input::{prev_char_boundary, TextInputState};
use jian_widgets::components::text_area::{TextArea, TextLine};

const FONT: f32 = 14.0;
const LINE_H: f32 = FONT * 1.35;
const PAD_Y: f32 = 4.0;

/// A snapshot measured with the same backend that paints the home input.
/// The source, caret and rectangles guard against reusing a stale wrap or
/// visible window after typing, scrolling, resizing or attaching references.
#[derive(Debug, Clone)]
pub struct HomeInputGeometry {
    source: String,
    caret: usize,
    composition: Option<(String, usize)>,
    input_box: Rect,
    text_rect: Rect,
    lines: Vec<MeasuredLine>,
    caret_rect: Rect,
}

#[derive(Debug, Clone)]
struct MeasuredLine {
    end: usize,
    midpoints: Vec<(usize, f32)>,
}

impl HomeSurface<'_> {
    pub(crate) fn input_text_rect(&self, input_box: Rect) -> Rect {
        super::references::input_text_rect(self, input_box)
    }

    pub(crate) fn input_clip_rect(&self, input_box: Rect) -> Rect {
        super::references::text_clip_rect(self, input_box)
    }

    pub(crate) fn input_text_area<'a>(
        &'a self,
        placeholder: &'a str,
        input_box: Rect,
    ) -> TextArea<'a> {
        let text_rect = self.input_text_rect(input_box);
        TextArea {
            state: &self.state.input,
            placeholder,
            focused: if self.ui.compact_layout() {
                self.state.composer_focused
            } else {
                self.state.visible
            },
            font_size: FONT,
            now_ms: self.now_ms,
            pad_x: if self.ui.compact_layout() { 10.0 } else { 13.0 },
            max_visible_lines: ((text_rect.size.y - PAD_Y) / LINE_H).floor().max(1.0) as usize,
        }
    }

    fn painted_input_box(&self, width: f32, height: f32) -> Rect {
        let mut rect = self.layout(width, height).input_box;
        let shown_at = self.ui.motion_stamp(self.state.shown_at_ms);
        rect.origin.y += super::paint::enter_phase(HomeEnterBlock::Panels, shown_at, self.now_ms).0;
        rect
    }

    pub fn measure_input_geometry(
        &self,
        backend: &mut dyn RenderBackend,
        width: f32,
        height: f32,
    ) -> HomeInputGeometry {
        let input_box = self.painted_input_box(width, height);
        let text_rect = self.input_text_rect(input_box);
        let area = self.input_text_area("", input_box);
        let text = self.state.input.text();
        let caret = prev_char_boundary(text, self.state.input.caret());
        let wrapped = TextArea::layout_lines(
            backend,
            text,
            FONT,
            (text_rect.size.x - 2.0 * area.pad_x).max(0.0),
        );
        let caret_line = caret_line_index(&wrapped, caret);
        let visible_count = area.max_visible_lines.min(wrapped.len());
        let visible_start = caret_line
            .saturating_add(1)
            .saturating_sub(visible_count)
            .min(wrapped.len().saturating_sub(visible_count));
        let base_x = text_rect.origin.x + area.pad_x;
        let lines = wrapped
            .iter()
            .skip(visible_start)
            .take(visible_count)
            .map(|line| {
                let mut x = base_x;
                let midpoints = line
                    .text
                    .char_indices()
                    .map(|(byte, ch)| {
                        let mut buf = [0; 4];
                        let width =
                            backend.measure_text_family(ch.encode_utf8(&mut buf), FONT, "Inter");
                        let midpoint = (line.start + byte, x + width / 2.0);
                        x += width;
                        midpoint
                    })
                    .collect();
                MeasuredLine {
                    end: line.end,
                    midpoints,
                }
            })
            .collect();
        let line = &wrapped[caret_line];
        let relative = prev_char_boundary(
            &line.text,
            caret.saturating_sub(line.start).min(line.text.len()),
        );
        let mut caret_x =
            base_x + backend.measure_text_family(&line.text[..relative], FONT, "Inter");
        let composition = composition(&self.state.input);
        if let Some((text, cursor)) = &composition {
            caret_x += backend.measure_text_family(&text[..*cursor], FONT, "Inter");
        }
        let caret_rect = Rect::xywh(
            caret_x,
            text_rect.origin.y + PAD_Y + caret_line.saturating_sub(visible_start) as f32 * LINE_H,
            1.5,
            FONT + 3.0,
        );
        HomeInputGeometry {
            source: text.to_owned(),
            caret,
            composition,
            input_box,
            text_rect,
            lines,
            caret_rect,
        }
    }

    /// Fallback before the first real paint. Once painted, hosts prefer the
    /// cached geometry so Chinese and proportional glyphs use real advances.
    pub fn input_offset_at(&self, width: f32, height: f32, point: Point2D) -> Option<usize> {
        let mut backend = super::super::text_input::MeasureOnlyBackend;
        self.measure_input_geometry(&mut backend, width, height)
            .byte_offset_at(self, width, height, point)
    }

    pub fn focused_input_caret_rect(&self, width: f32, height: f32) -> Rect {
        let mut backend = super::super::text_input::MeasureOnlyBackend;
        self.measure_input_geometry(&mut backend, width, height)
            .ime_anchor_rect()
    }
}

impl HomeInputGeometry {
    fn matches(&self, surface: &HomeSurface<'_>, width: f32, height: f32) -> bool {
        let input_box = surface.painted_input_box(width, height);
        self.source == surface.state.input.text()
            && self.caret == surface.state.input.caret()
            && self.composition == composition(&surface.state.input)
            && self.input_box == input_box
            && self.text_rect == surface.input_text_rect(input_box)
    }

    pub fn byte_offset_at(
        &self,
        surface: &HomeSurface<'_>,
        width: f32,
        height: f32,
        point: Point2D,
    ) -> Option<usize> {
        if !self.matches(surface, width, height) || !self.input_box.contains(point) {
            return None;
        }
        let row = ((point.y - self.text_rect.origin.y - PAD_Y) / LINE_H)
            .floor()
            .max(0.0) as usize;
        let line = self
            .lines
            .get(row.min(self.lines.len().saturating_sub(1)))?;
        Some(
            line.midpoints
                .iter()
                .find(|(_, midpoint)| point.x < *midpoint)
                .map_or(line.end, |(byte, _)| *byte),
        )
    }

    pub fn caret_rect(&self, surface: &HomeSurface<'_>, width: f32, height: f32) -> Option<Rect> {
        self.matches(surface, width, height)
            .then(|| self.ime_anchor_rect())
    }

    /// Long preedit runs are clipped by the text area. Keep the host's IME
    /// candidate window anchored inside that visible area while retaining
    /// the measured caret above for paint and hit-test consistency.
    fn ime_anchor_rect(&self) -> Rect {
        let mut anchor = self.caret_rect;
        anchor.size.x = anchor.size.x.min(self.text_rect.size.x.max(0.0));
        anchor.size.y = anchor.size.y.min(self.text_rect.size.y.max(0.0));
        anchor.origin.x = anchor.origin.x.clamp(
            self.text_rect.origin.x,
            self.text_rect.origin.x + self.text_rect.size.x.max(0.0) - anchor.size.x,
        );
        anchor.origin.y = anchor.origin.y.clamp(
            self.text_rect.origin.y,
            self.text_rect.origin.y + self.text_rect.size.y.max(0.0) - anchor.size.y,
        );
        anchor
    }
}

fn composition(input: &TextInputState) -> Option<(String, usize)> {
    input
        .composition()
        .filter(|value| !value.text.is_empty())
        .map(|value| {
            (
                value.text.clone(),
                prev_char_boundary(&value.text, value.cursor),
            )
        })
}

fn caret_line_index(lines: &[TextLine], caret: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .position(|(i, line)| {
            if line.start == line.end {
                return caret == line.start;
            }
            if lines.get(i + 1).is_some_and(|next| next.start == line.end) {
                caret >= line.start && caret < line.end
            } else {
                caret >= line.start && caret <= line.end
            }
        })
        .unwrap_or(lines.len().saturating_sub(1))
}

#[cfg(test)]
#[path = "home_surface_input_tests.rs"]
mod tests;
