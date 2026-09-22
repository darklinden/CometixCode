//! Maps to: CC `utils/todo/types.ts` — the todo item/list schemas.
//!
//! CC keeps these as shared `lazySchema()`s because several owners parse the
//! same shape: `TodoWriteTool`'s `inputSchema` and `outputSchema`, and
//! `sessionRestore.ts`'s replay of persisted todos. The Rust side had the
//! validation rules transcribed by hand into `session_restore.rs`
//! (`parse_todo_item` / `parse_todo_list`) with no schema owner; this file is
//! that owner, so the rules live in one place and project to JSON Schema.

/// Maps to: CC `types.ts:15` `TodoItem` (`z.infer` of `TodoItemSchema`).
///
/// The struct and the schema are two views of one shape and belong to the same
/// file, as they do in CC. This type previously lived in `app_state_store.rs`,
/// which in CC only *imports* it (`AppStateStore.ts:2`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

/// Maps to: CC `types.ts:4-6` `TodoStatusSchema`'s member union.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// Maps to: CC `types.ts:18` `TodoList`.
pub type TodoList = Vec<TodoItem>;

// Each schema is built once and shared, which is what `lazySchema()` buys CC:
// one instance per session. It also matters downstream — `zodToJsonSchema`
// caches by schema identity, so a freshly-built schema would miss the cache
// every time (`utils/zod_to_json_schema.rs:26-32`).

/// Maps to: CC `types.ts:4-6` `TodoStatusSchema` — module-private there, so
/// private here too.
fn todo_status_schema() -> &'static crate::utils::zod::Schema {
    static SCHEMA: std::sync::LazyLock<crate::utils::zod::Schema> =
        std::sync::LazyLock::new(|| {
            crate::utils::zod::enumeration(vec!["pending", "in_progress", "completed"])
        });
    &SCHEMA
}

/// Maps to: CC `types.ts:8-14` `TodoItemSchema`.
///
/// The `.min(1, …)` messages are CC's own and reach the model verbatim when a
/// TodoWrite call is rejected.
pub fn todo_item_schema() -> &'static crate::utils::zod::Schema {
    static SCHEMA: std::sync::LazyLock<crate::utils::zod::Schema> =
        std::sync::LazyLock::new(|| {
            use crate::utils::zod;
            zod::object(vec![
                (
                    "content",
                    zod::string().min_with_message(1, "Content cannot be empty"),
                ),
                // CC nests the same schema instance; `Schema` is an immutable
                // value, so cloning the shared one is the same shape.
                ("status", todo_status_schema().clone()),
                (
                    "activeForm",
                    zod::string().min_with_message(1, "Active form cannot be empty"),
                ),
            ])
        });
    &SCHEMA
}

/// Maps to: CC `types.ts:17` `TodoListSchema`.
pub fn todo_list_schema() -> &'static crate::utils::zod::Schema {
    static SCHEMA: std::sync::LazyLock<crate::utils::zod::Schema> =
        std::sync::LazyLock::new(|| crate::utils::zod::array(todo_item_schema().clone()));
    &SCHEMA
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::zod::safe_parse;
    use serde_json::json;

    /// A well-formed list round-trips unchanged.
    #[test]
    fn accepts_a_well_formed_list() {
        let input = json!([
            {"content": "Run tests", "status": "in_progress", "activeForm": "Running tests"}
        ]);
        assert_eq!(safe_parse(todo_list_schema(), &input).unwrap(), input);
    }

    /// Empty `content` reports CC's own message, not the generated one.
    #[test]
    fn empty_content_reports_the_official_message() {
        let input = json!([{"content": "", "status": "pending", "activeForm": "Doing"}]);
        let error = safe_parse(todo_list_schema(), &input).unwrap_err();
        assert_eq!(error.issues.len(), 1);
        assert_eq!(error.issues[0].message, "Content cannot be empty");
    }

    #[test]
    fn empty_active_form_reports_the_official_message() {
        let input = json!([{"content": "Do it", "status": "pending", "activeForm": ""}]);
        let error = safe_parse(todo_list_schema(), &input).unwrap_err();
        assert_eq!(error.issues[0].message, "Active form cannot be empty");
    }

    /// An unknown status is rejected — the enum is closed.
    #[test]
    fn rejects_an_unknown_status() {
        let input = json!([{"content": "x", "status": "blocked", "activeForm": "y"}]);
        assert!(safe_parse(todo_list_schema(), &input).is_err());
    }
}
