use crate::backend::TunnelState;
use crate::config::{self, Config, WebviewEnv};
use dioxus::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const NOTES_DIR: &str = "notes";
static GLOBAL_TUNNEL: std::sync::OnceLock<TunnelState> = std::sync::OnceLock::new();
const APP_STYLES: &str = r##"
    :root {
        color-scheme: light;
        --page: #efe7d7;
        --page-warm: #e4d2b6;
        --surface: rgba(255, 252, 245, 0.82);
        --surface-strong: #fffdf8;
        --surface-muted: #f7f0e1;
        --surface-accent: #e5f0ed;
        --text: #231c14;
        --muted: #726758;
        --line: rgba(79, 63, 45, 0.14);
        --line-strong: rgba(79, 63, 45, 0.24);
        --accent: #1f6c72;
        --accent-strong: #174f54;
        --accent-soft: rgba(31, 108, 114, 0.12);
        --accent-glow: rgba(31, 108, 114, 0.18);
        --alert: #9a4c38;
        --shadow: 0 24px 80px rgba(74, 54, 31, 0.14);
    }

    * { box-sizing: border-box; }

    html, body {
        margin: 0;
        min-height: 100%;
    }

    body {
        overflow: hidden;
        color: var(--text);
        font-family: "Avenir Next", "Segoe UI", "Noto Sans", sans-serif;
        background:
            radial-gradient(circle at top left, rgba(255, 255, 255, 0.72), transparent 28rem),
            radial-gradient(circle at top right, rgba(31, 108, 114, 0.14), transparent 24rem),
            linear-gradient(135deg, var(--page) 0%, #e9dfcf 48%, var(--page-warm) 100%);
    }

    button, input, textarea, select {
        font: inherit;
    }

    button {
        border: none;
        background: none;
        color: inherit;
    }

    ::selection {
        background: rgba(31, 108, 114, 0.24);
    }

    .app-shell {
        min-height: 100vh;
        display: flex;
        flex-direction: column;
        gap: 16px;
        padding: 18px;
    }

    .topbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
        padding: 18px 22px;
        border: 1px solid rgba(255, 255, 255, 0.34);
        border-radius: 26px;
        background: rgba(255, 250, 242, 0.76);
        backdrop-filter: blur(18px);
        box-shadow: var(--shadow);
    }

    .topbar-group {
        display: flex;
        align-items: center;
        gap: 18px;
        min-width: 0;
    }

    .brand {
        display: flex;
        align-items: center;
        gap: 14px;
        min-width: 0;
    }

    .brand-mark {
        width: 46px;
        height: 46px;
        flex: 0 0 46px;
        display: grid;
        place-items: center;
        border-radius: 16px;
        background: linear-gradient(135deg, #a24e34 0%, #d4884f 100%);
        color: #fffaf4;
        font-weight: 700;
        letter-spacing: 0.16em;
        text-indent: 0.16em;
        box-shadow: 0 16px 32px rgba(162, 78, 52, 0.22);
    }

    .brand-copy {
        display: flex;
        flex-direction: column;
        gap: 2px;
        min-width: 0;
    }

    .eyebrow {
        font-size: 11px;
        letter-spacing: 0.18em;
        text-transform: uppercase;
        color: var(--muted);
    }

    .brand-title {
        margin: 0;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Palatino, "Noto Serif", serif;
        font-size: 30px;
        line-height: 1;
        letter-spacing: 0.01em;
    }

    .module-switcher {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 6px;
        border-radius: 999px;
        border: 1px solid rgba(255, 255, 255, 0.42);
        background: rgba(255, 255, 255, 0.56);
        box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.52);
    }

    .nav-btn {
        display: inline-flex;
        align-items: center;
        gap: 10px;
        padding: 12px 18px;
        border-radius: 999px;
        cursor: pointer;
        color: var(--muted);
        font-size: 14px;
        font-weight: 600;
        transition: background 160ms ease, color 160ms ease, transform 160ms ease, box-shadow 160ms ease;
    }

    .nav-btn:hover {
        color: var(--text);
        background: rgba(255, 255, 255, 0.6);
        transform: translateY(-1px);
    }

    .nav-btn.active {
        color: #f8fbfb;
        background: linear-gradient(135deg, var(--accent) 0%, var(--accent-strong) 100%);
        box-shadow: 0 10px 24px rgba(31, 108, 114, 0.24);
    }

    .network-pill {
        display: inline-flex;
        align-items: center;
        gap: 12px;
        padding: 10px 14px;
        border-radius: 999px;
        border: 1px solid rgba(255, 255, 255, 0.38);
        background: rgba(255, 255, 255, 0.52);
        color: var(--muted);
        white-space: nowrap;
    }

    .status-dot {
        width: 11px;
        height: 11px;
        border-radius: 999px;
        flex: 0 0 11px;
    }

    .status-dot.online {
        background: #1e9a73;
        box-shadow: 0 0 0 6px rgba(30, 154, 115, 0.14);
    }

    .status-dot.offline {
        background: var(--alert);
        box-shadow: 0 0 0 6px rgba(154, 76, 56, 0.14);
    }

    .network-copy {
        display: flex;
        align-items: baseline;
        gap: 8px;
        font-size: 13px;
    }

    .network-copy strong {
        color: var(--text);
        font-size: 14px;
        font-weight: 700;
    }

    .workspace-surface {
        flex: 1;
        min-height: 0;
        display: flex;
        overflow: hidden;
        border-radius: 30px;
        border: 1px solid rgba(255, 255, 255, 0.36);
        background: linear-gradient(180deg, rgba(255, 252, 245, 0.82) 0%, rgba(251, 248, 241, 0.72) 100%);
        backdrop-filter: blur(20px);
        box-shadow: var(--shadow);
    }

    .editor-shell {
        display: flex;
        flex: 1;
        min-height: 0;
        width: 100%;
        overflow: hidden;
    }

    .explorer-panel {
        width: 310px;
        flex: 0 0 310px;
        display: flex;
        flex-direction: column;
        gap: 16px;
        padding: 24px 18px;
        background: linear-gradient(180deg, rgba(255, 255, 255, 0.66) 0%, rgba(248, 241, 228, 0.92) 100%);
        border-right: 1px solid var(--line);
    }

    .explorer-rail {
        width: 82px;
        flex: 0 0 82px;
        display: flex;
        align-items: flex-start;
        justify-content: center;
        padding: 24px 14px;
        border-right: 1px solid var(--line);
        background: rgba(247, 240, 225, 0.88);
    }

    .rail-toggle {
        width: 54px;
        height: 54px;
        display: grid;
        place-items: center;
        border-radius: 18px;
        cursor: pointer;
        color: var(--accent-strong);
        background: var(--surface-strong);
        border: 1px solid var(--line);
        box-shadow: 0 14px 28px rgba(74, 54, 31, 0.08);
    }

    .rail-toggle svg {
        transform: rotate(180deg);
    }

    .explorer-head,
    .preview-head,
    .editor-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 12px;
    }

    .explorer-title,
    .editor-title,
    .preview-title {
        margin: 4px 0 0;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Palatino, "Noto Serif", serif;
        font-size: 27px;
        line-height: 1.05;
    }

    .icon-btn,
    .toolbar-btn {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        cursor: pointer;
        border-radius: 999px;
        transition: background 160ms ease, color 160ms ease, transform 160ms ease;
    }

    .icon-btn {
        width: 40px;
        height: 40px;
        justify-content: center;
        background: rgba(255, 255, 255, 0.66);
        border: 1px solid var(--line);
        color: var(--muted);
    }

    .icon-btn:hover,
    .toolbar-btn:hover {
        color: var(--text);
        transform: translateY(-1px);
    }

    .toolbar-btn {
        padding: 11px 16px;
        background: rgba(255, 255, 255, 0.7);
        border: 1px solid var(--line);
        color: var(--muted);
        font-size: 13px;
        font-weight: 600;
    }

    .explorer-summary,
    .editor-meta,
    .preview-meta,
    .browser-meta {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
        align-items: center;
    }

    .meta-pill {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        padding: 7px 11px;
        border-radius: 999px;
        background: rgba(255, 255, 255, 0.74);
        border: 1px solid rgba(79, 63, 45, 0.1);
        color: var(--muted);
        font-size: 12px;
        font-weight: 600;
    }

    .meta-pill strong {
        color: var(--text);
    }

    .file-list {
        min-height: 0;
        display: flex;
        flex-direction: column;
        gap: 8px;
        overflow-y: auto;
        padding-right: 4px;
    }

    .file-item {
        width: 100%;
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 14px;
        text-align: left;
        border-radius: 18px;
        cursor: pointer;
        background: rgba(255, 255, 255, 0.42);
        border: 1px solid transparent;
        transition: transform 160ms ease, background 160ms ease, border-color 160ms ease, box-shadow 160ms ease;
    }

    .file-item:hover {
        transform: translateY(-1px);
        background: rgba(255, 255, 255, 0.78);
        border-color: rgba(79, 63, 45, 0.08);
    }

    .file-item.active {
        background: linear-gradient(135deg, rgba(31, 108, 114, 0.14) 0%, rgba(255, 255, 255, 0.94) 100%);
        border-color: rgba(31, 108, 114, 0.24);
        box-shadow: 0 12px 28px rgba(31, 108, 114, 0.12);
    }

    .file-icon {
        width: 42px;
        height: 42px;
        flex: 0 0 42px;
        display: grid;
        place-items: center;
        border-radius: 14px;
        background: rgba(31, 108, 114, 0.08);
        color: var(--accent-strong);
    }

    .file-copy {
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 4px;
    }

    .file-name {
        font-size: 14px;
        font-weight: 700;
        color: var(--text);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .file-hint,
    .editor-subtitle,
    .preview-subtitle,
    .browser-subtitle,
    .sidebar-empty {
        font-size: 13px;
        line-height: 1.55;
        color: var(--muted);
    }

    .sidebar-empty {
        padding: 16px;
        border-radius: 18px;
        background: rgba(255, 255, 255, 0.58);
        border: 1px dashed var(--line-strong);
    }

    .editor-panel,
    .preview-panel,
    .browser-shell {
        min-height: 0;
    }

    .editor-panel {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 18px;
        padding: 26px;
    }

    .editor-toolbar {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 10px;
    }

    .editor-canvas {
        position: relative;
        flex: 1;
        min-height: 0;
        overflow: hidden;
        border-radius: 26px;
        background: linear-gradient(180deg, rgba(255, 255, 255, 0.96) 0%, rgba(251, 248, 241, 0.96) 100%);
        border: 1px solid rgba(79, 63, 45, 0.08);
        box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.6);
    }

    .editor-input {
        width: 100%;
        height: 100%;
        padding: 30px 32px;
        border: none;
        resize: none;
        outline: none;
        background: transparent;
        color: var(--text);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, "Liberation Mono", monospace;
        font-size: 14px;
        line-height: 1.8;
    }

    .editor-input::placeholder {
        color: rgba(114, 103, 88, 0.72);
    }

    .editor-overlay {
        position: absolute;
        inset: 0;
        display: flex;
        align-items: flex-end;
        pointer-events: none;
        padding: 0 32px 30px;
    }

    .editor-overlay-card {
        max-width: 360px;
        padding: 14px 16px;
        border-radius: 18px;
        background: rgba(31, 108, 114, 0.08);
        border: 1px solid rgba(31, 108, 114, 0.16);
        color: var(--accent-strong);
        font-size: 13px;
        line-height: 1.55;
    }

    .empty-panel,
    .preview-empty {
        height: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .empty-card {
        max-width: 380px;
        padding: 24px;
        border-radius: 24px;
        background: rgba(255, 255, 255, 0.72);
        border: 1px solid rgba(79, 63, 45, 0.1);
        box-shadow: 0 18px 44px rgba(74, 54, 31, 0.08);
    }

    .empty-card h3 {
        margin: 6px 0 8px;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Palatino, "Noto Serif", serif;
        font-size: 26px;
        line-height: 1.1;
    }

    .preview-panel {
        width: 38%;
        min-width: 340px;
        display: flex;
        flex-direction: column;
        gap: 16px;
        padding: 26px 24px 26px 22px;
        border-left: 1px solid var(--line);
        background: linear-gradient(180deg, rgba(247, 240, 225, 0.76) 0%, rgba(255, 252, 246, 0.92) 100%);
    }

    .preview-body {
        min-height: 0;
        overflow-y: auto;
        padding-right: 6px;
    }

    .markdown-body {
        color: var(--text);
        font-size: 15px;
        line-height: 1.72;
    }

    .markdown-body h1,
    .markdown-body h2,
    .markdown-body h3 {
        margin: 1.4em 0 0.55em;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Palatino, "Noto Serif", serif;
        line-height: 1.15;
    }

    .markdown-body h1 {
        font-size: 2rem;
        border-bottom: 1px solid var(--line);
        padding-bottom: 0.4rem;
    }

    .markdown-body h2 { font-size: 1.55rem; }
    .markdown-body h3 { font-size: 1.2rem; }
    .markdown-body p,
    .markdown-body ul,
    .markdown-body ol,
    .markdown-body blockquote,
    .markdown-body pre,
    .markdown-body table {
        margin: 0 0 1rem;
    }

    .markdown-body a {
        color: var(--accent);
    }

    .markdown-body code {
        padding: 0.18rem 0.38rem;
        border-radius: 8px;
        background: rgba(31, 108, 114, 0.08);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, "Liberation Mono", monospace;
        font-size: 0.92em;
    }

    .markdown-body pre {
        padding: 1rem;
        border-radius: 18px;
        overflow-x: auto;
        background: #f4ede1;
        border: 1px solid rgba(79, 63, 45, 0.08);
    }

    .markdown-body pre code {
        padding: 0;
        background: transparent;
    }

    .markdown-body blockquote {
        padding: 0.1rem 0 0.1rem 1rem;
        border-left: 3px solid rgba(31, 108, 114, 0.4);
        color: #5f5649;
    }

    .markdown-body table {
        width: 100%;
        border-collapse: collapse;
    }

    .markdown-body th,
    .markdown-body td {
        padding: 0.7rem 0.8rem;
        text-align: left;
        border-bottom: 1px solid var(--line);
    }

    .browser-shell {
        flex: 1;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 36px;
    }

    .browser-panel {
        width: min(100%, 1040px);
        display: flex;
        flex-direction: column;
        gap: 22px;
        padding: 34px;
        border-radius: 34px;
        background:
            radial-gradient(circle at top right, rgba(31, 108, 114, 0.16), transparent 18rem),
            linear-gradient(180deg, rgba(255, 255, 255, 0.9) 0%, rgba(248, 241, 228, 0.88) 100%);
        border: 1px solid rgba(255, 255, 255, 0.4);
        box-shadow: 0 30px 80px rgba(74, 54, 31, 0.12);
    }

    .browser-title {
        margin: 6px 0 8px;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Palatino, "Noto Serif", serif;
        font-size: clamp(2rem, 3vw, 3.2rem);
        line-height: 0.98;
        max-width: 14ch;
    }

    .browser-search {
        position: relative;
    }

    .browser-search-icon {
        position: absolute;
        top: 50%;
        left: 20px;
        transform: translateY(-50%);
        color: var(--muted);
        pointer-events: none;
    }

    .browser-input {
        width: 100%;
        height: 66px;
        border-radius: 22px;
        border: 1px solid rgba(79, 63, 45, 0.12);
        padding: 0 24px 0 56px;
        background: rgba(255, 255, 255, 0.88);
        color: var(--text);
        font-size: 16px;
        outline: none;
        box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.5);
    }

    .browser-input:focus {
        border-color: rgba(31, 108, 114, 0.32);
        box-shadow: 0 0 0 4px rgba(31, 108, 114, 0.08);
    }

    .browser-grid {
        display: grid;
        grid-template-columns: repeat(5, minmax(0, 1fr));
        gap: 14px;
    }

    .pinned-card {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 14px;
        padding: 18px;
        min-height: 136px;
        border-radius: 24px;
        cursor: pointer;
        text-align: left;
        background: rgba(255, 255, 255, 0.66);
        border: 1px solid rgba(79, 63, 45, 0.1);
        transition: transform 160ms ease, box-shadow 160ms ease, border-color 160ms ease, background 160ms ease;
    }

    .pinned-card:hover {
        transform: translateY(-2px);
        background: rgba(255, 255, 255, 0.92);
        border-color: rgba(31, 108, 114, 0.2);
        box-shadow: 0 16px 32px rgba(31, 108, 114, 0.12);
    }

    .pinned-card-icon {
        width: 48px;
        height: 48px;
        display: grid;
        place-items: center;
        border-radius: 16px;
        background: var(--surface-accent);
        color: var(--accent-strong);
    }

    .pinned-card-label {
        font-size: 15px;
        font-weight: 700;
        color: var(--text);
    }

    .browser-footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 14px;
        flex-wrap: wrap;
    }

    .browser-launch {
        display: inline-flex;
        align-items: center;
        gap: 10px;
        padding: 14px 22px;
        border-radius: 999px;
        cursor: pointer;
        color: #f8fbfb;
        font-size: 14px;
        font-weight: 700;
        background: linear-gradient(135deg, var(--accent) 0%, var(--accent-strong) 100%);
        box-shadow: 0 16px 32px rgba(31, 108, 114, 0.24);
        transition: transform 160ms ease, box-shadow 160ms ease;
    }

    .browser-launch:hover {
        transform: translateY(-1px);
        box-shadow: 0 18px 34px rgba(31, 108, 114, 0.28);
    }

    .settings-shell {
        flex: 1;
        min-width: 0;
        max-height: calc(100vh - 160px);
        overflow-y: auto;
        padding: 32px;
    }

    .settings-panel {
        max-width: 720px;
        margin: 0 auto;
        display: flex;
        flex-direction: column;
        gap: 24px;
    }

    .settings-panel h2 {
        margin: 6px 0 0;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Palatino, "Noto Serif", serif;
        font-size: 32px;
    }

    .settings-field {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .settings-field label { font-weight: 600; }

    .settings-input {
        width: 100%;
        padding: 12px 14px;
        border: 1px solid var(--line-strong);
        border-radius: 12px;
        background: var(--surface-strong);
        color: var(--text);
    }

    .settings-input:focus-visible,
    .settings-panel button:focus-visible {
        outline: 3px solid var(--accent);
        outline-offset: 3px;
    }

    .settings-message { margin: 0; color: var(--accent-strong); }
    .settings-error { margin: 0; color: var(--alert); }

    .settings-actions {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 12px;
    }

    .settings-path { overflow-wrap: anywhere; }

    @media (max-width: 1180px) {
        .browser-grid {
            grid-template-columns: repeat(3, minmax(0, 1fr));
        }

        .preview-panel {
            min-width: 300px;
        }
    }

    @media (max-width: 960px) {
        .topbar {
            flex-direction: column;
            align-items: stretch;
        }

        .topbar-group {
            justify-content: space-between;
        }

        .workspace-surface,
        .editor-shell {
            flex-direction: column;
        }

        .explorer-panel,
        .explorer-rail,
        .preview-panel {
            width: 100%;
            min-width: 0;
            flex-basis: auto;
            border-left: none;
            border-right: none;
        }

        .explorer-panel,
        .explorer-rail {
            border-bottom: 1px solid var(--line);
        }

        .preview-panel {
            border-top: 1px solid var(--line);
        }
    }

    @media (max-width: 720px) {
        .app-shell {
            padding: 12px;
        }

        .browser-grid {
            grid-template-columns: repeat(2, minmax(0, 1fr));
        }

        .browser-panel,
        .editor-panel,
        .preview-panel,
        .explorer-panel {
            padding: 20px;
        }

        .module-switcher {
            width: 100%;
            justify-content: space-between;
        }

        .nav-btn {
            flex: 1;
            justify-content: center;
        }
    }
"##;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ActiveModule {
    Markdown,
    Browser,
    Settings,
}

