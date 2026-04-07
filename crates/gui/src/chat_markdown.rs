use std::ops::Range;

use gpui::*;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::{hex, hex_a, ACCENT_BLUE, BORDER, PANEL_BG, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

#[derive(Clone, Debug, Default, PartialEq)]
struct InlineContent {
    text: String,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
}

#[derive(Clone, Debug, PartialEq)]
enum MarkdownBlock {
    Paragraph(InlineContent),
    Heading {
        level: u8,
        content: InlineContent,
    },
    ListItem {
        marker: String,
        content: InlineContent,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
}

#[derive(Default)]
struct InlineBuilder {
    text: String,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    strong_depth: usize,
    emphasis_depth: usize,
    link_depth: usize,
}

enum ActiveBlock {
    Paragraph(InlineBuilder),
    Heading {
        level: u8,
        content: InlineBuilder,
    },
    ListItem {
        marker: String,
        content: InlineBuilder,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
}

struct ListState {
    next_number: Option<u64>,
}

impl InlineBuilder {
    fn push_text(&mut self, text: &str, is_inline_code: bool) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if let Some(style) = self.current_highlight(is_inline_code) {
            self.highlights.push((start..end, style));
        }
    }

    fn current_highlight(&self, is_inline_code: bool) -> Option<HighlightStyle> {
        let mut style = HighlightStyle::default();
        let mut active = false;

        if self.strong_depth > 0 {
            style.font_weight = Some(FontWeight::BOLD);
            active = true;
        }
        if self.emphasis_depth > 0 {
            style.font_style = Some(FontStyle::Italic);
            active = true;
        }
        if self.link_depth > 0 {
            style.color = Some(hex(ACCENT_BLUE));
            style.underline = Some(UnderlineStyle {
                color: Some(hex(ACCENT_BLUE)),
                thickness: px(1.0),
                wavy: false,
            });
            active = true;
        }
        if is_inline_code {
            style.background_color = Some(hex_a(BORDER, 0.85));
            style.color = Some(hex(TEXT_PRIMARY));
            active = true;
        }

        active.then_some(style)
    }

    fn build(self) -> InlineContent {
        InlineContent {
            text: self.text,
            highlights: self.highlights,
        }
    }
}

pub fn render_markdown_blocks(markdown: &str) -> Vec<AnyElement> {
    parse_markdown_blocks(markdown)
        .into_iter()
        .map(render_markdown_block)
        .collect()
}

fn parse_markdown_blocks(markdown: &str) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();
    let mut active_block: Option<ActiveBlock> = None;
    let mut list_stack: Vec<ListState> = Vec::new();
    let parser = Parser::new_ext(markdown, Options::empty());

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    finalize_active_block(&mut active_block, &mut blocks);
                    active_block = Some(ActiveBlock::Paragraph(InlineBuilder::default()));
                }
                Tag::Heading { level, .. } => {
                    finalize_active_block(&mut active_block, &mut blocks);
                    active_block = Some(ActiveBlock::Heading {
                        level: heading_level_number(level),
                        content: InlineBuilder::default(),
                    });
                }
                Tag::List(start) => list_stack.push(ListState { next_number: start }),
                Tag::Item => {
                    finalize_active_block(&mut active_block, &mut blocks);
                    let marker = list_stack
                        .last_mut()
                        .and_then(|state| state.next_number.as_mut())
                        .map(|number| {
                            let marker = format!("{number}.");
                            *number += 1;
                            marker
                        })
                        .unwrap_or_else(|| "•".to_string());
                    active_block = Some(ActiveBlock::ListItem {
                        marker,
                        content: InlineBuilder::default(),
                    });
                }
                Tag::CodeBlock(kind) => {
                    finalize_active_block(&mut active_block, &mut blocks);
                    active_block = Some(ActiveBlock::CodeBlock {
                        language: code_block_language(kind),
                        code: String::new(),
                    });
                }
                Tag::Emphasis => {
                    if let Some(inline) = inline_builder_mut(&mut active_block) {
                        inline.emphasis_depth += 1;
                    }
                }
                Tag::Strong => {
                    if let Some(inline) = inline_builder_mut(&mut active_block) {
                        inline.strong_depth += 1;
                    }
                }
                Tag::Link { .. } => {
                    if let Some(inline) = inline_builder_mut(&mut active_block) {
                        inline.link_depth += 1;
                    }
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph | TagEnd::Item | TagEnd::CodeBlock | TagEnd::Heading(_) => {
                    finalize_active_block(&mut active_block, &mut blocks);
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::Emphasis => {
                    if let Some(inline) = inline_builder_mut(&mut active_block) {
                        inline.emphasis_depth = inline.emphasis_depth.saturating_sub(1);
                    }
                }
                TagEnd::Strong => {
                    if let Some(inline) = inline_builder_mut(&mut active_block) {
                        inline.strong_depth = inline.strong_depth.saturating_sub(1);
                    }
                }
                TagEnd::Link => {
                    if let Some(inline) = inline_builder_mut(&mut active_block) {
                        inline.link_depth = inline.link_depth.saturating_sub(1);
                    }
                }
                _ => {}
            },
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if let Some(code) = code_block_mut(&mut active_block) {
                    code.push_str(&text);
                } else {
                    ensure_inline_builder(&mut active_block).push_text(&text, false);
                }
            }
            Event::Code(code) => {
                ensure_inline_builder(&mut active_block).push_text(&code, true);
            }
            Event::SoftBreak => {
                ensure_inline_builder(&mut active_block).push_text("\n", false);
            }
            Event::HardBreak => {
                ensure_inline_builder(&mut active_block).push_text("\n", false);
            }
            Event::Rule => {
                finalize_active_block(&mut active_block, &mut blocks);
            }
            _ => {}
        }
    }

    finalize_active_block(&mut active_block, &mut blocks);
    if blocks.is_empty() && !markdown.trim().is_empty() {
        blocks.push(MarkdownBlock::Paragraph(InlineContent {
            text: markdown.to_string(),
            highlights: Vec::new(),
        }));
    }
    blocks
}

