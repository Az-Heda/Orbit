//! The chat loop, proposals and undo, driven by a scripted model (no network).
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::*;
use crate::domain::activity::Actor;
use crate::domain::ai::{ChatContext, SharingSettings};
use crate::infrastructure::ai::client::*;
use crate::infrastructure::ai::secrets::MemorySecretStore;
use crate::infrastructure::database::open_in_memory;
use crate::state::AppState;

#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<ModelReply>>,
    requests: Mutex<Vec<ApiRequest>>,
}

#[async_trait]
impl ModelClient for Script {
    async fn stream(
        &self,
        req: &ApiRequest,
        sink: Sink<'_>,
        _c: &AtomicBool,
    ) -> Result<ModelReply, AiError> {
        self.requests.lock().unwrap().push(req.clone());
        let r = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("script exhausted");
        for b in &r.content {
            if b["type"] == "text" {
                sink(StreamEvent::Text(b["text"].as_str().unwrap().into()));
            }
        }
        Ok(r)
    }
}

fn text(t: &str) -> ModelReply {
    ModelReply {
        content: vec![json!({"type":"text","text":t})],
        stop_reason: "end_turn".into(),
        ..Default::default()
    }
}

fn tool(id: &str, name: &str, input: Value) -> ModelReply {
    ModelReply {
        content: vec![
            json!({"type":"thinking","thinking":"hm","signature":"sig"}),
            json!({"type":"tool_use","id":id,"name":name,"input":input}),
        ],
        stop_reason: "tool_use".into(),
        ..Default::default()
    }
}

#[derive(Default)]
struct Collect(Mutex<Vec<ChatEvent>>);
impl StreamSink for Collect {
    fn emit(&self, e: ChatEvent) {
        self.0.lock().unwrap().push(e);
    }
}

struct T {
    pool: sqlx::SqlitePool,
    s: AppState,
    script: Arc<Script>,
    column: String,
    done_column: String,
}

async fn setup(replies: Vec<ModelReply>) -> T {
    let pool = open_in_memory().await.unwrap();
    let script = Arc::new(Script {
        replies: Mutex::new(replies.into()),
        ..Default::default()
    });
    let s = AppState::with_ai(
        pool.clone(),
        Arc::new(NullNotifier),
        Arc::new(MemorySecretStore::default()),
        script.clone(),
    );
    let p = s
        .projects
        .create_project("P", None, None, Actor::User)
        .await
        .unwrap();
    let b = s.projects.list_boards(&p.id).await.unwrap().remove(0);
    let v = s.tasks.board_view(&b.id).await.unwrap();
    let column = v
        .columns
        .iter()
        .find(|c| c.kind == "backlog")
        .unwrap()
        .id
        .clone();
    let done_column = v
        .columns
        .iter()
        .find(|c| c.kind == "done")
        .unwrap()
        .id
        .clone();
    T {
        pool,
        s,
        script,
        column,
        done_column,
    }
}

async fn count(t: &T) -> usize {
    let col = t.s.tasks.column(&t.column).await.unwrap();
    let v = t.s.tasks.board_view(&col.board_id).await.unwrap();
    v.tasks.iter().filter(|x| x.column_id == t.column).count()
}

fn ctx() -> ChatContext {
    ChatContext {
        timezone: Some("Europe/Rome".into()),
        view: Some("/projects/x".into()),
        ..Default::default()
    }
}

async fn say(t: &T, conv: Option<&str>, msg: &str) -> (String, Vec<ChatEvent>) {
    let sink = Collect::default();
    let id = t.s.ai.send(conv, msg, ctx(), &sink).await.unwrap();
    let ev = std::mem::take(&mut *sink.0.lock().unwrap());
    (id, ev)
}

#[tokio::test]
async fn read_tool_loop_streams_and_persists_raw_blocks() {
    let t = setup(vec![
        tool("t1", "list_projects", json!({})),
        text("You have one project."),
    ])
    .await;
    let (cid, ev) = say(&t, None, "what projects do I have?").await;
    assert!(
        matches!(ev.first(), Some(ChatEvent::Started { .. }))
            && matches!(ev.last(), Some(ChatEvent::Done { .. }))
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, ChatEvent::TextDelta { text } if text.contains("one project")))
    );
    let d = t.s.ai.conversation(&cid).await.unwrap();
    let roles: Vec<_> = d.messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["user", "assistant", "user", "assistant"]);
    // The thinking block is stored verbatim and replayed on the follow-up request.
    assert_eq!(d.messages[1].content[0]["signature"], "sig");
    let reqs = t.script.requests.lock().unwrap();
    assert_eq!(reqs[1].messages[1]["content"][0]["signature"], "sig");
    let result = &reqs[1].messages[2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert!(result["content"].as_str().unwrap().contains(&t.column));
    // Per-turn context rides in the user message; system and tools never vary (thinking
    // signatures cover them).
    let ctx_text = reqs[0].messages[0]["content"][0]["text"].as_str().unwrap();
    assert!(ctx_text.contains("Europe/Rome") && ctx_text.contains("/projects/x"));
    assert!(
        ctx_text.contains("notes, inbox"),
        "unshared categories are announced"
    );
    assert_eq!(
        reqs[0].messages[0]["content"][1]["text"],
        "what projects do I have?"
    );
    assert_eq!(reqs[0].system.len(), 1);
    assert_eq!(reqs[0].system[0]["cache_control"]["type"], "ephemeral");
    assert!(reqs[0].tools.iter().any(|t| t["name"] == "get_note"));
}

