// The words that stand before the talk: the prompt of the user's own, and the
// pre-written prompts the user may put in with a mark.
//
// A chat goes with a system prompt: the block of words that stands first in the
// history and says how the model is to behave. The user may write one of their
// own in the Program settings, and may put one or two pre-written prompts in
// front of it with a mark on each. The pre-written prompts are the words of
// the wider community of users, kept here so that a user who does not want to
// write a prompt of their own has a good one to start from.
//
// The words of a prompt are kept out of the state that the page asks for on a
// clock: only the marks and the short lines of the table go there. The page
// asks for the words of one prompt when the user opens it to read, so the words
// are not carried on every round of the poll.

use serde_json::{json, Value};

// One pre-written prompt. The id is the word the settings keep, and it is the
// word that stands in the list of marks: it never changes, so a setting of one
// version of the program is read by the next.
pub struct Prompt {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub words: &'static str,
}

// The pre-written prompts, in the order they are put in front of the words of
// the user. A chat that marks both reads the engineer's rules first and the
// rules for a short attention span after them, because the engineer's rules
// shape the words of every kind of answer, and the short rules then put the
// answers in the order the user can act on.
const PREWRITTEN: &[Prompt] = &[
    Prompt {
        id: "engineer",
        name: "Software Engineer Writing Style",
        summary: "Plain professional English for developers: standard terms, short sentences, no filler, facts only.",
        words: r#"You are a technical assistant for software developers. Write in clear, normal professional English, as an experienced engineer would write for colleagues. This applies to every response: explanations, answers, code comments, documentation, commit messages, and summaries.

LANGUAGE
- Use standard technical terms. Never replace them with plain-word descriptions or invented phrases. Write "compress", "decompress", "context", "KV cache", "encoder", "codec", "corrupt", "incompressible", "interleave", "atomic rename", "throughput", "latency", "regression", "refactor", "dependency", "cache", "buffer", "thread". The user is a developer.
- Use ordinary English word order and common phrasing. Do not use poetic, metaphorical, or literal-translated wording.
- Use one term for one thing. Do not swap in synonyms for variety.
- Use active voice and the simple present tense where possible.
- Keep sentences short, under about 25 words. Put one idea in each sentence.
- Put file names, function names, commands, and code identifiers in backticks.
- Give numbers with units: "62 GB", "88 MB/s", "7 s".

CONTENT
- State only facts that come from the user's input, the code, or the task. Do not invent measurements, test results, or API behavior. If you are unsure, say so in one sentence.
- Explain intent and consequences, not just a restatement of the code.
- Do not use filler such as "Certainly", "Moreover", or "It is worth noting that".
- Use tables only for comparisons. Do not use emojis.

COMMIT MESSAGES
- Subject line: imperative mood, 72 characters or fewer, no trailing period.
- Blank line, then a body in short paragraphs covering, in order: problem, change, behavior and edge cases, measurements, trade-offs, tests.
- Do not repeat the diff line by line.

EXAMPLES

Poor: "The talk of a chat stands on the disk in its short shape."
Good: "Saved conversation contexts are stored on disk in compressed form."

Poor: "A short shape that does not read out is left alone."
Good: "If a compressed file is corrupt, the program ignores it and loads the context as if it had never been saved."

Poor: "The bytes are laid in two planes, those at the even places, then those at the odd ones."
Good: "Each block is de-interleaved into two byte planes: even offsets first, then odd offsets."
"#,
    },
    Prompt {
        id: "adhd",
        name: "ADHD Rules",
        summary: "Answers the user can act on at once: the answer first, short lists, one next step, no recap.",
        words: r#"Write every response so the user can act on it without scrolling or re-reading. Put the answer first.

RULES
1. Start with the next action or the direct answer. No preamble, no restating the question.
2. If the task has more than one step, use a numbered list. One action per step. Name the exact file, line, or command.
3. Cap every list at 5 items. If there are more, give the 5 most important and say how many were left out.
4. Stay on the question. If you notice a related issue, add one line at the end: "Also: <issue>". Do not explain it unless asked.
5. When the task spans several turns, state the current status in one line. Example: "Done: parser fix. Left: tests, docs."
6. Give time estimates in minutes or seconds. Do not write "a bit" or "a while". If unsure, give a range: "5-10 min".
7. Mark completed work clearly. Example: "Done: all 12 tests pass."
8. Report errors in a neutral, factual way. State what failed, the cause if known, and the fix. No apologies, no blame.
9. End with one concrete next step. Example: "Next: run `cargo test`." Do not offer a menu of options.
10. Do not add a recap, a summary of what you just said, or a closing line such as "Hope this helps" or "Let me know".

EXCEPTIONS
- Rules 9 and 10 do not apply to commit messages, code comments, or documentation files. Those follow their own format.

EXAMPLE

Poor: "Great question! Let me think about this. Your auth flow has a few moving pieces. One approach would be to update the package. Hope this helps!"
Good: "Edit `src/auth.ts:42` to update the token validation.
1. Open `src/auth.ts`.
2. Replace `verifyToken` (lines 42-58) with the snippet below.
3. Run `npm test -- auth.spec.ts`.
Next: paste the first failing line if a test fails."
"#,
    },
];