fn render_markdown_block(block: MarkdownBlock) -> AnyElement {
    match block {
        MarkdownBlock::Paragraph(content) => div()
            .w_full()
            .min_w(px(0.))
            .text_size(px(14.))
            .text_color(hex(TEXT_PRIMARY))
            .whitespace_normal()
            .child(styled_inline_text(content))
            .into_any_element(),
        MarkdownBlock::Heading { level, content } => {
            let size = match level {
                1 => 24.0,
                2 => 20.0,
                3 => 18.0,
                _ => 16.0,
            };
            div()
                .w_full()
                .min_w(px(0.))
                .text_size(px(size))
                .font_weight(FontWeight::BOLD)
                .text_color(hex(TEXT_PRIMARY))
                .whitespace_normal()
                .child(styled_inline_text(content))
                .into_any_element()
        }
        MarkdownBlock::ListItem { marker, content } => div()
            .w_full()
            .min_w(px(0.))
            .flex()
            .items_start()
            .gap(px(8.))
            .child(
                div()
                    .pt(px(1.))
                    .text_size(px(14.))
                    .text_color(hex(TEXT_SECONDARY))
                    .child(marker),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .text_color(hex(TEXT_PRIMARY))
                    .whitespace_normal()
                    .child(styled_inline_text(content)),
            )
            .into_any_element(),
        MarkdownBlock::CodeBlock { language, code } => {
            let mut block = div()
                .w_full()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .bg(hex(PANEL_BG))
                .border_1()
                .border_color(hex(BORDER))
                .rounded(px(10.))
                .px(px(12.))
                .py(px(10.));
            if let Some(language) = language.filter(|value| !value.is_empty()) {
                block = block.child(
                    div()
                        .text_size(px(10.))
                        .text_color(hex(TEXT_MUTED))
                        .child(language),
                );
            }
            block
                .child(
                    div()
                        .w_full()
                        .min_w(px(0.))
                        .font_family("Cascadia Code")
                        .text_size(px(12.))
                        .text_color(hex(TEXT_PRIMARY))
                        .whitespace_normal()
                        .child(code),
                )
                .into_any_element()
        }
    }
}

