use super::WidgetHostNative;
use op_editor_core::size_class::{EditorSizeClass, MobileSheetKind};
use op_editor_core::{AgentProvider, ModelEntry};
use op_editor_ui::widgets::ai_chat_model_picker::{
    max_picker_scroll_in_rect, model_at, MODEL_SEARCH_H,
};
use op_editor_ui::widgets::AIChatPlaceholder;
use op_editor_ui::{Point2D, Rect};

fn picker_host(home: bool, width: f32) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    let state = host.editor_state_mut();
    state.editor_ui.touch = true;
    state.editor_ui.size_class = if width < 600.0 {
        EditorSizeClass::Compact
    } else {
        EditorSizeClass::Medium
    };
    state.editor_ui.home.visible = home;
    state.editor_ui.mobile_sheet = (!home).then_some(MobileSheetKind::Ai);
    state.chat.collapsed = false;
    state.chat.focused = true;
    state.chat.available_models = (0..24)
        .map(|index| {
            ModelEntry::new(
                AgentProvider::CodexCli,
                format!("model-{index}"),
                format!("Model {index}"),
            )
        })
        .collect();
    state.chat.selected_model = 0;
    state.editor_ui.chat_model_picker.open = true;
    host
}

fn card(host: &WidgetHostNative, width: f32, height: f32) -> Rect {
    host.chat_model_picker_rect(width, height)
        .expect("visible model picker")
}

fn row_point(host: &WidgetHostNative, card: Rect, index: usize) -> Point2D {
    let state = host.editor_state();
    let x = card.origin.x + card.size.x / 2.0;
    let mut y = card.origin.y + MODEL_SEARCH_H;
    while y < card.origin.y + card.size.y {
        let point = Point2D::new(x, y);
        if model_at(
            card,
            point,
            &state.chat.available_models,
            state.editor_ui.chat_model_picker.scroll.offset,
            state.editor_ui.chat_model_picker_input.text(),
        ) == Some(index)
        {
            return point;
        }
        y += 1.0;
    }
    panic!("model {index} must have a visible hit target in {card:?}");
}

fn assert_picker_open(host: &WidgetHostNative, home: bool) {
    assert!(host.editor_state().editor_ui.chat_model_picker.open);
    assert_eq!(host.editor_state().chat.selected_model, 0);
    if !home {
        assert_eq!(
            host.editor_state().editor_ui.mobile_sheet,
            Some(MobileSheetKind::Ai)
        );
    }
}

#[test]
fn model_picker_touch_tap_selects_once_on_release_on_home_and_editor() {
    for home in [true, false] {
        for (width, height) in [(390.0, 844.0), (834.0, 1112.0)] {
            let mut host = picker_host(home, width);
            let point = row_point(&host, card(&host, width, height), 1);
            assert!(host.apply_press(point.x, point.y, width, height));
            assert_picker_open(&host, home);
            host.apply_cursor_move(point.x + 2.0, point.y + 2.0);
            assert_picker_open(&host, home);
            assert!(host.apply_release_with_viewport(width, height));
            assert_eq!(host.editor_state().chat.selected_model, 1);
            assert!(!host.editor_state().editor_ui.chat_model_picker.open);
            host.apply_release_with_viewport(width, height);
            assert_eq!(host.editor_state().chat.selected_model, 1);
        }
    }
}

#[test]
fn model_picker_touch_drag_scrolls_without_selecting_even_when_returning_to_start() {
    for home in [true, false] {
        let (width, height) = (390.0, 844.0);
        let mut host = picker_host(home, width);
        let point = row_point(&host, card(&host, width, height), 1);
        assert!(host.apply_press(point.x, point.y, width, height));
        assert_picker_open(&host, home);
        assert!(host.apply_cursor_move(point.x, point.y - 80.0));
        assert!(
            host.editor_state()
                .editor_ui
                .chat_model_picker
                .scroll
                .offset
                >= 80.0
        );
        assert!(host.apply_cursor_move(point.x, point.y));
        assert!(host.apply_release_with_viewport(width, height));
        assert_picker_open(&host, home);
        assert!(host.touch_panel_gesture.is_none());
    }
}

#[test]
fn model_picker_cancel_second_finger_and_hidden_overlay_discard_pending_taps() {
    for home in [true, false] {
        for cancellation in 0..3 {
            let (width, height) = (390.0, 844.0);
            let mut host = picker_host(home, width);
            let point = row_point(&host, card(&host, width, height), 1);
            assert!(host.apply_press(point.x, point.y, width, height));
            match cancellation {
                0 => assert!(host.cancel_native_touch_gestures()),
                1 => assert!(host.apply_press(point.x, point.y, width, height)),
                _ => host.editor_state_mut().editor_ui.login_modal_open = true,
            }
            host.apply_release_with_viewport(width, height);
            assert_eq!(host.editor_state().chat.selected_model, 0);
            assert!(host.touch_panel_gesture.is_none());
        }
    }
}

