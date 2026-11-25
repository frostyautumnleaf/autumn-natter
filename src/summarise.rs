// The handoff summary the model may request on its own word.
//
// The space of a conversation is created by a handoff summary: the program asks the model
// for a short account of the work, puts that account in the place of the text it means, and the
// conversation goes on in the space that is created. The program does this on its own when the
// conversation remains past the limit set in the Chatting settings. This tool lets the model
// request the same at a moment of its own choosing: when a large part of the work is finished
// and a new part is about to begin, or when the user requested it in text.
//
// The call runs in the middle of an answer. The answer is laid aside while the summary is
// written, exactly as it is when the space is freed for the weight of the conversation, and it
// is taken up again when the text is in the marker. The block of the call is put into the chat
// after the marker and not before it, so the model reads what the call came to after the
// summary rather than among the text the summary means: a block put before the marker would be
// cut away with the rest of them, and the model would see no answer to its own call.
//
// The tool is under the badge of the tools and under a line of its own in the drop-down of the
// badge, as the other tools of the program do. It remains only while the settings make the
// space by a handoff summary: with compaction off there is no summary to be made, the line
// of the drop-down is hidden on the page, and a call of the name is answered as a tool that is
// switched off.

use serde_json::{json, Value};

use crate::tools;

// The name the model uses to request the space.
pub const NAME: &str = "summarise_chat";

// The tool as the model is informed of it. The call carries nothing but the reason the model
// gives for it, and the reason is nothing the work needs: it is in the block of the run, so the
// user sees why the conversation was cut.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": tools::with_rule(String::from(
                "Request a handoff summary when a substantial phase is complete and the context is long, or when the user asks to summarize or compact the chat. Continue from the summary in the reduced context. Do not call at the start, more than once per answer, or because a step failed.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "why": {
                        "type": "string",
                        "description": "Brief reason for the handoff, or the user's request."
                    }
                },
                "required": []
            }
        }
    })
}

// The reason the model gave with the call, for the header of its block. Nothing returns for a
// call that carries no reason, or for text that is no object at all.
pub fn reason(raw: &str) -> String {
    let text = raw.trim();
    let cut = tools::extract_object(text).unwrap_or(text);
    let Ok(value) = serde_json::from_str::<Value>(cut) else {
        return String::new();
    };
    let said = value.get("why").and_then(Value::as_str).unwrap_or("").trim();
    said.chars().take(200).collect()
}

// The text the model is given back when the summary was written. The two numbers are the size
// of the context before the summary and after it, so the model can see what the call won and
// can judge the next step.
pub fn kept_words(count: usize, before: u64, after: u64) -> String {
    format!(
        "The conversation was put into handoff summary #{}. The size of the context went from {} to {}. The messages the summary replaced are no longer sent to you: read the summary instead, and continue the work from the point the chat reached.",
        count, before, after
    )
}

// The text the model is given back when there was nothing to summarise: the newest item of the
// chat is a summarisation already, so a second one would mean no conversation at all.
pub fn refused_words() -> String {
    String::from(
        "The newest item in this chat is already a summarisation, so nothing new remains that is worth a summary of its own. Nothing was written and the conversation goes on as it was.",
    )
}

// The text the model is given back when the summary was requested and never came. The
// conversation keeps every word it had, so the work is not the worse for the call; the space is
// simply not there.
pub fn failed_words(before: u64) -> String {
    format!(
        "The handoff summary was not written, so the conversation keeps every message it had and its size is still {}. Continue the work in small parts, and leave the context to the program.",
        before
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // The tool is proposed in the form every tool of the program is proposed in, and carries
    // the rule of the tools like every other one of them.
    #[test]
    fn the_tool_is_offered_in_the_shape_of_the_others() {
        let made = definition();
        assert_eq!(made["type"], "function");
        assert_eq!(made["function"]["name"], NAME);
        let said = made["function"]["description"].as_str().unwrap_or("");
        assert!(said.contains("handoff summary"));
        assert!(said.contains("If text alone answers it, do not call a tool"));
        assert_eq!(made["function"]["parameters"]["type"], "object");
    }

    // The reason is extracted from the text of the call, and a call with nothing in it returns
    // nothing rather than a note about its own.
    #[test]
    fn the_reason_of_a_call_is_read_out_of_its_words() {
        assert_eq!(reason(r#"{ "why": "the plan is finished" }"#), "the plan is finished");
        assert_eq!(reason(""), "");
        assert_eq!(reason("no object at all"), "");
        assert_eq!(reason(r#"{ "other": "words" }"#), "");
    }

    // The three results say which of the three items happened, and each of them reports it in
    // text a model can act on.
    #[test]
    fn the_three_receipts_tell_the_model_what_happened() {
        let kept = kept_words(3, 7000, 900);
        assert!(kept.contains("#3"));
        assert!(kept.contains("7000"));
        assert!(kept.contains("900"));
        assert!(refused_words().contains("already"));
        assert!(failed_words(7000).contains("7000"));
    }
}
