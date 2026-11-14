//! Live editor-font preview shown above the Editor font settings: a small code
//! sample set in the same font, size and line height the editor uses.

use gpui::{
    div, px, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    Styled, Window,
};
use labonair_theme::ThemeStore;

const SAMPLE: &[&str] = &[
    "fn fibonacci(n: u32) -> u64 {",
    "    match n {",
    "        0 | 1 => n as u64,",
    "        _ => fibonacci(n - 1) + fibonacci(n - 2),",
    "    }",
    "}  // 0O 1lI != =>  >=",
];

pub(crate) struct EditorPreview {
    theme: Entity<ThemeStore>,
}

impl EditorPreview {
    pub(crate) fn new(theme: Entity<ThemeStore>) -> Self {
        Self { theme }
    }
}

impl Render for EditorPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.theme.read(cx);
        let core = &store.theme().core;
        let font_px = store.buffer_font_size();
        let line_px = (font_px * store.metrics().buffer_line_height)
            .ceil()
            .max(1.0);
        div()
            .id("editor-settings-preview")
            .w_full()
            .p(px(12.0))
            .overflow_hidden()
            .rounded(px(store.theme().radius.lg))
            .border_1()
            .border_color(core.border)
            .bg(core.background)
            .text_color(core.foreground)
            .font(store.buffer_font())
            .text_size(px(font_px))
            .children(SAMPLE.iter().map(|line| {
                div()
                    .h(px(line_px))
                    .line_height(px(line_px))
                    .whitespace_nowrap()
                    .child(SharedString::from(*line))
            }))
    }
}
