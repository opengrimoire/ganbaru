//! Deterministic finish-to-start planning over complete, admitted canonical inputs.

use super::dependency_cascade::{
    CascadeConflict, CascadeItem, CascadeReason, ConflictReason, DependencyCascadePreview,
    MAX_BYTES,
};
use super::dependency_cascade_graph::{CascadeGraph, CascadeTask};
use chrono::{Datelike, Days, NaiveDate};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

#[derive(Clone, Copy)]
struct Dates {
    start: Option<NaiveDate>,
    due: Option<NaiveDate>,
    end: Option<NaiveDate>,
}

impl Dates {
    fn from_task(task: &CascadeTask) -> Result<Self, ConflictReason> {
        fn parse(value: &Option<String>) -> Result<Option<NaiveDate>, ConflictReason> {
            value
                .as_ref()
                .map(|value| {
                    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                        .map_err(|_| ConflictReason::InvalidDate)?;
                    if !(1..=9999).contains(&date.year()) || date.to_string() != *value {
                        return Err(ConflictReason::InvalidDate);
                    }
                    Ok(date)
                })
                .transpose()
        }
        Ok(Self {
            start: parse(&task.start_date)?,
            due: parse(&task.due_date)?,
            end: parse(&task.target_end_date)?,
        })
    }

    fn range(self, milestone: bool) -> Result<(NaiveDate, NaiveDate), ConflictReason> {
        let first = if milestone {
            self.end.or(self.due).or(self.start)
        } else {
            self.start.or(self.end).or(self.due)
        };
        let first = first.ok_or(ConflictReason::UndatedTask)?;
        let last = self
            .end
            .or(self.due)
            .or(self.start)
            .ok_or(ConflictReason::UndatedTask)?;
        Ok((first.min(last), first.max(last)))
    }

    fn shifted(self, days: u64) -> Result<Self, ConflictReason> {
        fn shift(value: Option<NaiveDate>, days: u64) -> Result<Option<NaiveDate>, ConflictReason> {
            value
                .map(|date| {
                    date.checked_add_days(Days::new(days))
                        .filter(|date| date.year() <= 9999)
                        .ok_or(ConflictReason::DateOverflow)
                })
                .transpose()
        }
        Ok(Self {
            start: shift(self.start, days)?,
            due: shift(self.due, days)?,
            end: shift(self.end, days)?,
        })
    }
}

struct PreviewBudget(usize);
impl PreviewBudget {
    fn charge(&mut self, values: &[&str]) -> Result<(), String> {
        self.0 += 1024 + values.iter().map(|value| value.len() * 6).sum::<usize>();
        if self.0 > MAX_BYTES {
            return Err("dependency preview exceeds its byte limit".into());
        }
        Ok(())
    }
}

