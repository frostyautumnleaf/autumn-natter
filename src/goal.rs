// A goal belongs to one chat until it is verified complete. The command starts or resumes it;
// the completion and pause tools are the only ways the model can end its automatic run.

use serde_json::{json, Value};

use crate::tools::{self, Report};

fn no(output: impl Into<String>) -> Report {
    Report {
        ok: false,
        output: output.into(),
    }
}

fn yes(output: impl Into<String>) -> Report {
    Report {
        ok: true,
        output: output.into(),
    }
}

pub const COMMAND_WORD: &str = "/goal";
pub const COMMAND_WORDS: &str = "Pursue a goal through the planning, work and checks it needs. The model keeps going until it verifies completion; use /goal stop to end it early.";
pub const CRITERIA_NAME: &str = "set_goal_criteria";
pub const COMPLETE_NAME: &str = "complete_goal";
pub const PAUSE_NAME: &str = "pause_goal";

// The persistent instruction sent with every model request while the goal remains active.
pub fn system_prompt(goal: &str, criteria: &[String], paused: bool) -> String {
    let criteria = if criteria.is_empty() {
        String::from("No acceptance criteria are registered yet.")
    } else {
        format!(
            "Registered acceptance criteria (all must pass):\n{}",
            criteria
                .iter()
                .enumerate()
                .map(|(index, criterion)| format!("{}. {}", index + 1, criterion))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    if paused {
        return format!(
            "GOAL PAUSED — waiting for the user\n\nGoal: {goal}\n{criteria}\n\nThe work is paused because essential user input is needed. Ask the question recorded by the pause tool, or explain the blocker briefly. Do not continue acting or claim completion until the user replies."
        );
    }

    format!(
        "ACTIVE GOAL — pursue this to verified completion\n\nGoal: {goal}\n{criteria}\n\nKeep this goal active across messages, tool results, errors and handoff summaries. A normal answer that sounds finished does not complete it. Continue working until the completion tool accepts a verified report, unless essential user input is required or the user explicitly sends /goal stop.\n\
         \n\
         Work to the user's actual request and constraints. Before changing anything, call {criteria_tool} to register concise, task-appropriate acceptance criteria and a measurable finish line. Scale the criteria and method to the task: a simple creative task may need only one criterion and an editorial review; do not invent code tests or unnecessary steps. For a nontrivial technical task, inspect the current state, make an ordered plan, implement the needed changes, run relevant checks, inspect their results and the final changes, then repair and rerun failures until every criterion passes. Research goals need sourced, current evidence when available. Use available tools when they materially help, and never say a check passed unless its result supports that claim.\n\
         \n\
         Preserve every registered criterion and progress in handoff summaries. Add new criteria when the user's clarification adds requirements; never remove or weaken one already registered. If the user explicitly replaces the objective, they can start a new one with /goal. If a tool fails, diagnose it and try a sound alternative. Do not abandon unfinished criteria or call the goal complete because the context is short, a step limit was reached, or work is difficult. If a required decision, permission, or fact is unavailable and you cannot make safe progress without it, use the pause tool with one specific question; otherwise keep working.\n\
         \n\
         Call the completion tool only after every registered criterion is met. Include the exact goal, a concise result, and exactly one passing evidence item for every registered criterion. Evidence must describe an actual observable check or result. A failed or unverified criterion means the goal is still active; continue the work instead. Once completion is accepted, give the user a concise final report.",
        criteria_tool = CRITERIA_NAME,
    )
}

// The user-role continuation placed after an ordinary model answer that did not complete the
// goal. It is transient and is not written into the visible transcript.
pub fn continue_prompt(goal: &str) -> String {
    format!(
        "[Continue pursuing the active goal: {goal}. Your previous message did not complete it. Work on the remaining acceptance criteria now; do not repeat that message or claim completion without passing evidence for every relevant criterion. Use the available tools when they help.]"
    )
}

// Extra summary instructions keep the objective and its unfinished criteria intact when the
// conversation is compacted.
pub fn handoff_prompt(prompt: &str, goal: &str, criteria: &[String], paused: bool) -> String {
    format!(
        "{prompt}\n\nACTIVE GOAL — NOT COMPLETE\nGoal: {goal}\nRegistered criteria: {}\nState: {}\nPreserve the exact objective, user constraints, every acceptance criterion, completed work, actual check results, failures still being repaired, and the next concrete step. Do not describe the goal as complete unless it was explicitly verified and accepted by the completion tool.",
        if criteria.is_empty() { String::from("none yet") } else { criteria.join("; ") },
        if paused { "paused while waiting for user input" } else { "in progress" }
    )
}

// Tools only offered while a goal is active.
pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": CRITERIA_NAME,
                "description": tools::with_rule(String::from(
                    "Before changing anything for an active goal, register concise, task-appropriate acceptance criteria. Include every requirement the final result must satisfy. Add criteria for new user requirements, but preserve all criteria already registered.",
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "goal": { "type": "string", "description": "The active goal copied exactly." },
                        "criteria": {
                            "type": "array",
                            "description": "Nonempty, observable finish criteria. Use a short list for complex work and only the relevant concise criterion or criteria for a simple creative task.",
                            "items": { "type": "string", "minLength": 1 }
                        }
                    },
                    "required": ["goal", "criteria"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": COMPLETE_NAME,
                "description": tools::with_rule(String::from(
                    "Close the active /goal only after all relevant acceptance criteria are met and verified. Include the exact goal and evidence for each criterion. This records completion; it does not perform the work. Do not call for partial, failed, or unverified work.",
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "goal": { "type": "string", "description": "The active goal copied exactly." },
                        "summary": { "type": "string", "description": "Concise description of the completed result." },
                        "checks": {
                            "type": "array",
                            "description": "One passing, task-appropriate evidence item for each acceptance criterion. For subjective work, describe the review against the stated constraints; technical tasks should report actual relevant checks.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "criterion": { "type": "string", "description": "A measurable or otherwise observable acceptance criterion." },
                                    "passed": { "type": "boolean", "description": "True only when the evidence confirms this criterion." },
                                    "evidence": { "type": "string", "description": "The actual result, observation, or review supporting completion." }
                                },
                                "required": ["criterion", "passed", "evidence"]
                            }
                        }
                    },
                    "required": ["goal", "summary", "checks"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": PAUSE_NAME,
                "description": tools::with_rule(String::from(
                    "Pause an active goal only when one essential user decision, permission, or fact blocks all safe progress. Ask one specific question and state why it is needed. The goal remains active and resumes when the user replies. Do not use this for ordinary uncertainty, tool failures, or work you can investigate yourself.",
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "reason": { "type": "string", "description": "Why no safe progress is possible without user input." },
                        "question": { "type": "string", "description": "One concise, necessary question for the user." }
                    },
                    "required": ["reason", "question"]
                }
            }
        }),
    ]
}

