//! Business logic. Used by both UI commands and (later) AI tools. Depends on repository traits only.

mod activity_service;
mod calendar_service;
mod note_service;
mod project_service;
mod search_service;
mod settings_service;
mod stats_service;
mod task_service;
mod template_service;
mod timer_service;

pub use activity_service::ActivityService;
pub use calendar_service::{CalendarService, CreateEventInput, OverrideInput, UpdateEventInput};
pub use note_service::{NoteDetail, NoteService};
pub use project_service::{ProjectService, UpdateProjectInput};
pub use search_service::SearchService;
pub use settings_service::SettingsService;
pub use stats_service::StatsService;
pub use task_service::{CreateTaskInput, TaskService, UpdateTaskInput};
pub use template_service::TemplateService;
pub use timer_service::{StartTimerInput, TimerService};

#[cfg(test)]
mod calendar_tests;
#[cfg(test)]
mod kanban_tests;
#[cfg(test)]
mod notes_search_tests;
#[cfg(test)]
mod stats_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod timer_tests;
