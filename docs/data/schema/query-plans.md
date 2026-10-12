# Hot SQLite query plans

Latency-sensitive reads use EXPLAIN QUERY PLAN tests against the fully migrated schema. An index is justified by an exact production query, not by speculation or a generic desire to index every foreign key.

## Centrally asserted plans

The ganbaru-db query-plan test protects these reads:

| Domain | Production read | Expected access path |
| --- | --- | --- |
| Projects | Task counts by status within one project | idx_project_tasks_project_status |
| Projects | Dependencies from one blocked task | idx_project_task_dependencies_blocked |
| Notes | Active sidebar pages in display order | idx_notes_pages_active |
| Notes | Visible child blocks under one parent | idx_notes_blocks_parent_block |
| Distraction blocker | Usage samples over a local-date window in source order | idx_distractions_usage_samples_date_source |
| Pomodoro | Open run lookup | idx_pomodoro_runs_open |
| Pomodoro | Global active segment lookup | idx_pomodoro_segments_single_active |
| Quick notes | Active notes in pin and manual order | idx_quick_notes_active |
| Quick notes | Active notes for one tag in display order | idx_quick_notes_tag_active |

These tests fail when the planner regresses to a full table scan or avoidable temporary sort where the named index should satisfy the predicate and ordering.

## Domain-owned plan tests

Some hot reads are best asserted beside their domain query because their SQL shape is more specialized:

- Calendar window overlap, notification hydration, exceptions, additional dates, overrides, attendees, and related import reads are asserted in calendar read tests.
- Notes full-text keyset paging is asserted in search tests. Database row-window reads also assert indexed filtering and ordering.
- Music library review uses idx_music_library_items_review.
- Ordered music playlist membership uses idx_music_playlist_memberships_order.
- Native Music checkpoint reads use the primary-key autoindex on music_session_checkpoints.device_id.

The absence of a query from this page does not mean it may scan without review. It may have a domain-local assertion or may not yet have demonstrated enough scale to justify another index.

## Adding or changing an index

1. Capture the exact production predicate, join, and ordering.
2. Populate representative cardinality and selectivity where planner choice depends on it.
3. Assert the useful access path with EXPLAIN QUERY PLAN.
4. Add the narrowest index that satisfies the read without duplicating an equivalent prefix.
5. Measure write and storage cost for large or frequently updated tables.
6. Add a new migration. Never rewrite the baseline or an applied migration.
7. Update this page only when the read belongs in the durable hot-path index.

Avoid assertions against the complete human-readable planner string when a stable index-name check is sufficient. SQLite planner wording can change without a performance regression.