// Parse a completion report and reject anything that does not document every criterion as
// passed with nonempty evidence. Completion is only committed after this succeeds.
pub fn set_criteria(
    raw: &str,
    active_goal: &str,
    existing: &[String],
) -> Result<Vec<String>, String> {
    let parsed =
        tools::extract_object(raw).and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(value) = parsed else {
        return Err(String::from("The acceptance criteria were not valid JSON. Register concise criteria for the active goal before making changes."));
    };
    let goal = value
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if goal != active_goal.trim() {
        return Err(String::from(
            "The criteria do not name the exact active goal. Nothing was changed.",
        ));
    }
    let Some(items) = value.get("criteria").and_then(Value::as_array) else {
        return Err(String::from(
            "Provide one or more concise acceptance criteria.",
        ));
    };
    if items.is_empty() || items.len() > 50 {
        return Err(String::from(
            "Provide between 1 and 50 task-appropriate acceptance criteria.",
        ));
    }
    let mut criteria = Vec::with_capacity(items.len());
    let mut total_chars = 0usize;
    for item in items {
        let Some(text) = item.as_str().map(str::trim).filter(|text| !text.is_empty()) else {
            return Err(String::from(
                "Every acceptance criterion must be nonempty text.",
            ));
        };
        let chars = text.chars().count();
        if chars > 500 || total_chars + chars > 16_000 {
            return Err(String::from(
                "Keep each acceptance criterion under 500 characters and the full list under 16,000 characters.",
            ));
        }
        total_chars += chars;
        if criteria.iter().any(|one| one == text) {
            return Err(String::from("Acceptance criteria must be unique."));
        }
        criteria.push(String::from(text));
    }
    if existing.iter().any(|old| !criteria.contains(old)) {
        return Err(String::from("Registered criteria cannot be removed or weakened. Preserve each existing criterion and add any new user requirements."));
    }
    Ok(criteria)
}