#[tokio::test]
async fn system_and_tools_are_identical_across_turns_and_settings_changes() {
    let t = setup(vec![text("a"), text("b")]).await;
    let (cid, _) = say(&t, None, "one").await;
    t.s.ai.add_memory("Likes tea").await.unwrap();
    t.s.ai
        .set_sharing(SharingSettings {
            notes: true,
            tasks: false,
            ..Default::default()
        })
        .await
        .unwrap();
    say(&t, Some(&cid), "two").await;
    let reqs = t.script.requests.lock().unwrap();
    assert_eq!(reqs[0].system, reqs[1].system);
    assert_eq!(reqs[0].tools, reqs[1].tools);
    // Earlier messages are replayed byte-for-byte (append-only), and the new context mentions
    // the memory exactly once.
    assert_eq!(
        reqs[1].messages[..2],
        reqs[0].messages[..1]
            .iter()
            .cloned()
            .chain([json!({"role":"assistant","content":[{"type":"text","text":"a"}]})])
            .collect::<Vec<_>>()[..]
    );
    let c1 = reqs[1].messages[2]["content"][0]["text"].as_str().unwrap();
    assert!(c1.contains("Likes tea") && c1.contains("tasks"));
}

#[tokio::test]
async fn unsigned_thinking_from_a_truncated_reply_is_not_stored() {
    let cut = ModelReply {
        content: vec![json!({"type":"thinking","thinking":"half a thou","signature":""})],
        stop_reason: "max_tokens".into(),
        ..Default::default()
    };
    let t = setup(vec![cut, text("fine")]).await;
    let (cid, _) = say(&t, None, "hi").await;
    say(&t, Some(&cid), "again").await;
    let reqs = t.script.requests.lock().unwrap();
    assert!(
        !reqs[1]
            .messages
            .to_vec()
            .iter()
            .any(|m| m.to_string().contains("half a thou"))
    );
}

#[tokio::test]
async fn undo_refuses_when_the_entity_changed_since() {
    let t = setup(vec![]).await;
    t.script.replies.lock().unwrap().extend([
        tool(
            "c",
            "create_task",
            json!({"column_id": t.column, "title": "Draft"}),
        ),
        text("ok"),
    ]);
    let (_, ev) = say(&t, None, "add").await;
    let p = ev
        .iter()
        .find_map(|e| {
            if let ChatEvent::Proposal { proposal } = e {
                Some((**proposal).clone())
            } else {
                None
            }
        })
        .unwrap();
    t.s.ai.apply_proposal(&p.id).await.unwrap();
    let id = p.id.clone();
    let col = t.s.tasks.column(&t.column).await.unwrap();
    let task =
        t.s.tasks
            .board_view(&col.board_id)
            .await
            .unwrap()
            .tasks
            .remove(0);
    t.s.tasks
        .update_task(
            &task.id,
            super::UpdateTaskInput {
                title: Some("Edited by me".into()),
                description: None,
                priority: None,
                due_at: None,
            },
            Actor::User,
        )
        .await
        .unwrap();
    assert!(t.s.ai.undo_proposal(&id).await.is_err());
    assert_eq!(
        t.s.tasks.get_task(&task.id).await.unwrap().title,
        "Edited by me"
    );
    assert_eq!(
        t.s.ai
            .conversation(&p.conversation_id)
            .await
            .unwrap()
            .proposals[0]
            .status,
        "applied"
    );
}

#[tokio::test]
async fn warnings_show_in_the_preview_and_the_confirmed_move_goes_through() {
    let t = setup(vec![]).await;
    let mk = |title: &str| super::CreateTaskInput {
        column_id: Some(t.column.clone()),
        parent_id: None,
        title: title.into(),
        description: None,
        priority: None,
        due_at: None,
        position: None,
    };
    let a =
        t.s.tasks
            .create_task(mk("Blocker"), Actor::User)
            .await
            .unwrap();
    let b =
        t.s.tasks
            .create_task(mk("Blocked"), Actor::User)
            .await
            .unwrap();
    t.s.tasks
        .add_dependency(&b.id, &a.id, Actor::User)
        .await
        .unwrap();
    t.script.replies.lock().unwrap().extend([
        tool(
            "m",
            "move_task",
            json!({"task_id": b.id, "column_id": t.done_column}),
        ),
        text("ok"),
    ]);
    let (_, ev) = say(&t, None, "finish Blocked").await;
    let p = ev
        .iter()
        .find_map(|e| {
            if let ChatEvent::Proposal { proposal } = e {
                Some((**proposal).clone())
            } else {
                None
            }
        })
        .unwrap();
    assert!(
        p.preview["changes"].as_array().unwrap().iter().any(|c| c["label"] == "Warning"
            && c["after"].as_str().unwrap().contains("blocked by"))
    );
    let applied = t.s.ai.apply_proposal(&p.id).await.unwrap();
    assert_eq!(applied.status, "applied");
    assert_eq!(
        t.s.tasks.get_task(&b.id).await.unwrap().column_id,
        t.done_column
    );
}

