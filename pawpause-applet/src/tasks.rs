use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::overlay::notify;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Project {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Task {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub done: bool,
    /// `None` means a top-level task.
    #[serde(default)]
    pub parent_id: Option<u64>,
    /// `None` means no project assigned.
    #[serde(default)]
    pub project_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TasksStore {
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub active_task_id: Option<u64>,
    #[serde(default)]
    pub next_task_id: u64,
    #[serde(default)]
    pub next_project_id: u64,
}

impl TasksStore {
    pub fn project_name(&self, id: u64) -> Option<&str> {
        self.projects.iter().find(|p| p.id == id).map(|p| p.name.as_str())
    }

    /// Resolves the active task's project name, for crediting a finished
    /// pomodoro work session. `""` if there's no active task, or it has no
    /// project.
    pub fn active_project_name(&self) -> String {
        self.active_task_id
            .and_then(|id| self.tasks.iter().find(|t| t.id == id))
            .and_then(|t| t.project_id)
            .and_then(|pid| self.project_name(pid))
            .unwrap_or_default()
            .to_string()
    }

    /// The starred task's title, if one is set — shown in the applet popup so
    /// starting a session names what you're about to work on.
    pub fn active_task_title(&self) -> Option<&str> {
        self.active_task_id
            .and_then(|id| self.tasks.iter().find(|t| t.id == id))
            .map(|t| t.title.as_str())
    }

    pub fn children_of(&self, id: u64) -> Vec<&Task> {
        self.tasks.iter().filter(|t| t.parent_id == Some(id)).collect()
    }

    pub fn root_tasks(&self) -> Vec<&Task> {
        self.tasks.iter().filter(|t| t.parent_id.is_none()).collect()
    }

    /// (done, total) counted recursively over every descendant of `id`
    /// (children, grandchildren, ...) — not including `id` itself.
    pub fn progress(&self, id: u64) -> (usize, usize) {
        let mut done = 0;
        let mut total = 0;
        for child in self.children_of(id) {
            total += 1;
            if child.done {
                done += 1;
            }
            let (child_done, child_total) = self.progress(child.id);
            done += child_done;
            total += child_total;
        }
        (done, total)
    }

    pub fn add_task(&mut self, parent_id: Option<u64>, title: String, project_id: Option<u64>) -> u64 {
        let id = self.next_task_id;
        self.next_task_id += 1;
        self.tasks.push(Task {
            id,
            title,
            done: false,
            parent_id,
            project_id,
        });
        id
    }

    pub fn edit(&mut self, id: u64, title: String, project_id: Option<u64>) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.title = title;
            task.project_id = project_id;
        }
    }

    pub fn toggle_done(&mut self, id: u64) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.done = !task.done;
        }
    }

    /// Removes `id` and every descendant of it, clearing `active_task_id` if
    /// it pointed anywhere inside the deleted subtree.
    pub fn delete(&mut self, id: u64) {
        let mut to_remove = vec![id];
        let mut frontier = vec![id];
        while let Some(current) = frontier.pop() {
            for child in self.children_of(current) {
                to_remove.push(child.id);
                frontier.push(child.id);
            }
        }
        self.tasks.retain(|t| !to_remove.contains(&t.id));
        if self.active_task_id.is_some_and(|active| to_remove.contains(&active)) {
            self.active_task_id = None;
        }
    }

    pub fn set_active(&mut self, id: Option<u64>) {
        self.active_task_id = id;
    }

    pub fn active_projects(&self) -> Vec<&Project> {
        self.projects.iter().filter(|p| !p.archived).collect()
    }

    pub fn archived_projects(&self) -> Vec<&Project> {
        self.projects.iter().filter(|p| p.archived).collect()
    }

    pub fn add_project(&mut self, name: String) -> u64 {
        let id = self.next_project_id;
        self.next_project_id += 1;
        self.projects.push(Project {
            id,
            name,
            archived: false,
        });
        id
    }

    /// Archives rather than deletes, so tasks/stats keep a meaningful project
    /// name forever — `project_id` lookups never dangle.
    pub fn set_project_archived(&mut self, id: u64, archived: bool) {
        if let Some(project) = self.projects.iter_mut().find(|p| p.id == id) {
            project.archived = archived;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_chain() -> (TasksStore, u64, u64, u64) {
        let mut store = TasksStore::default();
        let root = store.add_task(None, "root".into(), None);
        let child = store.add_task(Some(root), "child".into(), None);
        let grandchild = store.add_task(Some(child), "grandchild".into(), None);
        (store, root, child, grandchild)
    }

    #[test]
    fn progress_rolls_up_through_the_whole_tree() {
        let (mut store, root, child, grandchild) = store_with_chain();
        assert_eq!(store.progress(root), (0, 2));

        store.toggle_done(grandchild);
        assert_eq!(store.progress(root), (1, 2));
        assert_eq!(store.progress(child), (1, 1));

        store.toggle_done(child);
        assert_eq!(store.progress(root), (2, 2));
    }

    #[test]
    fn delete_cascades_to_all_descendants() {
        let (mut store, root, child, grandchild) = store_with_chain();
        store.set_active(Some(grandchild));

        store.delete(child);

        assert_eq!(store.tasks.len(), 1);
        assert_eq!(store.tasks[0].id, root);
        assert_eq!(store.active_task_id, None, "active task inside the deleted subtree should clear");
    }

    #[test]
    fn deleting_a_leaf_does_not_disturb_siblings() {
        let mut store = TasksStore::default();
        let root = store.add_task(None, "root".into(), None);
        let a = store.add_task(Some(root), "a".into(), None);
        let b = store.add_task(Some(root), "b".into(), None);

        store.delete(a);

        assert_eq!(store.children_of(root).into_iter().map(|t| t.id).collect::<Vec<_>>(), vec![b]);
    }

    #[test]
    fn active_project_name_resolves_through_task_and_project() {
        let mut store = TasksStore::default();
        let project = store.add_project("Work".into());
        let task = store.add_task(None, "t".into(), Some(project));
        store.set_active(Some(task));

        assert_eq!(store.active_project_name(), "Work");
    }

    #[test]
    fn active_project_name_empty_when_no_project() {
        let mut store = TasksStore::default();
        let task = store.add_task(None, "t".into(), None);
        store.set_active(Some(task));

        assert_eq!(store.active_project_name(), "");
    }

    #[test]
    fn archiving_a_project_keeps_it_resolvable() {
        let mut store = TasksStore::default();
        let project = store.add_project("Old".into());
        store.set_project_archived(project, true);

        assert_eq!(store.project_name(project), Some("Old"));
        assert!(store.active_projects().is_empty());
        assert_eq!(store.archived_projects().len(), 1);
    }
}
