//! Long briefs and staged references painted by the real Home widget.
use super::{home_host, shoot, HomeDevice, HomeFamily, Locale};
use op_editor_core::{size_class::EditorSizeClass, BrandKitPayload, ThemeMode};

pub(super) fn run(out_dir: &str) {
    for (w, h, compact, viewport) in [
        (1180.0, 820.0, false, "desktop"),
        (390.0, 844.0, true, "mobile"),
        (320.0, 844.0, true, "narrow-mobile"),
    ] {
        for (theme, theme_name) in [(ThemeMode::Dark, "dark"), (ThemeMode::Light, "light")] {
            let mut host = home_host(Locale::ZhCn, HomeFamily::AppUi, HomeDevice::Mobile);
            let state = host.editor_state_mut();
            let ui = &mut state.editor_ui;
            ui.theme_mode = theme;
            ui.touch = compact;
            if compact {
                ui.size_class = EditorSizeClass::Compact;
            }
            ui.home.composer_focused = true;
            ui.home.set_draft(
                "做一个咖啡点单 App。\n首页展示推荐饮品和分类。\n菜单页支持口味和规格。\n订单页展示取餐进度。\n参考图片里的暖色和留白。\n最后一行：文字和参考图都要看得清。",
            );
            for name in [
                "咖啡首页-参考.png",
                "菜单分类-参考.png",
                "取餐进度-参考.png",
            ] {
                state
                    .chat
                    .add_attachment(op_editor_core::chat::ChatAttachment {
                        name: name.into(),
                        media_type: "image/png".into(),
                        data: vec![1],
                    });
            }
            shoot(
                &mut host,
                w,
                h,
                out_dir,
                &format!("{viewport}-photos-{theme_name}"),
            );

            let brand = &mut host.editor_state_mut().editor_ui.home.brand;
            brand.available = true;
            assert!(brand.press("https://coffee.example", None, 10));
            let (generation, _) = brand.take_request().expect("brand request");
            assert!(brand.finish(
                generation,
                Ok(BrandKitPayload {
                    label: "咖啡品牌参考".into(),
                    swatches: vec!["#713F12".into(), "#FEF3C7".into(), "#EA580C".into()],
                    variables: Default::default(),
                    themes: Default::default(),
                    design_md: None,
                }),
                20
            ));
            shoot(
                &mut host,
                w,
                h,
                out_dir,
                &format!("{viewport}-photos-brand-{theme_name}"),
            );
        }
        click_edit_flow(out_dir, w, h, compact, viewport);
    }
}

/// Exercise the same pointer/text routes as the native app, with a real
/// measured paint before the pointer event. No model runner is installed.
fn click_edit_flow(out_dir: &str, w: f32, h: f32, compact: bool, viewport: &str) {
    use op_editor_ui::widgets::HomeSurface;
    let mut host = home_host(Locale::ZhCn, HomeFamily::AppUi, HomeDevice::Mobile);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.theme_mode = ThemeMode::Dark;
    ui.touch = compact;
    if compact {
        ui.size_class = EditorSizeClass::Compact;
    }
    ui.home.composer_focused = false;
    ui.home
        .set_draft("第一行：咖啡点单。\n第二行：选择口味。\n第三行：取餐进度。");
    shoot(&mut host, w, h, out_dir, &format!("{viewport}-edit-before"));
    let input = HomeSurface::for_editor(host.editor_state())
        .unwrap()
        .layout(w, h)
        .input_box;
    tap(
        &mut host,
        input.origin.x + 14.0,
        input.origin.y + 13.0,
        w,
        h,
    );
    assert_eq!(
        host.editor_state().editor_ui.home.input.caret(),
        0,
        "click the first visible line, not the draft's end"
    );
    assert!(host.apply_input_paste("修改后的"));
    let draft = host.editor_state().editor_ui.home.draft.clone();
    assert!(draft.starts_with("修改后的第一行"));
    let multiple = HomeSurface::for_editor(host.editor_state())
        .unwrap()
        .layout(w, h)
        .app_page_options[1];
    tap(
        &mut host,
        multiple.origin.x + multiple.size.x / 2.0,
        multiple.origin.y + multiple.size.y / 2.0,
        w,
        h,
    );
    assert_eq!(host.editor_state().editor_ui.home.draft, draft);
    assert_eq!(
        host.editor_state().editor_ui.home.task_draft().app_pages,
        op_editor_core::AppPages::Multiple
    );
    for tab_index in [1, 0] {
        let tab = HomeSurface::for_editor(host.editor_state())
            .unwrap()
            .layout(w, h)
            .tabs[tab_index];
        tap(
            &mut host,
            tab.origin.x + tab.size.x / 2.0,
            tab.origin.y + tab.size.y / 2.0,
            w,
            h,
        );
    }
    assert_eq!(
        host.editor_state().editor_ui.home.draft,
        draft,
        "task switching preserves the edit"
    );
    shoot(&mut host, w, h, out_dir, &format!("{viewport}-edit-after"));
    let send = HomeSurface::for_editor(host.editor_state())
        .unwrap()
        .layout(w, h)
        .send;
    tap(
        &mut host,
        send.origin.x + send.size.x / 2.0,
        send.origin.y + send.size.y / 2.0,
        w,
        h,
    );
    assert!(!host.editor_state().editor_ui.home.visible);
    assert_eq!(
        host.editor_state().editor_ui.workspace.options.app_pages,
        op_editor_core::AppPages::Multiple
    );
    assert!(host
        .editor_state()
        .chat
        .pending_send
        .as_deref()
        .is_some_and(|prompt| prompt.contains("修改后的第一行")));
}

/// Touch taps commit on release so a drag can scroll without activating a control.
fn tap(host: &mut super::WidgetHostNative, x: f32, y: f32, w: f32, h: f32) {
    assert!(host.apply_press(x, y, w, h));
    host.apply_release_with_viewport(w, h);
}
