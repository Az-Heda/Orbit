//! Business logic. Used by both UI commands and (later) AI tools. Depends on repository traits only.

mod activity_service;
mod ai_service;
mod automation_engine;
mod automation_service;
mod backup_service;
mod calendar_service;
mod external_calendar_service;
mod inbox_service;
mod note_service;
mod notifier;
mod project_service;
mod scheduler;
mod search_service;
mod settings_service;
mod stats_service;
mod task_service;
mod template_service;
mod timer_service;
mod update_service;

pub use activity_service::ActivityService;
pub use ai_service::{AiService, ChatEvent, ConversationDetail, StreamSink};
pub use automation_engine::AutomationEngine;
pub use automation_service::{AutomationService, RuleInput};
pub use backup_service::{BackupInfo, BackupService, BackupStatus, apply_pending_import};
pub use calendar_service::{
    CalendarService, CreateEventInput, OverrideInput, UpdateEventInput, ensure_editable,
};
pub use external_calendar_service::ExternalCalendarService;
pub use inbox_service::InboxService;
pub use note_service::{NoteDetail, NoteService};
pub use notifier::{Notifier, NullNotifier};
pub use project_service::{ProjectService, UpdateProjectInput};
pub use scheduler::{Scheduler, TickReport};
pub use search_service::SearchService;
pub use settings_service::SettingsService;
pub use stats_service::StatsService;
pub use task_service::{CreateTaskInput, TaskService, UpdateTaskInput, validate_priority};
pub use template_service::TemplateService;
pub use timer_service::{Clock, StartTimerInput, TimerService};
pub use update_service::{UpdateInfo, UpdateService, cleanup_old_exe};

#[cfg(test)]
mod ai_tests;
#[cfg(test)]
mod calendar_tests;
#[cfg(test)]
mod kanban_tests;
#[cfg(test)]
mod notes_search_tests;
#[cfg(test)]
mod phase2_tests;
#[cfg(test)]
mod stats_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod timer_tests;
