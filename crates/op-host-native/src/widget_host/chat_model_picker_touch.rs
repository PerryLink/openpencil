//! Shared touch capture and scrolling for Home and editor model pickers.

use super::touch_panel_gesture::TouchPanelTarget;
use super::WidgetHostNative;
use op_editor_ui::widgets::ai_chat_model_picker::max_picker_scroll_in_rect;
use op_editor_ui::Point2D;
use std::hash::{Hash, Hasher};

impl WidgetHostNative {
    /// A catalog/filter refresh must not turn a held finger into a tap on a
    /// different model occupying the same coordinates at release time.
    pub(in crate::widget_host) fn model_picker_touch_signature(&self) -> u64 {
        let mut signature = std::collections::hash_map::DefaultHasher::new();
        self.editor_state
            .editor_ui
            .chat_model_picker_input
            .text()
            .hash(&mut signature);
        for entry in &self.editor_state.chat.available_models {
            std::mem::discriminant(&entry.provider).hash(&mut signature);
            entry.value.hash(&mut signature);
            entry.display_name.hash(&mut signature);
            entry.builtin_provider_id.hash(&mut signature);
            entry.builtin_provider_display_name.hash(&mut signature);
        }
        signature.finish()
    }

    /// A model row is a pending tap until release. Moving beyond touch slop
    /// promotes it to list scrolling; cancellation discards the pending tap.
    pub(in crate::widget_host) fn begin_model_picker_touch_gesture(
        &mut self,
        x: f32,
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        if !self.editor_state.editor_ui.touch_chrome() {
            return false;
        }
        let point = Point2D::new(x, y);
        if !self
            .chat_model_picker_rect(viewport_width, viewport_height)
            .is_some_and(|card| card.contains(point))
        {
            return false;
        }
        if self.touch_panel_gesture.is_some() {
            self.cancel_touch_panel_gesture();
            return true;
        }
        self.arm_touch_panel_gesture_at(
            point,
            viewport_width,
            viewport_height,
            TouchPanelTarget::ModelPicker,
        );
        true
    }

    /// Wheel, trackpad and touch share the actual painted card's scroll range.
    pub(in crate::widget_host) fn try_scroll_model_picker(
        &mut self,
        x: f32,
        y: f32,
        delta_y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        let Some(card) = self.chat_model_picker_rect(viewport_width, viewport_height) else {
            return false;
        };
        if !card.contains(Point2D::new(x, y)) {
            return false;
        }
        let ui = &mut self.editor_state.editor_ui;
        let max = max_picker_scroll_in_rect(
            &self.editor_state.chat.available_models,
            ui.chat_model_picker_input.text(),
            card,
        );
        let picker = &mut ui.chat_model_picker;
        let next = (picker.scroll.offset.clamp(0.0, max) - delta_y).clamp(0.0, max);
        if next != picker.scroll.offset {
            picker.scroll.offset = next;
            self.mark_dirty();
        }
        true
    }
}