fn styled_inline_text(content: InlineContent) -> StyledText {
    let text: SharedString = content.text.into();
    if content.highlights.is_empty() {
        StyledText::new(text)
    } else {
        StyledText::new(text).with_highlights(content.highlights)
    }
}

fn finalize_active_block(active_block: &mut Option<ActiveBlock>, blocks: &mut Vec<MarkdownBlock>) {
    let Some(block) = active_block.take() else {
        return;
    };
    match block {
        ActiveBlock::Paragraph(content) => {
            let content = content.build();
            if !content.text.trim().is_empty() {
                blocks.push(MarkdownBlock::Paragraph(content));
            }
        }
        ActiveBlock::Heading { level, content } => {
            let content = content.build();
            if !content.text.trim().is_empty() {
                blocks.push(MarkdownBlock::Heading { level, content });
            }
        }
        ActiveBlock::ListItem { marker, content } => {
            let content = content.build();
            if !content.text.trim().is_empty() {
                blocks.push(MarkdownBlock::ListItem { marker, content });
            }
        }
        ActiveBlock::CodeBlock { language, code } => {
            if !code.trim().is_empty() {
                blocks.push(MarkdownBlock::CodeBlock { language, code });
            }
        }
    }
}

fn ensure_inline_builder(active_block: &mut Option<ActiveBlock>) -> &mut InlineBuilder {
    if active_block.is_none() {
        *active_block = Some(ActiveBlock::Paragraph(InlineBuilder::default()));
    }
    inline_builder_mut(active_block).expect("inline builder must exist")
}

fn inline_builder_mut(active_block: &mut Option<ActiveBlock>) -> Option<&mut InlineBuilder> {
    match active_block {
        Some(ActiveBlock::Paragraph(content))
        | Some(ActiveBlock::Heading { content, .. })
        | Some(ActiveBlock::ListItem { content, .. }) => Some(content),
        _ => None,
    }
}

fn code_block_mut(active_block: &mut Option<ActiveBlock>) -> Option<&mut String> {
    match active_block {
        Some(ActiveBlock::CodeBlock { code, .. }) => Some(code),
        _ => None,
    }
}

fn heading_level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn code_block_language(kind: CodeBlockKind<'_>) -> Option<String> {
    match kind {
        CodeBlockKind::Indented => None,
        CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .map(|value| value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_markdown_blocks() {
        let blocks = parse_markdown_blocks(
            "# Title\n\n- item one\n- item two\n\n```rust\nfn main() {}\n```",
        );
        assert!(matches!(
            blocks.first(),
            Some(MarkdownBlock::Heading { level: 1, .. })
        ));
        assert!(matches!(
            blocks.get(1),
            Some(MarkdownBlock::ListItem { marker, .. }) if marker == "•"
        ));
        assert!(matches!(
            blocks.get(2),
            Some(MarkdownBlock::ListItem { marker, .. }) if marker == "•"
        ));
        assert!(matches!(
            blocks.get(3),
            Some(MarkdownBlock::CodeBlock { language, .. }) if language.as_deref() == Some("rust")
        ));
    }

    #[test]
    fn captures_inline_highlights() {
        let blocks = parse_markdown_blocks("**bold** _italic_ `code` [link](https://example.com)");
        let MarkdownBlock::Paragraph(content) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(content.text, "bold\nitalic\ncode\nlink".replace('\n', " "));
        assert_eq!(content.highlights.len(), 4);
        assert!(content.highlights[0].1.font_weight.is_some());
        assert!(content.highlights[1].1.font_style.is_some());
        assert!(content.highlights[2].1.background_color.is_some());
        assert!(content.highlights[3].1.underline.is_some());
    }
}