#[derive(Clone, PartialEq, Debug)]
struct NoteFile {
    name: String,
    path: PathBuf,
}

fn collect_notes() -> Vec<NoteFile> {
    let mut list = match fs::read_dir(NOTES_DIR) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| NoteFile {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
            })
            .filter(|note| note.name.ends_with(".md"))
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

fn read_note_content(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

pub fn start_workspace(saved_config: Config, runtime_config: Config) -> anyhow::Result<()> {
    let tunnel = GLOBAL_TUNNEL.get_or_init(TunnelState::default).clone();
    let server_addr = runtime_config.backend_addr()?;

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            crate::client::run_background_tasks(server_addr, tunnel, runtime_config.tunnel_strict)
                .await;
        });
    });

    let window = dioxus::desktop::WindowBuilder::new()
        .with_title("Workspace")
        .with_inner_size(dioxus::desktop::tao::dpi::LogicalSize::new(1440.0, 920.0))
        .with_min_inner_size(dioxus::desktop::tao::dpi::LogicalSize::new(1024.0, 720.0));
    dioxus::LaunchBuilder::desktop()
        .with_context(saved_config)
        .with_cfg(dioxus::desktop::Config::new().with_window(window))
        .launch(app);
    Ok(())
}

fn app() -> Element {
    let mut active_module = use_signal(|| ActiveModule::Markdown);
    let sidebar_collapsed = use_signal(|| false);
    let preview_collapsed = use_signal(|| false);
    let settings = use_signal(consume_context::<Config>);

    rsx! {
        style { "{APP_STYLES}" }
        div {
            class: "app-shell",
            header {
                class: "topbar",
                div {
                    class: "topbar-group",
                    div {
                        class: "brand",
                        div { class: "brand-mark", "WS" }
                        div {
                            class: "brand-copy",
                            span { class: "eyebrow", "Private markdown tunnel" }
                            h1 { class: "brand-title", "Workspace" }
                        }
                    }
                    NetworkMonitor {}
                }

                div {
                    class: "module-switcher",
                    button {
                        class: if active_module() == ActiveModule::Markdown { "nav-btn active" } else { "nav-btn" },
                        onclick: move |_| active_module.set(ActiveModule::Markdown),
                        Icon { path: Icons::FILE_TEXT, size: 14 }
                        "Editor"
                    }
                    button {
                        class: if active_module() == ActiveModule::Browser { "nav-btn active" } else { "nav-btn" },
                        onclick: move |_| active_module.set(ActiveModule::Browser),
                        Icon { path: Icons::GLOBE, size: 14 }
                        "Browser"
                    }
                    button {
                        class: if active_module() == ActiveModule::Settings { "nav-btn active" } else { "nav-btn" },
                        onclick: move |_| active_module.set(ActiveModule::Settings),
                        Icon { path: Icons::SETTINGS, size: 14 }
                        "Settings"
                    }
                }
            }

            main {
                class: "workspace-surface",
                match active_module() {
                    ActiveModule::Markdown => rsx! {
                        MarkdownWorkspace { sidebar_collapsed, preview_collapsed }
                    },
                    ActiveModule::Browser => rsx! { BrowserView {} },
                    ActiveModule::Settings => rsx! { SettingsView { settings } },
                }
            }
        }
    }
}