struct GraphDigest {
    hash: Sha256,
    bytes: usize,
}
impl Write for GraphDigest {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_BYTES.saturating_sub(self.bytes) {
            return Err(io::Error::other(
                "dependency graph exceeds its serialized byte limit",
            ));
        }
        self.bytes += bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn graph_digest(project_id: &str, graph: &CascadeGraph) -> Result<String, String> {
    let mut writer = GraphDigest {
        hash: Sha256::new(),
        bytes: 0,
    };
    serde_json::to_writer(&mut writer, &(1_u8, project_id, graph))
        .map_err(|error| format!("digest dependency preview: {error}"))?;
    Ok(format!("{:x}", writer.hash.finalize()))
}

fn conflict(
    preview: &mut DependencyCascadePreview,
    budget: &mut PreviewBudget,
    dependency_id: &str,
    task_id: &str,
    title: &str,
    reason: ConflictReason,
) -> Result<(), String> {
    budget.charge(&[dependency_id, task_id, title])?;
    preview.conflicts.push(CascadeConflict {
        dependency_id: dependency_id.into(),
        task_id: task_id.into(),
        title: title.into(),
        reason,
    });
    Ok(())
}

/// Topological traversal visits each task and edge once, never repeatedly shifting a cycle.
pub(super) fn build_preview(
    project_id: &str,
    graph: &CascadeGraph,
) -> Result<DependencyCascadePreview, String> {
    let digest = graph_digest(project_id, graph)?;
    let mut preview = DependencyCascadePreview {
        project_id: project_id.into(),
        digest,
        items: Vec::new(),
        conflicts: Vec::new(),
    };
    let mut budget = PreviewBudget(1024);
    let by_id: BTreeMap<_, _> = graph
        .tasks
        .iter()
        .enumerate()
        .map(|(index, task)| (task.id.as_str(), index))
        .collect();
    let mut incoming = vec![Vec::new(); graph.tasks.len()];
    let mut outgoing = vec![Vec::new(); graph.tasks.len()];
    for (edge_index, edge) in graph.dependencies.iter().enumerate() {
        match (
            by_id.get(edge.blocking_task_id.as_str()),
            by_id.get(edge.blocked_task_id.as_str()),
        ) {
            (Some(&source), Some(&destination)) => {
                incoming[destination].push(edge_index);
                outgoing[source].push(destination);
            }
            _ => conflict(
                &mut preview,
                &mut budget,
                &edge.id,
                &edge.blocked_task_id,
                by_id
                    .get(edge.blocked_task_id.as_str())
                    .map_or(edge.blocked_task_id.as_str(), |&index| {
                        graph.tasks[index].title.as_str()
                    }),
                ConflictReason::MissingEndpoint,
            )?,
        }
    }
    let mut indegree = incoming.iter().map(Vec::len).collect::<Vec<_>>();
    let mut ready: BTreeSet<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect();
    let mut dates = graph.tasks.iter().map(Dates::from_task).collect::<Vec<_>>();
    let mut visited = vec![false; graph.tasks.len()];
    while let Some(index) = ready.pop_first() {
        visited[index] = true;
        let task = &graph.tasks[index];
        let current = dates[index]
            .and_then(|dates| dates.range(task.milestone != 0).map(|range| (dates, range)));
        let mut needed: Option<NaiveDate> = None;
        let mut reasons = Vec::new();
        for &edge_index in &incoming[index] {
            let edge = &graph.dependencies[edge_index];
            let source = by_id[edge.blocking_task_id.as_str()];
            let source_task = &graph.tasks[source];
            let required = dates[source]
                .and_then(|dates| dates.range(source_task.milestone != 0))
                .and_then(|(_, end)| {
                    end.checked_add_days(Days::new(1))
                        .filter(|date| date.year() <= 9999)
                        .ok_or(ConflictReason::DateOverflow)
                });
            let (required, (_, (start, _))) = match (required, current) {
                (Ok(required), Ok(current)) => (required, current),
                (Err(reason), _) | (_, Err(reason)) => {
                    conflict(
                        &mut preview,
                        &mut budget,
                        &edge.id,
                        &task.id,
                        &task.title,
                        reason,
                    )?;
                    continue;
                }
            };
            if start >= required {
                continue;
            }
            let protection = if task.archived != 0 {
                Some(ConflictReason::Archived)
            } else if task.completed != 0 {
                Some(ConflictReason::Completed)
            } else if task.scheduled != 0 {
                Some(ConflictReason::Scheduled)
            } else {
                None
            };
            if let Some(reason) = protection {
                conflict(
                    &mut preview,
                    &mut budget,
                    &edge.id,
                    &task.id,
                    &task.title,
                    reason,
                )?;
                continue;
            }
            needed = Some(needed.map_or(required, |date| date.max(required)));
            budget.charge(&[&edge.id, &source_task.id, &source_task.title])?;
            reasons.push(CascadeReason {
                dependency_id: edge.id.clone(),
                blocking_task_id: source_task.id.clone(),
                blocking_title: source_task.title.clone(),
                required_start_date: required.to_string(),
            });
        }
        if let (Some(required), Ok((original, (start, end)))) = (needed, current) {
            let shift_days = (required - start).num_days();
            match original.shifted(shift_days as u64) {
                Ok(next) => {
                    let (next_start, next_end) = next
                        .range(task.milestone != 0)
                        .map_err(|_| "shifted task lost its dates")?;
                    budget.charge(&[&task.id, &task.title])?;
                    preview.items.push(CascadeItem {
                        task_id: task.id.clone(),
                        title: task.title.clone(),
                        shift_days,
                        original_start_date: task.start_date.clone(),
                        original_due_date: task.due_date.clone(),
                        original_target_end_date: task.target_end_date.clone(),
                        original_range_start: start.to_string(),
                        original_range_end: end.to_string(),
                        next_start_date: next.start.map(|date| date.to_string()),
                        next_due_date: next.due.map(|date| date.to_string()),
                        next_target_end_date: next.end.map(|date| date.to_string()),
                        next_range_start: next_start.to_string(),
                        next_range_end: next_end.to_string(),
                        reasons,
                    });
                    dates[index] = Ok(next);
                }
                Err(reason) => {
                    for reason_edge in reasons {
                        conflict(
                            &mut preview,
                            &mut budget,
                            &reason_edge.dependency_id,
                            &task.id,
                            &task.title,
                            reason,
                        )?;
                    }
                }
            }
        }
        for &destination in &outgoing[index] {
            indegree[destination] -= 1;
            if indegree[destination] == 0 {
                ready.insert(destination);
            }
        }
    }
    for edge in &graph.dependencies {
        if let (Some(&source), Some(&destination)) = (
            by_id.get(edge.blocking_task_id.as_str()),
            by_id.get(edge.blocked_task_id.as_str()),
        ) {
            if !visited[source] && !visited[destination] {
                conflict(
                    &mut preview,
                    &mut budget,
                    &edge.id,
                    &edge.blocked_task_id,
                    &graph.tasks[destination].title,
                    ConflictReason::Cycle,
                )?;
            }
        }
    }
    preview.items.sort_by(|left, right| {
        (&left.next_range_start, &left.title, &left.task_id).cmp(&(
            &right.next_range_start,
            &right.title,
            &right.task_id,
        ))
    });
    preview.conflicts.sort_by(|left, right| {
        (&left.dependency_id, &left.task_id).cmp(&(&right.dependency_id, &right.task_id))
    });
    Ok(preview)
}