pub fn completion(raw: &str, active_goal: &str, criteria: &[String]) -> Report {
    let parsed =
        tools::extract_object(raw).and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(value) = parsed else {
        return no("The completion report was not valid JSON. The goal remains active; provide the exact goal and verified checks.");
    };
    let goal = value
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if goal != active_goal.trim() {
        return no("The reported goal does not exactly match the active goal. Nothing was completed; continue with the active goal.");
    }
    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if summary.is_empty() {
        return no(
            "The completion report needs a concise result summary. The goal remains active.",
        );
    }
    if summary.chars().count() > 2_000 {
        return no("Keep the completion summary under 2,000 characters. The goal remains active.");
    }
    let Some(checks) = value.get("checks").and_then(Value::as_array) else {
        return no("The completion report needs passing evidence for each acceptance criterion. The goal remains active.");
    };
    if criteria.is_empty() {
        return no("Acceptance criteria have not been registered. Register them before completing the goal.");
    }
    if checks.len() != criteria.len() {
        return no("The completion report must contain exactly one evidence check for every registered criterion. The goal remains active.");
    }
    let mut evidence = Vec::with_capacity(checks.len());
    let mut reported = Vec::with_capacity(checks.len());
    for (index, check) in checks.iter().enumerate() {
        let criterion = check
            .get("criterion")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let proof = check
            .get("evidence")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let passed = check
            .get("passed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if criterion.is_empty() || proof.is_empty() || !passed {
            return no(format!(
                "Completion check {} is missing a criterion or evidence, or is not marked passed. The goal remains active; resolve and verify it before closing the goal.",
                index + 1
            ));
        }
        if proof.chars().count() > 2_000 {
            return no(format!(
                "Completion evidence {} is too long. Keep each evidence item under 2,000 characters and continue the goal.",
                index + 1
            ));
        }
        if !criteria.iter().any(|registered| registered == criterion)
            || reported.iter().any(|one| one == criterion)
        {
            return no(format!(
                "Completion check {} does not match one unique registered criterion. The goal remains active.",
                index + 1
            ));
        }
        reported.push(String::from(criterion));
        evidence.push(format!("- {criterion}: {proof}"));
    }
    if criteria
        .iter()
        .any(|criterion| !reported.contains(criterion))
    {
        return no("At least one registered criterion has no evidence. The goal remains active.");
    }
    yes(format!(
        "Goal completion accepted. Result: {summary}\nVerified checks:\n{}\nNow give the user a concise final report grounded in this evidence.",
        evidence.join("\n")
    ))
}

