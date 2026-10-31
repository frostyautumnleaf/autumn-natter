// Convert markdown text into blocks that the window can render.
// Block-level parts (code, headings, lists) become separate blocks.
//
// The words of a block stay plain. The marks are taken out here, and only the
// kind of the block is kept, so a bold word reads as a plain word and never as
// <b> and </b> around it. The window draws the mark of a block itself: a heading
// gets a short bar of an accent colour, and a list stands behind a rule. The
// address of a link stands after the words of the link, which is how plain text
// shows an address.

/// One block of rendered markdown.
#[derive(Debug, Clone, Default)]
pub struct Block {
    /// "text", "code", "heading", "list"
    pub kind: String,
    /// Heading level (1-6). Zero for other kinds.
    pub level: i32,
    /// The words of the block. Marks are taken out, so this is plain text. For
    /// a code block this holds the code.
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
    // The address of the link that is being read.
    let mut link_url = String::new();

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
            // A list or a quote cannot start inside a paragraph, so the words
            // that came before it are made into a block first. Without this the
            // list would stand before the words that came before it.
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::List(_))
            | pulldown_cmark::Event::Start(pulldown_cmark::Tag::BlockQuote(_)) => {
                flush_text(&mut blocks, &mut current_text);
            }
            // The end of a paragraph is the end of a block of words.
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Paragraph) => {
                flush_text(&mut blocks, &mut current_text);
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
                // An inline piece of code keeps its words and loses the marks
                // around them.
                if in_code {
                    code_content.push_str(&text);
                } else if in_list {
                    current_item.push_str(&text);
                } else {
                    current_text.push_str(&text);
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
            // An italic word or a bold word keeps its words and loses its marks.
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Emphasis)
            | pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Emphasis)
            | pulldown_cmark::Event::Start(pulldown_cmark::Tag::Strong)
            | pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Strong) => {}
            // HTML inside the words is dropped, so no tag can reach the window.
            pulldown_cmark::Event::Html(_) | pulldown_cmark::Event::InlineHtml(_) => {}
            // A link gives its words where they fall. Its address follows them,
            // so the address can be read and typed in.
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                link_url = dest_url.to_string();
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Link) => {
                let url = link_url.trim().to_string();
                if !url.is_empty() {
                    if in_list {
                        if !current_item.ends_with(&url) {
                            current_item.push_str(" (");
                            current_item.push_str(&url);
                            current_item.push(')');
                        }
                    } else if !current_text.ends_with(&url) {
                        current_text.push_str(" (");
                        current_text.push_str(&url);
                        current_text.push(')');
                    }
                }
                link_url.clear();
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

// ---------------------------------------------------------------------------
// Chat template analysis for thinking capabilities.
// ---------------------------------------------------------------------------

/// The thinking modes a chat template supports.
#[derive(Debug, Clone, Default)]
pub struct ThinkingOptions {
    /// True if the template can switch thinking on and off.
    pub has_thinking: bool,
    /// True if the template takes an effort level.
    pub has_effort: bool,
    /// The effort levels the template names, in display order.
    pub efforts: Vec<String>,
    /// True if the template can keep the thinking of the turns before the last
    /// answer, so the model may work on its own thoughts again.
    pub has_preserve: bool,
}

/// The words a template reads when it is told to keep or to drop the thinking
/// of the earlier turns. llama.cpp sets all of them from the one switch, so a
/// template that names any of them offers the choice.
pub const PRESERVE_KEYS: [&str; 4] = [
    "preserve_thinking",
    "clear_thinking",
    "truncate_history_thinking",
    "drop_thinking",
];

impl ThinkingOptions {
    /// The modes the badge of the chat offers, in the order they are listed.
    /// There is no "auto": the user chooses, and thinking is off unless it
    /// is asked for. An empty list means the model has no thinking mode.
    pub fn modes(&self) -> Vec<&'static str> {
        if self.has_effort {
            let all: &[&str] = &["low", "medium", "high", "xhigh"];
            let levels: Vec<&str> = if self.efforts.is_empty() {
                all.to_vec()
            } else {
                all.iter()
                    .copied()
                    .filter(|level| self.efforts.iter().any(|named| named == *level))
                    .collect()
            };
            let mut list = vec!["off"];
            list.extend(levels);
            return list;
        }
        if self.has_thinking {
            return vec!["off", "on"];
        }
        Vec::new()
    }
}