#[tokio::test]
async fn writes_are_proposals_until_applied_and_can_be_undone() {
    let t = setup(vec![
        tool(
            "t1",
            "create_task",
            json!({"column_id": "", "title": "Write report", "due": "2026-12-01T09:00"}),
        ),
        text("Proposed."),
        text("Great."),
    ])
    .await;
    // Fill in the real column id (the script is built before ids exist).
    {
        let mut r = t.script.replies.lock().unwrap();
        r[0].content[1]["input"]["column_id"] = json!(t.column);
    }
    let (cid, ev) = say(&t, None, "add a task to write the report").await;
    let proposal = ev
        .iter()
        .find_map(|e| {
            if let ChatEvent::Proposal { proposal } = e {
                Some((**proposal).clone())
            } else {
                None
            }
        })
        .expect("proposal event");
    assert_eq!(proposal.status, "pending");
    assert_eq!(proposal.preview["tool_use_id"], "t1");
    assert!(
        proposal.preview["changes"]
            .to_string()
            .contains("Write report")
    );
    // Due time was read in Rome (UTC+1 in December) and shown back locally.
    assert!(proposal.preview["changes"].to_string().contains("09:00"));
    let board = count(&t).await;
    assert_eq!(board, 0, "nothing was created yet");

    let applied = t.s.ai.apply_proposal(&proposal.id).await.unwrap();
    assert_eq!(applied.status, "applied");
    assert_eq!(count(&t).await, 1);
    let log = t.s.activity.recent(50).await.unwrap();
    let row = log
        .iter()
        .find(|a| a.entity_type == "task" && a.action == "create")
        .unwrap();
    assert_eq!(row.actor, "ai");
    assert_eq!(row.batch_id.as_deref(), Some(proposal.id.as_str()));
    assert!(
        t.s.ai.apply_proposal(&proposal.id).await.is_err(),
        "no double apply"
    );

    // The next message tells the model what happened.
    say(&t, Some(&cid), "thanks").await;
    {
        let reqs = t.script.requests.lock().unwrap();
        let last_user = reqs[2]
            .messages
            .iter()
            .rev()
            .find(|m| m["role"] == "user")
            .unwrap();
        assert!(
            last_user["content"][1]["text"]
                .as_str()
                .unwrap()
                .contains("was applied")
        );
    }

    let (p, rep) = t.s.ai.undo_proposal(&proposal.id).await.unwrap();
    assert_eq!((p.status.as_str(), rep.reverted), ("undone", 1));
    assert_eq!(count(&t).await, 0);
}

