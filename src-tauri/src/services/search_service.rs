use std::sync::Arc;

use crate::domain::search::{KINDS, SearchHit, match_expression};
use crate::domain::validation::invalid;
use crate::error::DomainResult;
use crate::infrastructure::database::SearchRepository;

/// Global search over FTS5 indexes. Shared by the UI and (later) the AI `search_all` tool.
pub struct SearchService {
    repo: Arc<dyn SearchRepository>,
}

impl SearchService {
    pub fn new(repo: Arc<dyn SearchRepository>) -> Self {
        Self { repo }
    }

    /// `kinds`: restrict to these kinds (empty/None = all). `limit` is 1..=50 (default 20).
    pub async fn search(
        &self,
        query: &str,
        kinds: Option<Vec<String>>,
        limit: Option<i64>,
    ) -> DomainResult<Vec<SearchHit>> {
        let limit = limit.unwrap_or(20);
        if !(1..=50).contains(&limit) {
            return Err(invalid("limit must be between 1 and 50"));
        }
        let selected: Vec<&'static str> = match kinds.filter(|k| !k.is_empty()) {
            None => KINDS.to_vec(),
            Some(k) => k
                .iter()
                .map(|s| {
                    KINDS
                        .iter()
                        .copied()
                        .find(|x| x == s)
                        .ok_or_else(|| invalid(format!("unknown kind '{s}'")))
                })
                .collect::<DomainResult<_>>()?,
        };
        let Some(expr) = match_expression(query) else {
            return Ok(vec![]);
        };
        let mut hits = self.repo.search(&expr, &selected, limit).await?;
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit as usize);
        Ok(hits)
    }
}