/// Parse a jinja chat template and detect its thinking capabilities.
pub fn analyze_template(template: &str) -> ThinkingOptions {
    let mut options = ThinkingOptions::default();
    if template.trim().is_empty() {
        return options;
    }
    // The on and off switch is the enable_thinking keyword.
    options.has_thinking = template.contains("enable_thinking");
    // The effort level comes in through the reasoning_effort keyword.
    options.has_effort = template.contains("reasoning_effort");
    // The choice to keep the thinking of the earlier turns is taken from the
    // same look at the template: one of the words for it stands in the template
    // when the model can do the thing.
    options.has_preserve = PRESERVE_KEYS.iter().any(|key| template.contains(key));
    if options.has_effort {
        // A level counts when the template compares the effort against it.
        // The comparison quotes the level, so a word like "following" does
        // not count as "low".
        for level in ["low", "medium", "high", "xhigh"] {
            if template.contains(&format!("'{}'", level))
                || template.contains(&format!("\"{}\"", level))
            {
                options.efforts.push(level.to_string());
            }
        }
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    // The words of the first text block of a one-block answer.
    fn words(markdown: &str) -> String {
        let blocks = parse(markdown);
        assert_eq!(blocks.len(), 1, "one block was expected for: {markdown}");
        blocks[0].content.clone()
    }

    #[test]
    fn bold_and_italic_words_keep_their_words() {
        assert_eq!(
            words("Take the **Wellington boots** and a *bag* for the litter."),
            "Take the Wellington boots and a bag for the litter."
        );
    }

    #[test]
    fn code_in_a_sentence_keeps_its_words() {
        assert_eq!(
            words("Run `ls -l west-beds` first."),
            "Run ls -l west-beds first."
        );
    }

    #[test]
    fn a_link_shows_its_address_after_the_words() {
        assert_eq!(
            words("Read [the field manual](https://example.com/manual) today."),
            "Read the field manual (https://example.com/manual) today."
        );
    }

    #[test]
    fn a_link_of_only_an_address_shows_the_address_once() {
        assert_eq!(
            words("See https://example.com/log for the run."),
            "See https://example.com/log for the run."
        );
    }

    #[test]
    fn no_tag_reaches_the_window() {
        let mixed = "# Head\n\nA **bold** word, a [link](https://example.com/a), and `code`.\n\n- one\n- two\n\n<b>raw</b> text\n";
        for block in parse(mixed) {
            assert!(
                !block.content.contains('<') && !block.content.contains('>'),
                "a tag reached the words: {}",
                block.content
            );
            assert!(
                !block.content.contains("**"),
                "marks reached the words: {}",
                block.content
            );
        }
    }

    #[test]
    fn an_empty_template_has_no_modes() {
        let options = analyze_template("");
        assert!(!options.has_thinking);
        assert!(!options.has_effort);
        assert!(options.modes().is_empty());
    }

    #[test]
    fn a_template_with_only_the_switch() {
        let template = "{%- if enable_thinking %}A< /think>{% endif %}B";
        let options = analyze_template(template);
        assert!(options.has_thinking);
        assert!(!options.has_effort);
        assert_eq!(options.modes(), vec!["off", "on"]);
    }

    #[test]
    fn a_template_with_quoted_effort_levels() {
        let template =
            "{%- if reasoning_effort == 'high' %}A{% elif reasoning_effort == 'low' %}B{% endif %}";
        let options = analyze_template(template);
        assert!(options.has_effort);
        assert_eq!(options.efforts, vec!["low".to_string(), "high".to_string()]);
        assert_eq!(options.modes(), vec!["off", "low", "high"]);
    }

    #[test]
    fn a_template_with_no_quoted_levels_gets_all_levels() {
        let template = "{{- reasoning_effort }}";
        let options = analyze_template(template);
        assert!(options.has_effort);
        assert!(options.efforts.is_empty());
        assert_eq!(
            options.modes(),
            vec!["off", "low", "medium", "high", "xhigh"]
        );
    }

    #[test]
    fn a_word_is_not_a_level() {
        // The word "following" holds "low", but it is not a quoted level.
        let template = "If you choose to call a function ONLY reply in the following format. {{- reasoning_effort }}";
        let options = analyze_template(template);
        assert!(options.has_effort);
        assert!(options.efforts.is_empty());
    }

    // A template that reads the word for keeping the thinking of the earlier
    // turns offers the choice, and one that does not read it does not.
    #[test]
    fn the_choice_to_keep_the_thinking_of_history_is_seen() {
        let template = "{%- if preserve_thinking is true %}A{% endif %}{{ enable_thinking }}";
        let options = analyze_template(template);
        assert!(options.has_preserve);
        assert!(options.has_thinking);
        // The choice is not a mode, so the list of modes stays the two.
        assert_eq!(options.modes(), vec!["off", "on"]);

        let plain = "{%- if enable_thinking %}A{% endif %}B";
        let options = analyze_template(plain);
        assert!(options.has_thinking);
        assert!(!options.has_preserve);

        // The other words for the same switch count as well.
        let cleared = "{{ clear_thinking }}{{ truncate_history_thinking }}{{ drop_thinking }}";
        assert!(analyze_template(cleared).has_preserve);
    }

    #[test]
    fn the_blocks_keep_the_order_they_were_written_in() {
        // A list that was written after a paragraph must not move in front of
        // it, and the same for the block of code after the list.
        let answer = "## Ways\n\nOne command does the work.\n\n- one file\n- many files\n\n```rust\nlet n = 1;\n```\n\nThe read is one call.";
        let blocks = parse(answer);
        let kinds: Vec<&str> = blocks.iter().map(|one| one.kind.as_str()).collect();
        assert_eq!(kinds, vec!["heading", "text", "list", "code", "text"]);
        assert_eq!(blocks[1].content, "One command does the work.");
        assert!(blocks[2].content.starts_with("1. one file"));
        assert_eq!(blocks[4].content, "The read is one call.");
    }
}