#[test]
fn model_picker_small_screen_keyboard_keeps_search_rows_and_footer_inside_visible_bounds() {
    let (width, height, keyboard) = (320.0, 568.0, 300.0);
    for home in [true, false] {
        let mut host = picker_host(home, width);
        host.set_keyboard_occlusion(keyboard);
        let picker = card(&host, width, height);
        assert!(picker.origin.x >= 0.0 && picker.origin.y >= 0.0);
        assert!(picker.origin.x + picker.size.x <= width);
        assert!(picker.origin.y + picker.size.y <= height - keyboard);
        assert!(picker.size.y > MODEL_SEARCH_H + 100.0);
        if !home {
            let panel = host.ai_chat_rect(width, height).unwrap();
            assert_eq!(
                AIChatPlaceholder::from_editor(host.editor_state()).model_picker_bounds(panel),
                Some(picker),
                "paint, hit-test and scroll use the same constrained card",
            );
        }
        let point = row_point(&host, picker, 1);
        assert!(host.apply_press(point.x, point.y, width, height));
        assert_picker_open(&host, home);
        assert!(host.apply_release_with_viewport(width, height));
        assert_eq!(host.editor_state().chat.selected_model, 1);
        if !home {
            assert_eq!(
                host.editor_state().editor_ui.mobile_sheet,
                Some(MobileSheetKind::Ai)
            );
        }
    }
}

#[test]
fn model_picker_keyboard_constrained_list_can_scroll_to_and_select_last_model() {
    let (width, height) = (320.0, 568.0);
    for home in [true, false] {
        let mut host = picker_host(home, width);
        host.set_keyboard_occlusion(300.0);
        let picker = card(&host, width, height);
        let point = row_point(&host, picker, 1);
        assert!(host.apply_press(point.x, point.y, width, height));
        assert!(host.apply_cursor_move(point.x, point.y - 2000.0));
        assert!(host.apply_release_with_viewport(width, height));
        assert_picker_open(&host, home);
        assert_eq!(
            host.editor_state()
                .editor_ui
                .chat_model_picker
                .scroll
                .offset,
            max_picker_scroll_in_rect(&host.editor_state().chat.available_models, "", picker),
        );
        let last = host.editor_state().chat.available_models.len() - 1;
        let point = row_point(&host, picker, last);
        assert!(host.apply_press(point.x, point.y, width, height));
        assert_picker_open(&host, home);
        assert!(host.apply_release_with_viewport(width, height));
        assert_eq!(host.editor_state().chat.selected_model, last);
    }
}

#[test]
fn model_picker_keyboard_geometry_change_during_touch_cancels_pending_selection() {
    for home in [true, false] {
        let (width, height) = (320.0, 568.0);
        let mut host = picker_host(home, width);
        let point = row_point(&host, card(&host, width, height), 1);
        assert!(host.apply_press(point.x, point.y, width, height));
        host.set_keyboard_occlusion(300.0);
        host.apply_release_with_viewport(width, height);
        assert_picker_open(&host, home);
    }
}

#[test]
fn model_picker_catalog_refresh_during_touch_does_not_select_replacement_row() {
    for home in [true, false] {
        let (width, height) = (390.0, 844.0);
        let mut host = picker_host(home, width);
        let point = row_point(&host, card(&host, width, height), 1);
        assert!(host.apply_press(point.x, point.y, width, height));
        host.editor_state_mut().chat.available_models[1].value = "replacement-route".into();
        host.apply_release_with_viewport(width, height);
        assert_picker_open(&host, home);
    }
}

#[test]
fn model_picker_keyboard_scroll_range_matches_touch_wheel_and_trackpad() {
    for home in [true, false] {
        let (width, height) = (320.0, 568.0);
        for trackpad in [false, true] {
            let mut host = picker_host(home, width);
            host.set_keyboard_occlusion(300.0);
            let picker = card(&host, width, height);
            let point = row_point(&host, picker, 1);
            let consumed = if trackpad {
                host.apply_pan_gesture(point.x, point.y, 0.0, -2000.0, width, height)
            } else {
                host.apply_wheel(point.x, point.y, -2000.0, width, height)
            };
            assert!(consumed);
            assert_picker_open(&host, home);
            assert_eq!(
                host.editor_state()
                    .editor_ui
                    .chat_model_picker
                    .scroll
                    .offset,
                max_picker_scroll_in_rect(&host.editor_state().chat.available_models, "", picker),
            );
        }
    }
}