#[tokio::test]
async fn delete_is_flagged_destructive_and_reject_changes_nothing() {
    let t = setup(vec![]).await;
    let task =
        t.s.tasks
            .create_task(
                super::CreateTaskInput {
                    column_id: Some(t.column.clone()),
                    parent_id: None,
                    title: "Old".into(),
                    description: None,
                    priority: None,
                    due_at: None,
                    position: None,
                },
                Actor::User,
            )
            .await
            .unwrap();
    t.script.replies.lock().unwrap().extend([
        tool("d1", "delete_task", json!({"task_id": task.id})),
        text("Asked."),
    ]);
    let (_, ev) = say(&t, None, "delete Old").await;
    let p = ev
        .iter()
        .find_map(|e| {
            if let ChatEvent::Proposal { proposal } = e {
                Some((**proposal).clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(p.preview["destructive"], true);
    let r = t.s.ai.reject_proposal(&p.id).await.unwrap();
    assert_eq!(r.status, "rejected");
    assert!(t.s.tasks.get_task(&task.id).await.is_ok());
    assert!(
        t.s.ai.apply_proposal(&p.id).await.is_err(),
        "rejected proposals cannot be applied"
    );
}

#[tokio::test]
async fn invalid_calls_and_unshared_data_come_back_as_tool_errors() {
    let t = setup(vec![
        ModelReply { content: vec![
            json!({"type":"tool_use","id":"a","name":"move_task","input":{"task_id":"not-a-uuid","column_id":"x"}}),
            json!({"type":"tool_use","id":"b","name":"get_note","input":{"note_id":"00000000-0000-4000-8000-000000000000"}}),
            json!({"type":"tool_use","id":"c","name":"nope","input":{}}),
            json!({"type":"tool_use","id":"d","name":"create_event","input":{"title":"x","start":"garbage"}}),
        ], stop_reason: "tool_use".into(), ..Default::default() },
        text("ok"),
    ])
    .await;
    let (cid, _) = say(&t, None, "go").await;
    let d = t.s.ai.conversation(&cid).await.unwrap();
    let results = d.messages[2].content.as_array().unwrap();
    assert!(results.iter().all(|r| r["is_error"] == true), "{results:?}");
    assert!(
        results[1]["content"]
            .as_str()
            .unwrap()
            .contains("not allowed sharing notes")
    );
    assert!(d.proposals.is_empty());
}

#[tokio::test]
async fn notes_are_readable_only_when_shared() {
    let t = setup(vec![]).await;
    let note =
        t.s.notes
            .create("Secret plans", "body text", Actor::User)
            .await
            .unwrap();
    t.script.replies.lock().unwrap().extend([
        tool("n1", "get_note", json!({"note_id": note.id})),
        text("done"),
    ]);
    t.s.ai
        .set_sharing(SharingSettings {
            notes: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let (cid, _) = say(&t, None, "read it").await;
    let d = t.s.ai.conversation(&cid).await.unwrap();
    assert!(
        d.messages[2].content[0]["content"]
            .as_str()
            .unwrap()
            .contains("body text")
    );
    assert!(
        t.script.requests.lock().unwrap()[0]
            .tools
            .iter()
            .any(|x| x["name"] == "get_note")
    );
}

#[tokio::test]
async fn refusal_and_truncation_are_handled() {
    let refusal = ModelReply {
        content: vec![],
        stop_reason: "refusal".into(),
        refusal_category: Some("cyber".into()),
        ..Default::default()
    };
    let t = setup(vec![refusal]).await;
    let (cid, ev) = say(&t, None, "hi").await;
    assert!(ev.iter().any(|e| matches!(e, ChatEvent::TextDelta { text } if text.contains("declined") && text.contains("cyber"))));
    assert!(matches!(ev.last(), Some(ChatEvent::Done { .. })));
    assert_eq!(t.s.ai.conversation(&cid).await.unwrap().messages.len(), 2);

    let cut = ModelReply {
        content: vec![json!({"type":"tool_use","id":"x","name":"create_task","input":{}})],
        stop_reason: "max_tokens".into(),
        ..Default::default()
    };
    let t = setup(vec![cut]).await;
    let (cid, _) = say(&t, None, "hi").await;
    let d = t.s.ai.conversation(&cid).await.unwrap();
    assert_eq!(
        d.messages[2].content[0]["is_error"], true,
        "truncated tool call is closed, not run"
    );
    assert!(d.proposals.is_empty());
}

#[tokio::test]
async fn memory_requires_confirmation_and_rejects_secrets() {
    let t = setup(vec![
        tool(
            "m1",
            "remember",
            json!({"text": "Prefers mornings for deep work"}),
        ),
        tool(
            "m2",
            "remember",
            json!({"text": "my api key is sk-ant-abc"}),
        ),
        text("ok"),
    ])
    .await;
    // Both calls happen in separate turns of the same message loop; the second is rejected.
    let (cid, ev) = say(&t, None, "remember that").await;
    assert!(t.s.ai.memories().await.unwrap().is_empty());
    let p = ev
        .iter()
        .find_map(|e| {
            if let ChatEvent::Proposal { proposal } = e {
                Some((**proposal).clone())
            } else {
                None
            }
        })
        .unwrap();
    let d = t.s.ai.conversation(&cid).await.unwrap();
    assert_eq!(
        d.proposals.len(),
        1,
        "the secret-looking memory never becomes a proposal"
    );
    t.s.ai.apply_proposal(&p.id).await.unwrap();
    let ms = t.s.ai.memories().await.unwrap();
    assert_eq!(ms.len(), 1);
    // Memories are injected into the next prompt (with their ids).
    t.script.replies.lock().unwrap().push_back(text("fine"));
    say(&t, Some(&cid), "again").await;
    {
        let reqs = t.script.requests.lock().unwrap();
        assert!(
            reqs.last().unwrap().messages.last().unwrap()["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Prefers mornings")
        );
    }
    assert!(t.s.ai.add_memory("password: hunter2").await.is_err());
}

#[tokio::test]
async fn blocked_tasks_are_flagged_and_events_round_trip() {
    let t = setup(vec![]).await;
    let mk = |title: &str| super::CreateTaskInput {
        column_id: Some(t.column.clone()),
        parent_id: None,
        title: title.into(),
        description: None,
        priority: None,
        due_at: None,
        position: None,
    };
    let a = t.s.tasks.create_task(mk("A"), Actor::User).await.unwrap();
    let b = t.s.tasks.create_task(mk("B"), Actor::User).await.unwrap();
    t.s.tasks
        .add_dependency(&b.id, &a.id, Actor::User)
        .await
        .unwrap();
    t.script.replies.lock().unwrap().extend([
        tool("g", "get_tasks", json!({})),
        tool(
            "e",
            "create_event",
            json!({"title": "Dentist", "start": "2026-12-02T10:00", "end": "2026-12-02T11:00"}),
        ),
        text("ok"),
    ]);
    let (cid, ev) = say(&t, None, "plan").await;
    let d = t.s.ai.conversation(&cid).await.unwrap();
    let tasks: Value =
        serde_json::from_str(d.messages[2].content[0]["content"].as_str().unwrap()).unwrap();
    let blocked: Vec<_> = tasks["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["title"].as_str().unwrap().to_string(),
                x["blocked"].as_bool().unwrap(),
            )
        })
        .collect();
    assert!(blocked.contains(&("B".into(), true)) && blocked.contains(&("A".into(), false)));
    let p = ev
        .iter()
        .find_map(|e| {
            if let ChatEvent::Proposal { proposal } = e {
                Some((**proposal).clone())
            } else {
                None
            }
        })
        .unwrap();
    t.s.ai.apply_proposal(&p.id).await.unwrap();
    let evs =
        t.s.calendar
            .list_range("2026-12-02T00:00:00.000Z", "2026-12-03T00:00:00.000Z")
            .await
            .unwrap();
    let e = evs.iter().find(|e| e.title == "Dentist").unwrap();
    assert_eq!(
        e.start_at, "2026-12-02T09:00:00.000Z",
        "10:00 Rome = 09:00 UTC"
    );
    let _ = t.done_column;
}

#[tokio::test]
async fn errors_surface_and_context_ids_are_validated() {
    let t = setup(vec![]).await;
    let sink = Collect::default();
    let bad = ChatContext {
        task_id: Some("'; drop table tasks;--".into()),
        ..Default::default()
    };
    assert!(t.s.ai.send(None, "hi", bad, &sink).await.is_err());
    assert!(t.s.ai.send(None, "   ", ctx(), &sink).await.is_err());
    assert!(
        t.s.ai
            .send(
                None,
                "hi",
                ChatContext {
                    timezone: Some("Mars/Base".into()),
                    ..Default::default()
                },
                &sink
            )
            .await
            .is_err()
    );
    assert!(
        t.s.ai
            .set_config(crate::domain::ai::AiConfig {
                model: "bad model".into(),
                effort: "low".into()
            })
            .await
            .is_err()
    );
    assert_eq!(t.s.ai.config().await.model, "claude-opus-5-5");
}

// ---------- Phase 4: planning, breakdown, organisation, focus ----------

fn proposal_of(ev: &[ChatEvent]) -> crate::domain::ai::Proposal {
    ev.iter()
        .find_map(|e| {
            if let ChatEvent::Proposal { proposal } = e {
                Some((**proposal).clone())
            } else {
                None
            }
        })
        .expect("a proposal")
}

/// Content of the most recent tool_result the model was shown.
fn last_result(t: &T) -> String {
    let reqs = t.script.requests.lock().unwrap();
    let msgs = &reqs.last().unwrap().messages;
    msgs.iter()
        .rev()
        .find_map(|m| {
            m["content"]
                .as_array()?
                .iter()
                .find(|b| b["type"] == "tool_result")
                .map(|b| b["content"].as_str().unwrap_or_default().to_string())
        })
        .expect("a tool result")
}

async fn new_task(t: &T, title: &str) -> crate::domain::kanban::Task {
    t.s.tasks
        .create_task(
            super::CreateTaskInput {
                column_id: Some(t.column.clone()),
                parent_id: None,
                title: title.into(),
                description: None,
                priority: None,
                due_at: None,
                position: None,
            },
            Actor::User,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn breakdown_creates_all_subtasks_in_one_undoable_proposal() {
    let t = setup(vec![]).await;
    let parent = new_task(&t, "Ship v1").await;
    t.script.replies.lock().unwrap().extend([
        tool(
            "c",
            "create_subtasks",
            json!({"parent_id": parent.id, "subtasks": [{"title": "Spec"}, {"title": "Build", "priority": 2}, {"title": "Test"}]}),
        ),
        text("ok"),
    ]);
    let (_, ev) = say(&t, None, "break it down").await;
    let p = proposal_of(&ev);
    assert_eq!(p.preview["changes"].as_array().unwrap().len(), 3);
    assert_eq!(count(&t).await, 1, "nothing is created before confirmation");
    t.s.ai.apply_proposal(&p.id).await.unwrap();
    assert_eq!(count(&t).await, 4);
    t.s.ai.undo_proposal(&p.id).await.unwrap();
    assert_eq!(count(&t).await, 1);
}

#[tokio::test]
async fn breakdown_rejects_empty_and_oversized_lists() {
    let t = setup(vec![]).await;
    let parent = new_task(&t, "X").await;
    let many: Vec<_> = (0..21).map(|i| json!({"title": format!("s{i}")})).collect();
    t.script.replies.lock().unwrap().extend([
        tool(
            "a",
            "create_subtasks",
            json!({"parent_id": parent.id, "subtasks": []}),
        ),
        tool(
            "b",
            "create_subtasks",
            json!({"parent_id": parent.id, "subtasks": many}),
        ),
        text("done"),
    ]);
    let (_, ev) = say(&t, None, "x").await;
    assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Proposal { .. })));
}

#[tokio::test]
async fn bulk_move_crosses_boards_is_destructive_and_undoable() {
    let t = setup(vec![]).await;
    let a = new_task(&t, "Borrow checker notes").await;
    let b = new_task(&t, "Lifetimes").await;
    let rust =
        t.s.projects
            .create_project("Rust", None, None, Actor::User)
            .await
            .unwrap();
    let rb = t.s.projects.list_boards(&rust.id).await.unwrap().remove(0);
    let rv = t.s.tasks.board_view(&rb.id).await.unwrap();
    let target = rv
        .columns
        .iter()
        .find(|c| c.kind == "backlog")
        .unwrap()
        .id
        .clone();
    t.script.replies.lock().unwrap().extend([
        tool(
            "m",
            "move_tasks",
            json!({"task_ids": [a.id, b.id], "column_id": target}),
        ),
        text("ok"),
    ]);
    let (_, ev) = say(&t, None, "gather rust tasks").await;
    let p = proposal_of(&ev);
    assert_eq!(
        p.preview["destructive"], true,
        "bulk always needs confirmation"
    );
    t.s.ai.apply_proposal(&p.id).await.unwrap();
    assert_eq!(t.s.tasks.get_task(&a.id).await.unwrap().column_id, target);
    assert_eq!(t.s.tasks.get_task(&b.id).await.unwrap().column_id, target);
    t.s.ai.undo_proposal(&p.id).await.unwrap();
    assert_eq!(t.s.tasks.get_task(&a.id).await.unwrap().column_id, t.column);
}

#[tokio::test]
async fn bulk_move_refuses_tasks_with_dependencies_across_boards() {
    let t = setup(vec![]).await;
    let a = new_task(&t, "A").await;
    let b = new_task(&t, "B").await;
    t.s.tasks
        .add_dependency(&a.id, &b.id, Actor::User)
        .await
        .unwrap();
    let rust =
        t.s.projects
            .create_project("Rust", None, None, Actor::User)
            .await
            .unwrap();
    let rb = t.s.projects.list_boards(&rust.id).await.unwrap().remove(0);
    let rv = t.s.tasks.board_view(&rb.id).await.unwrap();
    let target = rv.columns[0].id.clone();
    t.script.replies.lock().unwrap().extend([
        tool(
            "m",
            "move_tasks",
            json!({"task_ids": [a.id], "column_id": target}),
        ),
        text("no"),
    ]);
    let (_, ev) = say(&t, None, "move A").await;
    assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Proposal { .. })));
    assert!(last_result(&t).contains("dependencies"));
    assert_eq!(t.s.tasks.get_task(&a.id).await.unwrap().column_id, t.column);
}

