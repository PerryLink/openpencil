//! Native pointer cancellation, shared by touch shells and modal transitions.

use super::WidgetHostNative;

impl WidgetHostNative {
    /// Cancel every in-flight native pointer gesture without routing it
    /// through the release ladder. This is the platform `touchesCancelled` /
    /// `ACTION_CANCEL` seam: release-delayed taps are discarded, preview gets
    /// a real Cancel phase, and drag-only history/drop commits never run.
    pub fn cancel_native_touch_gestures(&mut self) -> bool {
        self.cancel_native_touch_gestures_at(self.preview_pointer_time_ms())
    }

    /// [`Self::cancel_native_touch_gestures`] with an explicit factual
    /// timestamp for the preview Cancel dispatch (the scoped event
    /// time from a timestamped entry variant, or the caller's `t_ms`).
    /// The host's global clock is not advanced here — the caller
    /// advances it monotonically before calling, so a Cancel whose
    /// `t_ms` sits behind the last frame pump keeps the clock where it
    /// is while the runtime still receives the factual Cancel time. The
    /// scoped context is restored afterwards; on a panic the engine is
    /// poisoned at the ABI boundary, so no later event can observe a
    /// stale ticket.
    pub fn cancel_native_touch_gestures_at(&mut self, t_ms: u64) -> bool {
        let previous = self.preview_event_time_ms.replace(t_ms);
        let cancelled = self.cancel_native_touch_gestures_inner();
        self.preview_event_time_ms = previous;
        cancelled
    }

    fn cancel_native_touch_gestures_inner(&mut self) -> bool {
        let mut cancelled = self.agent_settings_touch_gesture.take().is_some();
        cancelled |= self.cancel_touch_panel_gesture();

        cancelled |= self.editor_state.editor_ui.pressed_button.take().is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .git_panel
            .tracked_picker
            .pressed
            .take()
            .is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .icon_picker
            .pressed
            .take()
            .is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .chat_model_picker
            .pressed
            .take()
            .is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .preview
            .switcher_pressed
            .take()
            .is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .preview
            .screen_switcher_pressed
            .take()
            .is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .preview
            .toolbar_pressed
            .take()
            .is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .slides_panel
            .pressed
            .take()
            .is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .slides_panel
            .drag
            .take()
            .is_some();
        cancelled |= self
            .editor_state
            .editor_ui
            .agent_settings_drag
            .take()
            .is_some();
        if self
            .editor_state
            .ui
            .color_picker
            .as_ref()
            .is_some_and(|picker| picker.drag.is_some())
        {
            self.editor_state.color_picker_set_drag(None);
            cancelled = true;
        }
        if self.editor_state.ui.pen_dragging_handle {
            self.editor_state.pen_release();
            cancelled = true;
        }

        if !self.preview_pressed_pids.is_empty() {
            let t_ms = self.preview_pointer_time_ms();
            let pids = std::mem::take(&mut self.preview_pressed_pids);
            for pid in pids {
                if let Some(preview) = self.preview.as_mut() {
                    preview.cancel_pointer(pid, t_ms);
                }
            }
            self.preview_last_doc_by_pid.clear();
            self.preview_surface_capture = None;
            cancelled = true;
        }
        cancelled |= self.preview_edge_swipe_start_x.take().is_some();
        cancelled |= self.slideshow_press_screen.take().is_some();

        cancelled |= self.drag.take().is_some();
        cancelled |= self.chat_drag.take().is_some();
        cancelled |= self.chat_resize.take().is_some();
        cancelled |= self.design_md_drag.take().is_some();
        cancelled |= self.component_browser_drag.take().is_some();
        cancelled |= self.icon_picker_drag.take().is_some();
        cancelled |= self.image_adjustment_drag.take().is_some();
        cancelled |= self.image_crop_drag.take().is_some();
        cancelled |= self.effect_radius_drag.take().is_some();
        cancelled |= self.code_selection_drag.take().is_some();
        cancelled |= self.chat_input_selection_drag.take().is_some();
        cancelled |= self.image_input_selection_drag.take().is_some();
        cancelled |= self.chat_text_selection_drag.take().is_some();
        cancelled |= self.text_edit_selection_drag.take().is_some();
        cancelled |= self.panel_resize.take().is_some();
        cancelled |= self.variables_resize.take().is_some();
        cancelled |= self.node_drag.take().is_some();
        cancelled |= self.handle_drag.take().is_some();
        cancelled |= self.rotate_drag.take().is_some();
        cancelled |= self.create_drag.take().is_some();
        cancelled |= self.marquee_drag.take().is_some();
        cancelled |= self.layer_drag.take().is_some();
        cancelled |= self.path_anchor_drag.take().is_some();
        cancelled |= self.arc_handle_drag.take().is_some();

        if !self.option_drag_source_ids.is_empty() {
            self.option_drag_source_ids.clear();
            cancelled = true;
        }
        if !self.editor_state.editor_ui.active_guides.is_empty() {
            self.editor_state.editor_ui.active_guides.clear();
            cancelled = true;
        }
        cancelled |= self
            .editor_state
            .editor_ui
            .canvas_drop_indicator
            .take()
            .is_some();
        if cancelled {
            self.mark_dirty();
        }
        cancelled
    }
}
