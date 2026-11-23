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
        pool,
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