#[tokio::test]
async fn free_slots_avoid_events_and_next_tasks_skip_blocked() {
    let t = setup(vec![]).await;
    t.s.calendar
        .create_event(
            super::CreateEventInput {
                title: "Meeting".into(),
                description: None,
                location: None,
                all_day: Some(false),
                start_at: "2027-01-11T09:00:00Z".into(),
                end_at: "2027-01-11T12:00:00Z".into(),
                timezone: "Europe/Rome".into(),
                recurrence_rule: None,
            },
            Actor::User,
        )
        .await
        .unwrap();
    t.script.replies.lock().unwrap().extend([
        tool(
            "f",
            "find_free_slots",
            json!({"from": "2027-01-11T00:00", "to": "2027-01-12T00:00", "duration_minutes": 120}),
        ),
        text("ok"),
    ]);
    say(&t, None, "find 2 hours").await;
    let r: Value = serde_json::from_str(&last_result(&t)).unwrap();
    let slots = r["slots"].as_array().unwrap();
    assert!(!slots.is_empty());
    // 10:00-13:00 local is busy; nothing may start inside it.
    for s in slots {
        let st = s["start"].as_str().unwrap();
        assert!(
            !("2027-01-11T10:00".."2027-01-11T13:00").contains(&st),
            "{st}"
        );
    }

    let a = new_task(&t, "Blocker").await;
    let b = new_task(&t, "Blocked").await;
    t.s.tasks
        .add_dependency(&b.id, &a.id, Actor::User)
        .await
        .unwrap();
    t.script
        .replies
        .lock()
        .unwrap()
        .extend([tool("n", "suggest_next_tasks", json!({})), text("ok")]);
    say(&t, None, "what next").await;
    let r: Value = serde_json::from_str(&last_result(&t)).unwrap();
    assert_eq!(r["blocked_excluded"], 1);
    let ids: Vec<_> = r["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&a.id) && !ids.contains(&b.id));
}

#[tokio::test]
async fn focus_data_needs_its_own_sharing_permission() {
    let t = setup(vec![]).await;
    t.script.replies.lock().unwrap().extend([
        tool(
            "g",
            "get_focus_sessions",
            json!({"from": "2027-01-01", "to": "2027-01-02"}),
        ),
        text("no"),
    ]);
    say(&t, None, "sessions?").await;
    assert!(last_result(&t).contains("not allowed"));
    assert!(!t.s.ai.sharing().await.focus, "off by default");
}

#[tokio::test]
async fn focus_timer_links_to_the_sole_active_task_and_can_be_relinked_and_undone() {
    let t = setup(vec![]).await;
    let task = new_task(&t, "Deep work").await;
    let other = new_task(&t, "Other").await;
    let col = t.s.tasks.column(&t.column).await.unwrap();
    let v = t.s.tasks.board_view(&col.board_id).await.unwrap();
    let active = v
        .columns
        .iter()
        .find(|c| c.kind == "active")
        .unwrap()
        .id
        .clone();
    t.s.tasks
        .move_task(&task.id, &active, 0, true, Actor::User)
        .await
        .unwrap();

    let timer =
        t.s.timers
            .start(
                super::StartTimerInput {
                    kind: "countdown".into(),
                    label: None,
                    task_id: None,
                    duration_ms: Some(60_000),
                    phase: None,
                },
                Actor::User,
            )
            .await
            .unwrap();
    assert_eq!(timer.timer.task_id.as_deref(), Some(task.id.as_str()));
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    t.s.timers.stop(&timer.timer.id).await.unwrap();
    let sessions =
        t.s.timers
            .sessions("2000-01-01T00:00:00Z", "2100-01-01T00:00:00Z", None)
            .await
            .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].task_id.as_deref(), Some(task.id.as_str()));

    t.s.ai
        .set_sharing(crate::domain::ai::SharingSettings {
            focus: true,
            ..t.s.ai.sharing().await
        })
        .await
        .unwrap();
    t.script.replies.lock().unwrap().extend([
        tool(
            "l",
            "link_focus_session",
            json!({"session_id": sessions[0].id, "task_id": other.id}),
        ),
        text("ok"),
    ]);
    let (_, ev) = say(&t, None, "relink").await;
    let p = proposal_of(&ev);
    t.s.ai.apply_proposal(&p.id).await.unwrap();
    let after = t.s.timers.get_session(&sessions[0].id).await.unwrap();
    assert_eq!(after.task_id.as_deref(), Some(other.id.as_str()));
    t.s.ai.undo_proposal(&p.id).await.unwrap();
    let back = t.s.timers.get_session(&sessions[0].id).await.unwrap();
    assert_eq!(back.task_id.as_deref(), Some(task.id.as_str()));
}

