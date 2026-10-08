// Convert markdown text into blocks that the Slint UI can render.
// Slint's Text element supports inline HTML: <b>, <i>, <u>, <s>, <font>, <a>.
// Block-level elements (code, headings, lists) become separate blocks.

/// One block of rendered markdown.
#[derive(Debug, Clone, Default)]
pub struct Block {
    /// "text", "code", "heading", "list"
    pub kind: String,
    /// Heading level (1-6). Zero for other kinds.
    pub level: i32,
    /// The content. For "text" and "heading" this has inline HTML markup.
    pub content: String,
}

/// Parse markdown into a list of blocks.
pub fn parse(markdown: &str) -> Vec<Block> {
    let parser = pulldown_cmark::Parser::new(markdown);
    let mut blocks: Vec<Block> = Vec::new();
    let mut current_text = String::new();
    let mut current_heading_level: i32 = 0;
    let mut in_code = false;
    let mut code_content = String::new();
    let mut in_list = false;
    let mut list_items: Vec<String> = Vec::new();
    let mut current_item = String::new();

    fn flush_text(blocks: &mut Vec<Block>, text: &mut String) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            blocks.push(Block {
                kind: "text".to_string(),
                level: 0,
                content: trimmed.to_string(),
            });
        }
        text.clear();
    }

    fn flush_list(blocks: &mut Vec<Block>, items: &mut Vec<String>) {
        if items.is_empty() {
            return;
        }
        let content = items
            .iter()
            .enumerate()
            .map(|(i, item)| format!("{}. {}", i + 1, item))
            .collect::<Vec<_>>()
            .join("\n");
        blocks.push(Block {
            kind: "list".to_string(),
            level: 0,
            content,
        });
        items.clear();
    }

    for event in parser {
        match event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_)) => {
                flush_text(&mut blocks, &mut current_text);
                in_code = true;
                code_content.clear();
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::CodeBlock) => {
                in_code = false;
                blocks.push(Block {
                    kind: "code".to_string(),
                    level: 0,
                    content: code_content.trim_end().to_string(),
                });
            }
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Heading { level, .. }) => {
                flush_text(&mut blocks, &mut current_text);
                current_heading_level = level as u8 as i32;
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Heading(_)) => {
                let text = current_text.trim().to_string();
                current_text.clear();
                blocks.push(Block {
                    kind: "heading".to_string(),
                    level: current_heading_level,
                    content: text,
                });
                current_heading_level = 0;
            }
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Item) => {
                in_list = true;
                current_item.clear();
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Item) => {
                if in_list {
                    let item = current_item.trim().to_string();
                    if !item.is_empty() {
                        list_items.push(item);
                    }
                    current_item.clear();
                }
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::List(_)) => {
                in_list = false;
                flush_list(&mut blocks, &mut list_items);
            }
            pulldown_cmark::Event::Text(text) => {
                if in_code {
                    code_content.push_str(&text);
                } else if in_list {
                    current_item.push_str(&text);
                } else {
                    current_text.push_str(&text);
                }
            }
            pulldown_cmark::Event::Code(text) => {
                if in_code {
                    code_content.push_str(&text);
                } else {
                    current_text.push_str("<b>");
                    current_text.push_str(&escape_html(&text));
                    current_text.push_str("</b>");
                }
            }
            pulldown_cmark::Event::SoftBreak | pulldown_cmark::Event::HardBreak => {
                if in_code {
                    code_content.push('\n');
                } else if in_list {
                    current_item.push('\n');
                } else {
                    current_text.push('\n');
                }
            }
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Emphasis) => {
                if !in_code {
                    if in_list {
                        current_item.push_str("<i>");
                    } else {
                        current_text.push_str("<i>");
                    }
                }
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Emphasis) => {
                if !in_code {
                    if in_list {
                        current_item.push_str("</i>");
                    } else {
                        current_text.push_str("</i>");
                    }
                }
            }
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Strong) => {
                if !in_code {
                    if in_list {
                        current_item.push_str("<b>");
                    } else {
                        current_text.push_str("<b>");
                    }
                }
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Strong) => {
                if !in_code {
                    if in_list {
                        current_item.push_str("</b>");
                    } else {
                        current_text.push_str("</b>");
                    }
                }
            }
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                if !in_code {
                    let url = dest_url.to_string();
                    let tag = format!("<a href=\"{}\">", escape_html(&url));
                    if in_list {
                        current_item.push_str(&tag);
                    } else {
                        current_text.push_str(&tag);
                    }
                }
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Link) => {
                if !in_code {
                    if in_list {
                        current_item.push_str("</a>");
                    } else {
                        current_text.push_str("</a>");
                    }
                }
            }
            _ => {}
        }
    }

    // Flush any remaining content.
    if in_list {
        flush_list(&mut blocks, &mut list_items);
    }
    flush_text(&mut blocks, &mut current_text);

    // If nothing was parsed (plain text with no markdown), make one block.
    if blocks.is_empty() && !markdown.trim().is_empty() {
        blocks.push(Block {
            kind: "text".to_string(),
            level: 0,
            content: markdown.trim().to_string(),
        });
    }

    blocks
}

/// Escape HTML special characters for safe inclusion in Slint markup.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
