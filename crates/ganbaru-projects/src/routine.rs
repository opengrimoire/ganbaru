//! Built-in Routine group and projects seeded by the baseline schema.
//!
//! Commands refuse renaming, moving, reordering, or deleting them. Loads never repair them: seeds descend from one
//! database on every device, and no command removes them.

pub(super) const ROUTINE_GROUP_ID: &str = "group-routine";

#[derive(Clone, Copy)]
pub(super) struct BuiltInRoutineProject {
    pub(super) id: &'static str,
    pub(super) name: &'static str,
}

pub(super) const BUILT_IN_ROUTINE_PROJECTS: &[BuiltInRoutineProject] = &[
    BuiltInRoutineProject {
        id: "project-routine-learning",
        name: "Learning",
    },
    BuiltInRoutineProject {
        id: "project-routine-reading",
        name: "Reading",
    },
    BuiltInRoutineProject {
        id: "project-routine-exercise",
        name: "Exercise",
    },
    BuiltInRoutineProject {
        id: "project-routine-hygiene",
        name: "Hygiene",
    },
    BuiltInRoutineProject {
        id: "project-routine-eat",
        name: "Eating",
    },
    BuiltInRoutineProject {
        id: "project-routine-commute",
        name: "Commute",
    },
    BuiltInRoutineProject {
        id: "project-routine-social",
        name: "Social",
    },
    BuiltInRoutineProject {
        id: "project-routine-chores",
        name: "Chores",
    },
    BuiltInRoutineProject {
        id: "project-routine-leisure",
        name: "Leisure",
    },
    BuiltInRoutineProject {
        id: "project-routine-meditate",
        name: "Meditate",
    },
    BuiltInRoutineProject {
        id: "project-routine-health",
        name: "Health",
    },
    BuiltInRoutineProject {
        id: "project-routine-sleep",
        name: "Sleep",
    },
];

pub(super) fn built_in_routine_project(project_id: &str) -> Option<&'static BuiltInRoutineProject> {
    BUILT_IN_ROUTINE_PROJECTS
        .iter()
        .find(|project| project.id == project_id)
}