async fn rust_target(t: &T) -> String {
    let rust =
        t.s.projects
            .create_project("Rust", None, None, Actor::User)
            .await
            .unwrap();
    let rb = t.s.projects.list_boards(&rust.id).await.unwrap().remove(0);
    let rv = t.s.tasks.board_view(&rb.id).await.unwrap();
    rv.columns
        .iter()
        .find(|c| c.kind == "backlog")
        .unwrap()
        .id
        .clone()
}

#[tokio::test]
async fn bulk_move_warns_about_open_subtasks_and_undo_brings_subtasks_back() {
    let t = setup(vec![]).await;
    let parent = new_task(&t, "Parent").await;
    let sub =
        t.s.tasks
            .create_task(
                super::CreateTaskInput {
                    column_id: None,
                    parent_id: Some(parent.id.clone()),
                    title: "Child".into(),
                    description: None,
                    priority: None,
                    due_at: None,
                    position: None,
                },
                Actor::User,
            )
            .await
            .unwrap();
    // Same-board move to Done with an open subtask: the preview carries the warning.
    t.script.replies.lock().unwrap().extend([
        tool(
            "a",
            "move_tasks",
            json!({"task_ids": [parent.id], "column_id": t.done_column}),
        ),
        text("ok"),
    ]);
    let (_, ev) = say(&t, None, "finish parent").await;
    let p = proposal_of(&ev);
    assert!(
        p.preview["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["label"] == "Warning")
    );

    // Cross-board move and undo: the subtask travels both ways.
    let target = rust_target(&t).await;
    t.script.replies.lock().unwrap().extend([
        tool(
            "b",
            "move_tasks",
            json!({"task_ids": [parent.id], "column_id": target}),
        ),
        text("ok"),
    ]);
    let (_, ev) = say(&t, None, "to rust").await;
    let p = proposal_of(&ev);
    t.s.ai.apply_proposal(&p.id).await.unwrap();
    assert_eq!(t.s.tasks.get_task(&sub.id).await.unwrap().column_id, target);
    t.s.ai.undo_proposal(&p.id).await.unwrap();
    assert_eq!(
        t.s.tasks.get_task(&parent.id).await.unwrap().column_id,
        t.column
    );
    assert_eq!(
        t.s.tasks.get_task(&sub.id).await.unwrap().column_id,
        t.column
    );
}

#[tokio::test]
async fn failed_bulk_apply_rolls_back_the_partial_changes() {
    let t = setup(vec![]).await;
    let (a, b, c) = (
        new_task(&t, "A").await,
        new_task(&t, "B").await,
        new_task(&t, "C").await,
    );
    let target = rust_target(&t).await;
    t.script.replies.lock().unwrap().extend([
        tool(
            "m",
            "move_tasks",
            json!({"task_ids": [a.id, b.id], "column_id": target}),
        ),
        text("ok"),
    ]);
    let (cid, ev) = say(&t, None, "move both").await;
    let p = proposal_of(&ev);
    // After the preview, B gains a dependency, so moving it across boards now fails.
    t.s.tasks
        .add_dependency(&b.id, &c.id, Actor::User)
        .await
        .unwrap();
    assert!(t.s.ai.apply_proposal(&p.id).await.is_err());
    assert_eq!(
        t.s.tasks.get_task(&a.id).await.unwrap().column_id,
        t.column,
        "A rolled back"
    );
    assert_eq!(t.s.tasks.get_task(&b.id).await.unwrap().column_id, t.column);
    let d = t.s.ai.conversation(&cid).await.unwrap();
    assert_eq!(d.proposals[0].status, "failed");
}

#[tokio::test]
async fn ai_cannot_propose_changes_to_feed_events() {
    use crate::infrastructure::database::{
        ExternalCalendarRepository, ImportEvent, SqliteExternalCalendarRepository,
    };
    let t = setup(vec![]).await;
    let repo = SqliteExternalCalendarRepository::new(t.pool.clone());
    let cal = repo.create("Feed").await.unwrap();
    repo.replace_events(
        &cal.id,
        &[ImportEvent {
            event: crate::domain::calendar::NewEvent {
                uid: "f@x".into(),
                title: "Feed event".into(),
                description: None,
                location: None,
                all_day: false,
                start_at: "2027-01-11T09:00:00Z".into(),
                end_at: "2027-01-11T10:00:00Z".into(),
                timezone: "UTC".into(),
                recurrence_rule: None,
            },
            exceptions: vec![],
        }],
    )
    .await
    .unwrap();
    t.script.replies.lock().unwrap().extend([
        tool(
            "s",
            "get_schedule",
            json!({"from": "2027-01-11T00:00", "to": "2027-01-12T00:00"}),
        ),
        text("seen"),
    ]);
    say(&t, None, "what's on").await;
    let r: Value = serde_json::from_str(&last_result(&t)).unwrap();
    assert_eq!(r["items"][0]["read_only"], true);
    let id = r["items"][0]["id"].as_str().unwrap().to_string();

    t.script.replies.lock().unwrap().extend([
        tool(
            "u",
            "update_event",
            json!({"event_id": id, "title": "Hacked"}),
        ),
        tool("d", "delete_event", json!({"event_id": id})),
        text("no"),
    ]);
    let (_, ev) = say(&t, None, "rename it").await;
    assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Proposal { .. })));
    assert!(last_result(&t).contains("read-only"));
}

