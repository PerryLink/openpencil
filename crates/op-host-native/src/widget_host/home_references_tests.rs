//! Reference management through real Home host routing.

use super::WidgetHostNative;
use op_editor_core::{chat::ChatAttachment, HomeHit};
use op_editor_ui::widgets::HomeSurface;

const W: f32 = 1180.0;
const H: f32 = 900.0;

fn host() -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    host.last_viewport_w = W;
    host.last_viewport_h = H;
    let state = host.editor_state_mut();
    state.editor_ui.home.visible = true;
    state.editor_ui.home.set_draft("保留我的设计需求");
    for index in 0..4 {
        state.chat.add_attachment(ChatAttachment {
            name: format!("reference-{index}.png"),
            media_type: "image/png".into(),
            data: vec![index],
        });
    }
    host
}

fn press(host: &mut WidgetHostNative, hit: HomeHit) {
    let surface = HomeSurface::for_editor(host.editor_state()).unwrap();
    let rect = surface.focus_rect(&surface.layout(W, H), hit).unwrap();
    assert!(host.apply_press(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
        W,
        H
    ));
}

#[test]
fn home_reference_management_removes_the_selected_image_and_preserves_the_brief() {
    let mut host = host();
    press(&mut host, HomeHit::References);
    assert!(host.editor_state().editor_ui.home.references_open);
    press(&mut host, HomeHit::ReferenceRemove(3));
    let names: Vec<_> = host
        .editor_state()
        .chat
        .pending_attachments
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["reference-0.png", "reference-1.png", "reference-2.png"]
    );
    press(&mut host, HomeHit::ReferenceRemove(1));
    assert_eq!(host.editor_state().chat.pending_attachments[1].data, [2]);
    assert_eq!(host.editor_state().editor_ui.home.draft, "保留我的设计需求");
    assert!(host.apply_escape());
    assert!(!host.editor_state().editor_ui.home.references_open);
    assert_eq!(host.editor_state().chat.pending_attachments.len(), 2);
}

#[test]
fn home_reference_modal_blocks_typing_send_and_background_scroll() {
    let mut host = host();
    press(&mut host, HomeHit::References);
    assert!(host.apply_text('x'));
    assert!(host.apply_backspace());
    assert!(host.apply_delete());
    assert!(host.apply_send());
    assert!(!host.editor_state().editor_ui.home_composer_owns_keyboard());
    assert!(!host.home_send());
    assert!(!host.queue_home_send());
    host.try_scroll_home(500.0, 450.0, -500.0, W, H);
    assert_eq!(host.editor_state().editor_ui.home.scroll_y, 0.0);
    assert_eq!(host.editor_state().editor_ui.home.draft, "保留我的设计需求");
    assert!(host.editor_state().chat.pending_send.is_none());
    assert!(host.editor_state().editor_ui.home.visible);
}

#[test]
fn home_reference_keyboard_removal_keeps_safe_focus_after_list_changes() {
    let mut host = host();
    host.editor_state_mut().editor_ui.home.key_focus = Some(HomeHit::References);
    assert!(host.activate_studio_key_focus());
    assert_eq!(
        host.editor_state().editor_ui.home.key_focus,
        Some(HomeHit::ReferencesClose)
    );
    for remaining in (0..4).rev() {
        assert!(host.apply_studio_focus_step(false));
        assert_eq!(
            host.editor_state().editor_ui.home.key_focus,
            Some(HomeHit::ReferenceRemove(0))
        );
        assert!(host.activate_studio_key_focus());
        assert_eq!(
            host.editor_state().chat.pending_attachments.len(),
            remaining
        );
        assert_eq!(
            host.editor_state().editor_ui.home.key_focus,
            Some(if remaining > 0 {
                HomeHit::ReferencesClose
            } else {
                HomeHit::Attachment
            })
        );
    }
    assert!(!host.editor_state().editor_ui.home.references_open);
    assert!(host.editor_state().editor_ui.home_composer_owns_keyboard());
    assert_eq!(host.editor_state().editor_ui.home.draft, "保留我的设计需求");
}