#[component]
fn SettingsView(mut settings: Signal<Config>) -> Element {
    let mut message = use_signal(String::new);
    let mut error = use_signal(String::new);
    let path = config::config_path()
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    rsx! {
        section {
            class: "settings-shell",
            form {
                class: "settings-panel",
                onsubmit: move |event| {
                    event.prevent_default();
                    message.set(String::new());
                    error.set(String::new());
                    let save = || -> anyhow::Result<Config> {
                        let text = |name| match event.get_first(name) {
                            Some(FormValue::Text(value)) => Ok(value),
                            _ => anyhow::bail!("Missing setting: {name}"),
                        };
                        let config = Config {
                            backend: text("backend")?.trim().to_string(),
                            tunnel_strict: text("tunnel_strict")?.parse()?,
                            webview_env: clap::ValueEnum::from_str(&text("webview_env")?, false)
                                .map_err(anyhow::Error::msg)?,
                        };
                        config::save_config(&config)?;
                        Ok(config)
                    };
                    match save() {
                        Ok(config) => {
                            settings.set(config);
                            message.set("Settings saved. Restart Workspace to apply changes.".to_string());
                        }
                        Err(e) => error.set(format!("Could not save settings: {e:#}")),
                    }
                },
                div {
                    span { class: "eyebrow", "Settings" }
                    h2 { "Workspace preferences" }
                    p {
                        class: "browser-subtitle",
                        "Changes take effect after restarting Workspace. Command-line options and WORKSPACE_TUNNEL_STRICT / WORKSPACE_WEBVIEW_ENV environment variables override saved preferences."
                    }
                }
                div {
                    class: "settings-field",
                    label { r#for: "settings-backend", "Backend address" }
                    input {
                        id: "settings-backend",
                        name: "backend",
                        class: "settings-input",
                        r#type: "text",
                        required: true,
                        // Read the complete form on submit to avoid desktop input round trips.
                        initial_value: "{settings.read().backend}",
                        aria_describedby: "backend-help",
                    }
                    span {
                        id: "backend-help",
                        class: "browser-subtitle",
                        "IP address with optional port (default 3000), e.g. 127.0.0.1:3000 or [::1]:3000. URLs and hostnames are not supported."
                    }
                }
                div {
                    class: "settings-field",
                    label { r#for: "settings-tunnel", "Tunnel mode" }
                    select {
                        id: "settings-tunnel",
                        name: "tunnel_strict",
                        class: "settings-input",
                        aria_describedby: "tunnel-help",
                        option { value: "false", initial_selected: !settings.read().tunnel_strict, "Allow direct fallback" }
                        option { value: "true", initial_selected: settings.read().tunnel_strict, "Strict — require tunnel" }
                    }
                    span {
                        id: "tunnel-help",
                        class: "browser-subtitle",
                        "Direct fallback connects without the tunnel when it is unavailable. Strict mode blocks those requests instead."
                    }
                }
                div {
                    class: "settings-field",
                    label { r#for: "settings-webview", "WebView environment (Linux)" }
                    select {
                        id: "settings-webview",
                        name: "webview_env",
                        class: "settings-input",
                        aria_describedby: "webview-help",
                        option { value: "safe", initial_selected: settings.read().webview_env == WebviewEnv::Safe, "Safe — compatibility overrides" }
                        option { value: "minimal", initial_selected: settings.read().webview_env == WebviewEnv::Minimal, "Minimal — X11 only" }
                        option { value: "off", initial_selected: settings.read().webview_env == WebviewEnv::Off, "Off — use system environment" }
                    }
                    span {
                        id: "webview-help",
                        class: "browser-subtitle",
                        "Safe uses software rendering and disables WebKit's sandbox for compatibility. Minimal only defaults to X11. Off adds no overrides. Existing system environment values are preserved."
                    }
                }
                div {
                    class: "settings-actions",
                    button { class: "browser-launch", r#type: "submit", "Save settings" }
                    button {
                        class: "toolbar-btn",
                        r#type: "reset",
                        onclick: move |_| {
                            message.set(String::new());
                            error.set(String::new());
                        },
                        "Discard changes"
                    }
                }
                p { class: "settings-message", role: "status", "{message}" }
                p { class: "settings-error", role: "alert", "{error}" }
                p { class: "browser-subtitle settings-path", "Saved in {path}" }
            }
        }
    }
}

#[component]
fn NetworkMonitor() -> Element {
    let mut latency = use_signal(|| "---".to_string());
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let rtt = if let Some(t) = GLOBAL_TUNNEL.get() {
                t.get_rtt().await
            } else {
                None
            };
            match rtt {
                Some(d) => latency.set(format!("{}ms", d.as_millis())),
                None => latency.set("Offline".to_string()),
            }
        }
    });

    let offline = latency() == "Offline";

    rsx! {
        div {
            class: "network-pill",
            div { class: if offline { "status-dot offline" } else { "status-dot online" } }
            div {
                class: "network-copy",
                span { "Tunnel" }
                strong { "{latency}" }
            }
        }
    }
}

#[component]
fn BrowserView() -> Element {
    let mut url = use_signal(|| "https://www.google.com".to_string());

    rsx! {
        div {
            class: "browser-shell",
            section {
                class: "browser-panel",
                div {
                    span { class: "eyebrow", "Private browsing" }
                    h2 { class: "browser-title", "Launch the web through Workspace." }
                    p {
                        class: "browser-subtitle",
                        "Use the local proxy-backed browser window for quick tunnel validation, docs lookup, and focused browsing without dropping into a generic demo page."
                    }
                }

                div {
                    class: "browser-meta",
                    span { class: "meta-pill", strong { "Tip" } "Press Enter to open the current address immediately." }
                    span { class: "meta-pill", strong { "Proxy" } "127.0.0.1:8080" }
                }

                div {
                    class: "browser-search",
                    div {
                        class: "browser-search-icon",
                        Icon { path: Icons::SEARCH, size: 20 }
                    }
                    input {
                        class: "browser-input",
                        placeholder: "Search or enter address...",
                        value: "{url}",
                        oninput: move |e| url.set(e.value()),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter {
                                let _ = crate::client::start_browser_window(&url());
                            }
                        }
                    }
                }

                div {
                    class: "browser-grid",
                    PinnedCard { icon_path: Icons::CHROME, label: "Google", url: "https://www.google.com" }
                    PinnedCard { icon_path: Icons::TWITTER, label: "Twitter", url: "https://twitter.com" }
                    PinnedCard { icon_path: Icons::REDDIT, label: "Reddit", url: "https://www.reddit.com" }
                    PinnedCard { icon_path: Icons::YOUTUBE, label: "YouTube", url: "https://www.youtube.com" }
                    PinnedCard { icon_path: Icons::FACEBOOK, label: "Facebook", url: "https://www.facebook.com" }
                }

                div {
                    class: "browser-footer",
                    p {
                        class: "browser-subtitle",
                        "Pinned shortcuts stay visible, the URL bar is clearer, and the layout now behaves like a desktop product instead of an unstyled web mock."
                    }
                    button {
                        class: "browser-launch",
                        onclick: move |_| { let _ = crate::client::start_browser_window(&url()); },
                        Icon { path: Icons::GLOBE, size: 16 }
                        "Launch Secure Browser"
                    }
                }
            }
        }
    }
}