fn maybe_proposal(ev: &[ChatEvent]) -> Option<crate::domain::ai::Proposal> {
    ev.iter().find_map(|e| {
        if let ChatEvent::Proposal { proposal } = e {
            Some((**proposal).clone())
        } else {
            None
        }
    })
}

#[tokio::test]
async fn ai_drafts_a_chained_rule_that_notifies_a_channel_by_name() {
    use crate::domain::channels::{ChannelInput, ChannelKind};
    let t = setup(vec![]).await;
    let hook = "https://discord.com/api/webhooks/42/SECRETHOOK";
    let ch =
        t.s.channels
            .create(
                ChannelInput {
                    name: "Team chat".into(),
                    kind: ChannelKind::Discord,
                    template: "{{task.title}} done".into(),
                    chat_id: None,
                    secret: Some(hook.into()),
                },
                Actor::User,
            )
            .await
            .unwrap();
    t.script.replies.lock().unwrap().extend([
        tool("l", "list_notification_channels", json!({})),
        tool(
            "d",
            "draft_automation_rule",
            json!({
                "name": "Tell the team",
                "trigger": {"kind": "task_completed"},
                "actions": [
                    {"kind": "create_note", "title": "Done: {{task.title}}", "body": ""},
                    {"kind": "send_notification", "channel": "team CHAT"}
                ]
            }),
        ),
        text("drafted"),
    ]);
    let (_, ev) = say(&t, None, "tell the team when I finish a task").await;

    // The model only ever sees names and kinds, never ids or secrets.
    {
        let reqs = t.script.requests.lock().unwrap();
        let all =
            serde_json::to_string(&reqs.iter().map(|r| &r.messages).collect::<Vec<_>>()).unwrap();
        assert!(all.contains("Team chat") && all.contains("discord"));
        assert!(!all.contains("SECRETHOOK") && !all.contains(&ch.id));
    }

    let p = maybe_proposal(&ev).expect("a proposal");
    let changes = p.preview["changes"].to_string();
    assert!(
        changes.contains("Then 1") && changes.contains("Then 2"),
        "{changes}"
    );
    assert!(
        changes.contains("Team chat") && !changes.contains(&ch.id),
        "{changes}"
    );
    assert!(
        t.s.rules.list().await.unwrap().is_empty(),
        "nothing before confirmation"
    );

    t.s.ai.apply_proposal(&p.id).await.unwrap();
    let rules = t.s.rules.list().await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].actions.len(), 2);
    assert_eq!(
        rules[0].actions[1],
        crate::domain::automation::Action::SendNotification {
            channel_id: ch.id.clone()
        }
    );
    // Same activity-log trail as any AI mutation.
    let actor: String = sqlx::query_scalar(
        "SELECT actor FROM activity_log WHERE entity_type = 'automation_rule' AND action = 'create'",
    )
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(actor, "ai");
}