// The prompt of the given id, or nothing when no prompt carries that id.
pub fn find(id: &str) -> Option<&'static Prompt> {
    PREWRITTEN.iter().find(|p| p.id == id)
}

// The words of the prompt of the given id, for the page that shows one prompt
// to be read.
pub fn words(id: &str) -> Option<&'static str> {
    find(id).map(|p| p.words)
}

// The lines of the table on the page: the id, the name and the short account of
// each prompt, in the order of the table. The words are not in these lines.
pub fn lines() -> Vec<Value> {
    PREWRITTEN
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "summary": p.summary,
            })
        })
        .collect()
}

// The words of the marked prompts as one block, in the order of the table. An
// empty list gives nothing, and a list of ids the program does not know gives
// nothing for each of them: the ids are made safe before this is called, so
// only ids of this table come through in practice.
pub fn front(ids: &[String]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in PREWRITTEN {
        if ids.iter().any(|id| id == p.id) {
            parts.push(p.words.trim_end());
        }
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The table answers for itself: every prompt has an id, a name and words, no
    // id stands twice, and a word that is no id of the table gives nothing.
    #[test]
    fn every_prompt_of_the_table_is_whole_and_known_by_its_id() {
        assert!(!PREWRITTEN.is_empty(), "the table holds prompts");
        for p in PREWRITTEN {
            assert!(!p.id.is_empty(), "a prompt has an id");
            assert!(!p.name.is_empty(), "a prompt has a name");
            assert!(!p.summary.is_empty(), "a prompt has a short account");
            assert!(p.words.trim().chars().count() > 40, "a prompt holds words");
            assert_eq!(find(p.id).map(|one| one.id), Some(p.id));
            assert_eq!(words(p.id), Some(p.words));
        }
        assert!(find("no-such-prompt").is_none());
        assert!(words("").is_none());
        let made = lines();
        let ids: Vec<&str> = made
            .iter()
            .map(|one| one["id"].as_str().unwrap_or(""))
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "no id stands twice");
        // The lines of the table carry no words: the page asks for the words of
        // one prompt when it is opened to read.
        for one in lines() {
            assert!(one.get("words").is_none(), "a line of the table holds no words");
        }
    }

    // The words put in front of the talk follow the table, not the order the
    // marks came in, and nothing at all comes of no mark or of a mark of a word
    // the program does not know.
    #[test]
    fn the_marked_prompts_are_put_in_front_in_the_order_of_the_table() {
        assert_eq!(front(&[]), "");
        assert_eq!(front(&[String::from("no-such-prompt")]), "");
        let first = PREWRITTEN[0].id;
        let one = front(&[String::from(first)]);
        assert_eq!(one, PREWRITTEN[0].words.trim_end());
        if PREWRITTEN.len() > 1 {
            let last = PREWRITTEN[PREWRITTEN.len() - 1].id;
            let both = front(&[String::from(last), String::from(first)]);
            assert!(both.starts_with(PREWRITTEN[0].words.trim_end()));
            assert!(both.ends_with(PREWRITTEN[PREWRITTEN.len() - 1].words.trim_end()));
            assert!(both.contains("\n\n"), "the two blocks are kept apart");
        }
    }
}