#[component]
fn PinnedCard(icon_path: &'static str, label: String, url: String) -> Element {
    rsx! {
        button {
            class: "pinned-card",
            onclick: move |_| { let _ = crate::client::start_browser_window(&url); },
            div {
                class: "pinned-card-icon",
                Icon { path: icon_path, size: 24 }
            }
            div {
                class: "file-copy",
                span { class: "pinned-card-label", "{label}" }
                span { class: "file-hint", "Open in tunneled window" }
            }
        }
    }
}

#[component]
fn MarkdownWorkspace(sidebar_collapsed: Signal<bool>, preview_collapsed: Signal<bool>) -> Element {
    let mut files = use_signal(Vec::<NoteFile>::new);
    let mut current_note = use_signal(|| None::<NoteFile>);
    let mut content = use_signal(|| String::new());

    let mut open_note = move |note: NoteFile| {
        content.set(read_note_content(&note.path));
        current_note.set(Some(note));
    };

    use_effect(move || {
        let notes = collect_notes();
        let first_note = notes.first().cloned();
        files.set(notes);

        if let Some(note) = first_note {
            content.set(read_note_content(&note.path));
            current_note.set(Some(note));
        } else {
            content.set(String::new());
            current_note.set(None);
        }
    });

    let note_list = files();
    let selected_note = current_note();
    let editor_text = content();
    let is_empty_note = editor_text.trim().is_empty();
    let word_count = editor_text.split_whitespace().count();
    let line_count = if editor_text.is_empty() {
        0
    } else {
        editor_text.lines().count()
    };
    let file_count = note_list.len();

    rsx! {
        div {
            class: "editor-shell",
            if sidebar_collapsed() {
                aside {
                    class: "explorer-rail",
                    button {
                        class: "rail-toggle",
                        onclick: move |_| sidebar_collapsed.set(false),
                        Icon { path: Icons::CHEVRON_LEFT, size: 18 }
                    }
                }
            } else {
                aside {
                    class: "explorer-panel",
                    div {
                        class: "explorer-head",
                        div {
                            span { class: "eyebrow", "Notes" }
                            h2 { class: "explorer-title", "Explorer" }
                        }
                        button {
                            class: "icon-btn",
                            onclick: move |_| sidebar_collapsed.set(true),
                            Icon { path: Icons::CHEVRON_LEFT, size: 16 }
                        }
                    }

                    div {
                        class: "explorer-summary",
                        span { class: "meta-pill", strong { "{file_count}" } if file_count == 1 { "file" } else { "files" } }
                        span { class: "meta-pill", "Directory: notes/" }
                    }

                    div {
                        class: "file-list",
                        if note_list.is_empty() {
                            div {
                                class: "sidebar-empty",
                                "No markdown notes were found in `notes/`. Add a `.md` file and it will appear here."
                            }
                        } else {
                            for note in note_list {
                                button {
                                    class: if Some(note.clone()) == selected_note { "file-item active" } else { "file-item" },
                                    onclick: move |_| open_note(note.clone()),
                                    div {
                                        class: "file-icon",
                                        Icon { path: Icons::FILE, size: 16 }
                                    }
                                    div {
                                        class: "file-copy",
                                        span { class: "file-name", "{note.name}" }
                                        span {
                                            class: "file-hint",
                                            if Some(note.clone()) == selected_note { "Selected note" } else { "Markdown note" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            section {
                class: "editor-panel",
                if let Some(note) = selected_note.clone() {
                    header {
                        class: "editor-head",
                        div {
                            span { class: "eyebrow", "Active note" }
                            h2 { class: "editor-title", "{note.name}" }
                            p {
                                class: "editor-subtitle",
                                if is_empty_note {
                                    "This file is empty, so the editor now keeps a visible hint instead of turning into a blank void."
                                } else {
                                    "Type directly into the note. Changes are written back to disk immediately and previewed live."
                                }
                            }
                        }

                        div {
                            class: "editor-toolbar",
                            span { class: "meta-pill", strong { "{word_count}" } "words" }
                            span { class: "meta-pill", strong { "{line_count}" } "lines" }
                            button {
                                class: "toolbar-btn",
                                onclick: move |_| preview_collapsed.set(!preview_collapsed()),
                                if preview_collapsed() {
                                    Icon { path: Icons::GLOBE, size: 14 }
                                    "Show Preview"
                                } else {
                                    Icon { path: Icons::SETTINGS, size: 14 }
                                    "Hide Preview"
                                }
                            }
                        }
                    }

                    div {
                        class: "editor-canvas",
                        textarea {
                            class: "editor-input",
                            placeholder: "This note is empty. Start typing to make the selection visibly useful.",
                            value: "{editor_text}",
                            oninput: move |evt| {
                                let next = evt.value();
                                content.set(next.clone());
                                let _ = fs::write(&note.path, next);
                            }
                        }

                        if is_empty_note {
                            div {
                                class: "editor-overlay",
                                div {
                                    class: "editor-overlay-card",
                                    "Empty file selected. Type here and the note will persist to disk immediately, with preview appearing as soon as there is content."
                                }
                            }
                        }
                    }
                } else {
                    div {
                        class: "empty-panel",
                        div {
                            class: "empty-card",
                            span { class: "eyebrow", "Editor" }
                            h3 { "Pick a note from Explorer." }
                            p {
                                class: "editor-subtitle",
                                "The previous blank-state behavior was misleading. This panel now stays explicit when nothing is selected."
                            }
                        }
                    }
                }
            }

            if !preview_collapsed() {
                aside {
                    class: "preview-panel",
                    div {
                        class: "preview-head",
                        div {
                            span { class: "eyebrow", "Rendered markdown" }
                            h2 {
                                class: "preview-title",
                                if let Some(note) = selected_note.clone() {
                                    "{note.name}"
                                } else {
                                    "Preview"
                                }
                            }
                            p {
                                class: "preview-subtitle",
                                if selected_note.is_some() {
                                    "Live output updates as the file changes."
                                } else {
                                    "Preview activates when a markdown note is selected."
                                }
                            }
                        }

                        div {
                            class: "preview-meta",
                            span { class: "meta-pill", if is_empty_note { "Awaiting content" } else { "Live" } }
                        }
                    }

                    if let Some(_) = selected_note {
                        if is_empty_note {
                            div {
                                class: "preview-empty",
                                div {
                                    class: "empty-card",
                                    span { class: "eyebrow", "Preview" }
                                    h3 { "Nothing to render yet." }
                                    p {
                                        class: "preview-subtitle",
                                        "The note exists and is selected. It is simply empty, which is now shown explicitly instead of looking broken."
                                    }
                                }
                            }
                        } else {
                            div {
                                class: "preview-body",
                                MarkdownRenderer { text: editor_text.clone() }
                            }
                        }
                    } else {
                        div {
                            class: "preview-empty",
                            div {
                                class: "empty-card",
                                span { class: "eyebrow", "Preview" }
                                h3 { "Select a document to preview." }
                                p {
                                    class: "preview-subtitle",
                                    "Choose a note in Explorer and the rendered markdown will appear here."
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn MarkdownRenderer(text: String) -> Element {
    use pulldown_cmark::{Options, Parser};
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let mut html_output = String::new();
    let parser = Parser::new_ext(&text, options);
    pulldown_cmark::html::push_html(&mut html_output, parser);
    rsx! { div { class: "markdown-body text-sm", dangerous_inner_html: "{html_output}" } }
}

// --- Icons Module ---
// Raw SVG paths for Lucide icons to avoid external dependencies.
#[component]
fn Icon(path: &'static str, size: u32) -> Element {
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "{path}" }
        }
    }
}

struct Icons;
impl Icons {
    const FILE_TEXT: &'static str = "M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z M14 2v6h6 M16 13H8 M16 17H8 M10 9H8";
    const GLOBE: &'static str = "M21 12a9 9 0 1 1-6.219-8.56 M2 12h20 M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z";
    const SETTINGS: &'static str = "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.47a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.39a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z";
    const SEARCH: &'static str = "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35";
    const CHEVRON_LEFT: &'static str = "M15 18l-6-6 6-6";
    const FILE: &'static str =
        "M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z M14 2v6h6";
    // Brand Icons (Simplified/Proxies)
    const CHROME: &'static str =
        "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z M12 12a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"; // Generic Circle
    const TWITTER: &'static str = "M22 4s-.7 2.1-2 3.4c1.6 10-9.4 17.3-18 11.6 2.2.1 4.4-.6 6-2C3 15.5.5 9.6 3 5c2.2 2.6 5.6 4.1 9 4-.9-4.2 4-6.6 7-3.8 1.1 0 3-1.2 3-1.2z";
    const REDDIT: &'static str = "M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"; // Message Square
    const YOUTUBE: &'static str = "M22.54 6.42a2.78 2.78 0 0 0-1.94-2C18.88 4 12 4 12 4s-6.88 0-8.6.46a2.78 2.78 0 0 0-1.94 2A29 29 0 0 0 1 11.75a29 29 0 0 0 .46 5.33A2.78 2.78 0 0 0 3.4 19c1.72.46 8.6.46 8.6.46s6.88 0 8.6-.46a2.78 2.78 0 0 0 1.94-2 29 29 0 0 0 .46-5.33 29 29 0 0 0-.46-5.33z M9.75 15.02l5.75-3.27-5.75-3.27z";
    const FACEBOOK: &'static str =
        "M18 2h-3a5 5 0 0 0-5 5v3H7v4h3v8h4v-8h3l1-4h-4V7a1 1 0 0 1 1-1h3z";
}
