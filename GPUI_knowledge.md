# GPUI Quick Notes — Media Render & UI Navigation

## Layout & Scroll Basics
- For flex children that host scroll areas, set `min_h(px(0.))` (and `min_w` if horizontal) so they can shrink within the parent and actually overflow; otherwise scrolling is blocked by the default min-content sizing.
- Use `.flex_1().size_full()` on the main pane, then put scrollable content inside a child `div` with `.overflow_y_scroll()` and `track_scroll(&ScrollHandle)`.
- When multiple panes coexist (tabs), keep the scrollable containers inside a parent that also has `min_h(px(0.))` to prevent hidden overflow.

## ScrollHandle Usage
- Create per-view handles: `let handle = ScrollHandle::new();` then `.track_scroll(&handle)` on the scrollable `div`.
- Keyboard-driven scrolling: read `handle.offset()`, clamp with `handle.max_offset().height`, then `handle.set_offset(new_offset)` and `cx.notify()`. Map keys Up/Down/PageUp/PageDown/Home/End to line/page deltas.
- To auto-focus a scroll area for keyboard, assign a `FocusHandle`, set `.tab_index(0)` and `.track_focus(&focus)`, then focus it on tab switch or mouse down inside the area.
- Mouse wheel is handled automatically when `overflow_*` is `Scroll`; no extra listeners needed.

## Focus & Navigation Patterns
- Keep dedicated `FocusHandle`s per major pane (e.g., Reader, Logs). On tab change, call `focus_handle.focus(window)` so arrow keys act on the correct scroll region.
- Add `.on_mouse_down(MouseButton::Left, ...)` to click-focus the scroll container without stealing clicks from inner controls.
- Input fields (URL bar, editors) retain their own focus; returning to the scroll view just calls `focus()` on its handle.

## Media Rendering (Images & Text)
- For article-like pages, render blocks with semantic styles: headers at larger `text_size` and `font_weight::BOLD`, paragraphs at readable `px(22.)`, and muted colors on dark backgrounds.
- Images: use `img(url).max_w_full().rounded_md().border_1()` with `with_loading` and `with_fallback` placeholders to avoid layout shifts and handle missing cache entries. Wrap in a centered `div().mb_4()`.
- Keep consistent padding/gaps (`.p_4()`, `.gap_3()`) and use `max_w_full` to prevent horizontal scroll.

## Defensive Tips
- Always combine scrollable areas with explicit overflow + min sizes; otherwise keyboard scroll changes the offset but the layout won’t move.
- After programmatic scroll changes, call `cx.notify()` to repaint.
- When adding more keybindings, remember to `prevent_default()` once the key is handled to avoid double-handling by focused inputs.

## Reuse Checklist
1. Add `min_h(0)` to any flex parent that should allow child scrolling.
2. Attach `ScrollHandle` + `track_scroll` to the scroll `div`.
3. Make the scroll `div` focusable: `tab_index(0)`, `track_focus`, click-to-focus.
4. Wire `on_key_down` to call a shared `handle_scroll_keys` helper (line/page/home/end).
5. For media blocks, keep placeholders and `max_w_full` to avoid overflow.
