use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::overlay::notify;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Task {
    pub id: u64,
    pub title: String,
    /// Empty string means "no project".
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TasksStore {
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub active_task_id: Option<u64>,
    #[serde(default)]
    pub next_id: u64,
}

impl TasksStore {
    pub fn active_project(&self) -> String {
        self.active_task_id
            .and_then(|id| self.tasks.iter().find(|t| t.id == id))
            .map(|t| t.project.clone())
            .unwrap_or_default()
    }

    pub fn add(&mut self, title: String, project: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(Task {
            id,
            title,
            project,
            done: false,
        });
        id
    }

    pub fn edit(&mut self, id: u64, title: String, project: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.title = title;
            task.project = project;
        }
    }

    pub fn toggle_done(&mut self, id: u64) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.done = !task.done;
        }
    }

    pub fn delete(&mut self, id: u64) {
        self.tasks.retain(|t| t.id != id);
        if self.active_task_id == Some(id) {
            self.active_task_id = None;
        }
    }

    pub fn set_active(&mut self, id: Option<u64>) {
        self.active_task_id = id;
    }
}

fn store_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("pawpause")
        .join("tasks.json")
}

/// Loads the tasks store, creating it empty if missing. A store that fails to
/// parse falls back to empty rather than silently discarding the user's tasks
/// file — they're notified so they can investigate.
pub fn load() -> TasksStore {
    let path = store_path();
    if !path.exists() {
        return TasksStore::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(store) => store,
            Err(err) => {
                notify(
                    "PawPause",
                    &format!("Tasks file at {} is invalid ({err}) — starting empty.", path.display()),
                );
                TasksStore::default()
            }
        },
        Err(err) => {
            notify("PawPause", &format!("Could not read tasks: {err} — starting empty."));
            TasksStore::default()
        }
    }
}

pub fn save(store: &TasksStore) {
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(store) {
        Ok(json) => {
            if let Err(err) = std::fs::write(&path, format!("{json}\n")) {
                notify("PawPause", &format!("Could not save tasks: {err}"));
            }
        }
        Err(err) => notify("PawPause", &format!("Could not serialize tasks: {err}")),
    }
}