// A blocked goal remains stored but stops automatic continuation until the user responds.
pub fn pause(raw: &str) -> Report {
    let parsed =
        tools::extract_object(raw).and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(value) = parsed else {
        return no("The pause request was not valid JSON. Continue the goal or provide a valid reason and question.");
    };
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let question = value
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if reason.is_empty() || question.is_empty() {
        return no(
            "A pause needs a specific blocker and a concise question. The goal remains active.",
        );
    }
    if reason.chars().count() > 2_000 || question.chars().count() > 1_000 {
        return no("Keep the pause reason under 2,000 characters and the question under 1,000. The goal remains active.");
    }
    yes(format!(
        "The goal is paused, not complete. The user must answer this question before work can safely continue: {question}\nReason: {reason}\nAsk the user this question plainly, then wait."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requires_exact_goal_and_passing_evidence() {
        let active = "Create and verify the parser";
        let criteria = vec![String::from("Valid syntax")];
        let valid = r#"{"goal":"Create and verify the parser","summary":"Parser added","checks":[{"criterion":"Valid syntax","passed":true,"evidence":"Compilation succeeded"}]}"#;
        let accepted = completion(valid, active, &criteria);
        assert!(accepted.ok);
        assert!(accepted.output.contains("Compilation succeeded"));

        let wrong_goal = valid.replace("Create and verify the parser", "Create parser");
        assert!(!completion(&wrong_goal, active, &criteria).ok);
        let failed = valid.replace("\"passed\":true", "\"passed\":false");
        assert!(!completion(&failed, active, &criteria).ok);
        let no_evidence = valid.replace("Compilation succeeded", "");
        assert!(!completion(&no_evidence, active, &criteria).ok);
        let omitted = vec![String::from("Valid syntax"), String::from("No regressions")];
        assert!(!completion(valid, active, &omitted).ok);
    }

    #[test]
    fn acceptance_criteria_are_task_scaled_and_append_only() {
        let active = "Write a short story";
        let first = set_criteria(
            r#"{"goal":"Write a short story","criteria":["Story is under 500 words"]}"#,
            active,
            &[],
        )
        .unwrap();
        assert_eq!(first, ["Story is under 500 words"]);
        let added = set_criteria(
            r#"{"goal":"Write a short story","criteria":["Story is under 500 words","Includes the requested character"]}"#,
            active,
            &first,
        )
        .unwrap();
        assert_eq!(added.len(), 2);
        assert!(set_criteria(
            r#"{"goal":"Write a short story","criteria":["Includes the requested character"]}"#,
            active,
            &first,
        )
        .is_err());
    }

    #[test]
    fn a_pause_requires_a_reason_and_question_but_keeps_the_goal_open() {
        let report = pause(
            r#"{"reason":"The target environment is unknown","question":"Which environment should I target?"}"#,
        );
        assert!(report.ok);
        assert!(report.output.contains("not complete"));
        assert!(!pause(r#"{"reason":"unknown","question":" "}"#).ok);
    }

    #[test]
    fn goal_prompt_scales_verification_to_the_requested_work() {
        let prompt = system_prompt("Write a story", &[], false);
        assert!(prompt.contains("do not invent code tests"));
        assert!(prompt.contains("set_goal_criteria"));
        assert!(prompt.contains("completion tool"));
        assert!(continue_prompt("Write a story").contains("previous message did not complete it"));
        assert!(handoff_prompt("Summarize", "Write a story", &[], false).contains("NOT COMPLETE"));
    }

    #[test]
    fn active_goal_and_criteria_survive_chat_metadata_serialization() {
        let mut saved = crate::store::ChatMeta::default();
        saved.goal = Some(String::from("Repair and verify the feature"));
        saved.goal_criteria = vec![String::from("The regression check passes")];
        saved.goal_paused = true;
        let encoded = serde_json::to_string(&saved).expect("goal metadata serializes");
        let restored: crate::store::ChatMeta =
            serde_json::from_str(&encoded).expect("goal metadata restores");
        assert_eq!(restored.goal, saved.goal);
        assert_eq!(restored.goal_criteria, saved.goal_criteria);
        assert!(restored.goal_paused);
    }
}
