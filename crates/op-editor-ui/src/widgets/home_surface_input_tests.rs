use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::EditorState;

fn state(compact: bool, text: &str) -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.home.visible = true;
    state.editor_ui.home.composer_focused = true;
    state.editor_ui.home.set_draft(text);
    if compact {
        state.editor_ui.touch = true;
        state.editor_ui.size_class = op_editor_core::size_class::EditorSizeClass::Compact;
    }
    state
}

#[test]
fn home_input_clicks_follow_painted_multiline_utf8_text() {
    let text = "咖啡🙂点单\n第二行描述\n末行";
    for compact in [false, true] {
        let state = state(compact, text);
        let home = HomeSurface::for_editor(&state).unwrap();
        let width = if compact { 320.0 } else { 1180.0 };
        let mut backend = CaptureBackend::default();
        let geometry = home.measure_input_geometry(&mut backend, width, 844.0);
        let area = home.input_text_area("", geometry.input_box);
        area.paint(
            &mut backend,
            geometry.text_rect,
            &jian_widgets::Tokens::dark(),
        );
        assert_eq!(backend.texts.len(), 3);
        for (label, origin) in &backend.texts {
            let expected = text.find(label).unwrap();
            let point = Point2D::new(origin.x, origin.y + 4.0);
            assert_eq!(
                geometry.byte_offset_at(&home, width, 844.0, point),
                Some(expected)
            );
            assert_eq!(home.input_offset_at(width, 844.0, point), Some(expected));
            let end = Point2D::new(
                geometry.input_box.origin.x + geometry.input_box.size.x - 2.0,
                point.y,
            );
            assert_eq!(
                geometry.byte_offset_at(&home, width, 844.0, end),
                Some(expected + label.len())
            );
        }
        assert!(backend
            .rect_fills
            .iter()
            .any(|(rect, _)| *rect == geometry.caret_rect));
    }
}

#[test]
fn long_home_prompt_uses_the_painted_visible_window_and_keeps_caret_inside_text() {
    let text = "第一行\n第二行\n第三行\n第四行\n第五行\n最后一行";
    for compact in [false, true] {
        let mut state = state(compact, text);
        state
            .chat
            .add_attachment(op_editor_core::chat::ChatAttachment {
                name: "参考.png".into(),
                media_type: "image/png".into(),
                data: vec![1],
            });
        let home = HomeSurface::for_editor(&state).unwrap();
        let width = if compact { 320.0 } else { 1180.0 };
        let mut backend = CaptureBackend::default();
        let geometry = home.measure_input_geometry(&mut backend, width, 844.0);
        let area = home.input_text_area("", geometry.input_box);
        area.paint(
            &mut backend,
            geometry.text_rect,
            &jian_widgets::Tokens::dark(),
        );
        assert!(backend.texts.len() < 6);
        assert_eq!(backend.texts.last().unwrap().0, "最后一行");
        assert!(
            geometry.caret_rect.origin.y + geometry.caret_rect.size.y
                <= geometry.text_rect.origin.y + geometry.text_rect.size.y
        );
        let (first, origin) = &backend.texts[0];
        assert_eq!(
            geometry.byte_offset_at(&home, width, 844.0, Point2D::new(origin.x, origin.y + 4.0)),
            text.find(first)
        );
    }
}

#[test]
fn soft_wrap_boundaries_and_composition_match_the_painted_caret() {
    let text = "中文描述".repeat(30);
    let mut state = state(true, &text);
    let mut backend = CaptureBackend::default();
    let home = HomeSurface::for_editor(&state).unwrap();
    let input = home.layout(320.0, 844.0).input_box;
    let rect = home.input_text_rect(input);
    let area = home.input_text_area("", input);
    let lines = TextArea::layout_lines(&mut backend, &text, FONT, rect.size.x - area.pad_x * 2.0);
    let pad_x = area.pad_x;
    let boundary = lines[1].start;
    state.editor_ui.home.set_caret(boundary, 0);
    state
        .editor_ui
        .home
        .input
        .set_composing_text("你好🙂", "你好".len(), "你好".len(), 0);
    let home = HomeSurface::for_editor(&state).unwrap();
    let geometry = home.measure_input_geometry(&mut backend, 320.0, 844.0);
    home.input_text_area("", input)
        .paint(&mut backend, rect, &jian_widgets::Tokens::dark());
    assert_eq!(geometry.caret_rect.origin.y, rect.origin.y + PAD_Y + LINE_H);
    assert!(backend
        .rect_fills
        .iter()
        .any(|(rect, _)| *rect == geometry.caret_rect));
    assert_eq!(
        geometry.byte_offset_at(
            &home,
            320.0,
            844.0,
            Point2D::new(rect.origin.x + pad_x, rect.origin.y + PAD_Y + LINE_H + 1.0)
        ),
        Some(boundary)
    );
}

#[test]
fn home_geometry_rejects_stale_draft_window_and_viewport() {
    let mut state = state(true, "一行\n二行\n三行");
    let mut backend = CaptureBackend::default();
    let geometry = HomeSurface::for_editor(&state)
        .unwrap()
        .measure_input_geometry(&mut backend, 320.0, 844.0);
    let home = HomeSurface::for_editor(&state).unwrap();
    assert!(geometry.caret_rect(&home, 390.0, 844.0).is_none());
    state.editor_ui.home.set_caret(0, 0);
    assert!(geometry
        .caret_rect(&HomeSurface::for_editor(&state).unwrap(), 320.0, 844.0)
        .is_none());
    state.editor_ui.home.set_draft("新描述");
    assert!(geometry
        .caret_rect(&HomeSurface::for_editor(&state).unwrap(), 320.0, 844.0)
        .is_none());
}

#[test]
fn long_line_end_composition_keeps_cached_and_fallback_ime_anchors_visible() {
    for (compact, width) in [(true, 320.0), (true, 390.0), (false, 1180.0)] {
        let mut state = state(compact, "第一行\n第二行末尾");
        let preedit = "chaochangdepininyinzuhe".repeat(20);
        state
            .editor_ui
            .home
            .input
            .set_composing_text(&preedit, preedit.len(), preedit.len(), 0);
        let home = HomeSurface::for_editor(&state).unwrap();
        let mut backend = CaptureBackend::default();
        let geometry = home.measure_input_geometry(&mut backend, width, 844.0);
        let text = geometry.text_rect;
        assert!(
            geometry.caret_rect.origin.x > text.origin.x + text.size.x,
            "the measured preedit extends beyond the painted clip"
        );
        let cached = geometry.caret_rect(&home, width, 844.0).unwrap();
        let fallback = home.focused_input_caret_rect(width, 844.0);
        assert_eq!(cached, fallback);
        for anchor in [cached, fallback] {
            assert!(anchor.origin.x >= text.origin.x);
            assert!(anchor.origin.x + anchor.size.x <= text.origin.x + text.size.x);
            assert!(anchor.origin.y >= text.origin.y);
            assert!(anchor.origin.y + anchor.size.y <= text.origin.y + text.size.y);
        }
    }
}
