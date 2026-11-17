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
- `GTK_IM_MODULE=fcitx nsh test --target linux` now passes both editing flows after the user installed WebKitGTK 6 and `fcitx5-gtk`.
- A temporary native input harness observed real fcitx `shuangpin` preedit `n` → `ni`, then a space committed `你`; Ctrl+Z undid it and Ctrl+Y redid it. Shift+Left selected the character. AT-SPI reported the editor's labelled grouping, editable Chinese text and the selection range 0–1. This verifies the exposed interfaces, not the full experience in a screen reader.
- The native window was resized to an actual 800×600 with font scale 1.5. Its own render-tree capture, `.nsh/p0-ime-small.png`, shows both panes, headings and the Chinese input/preview without overlap. The harness was removed, and input method/focus were restored. This supplements the original light/dark captures; it does not establish a product minimum-window policy.

| Finishing group | Evidence / remaining work |
| --- | --- |
| Radius, typography | Light/dark screenshots: shared theme radius, platform UI text, monospace editor, legible CJK, table and code text |
| Spacing, alignment | Screenshots: matching pane header baselines, equal widths and insets, one border per pane; no overlap at the captured size |
| Color and surfaces | Screenshots: semantic neutral colors in both appearances; no floating or nested card surfaces |
| Hover and pressed | No application press controls in this probe; native text-selection/link behavior still needs manual input review |
| App `ui` / component standards | The only controls are the named native multiline input and the native preview, styled in one module using theme tokens |
| Focus and other states | Accessible editor name, empty/re-entry flows and focused native capture verified; actual input, undo/redo and selection checked; broader keyboard traversal remains open |
| Motion inventory | Preview stays in place on native buffer update; focus transition timing and runtime reduced-motion changes remain unverified on the native platform |
| Accessibility, platforms and content | Real fcitx composition and AT-SPI text/selection verified; actual 800×600 and 150% text captured; full screen reader use and broader keyboard navigation remain open |

The native runner previously logged `No IM module matching GTK_IM_MODULE=fcitx5 found`; the user has now installed `fcitx5-gtk` and WebKitGTK 6. Native editor tests and real composition pass with the process-local `GTK_IM_MODULE=fcitx`; no global environment is changed. Headless bulk typing still logs `a kept row is gone` in NSH 0.5.1 even though final content assertions pass. An independent input + TextView reproducer, without application styling, reports the same five errors; awaiting each character separately reports zero. In a temporary NSH copy, retaining the row owner on `Keep` filters disposed queued rows and removes all five errors in that reproducer. The candidate is preserved locally at `.nsh/p0-keep-row-candidate.patch`; it has not passed the NSH test suite or a performance review of the existing kept-row fast path, and is not applied to the app or installed dependencies. Keep this as an open P0 finding; final assertions and paced typing do not exclude defects during queued updates.

## Browser feasibility component (inventory before implementation)

The browser is a Linux native component registered as `workspace.browser`. NSH owns its frame and lifecycle, while WebKit owns page layout, keyboard focus and page interaction. The embedding app configures a dedicated ephemeral network session with an explicit HTTP proxy before constructing any WebView. The URL is a reactive input, never replaced with a fixed homepage. Only HTTP/HTTPS navigation is allowed. Loading/error events cross to the engine as values; no page bridge or account credential is exposed.

This round verifies the component through a native integration fixture, without adding a Browser feature screen or navigation chrome. State changes in the fixture: page loads/errors update test results without animation; page content follows its own browser behavior. There are no application controls, overlays, navigation selection or layout animations. Reduced motion does not change the network contract. WebRTC and DNS prefetch are disabled so the component does not intentionally add those bypass paths; this is not proof of whole-device routing.

Acceptance: real WebKit HTTP/HTTPS, redirect/subresource/WebSocket traffic must traverse the local proxy and QUIC fixture; closing QUIC must fail closed in strict mode, and closing the proxy must not cause WebKit to connect directly. Test-only TLS trust is scoped to the fixture's certificate and hostname. Production TLS verification stays enabled.

`tests/browser_proxy.rs` passes these checks with real WebKit: HTTP/HTTPS, redirects, CSS, SVG image, script, fetch and WS/WSS all reach a restricted local QUIC egress fixture. A loopback URL also traverses QUIC. A server-initiated QUIC close leaves the stale connection in tunnel state; subsequent HTTP and HTTPS produce no origin TCP connections. Stopping the local proxy also fails without direct origin connections. Invalid file URLs are rejected. These checks establish this session's tested request paths, not whole-device routing or public-server readiness.

The browser test exposed a shared proxy defect: absolute-form HTTP/WS targets were forwarded unchanged to origins. The original backend now translates them to origin-form, preserves request bodies and Upgrade, removes proxy-only headers (including Proxy-Authorization), and explicitly closes ordinary HTTP connections after one request. Direct-fallback POST and strict GET/CONNECT regressions are included alongside browser routing coverage.
