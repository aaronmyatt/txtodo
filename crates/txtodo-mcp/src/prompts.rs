//! `plan_today`, `weekly_review`, `triage_inbox` (design §6.4): parameterised templates that pull
//! the relevant resources in. Each embeds real backend data rather than only a `ResourceLink`, but
//! stays honest about `txtodo-query`'s absence (see `parse.rs`'s module doc) — `plan_today` and
//! `weekly_review` embed the file as-is rather than claiming a `due<=today` filter that does not
//! exist yet; `triage_inbox`'s `@context` filter is real (`parse::matches_query`, the same matching as
//! `txtodo list`), so it does filter.

use rmcp::ErrorData;
use rmcp::model::{
    GetPromptResult, ListPromptsResult, Prompt, PromptArgument, PromptMessage, Role,
};

use crate::backend::McpBackend;
use crate::error::McpError;
use crate::parse::Token;

const PLAN_TODAY: &str = "plan_today";
const WEEKLY_REVIEW: &str = "weekly_review";
const TRIAGE_INBOX: &str = "triage_inbox";

/// `list_prompts`.
pub fn list() -> ListPromptsResult {
    let prompts = vec![
        Prompt::new(
            PLAN_TODAY,
            Some("Prioritise today's actionable tasks from todo.txt"),
            Some(vec![
                PromptArgument::new("file").with_description("defaults to todo.txt"),
            ]),
        ),
        Prompt::new(
            WEEKLY_REVIEW,
            Some("Review the week: what shipped, what's stuck, what's next"),
            Some(vec![
                PromptArgument::new("file").with_description("defaults to todo.txt"),
            ]),
        ),
        Prompt::new(
            TRIAGE_INBOX,
            Some("Triage tasks an agent quarantined into a context (default @inbox)"),
            Some(vec![
                PromptArgument::new("context").with_description("defaults to inbox, no leading @"),
            ]),
        ),
    ];
    ListPromptsResult::with_all_items(prompts)
}

/// `get_prompt`.
pub async fn get(
    backend: &dyn McpBackend,
    name: &str,
    arg: impl Fn(&str) -> Option<String>,
) -> Result<GetPromptResult, ErrorData> {
    match name {
        PLAN_TODAY => plan_today(backend, arg("file")).await,
        WEEKLY_REVIEW => weekly_review(backend, arg("file")).await,
        TRIAGE_INBOX => triage_inbox(backend, arg("context")).await,
        other => Err(McpError::not_found(format!("no prompt named {other}")).into()),
    }
}

async fn plan_today(
    backend: &dyn McpBackend,
    file: Option<String>,
) -> Result<GetPromptResult, ErrorData> {
    let text = backend
        .get_file(file.unwrap_or_else(|| "todo.txt".into()), None)
        .await?;
    let instructions =
        "Prioritise today's actionable tasks (skip anything already done or blocked):";
    Ok(GetPromptResult::new(vec![
        PromptMessage::new_text(Role::User, instructions),
        PromptMessage::new_text(Role::User, text),
    ]))
}

async fn weekly_review(
    backend: &dyn McpBackend,
    file: Option<String>,
) -> Result<GetPromptResult, ErrorData> {
    let path = file.unwrap_or_else(|| "todo.txt".into());
    let text = backend.get_file(path, None).await?;
    let instructions = "Summarise this week: what's done, what's stuck, what to carry forward:";
    Ok(GetPromptResult::new(vec![
        PromptMessage::new_text(Role::User, instructions),
        PromptMessage::new_text(Role::User, text),
    ]))
}

async fn triage_inbox(
    backend: &dyn McpBackend,
    context: Option<String>,
) -> Result<GetPromptResult, ErrorData> {
    let context = context.unwrap_or_else(|| "inbox".into());
    let rows = crate::resources::rows_with_token(backend, Token::Context(&context), None).await?;
    let instructions =
        format!("Triage these @{context} tasks: assign a project, priority, or delete them:");
    let json = serde_json::to_string(&rows).unwrap_or_default();
    Ok(GetPromptResult::new(vec![
        PromptMessage::new_text(Role::User, instructions),
        PromptMessage::new_text(Role::User, json),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_registers_exactly_the_three_prompts() {
        let registered = list();
        let names: Vec<&str> = registered.prompts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, [PLAN_TODAY, WEEKLY_REVIEW, TRIAGE_INBOX]);
    }
}