#[tokio::test]
async fn ai_cannot_use_unknown_channels_or_manage_channels() {
    let t = setup(vec![]).await;
    t.script.replies.lock().unwrap().extend([
        tool(
            "d",
            "draft_automation_rule",
            json!({
                "name": "x",
                "trigger": {"kind": "task_completed"},
                "actions": [{"kind": "send_notification", "channel": "Nope"}]
            }),
        ),
        tool(
            "raw",
            "draft_automation_rule",
            json!({
                "name": "x",
                "trigger": {"kind": "task_completed"},
                "action": {"kind": "send_notification", "channel_id": "11111111-1111-4111-8111-111111111111"}
            }),
        ),
        tool("c", "create_notification_channel", json!({"name": "x"})),
        text("done"),
    ]);
    let (_, ev) = say(&t, None, "notify me").await;
    assert!(maybe_proposal(&ev).is_none());
    let reqs = t.script.requests.lock().unwrap();
    let last = serde_json::to_string(&reqs.last().unwrap().messages).unwrap();
    assert!(last.contains("no notification channel"), "{last}");
    assert!(
        last.contains("needs `channel`"),
        "raw ids are refused: {last}"
    );
    assert!(last.contains("Unknown tool"), "{last}");
    assert!(
        !reqs[0].tools.iter().any(|t| t["name"]
            .as_str()
            .is_some_and(|n| n.contains("channel") && n != "list_notification_channels")),
        "no channel-mutating tool is exposed"
    );
}
