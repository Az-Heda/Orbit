//! In-process domain events, published by services after a mutation commits.
//! The automation engine subscribes; nothing else may depend on them for correctness.

use std::sync::Arc;

use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq)]
pub enum DomainEvent {
    TaskCreated {
        task_id: String,
        title: String,
    },
    TaskMoved {
        task_id: String,
        title: String,
        to_column_id: String,
        to_column_kind: String,
    },
    TaskCompleted {
        task_id: String,
        title: String,
    },
    TimerEnded {
        timer_id: String,
        label: Option<String>,
    },
    InboxAdded {
        item_id: String,
        text: String,
    },
}

/// An event plus its causal chain, used to stop automation loops.
#[derive(Debug, Clone)]
pub struct Envelope {
    pub event: DomainEvent,
    /// 0 for user-initiated changes; +1 for each automation hop.
    pub depth: u8,
    /// Rule whose action (directly) caused this event.
    pub origin_rule: Option<String>,
}

tokio::task_local! {
    /// Set by the automation engine while it runs an action, so events published by the
    /// services the action calls are stamped with their cause.
    pub static CAUSE: (u8, String);
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Envelope>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(256).0,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.tx.subscribe()
    }

    pub fn publish(&self, event: DomainEvent) {
        let (depth, origin_rule) = CAUSE
            .try_with(|(d, r)| (d.saturating_add(1), Some(r.clone())))
            .unwrap_or((0, None));
        let _ = self.tx.send(Envelope {
            event,
            depth,
            origin_rule,
        }); // no subscribers is fine
    }
}

/// Optional bus handle held by services.
pub type MaybeBus = Option<Arc<EventBus>>;

pub fn emit(bus: &MaybeBus, event: DomainEvent) {
    if let Some(b) = bus {
        b.publish(event);
    }
}
