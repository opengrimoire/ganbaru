use std::collections::HashSet;

use super::templates::insert_default_project_graphs;

pub(super) const ROUTINE_GROUP_ID: &str = "group-routine";

#[derive(Clone, Copy)]
pub(super) struct BuiltInRoutineProject {
    id: &'static str,
    pub(super) name: &'static str,
    icon: &'static str,
    color: i64,
    pub(super) sort_order: i64,
    default_pomodoro_mode: &'static str,
    default_pomodoro_preset_key: Option<&'static str>,
}

pub(super) const BUILT_IN_ROUTINE_PROJECTS: &[BuiltInRoutineProject] = &[
    BuiltInRoutineProject {
        id: "project-routine-learning",
        name: "Learning",
        icon: "lucide:graduation-cap",
        color: 8,
        sort_order: 0,
        default_pomodoro_mode: "preset",
        default_pomodoro_preset_key: Some("adaptive"),
    },
    BuiltInRoutineProject {
        id: "project-routine-reading",
        name: "Reading",
        icon: "lucide:book-open",
        color: 25,
        sort_order: 10,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-exercise",
        name: "Exercise",
        icon: "lucide:sport-shoe",
        color: 0,
        sort_order: 20,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-hygiene",
        name: "Hygiene",
        icon: "lucide:bath",
        color: 15,
        sort_order: 30,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-eat",
        name: "Eating",
        icon: "lucide:apple",
        color: 13,
        sort_order: 40,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-commute",
        name: "Commute",
        icon: "lucide:bike",
        color: 17,
        sort_order: 50,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-social",
        name: "Social",
        icon: "lucide:heart",
        color: 21,
        sort_order: 60,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-chores",
        name: "Chores",
        icon: "lucide:shopping-cart",
        color: 4,
        sort_order: 70,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-leisure",
        name: "Leisure",
        icon: "lucide:clapperboard",
        color: 31,
        sort_order: 80,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-meditate",
        name: "Meditate",
        icon: "lucide:smile",
        color: 23,
        sort_order: 90,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-health",
        name: "Health",
        icon: "lucide:pill",
        color: 3,
        sort_order: 100,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
    BuiltInRoutineProject {
        id: "project-routine-sleep",
        name: "Sleep",
        icon: "lucide:bed",
        color: 30,
        sort_order: 110,
        default_pomodoro_mode: "none",
        default_pomodoro_preset_key: None,
    },
];

pub(super) fn built_in_routine_project(project_id: &str) -> Option<&'static BuiltInRoutineProject> {
    BUILT_IN_ROUTINE_PROJECTS
        .iter()
        .find(|project| project.id == project_id)
}

async fn built_in_routine_defaults_intact(pool: &sqlx::SqlitePool) -> Result<bool, String> {
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT EXISTS(SELECT 1 FROM project_groups WHERE id = ",
    );
    query
        .push_bind(ROUTINE_GROUP_ID)
        .push(" AND name = 'Routine') AND (SELECT COUNT(*) FROM projects WHERE ");
    for (index, project) in BUILT_IN_ROUTINE_PROJECTS.iter().enumerate() {
        if index > 0 {
            query.push(" OR ");
        }
        query
            .push("(id = ")
            .push_bind(project.id)
            .push(" AND group_id = ")
            .push_bind(ROUTINE_GROUP_ID)
            .push(" AND name = ")
            .push_bind(project.name)
            .push(" AND sort_order = ")
            .push_bind(project.sort_order)
            .push(")");
    }
    query
        .push(") = ")
        .push_bind(BUILT_IN_ROUTINE_PROJECTS.len() as i64);
    query
        .build_query_scalar::<i64>()
        .fetch_one(pool)
        .await
        .map(|value| value != 0)
        .map_err(|e| format!("check built-in Routine integrity: {e}"))
}

