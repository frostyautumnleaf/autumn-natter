// The handoff summary the model may ask for on its own word.
//
// The room of a talk is made by a handoff summary: the program asks the model
// for a short account of the work, puts that account in the place of the words
// it stands for, and the talk goes on in the room that is made. The program
// does this on its own when the talk stands past the limit set in the Chatting
// settings. This tool lets the model ask for the same thing at a moment of its
// own choosing: when a large part of the work is finished and a new part is
// about to begin, or when the user asked for it in words.
//
// The call runs in the middle of an answer. The answer is laid aside while the
// summary is written, exactly as it is when the room is made for the weight of
// the talk, and it is taken up again when the words are in the mark. The block
// of the call is put into the chat after the mark and not before it, so the
// model reads what the call came to after the summary rather than among the
// words the summary stands for: a block put before the mark would be cut away
// with the rest of them, and the model would see no answer to its own call.
//
// The tool stands under the badge of the tools and under a line of its own in
// the drop-down of the badge, as the other tools of the program do. It stands
// only while the settings make the room by a handoff summary: with compaction
// off there is no summary to be made, the line of the drop-down is hidden on
// the page, and a call of the name is answered as a tool that is switched off.

use serde_json::{json, Value};

use crate::tools;

// The name the model uses to ask for the room.
pub const NAME: &str = "summarise_chat";

// The tool as the model is told of it. The call carries nothing but the reason
// the model gives for it, and the reason is nothing the work needs: it stands
// in the block of the run, so the user sees why the talk was cut.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": tools::with_rule(String::from(
                "Put the talk of this chat into a handoff summary now, and go on with the work in the room that is made. The program writes the summary, keeps it in the chat where the talk was cut, and sends you the words it stands for no more: you read the summary in their place. Nothing of the work is lost: the files stand where they were written, and every word the user read stands in the chat. Call it when a large part of the work is finished and the talk has grown long, or when the user asks you to summarise, compact or clear the chat. Do not call it at the start of a talk, do not call it twice in one answer, and do not call it because a step went wrong.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "why": {
                        "type": "string",
                        "description": "The reason the room is wanted now, in a few words, or the words of the user that asked for it."
                    }
                },
                "required": []
            }
        }
    })
}

// The reason the model gave with the call, for the head of its block. Nothing
// comes back for a call that carries no reason, or for words that are no
// object at all.
pub fn reason(raw: &str) -> String {
    let text = raw.trim();
    let cut = tools::extract_object(text).unwrap_or(text);
    let Ok(value) = serde_json::from_str::<Value>(cut) else {
        return String::new();
    };
    let said = value.get("why").and_then(Value::as_str).unwrap_or("").trim();
    said.chars().take(200).collect()
}

// The words the model is given back when the summary was written. The two
// numbers are the room of the talk before the summary and after it, so the
// model can see what the call won and can judge the next step.
pub fn kept_words(count: usize, before: u64, after: u64) -> String {
    format!(
        "The talk was put into handoff summary #{}. The room of the talk went from {} to {}. The words the summary stands for are sent to you no more: read the summary in their place, and go on with the work from the point it reached.",
        count, before, after
    )
}

// The words the model is given back when there was nothing to summarise: the
// newest thing of the chat is a summarisation already, so a second one would
// stand for no talk at all.
pub fn refused_words() -> String {
    String::from(
        "The newest thing of this chat is a summarisation already, so nothing new stands that is worth a summary of its own. Nothing was written and the talk goes on as it stood.",
    )
}

// The words the model is given back when the summary was asked for and never
// came. The talk keeps every word it had, so the work is not the worse for the
// call; the room is simply not there.
pub fn failed_words(before: u64) -> String {
    format!(
        "The handoff summary was not written, so the talk keeps every word it had and the room of it still stands at {}. Go on with the work in small parts, and leave the room to the program.",
        before
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // The tool is offered in the shape every tool of the program is offered in,
    // and carries the rule of the tools like every other one of them.
    #[test]
    fn the_tool_is_offered_in_the_shape_of_the_others() {
        let made = definition();
        assert_eq!(made["type"], "function");
        assert_eq!(made["function"]["name"], NAME);
        let said = made["function"]["description"].as_str().unwrap_or("");
        assert!(said.contains("handoff summary"));
        assert!(said.contains("Never call a tool to fill a turn"));
        assert_eq!(made["function"]["parameters"]["type"], "object");
    }

    // The reason is read out of the words of the call, and a call with nothing
    // in it gives back nothing rather than a word of its own.
    #[test]
    fn the_reason_of_a_call_is_read_out_of_its_words() {
        assert_eq!(reason(r#"{ "why": "the plan is finished" }"#), "the plan is finished");
        assert_eq!(reason(""), "");
        assert_eq!(reason("no object at all"), "");
        assert_eq!(reason(r#"{ "other": "words" }"#), "");
    }

    // The three receipts say which of the three things happened, and each of
    // them says it in words a model can act on.
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
