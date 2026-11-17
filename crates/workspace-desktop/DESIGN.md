# Workspace desktop design

## Scope and foundations

P0 verifies a Linux desktop Markdown editing surface in Rust NSH 0.5.1. The primary task is editing Chinese Markdown and reading its native preview. The draft belongs to `App` and lives only until the window closes; this probe does not open or save notes. The existing Dioxus entry remains the comparison baseline.

Use the NSH neutral semantic theme, recorded in `theme.json`, in light and dark. Background, foreground, muted, input, border and ring have their standard roles. Use platform UI typography; only the editing buffer uses monospace. Spacing follows the NSH scale. Two equal panes share a single header and have independent vertical scrolling. Linux is the only validated target for this phase; no mobile, web or TUI support is claimed.

`src/ui/mod.rs` owns the styled multiline input and native Markdown preview. Add other controls only when a real screen needs them. The native input owns selection, undo, IME and scrolling; the preview's scroll owns document scrolling. `App` owns the unmodified `String` editing buffer and passes it through reactive bindings. Preview links only report; remote images render as text so this probe makes no preview network requests.

## Motion inventory (written before implementation)

| State change | Tool and token | Reduced motion |
| --- | --- | --- |
| Editor focus enters or leaves | Border-color transition, `instant` = 120 ms, ease-out | Duration zero |
| Preview changes while typing | No motion: keep reading position stable during frequent edits | Same |
| Empty draft | Native input placeholder; empty preview remains in its labelled pane | Same |
| Press/hover actions, selection navigation, popup, layout changes, drag release, loading | None: this probe has no application buttons, navigation, overlays or async loading | Same |

Keyboard: Tab reaches the named editor and native preview content; text entry, selection, scrolling and undo belong to the native platform. The labelled panes retain their headings in either appearance. Do not rewrite input during composition.

## Verification

- `nsh check` and the two headless flows pass: Chinese edit → preview → clear → edit again, and 80 paragraphs at 800×600 in dark appearance with reduced motion and a scroll followed by further editing. These tests also locate the editor by its accessible name.
- The standalone Linux runner opens. A temporary GTK capture harness changed the native multiline buffer and captured the window's own render tree in light and dark; the source was removed after verification. Local evidence: `.nsh/p0-light.png` and `.nsh/p0-dark.png`. The tiling window manager supplied a 1920×2120 viewport, so this does not verify a native minimum-size window.
- `nsh test --target linux` is blocked because its development host enables WebView and this machine lacks WebKitGTK 6. Headless and direct-run evidence are recorded separately.

| Finishing group | Evidence / remaining work |
| --- | --- |
| Radius, typography | Light/dark screenshots: shared theme radius, platform UI text, monospace editor, legible CJK, table and code text |
| Spacing, alignment | Screenshots: matching pane header baselines, equal widths and insets, one border per pane; no overlap at the captured size |
| Color and surfaces | Screenshots: semantic neutral colors in both appearances; no floating or nested card surfaces |
| Hover and pressed | No application press controls in this probe; native text-selection/link behavior still needs manual input review |
| App `ui` / component standards | The only controls are the named native multiline input and the native preview, styled in one module using theme tokens |
| Focus and other states | Input label and clear/re-entry covered headless; focus border visible in native capture; keyboard focus outline, empty native view and screen reader review pending |
| Motion inventory | Preview stays in place on native buffer update; focus transition timing and runtime reduced-motion changes remain unverified on the native platform |
| Accessibility, platforms and content | Long content, smaller viewport and reduced motion covered headless; native IME, undo, keyboard, screen reader, larger text and minimum window still pending |

The native runner logs `No IM module matching GTK_IM_MODULE=fcitx5 found`; `fcitx5-gtk` is absent and the process environment currently names `fcitx5`, which also needs checking against the installed module name. Headless typing tables also logs `a kept row is gone` in NSH 0.5.1 even though final content assertions pass. Keep both as open P0 findings; do not call the screen or the migration fully accepted until they are resolved or independently verified. Headless injection and GTK buffer updates do not prove a real Chinese input method.
