use nsh_base::{input, text_view, theme};
use nsh_core::prelude::*;

const INSTANT_MS: f32 = 120.0;

pub fn editor(draft: Signal<String>) -> impl IntoElement {
    input()
        .value(move || draft.get())
        .on_change(move |value: &str| draft.set(value.to_owned()))
        .multiline(true)
        .placeholder("写下第一行 Markdown…")
        .label("Markdown 编辑器")
        .test_id("markdown-editor")
        .flex_1()
        .min_h_0()
        .p_3()
        .text_base()
        .font_family(nsh_core::element::FONT_MONO)
        .text_color(move || theme().foreground)
        .placeholder_color(|| color(theme().muted_foreground))
        .caret_color(|| color(theme().primary))
        .bg(move || theme().background)
        .border_1()
        .border_color(move || theme().input)
        .rounded(move || theme().radius)
        .transition_colors()
        .duration(move || if reduce_motion() { 0.0 } else { INSTANT_MS })
        .ease_out()
        .focus(|s| s.border_color(move || theme().ring))
        .focus_visible(|s| s.outline(px(2.0)).outline_color(move || theme().primary))
}

pub fn preview(draft: Signal<String>) -> impl IntoElement {
    scroll()
        .test_id("markdown-preview")
        .focusable(true)
        .label("Markdown 预览")
        .flex_1()
        .min_h_0()
        .border_1()
        .border_color(move || theme().border)
        .rounded(move || theme().radius)
        .focus_visible(|s| s.outline(px(2.0)).outline_color(move || theme().primary))
        .child(
            text_view()
                .markdown(move || Some(SharedString::from(draft.get())))
                .selectable(true)
                .render_image(|_: &nsh_base::text_view::Image| {
                    text("[图片预览已关闭]").text_color(move || theme().muted_foreground)
                })
                .p_3()
                .text_base()
                .text_color(move || theme().foreground),
        )
}
