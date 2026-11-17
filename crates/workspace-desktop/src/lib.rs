//! Workspace's Rust NSH desktop entry.

#[cfg(target_os = "linux")]
pub mod browser;
mod ui;

use nsh_base::theme;
use nsh_core::prelude::*;

#[native_module("workspace/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

const SAMPLE: &str = "# 中文笔记\n\n在左侧编辑，右侧实时预览。\n\n**加粗**、~~删除线~~与 [链接](https://example.com)。\n\n| 功能 | 状态 |\n| --- | --- |\n| 中文编辑 | 验证中 |\n| 原生预览 | 验证中 |\n\n```rust\nfn main() {\n    println!(\"你好，Workspace\");\n}\n```\n";

/// The P0 editor: an in-memory draft and a native Markdown preview.
#[component]
pub fn App() -> impl IntoElement {
    // shortcut: P0 keeps the draft in memory; connect the notes service in P5.
    let draft = signal(SAMPLE.to_owned());
    view()
        .flex_1()
        .min_h_0()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .p_safe_area()
        .child(
            view()
                .px_5()
                .py_4()
                .gap_1()
                .child(
                    text("Workspace")
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(
                    text("编辑验证 · 草稿仅保存在本窗口，关闭后不保留")
                        .text_sm()
                        .text_color(move || theme().muted_foreground),
                ),
        )
        .child(
            view()
                .flex_row()
                .flex_1()
                .min_h_0()
                .gap_4()
                .px_5()
                .pb_5()
                .child(pane("Markdown", ui::editor(draft)))
                .child(pane("预览", ui::preview(draft))),
        )
}

fn pane(title: &'static str, content: impl IntoElement) -> impl IntoElement {
    view()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .gap_2()
        .child(text(title).text_sm().font_weight(FontWeight::SEMIBOLD))
        .child(content)
}