async fn repair_built_in_routine_defaults(pool: &sqlx::SqlitePool) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin built-in Routine repair: {e}"))?;

    let mut existing_query =
        sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT id FROM projects WHERE id IN (");
    {
        let mut separated = existing_query.separated(", ");
        for project in BUILT_IN_ROUTINE_PROJECTS {
            separated.push_bind(project.id);
        }
    }
    existing_query.push(")");
    let existing_ids: HashSet<String> = existing_query
        .build_query_scalar()
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("load existing built-in Routine projects: {e}"))?
        .into_iter()
        .collect();
    let missing_project_ids: Vec<&str> = BUILT_IN_ROUTINE_PROJECTS
        .iter()
        .filter(|project| !existing_ids.contains(project.id))
        .map(|project| project.id)
        .collect();

    sqlx::query(
        "INSERT OR IGNORE INTO project_groups (id, name, icon, color, sort_order)
         VALUES (?, 'Routine', 'lucide:repeat', 0, 0)",
    )
    .bind(ROUTINE_GROUP_ID)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("restore built-in Routine group: {e}"))?;
    sqlx::query(
        "UPDATE project_groups
         SET name = 'Routine'
         WHERE id = ? AND name <> 'Routine'",
    )
    .bind(ROUTINE_GROUP_ID)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("normalize built-in Routine group: {e}"))?;

    let mut insert_projects = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "INSERT OR IGNORE INTO projects (id, group_id, name, icon, color, sort_order, default_pomodoro_mode, default_pomodoro_preset_key) ",
    );
    insert_projects.push_values(BUILT_IN_ROUTINE_PROJECTS, |mut row, project| {
        row.push_bind(project.id)
            .push_bind(ROUTINE_GROUP_ID)
            .push_bind(project.name)
            .push_bind(project.icon)
            .push_bind(project.color)
            .push_bind(project.sort_order)
            .push_bind(project.default_pomodoro_mode)
            .push_bind(project.default_pomodoro_preset_key);
    });
    insert_projects
        .build()
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("restore built-in Routine projects: {e}"))?;

    for project in BUILT_IN_ROUTINE_PROJECTS {
        sqlx::query(
            "INSERT OR IGNORE INTO project_working_folders
                (id, project_id, display_name, kind, managed_relative_path, sort_order)
             VALUES (?, ?, ?, 'managed', ?, 0)",
        )
        .bind(format!(
            "working-folder-{}",
            project.id.strip_prefix("project-").unwrap_or(project.id)
        ))
        .bind(project.id)
        .bind(project.name)
        .bind(format!("projects/{}", project.id))
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("restore built-in Routine working folder: {e}"))?;
    }

    let mut normalize_projects =
        sqlx::QueryBuilder::<sqlx::Sqlite>::new("UPDATE projects SET group_id = ");
    normalize_projects
        .push_bind(ROUTINE_GROUP_ID)
        .push(", name = CASE id ");
    for project in BUILT_IN_ROUTINE_PROJECTS {
        normalize_projects
            .push("WHEN ")
            .push_bind(project.id)
            .push(" THEN ")
            .push_bind(project.name)
            .push(" ");
    }
    normalize_projects.push("END, sort_order = CASE id ");
    for project in BUILT_IN_ROUTINE_PROJECTS {
        normalize_projects
            .push("WHEN ")
            .push_bind(project.id)
            .push(" THEN ")
            .push_bind(project.sort_order)
            .push(" ");
    }
    normalize_projects.push("END WHERE id IN (");
    {
        let mut separated = normalize_projects.separated(", ");
        for project in BUILT_IN_ROUTINE_PROJECTS {
            separated.push_bind(project.id);
        }
    }
    normalize_projects.push(")");
    normalize_projects
        .build()
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("normalize built-in Routine projects: {e}"))?;

    insert_default_project_graphs(&mut tx, &missing_project_ids).await?;

    tx.commit()
        .await
        .map_err(|e| format!("commit built-in Routine repair: {e}"))
}

pub(super) async fn ensure_built_in_routine_defaults(
    pool: &sqlx::SqlitePool,
) -> Result<(), String> {
    if built_in_routine_defaults_intact(pool).await? {
        return Ok(());
    }
    repair_built_in_routine_defaults(pool).await
}
