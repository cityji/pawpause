use std::collections::HashSet;

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{Alignment, Length, Size};
use cosmic::widget::dropdown::dropdown;
use cosmic::widget::{self, checkbox, column, icon, nav_bar, row, text, text_input, Space};
use cosmic::{theme, Element};

use pawpause_applet::stats::{self, format_hhmm};
use pawpause_applet::tasks::{self, TasksStore};

const APP_ID: &str = "com.pawpause.App";
const INDENT_PX: f32 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Tasks,
    Projects,
    Statistics,
}

struct App {
    core: Core,
    nav_model: nav_bar::Model,
    store: TasksStore,
    sessions: Vec<stats::SessionRecord>,

    /// Nodes whose children are hidden. Empty by default — everything starts
    /// expanded.
    collapsed: HashSet<u64>,

    new_task_title: String,
    new_task_project: Option<u64>,
    /// `Some(id)` while composing a subtask of `id`; `None` adds a top-level task.
    new_task_parent: Option<u64>,

    editing: Option<u64>,
    edit_title: String,
    edit_project: Option<u64>,

    new_project_name: String,
}

#[derive(Clone, Debug)]
enum Message {
    NewTaskTitleChanged(String),
    NewTaskProjectSelected(usize),
    AddTask,
    StartAddSubtask(u64),
    CancelAddSubtask,

    ToggleDone(u64),
    ToggleExpanded(u64),
    SetActive(u64),
    ClearActive,
    DeleteTask(u64),

    StartEdit(u64),
    EditTitleChanged(String),
    EditProjectSelected(usize),
    SaveEdit,
    CancelEdit,

    NewProjectNameChanged(String),
    AddProject,
    ArchiveProject(u64),
    RestoreProject(u64),
}

impl App {
    fn save_tasks(&self) {
        tasks::save(&self.store);
    }

    /// Depth-first, pre-order (id, depth) pairs for every visible row —
    /// children of a collapsed node are skipped entirely.
    fn visible_rows(&self) -> Vec<(u64, usize)> {
        fn walk(store: &TasksStore, collapsed: &HashSet<u64>, id: u64, depth: usize, out: &mut Vec<(u64, usize)>) {
            out.push((id, depth));
            if collapsed.contains(&id) {
                return;
            }
            for child in store.children_of(id) {
                walk(store, collapsed, child.id, depth + 1, out);
            }
        }

        let mut out = Vec::new();
        for root in self.store.root_tasks() {
            walk(&self.store, &self.collapsed, root.id, 0, &mut out);
        }
        out
    }

    fn project_index(&self, project_id: Option<u64>) -> Option<usize> {
        project_id.and_then(|id| self.store.active_projects().iter().position(|p| p.id == id))
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let mut nav_model = nav_bar::Model::default();
        nav_model.insert().text("Tasks").data(Page::Tasks);
        nav_model.insert().text("Projects").data(Page::Projects);
        nav_model.insert().text("Statistics").data(Page::Statistics);
        nav_model.activate_position(0);

        let app = App {
            core,
            nav_model,
            store: tasks::load(),
            sessions: stats::load_sessions(),
            collapsed: HashSet::new(),
            new_task_title: String::new(),
            new_task_project: None,
            new_task_parent: None,
            editing: None,
            edit_title: String::new(),
            edit_project: None,
            new_project_name: String::new(),
        };

        (app, Task::none())
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav_model)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Message> {
        self.nav_model.activate(id);
        match self.nav_model.active_data::<Page>() {
            Some(Page::Statistics) => self.sessions = stats::load_sessions(),
            _ => self.store = tasks::load(),
        }
        Task::none()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NewTaskTitleChanged(v) => self.new_task_title = v,
            Message::NewTaskProjectSelected(index) => {
                self.new_task_project = self.store.active_projects().get(index).map(|p| p.id);
            }
            Message::AddTask => {
                let title = self.new_task_title.trim().to_string();
                if !title.is_empty() {
                    self.store.add_task(self.new_task_parent, title, self.new_task_project);
                    self.new_task_title.clear();
                    self.new_task_project = None;
                    self.new_task_parent = None;
                    self.save_tasks();
                }
            }
            Message::StartAddSubtask(id) => {
                self.new_task_parent = Some(id);
                self.collapsed.remove(&id);
            }
            Message::CancelAddSubtask => self.new_task_parent = None,

            Message::ToggleDone(id) => {
                self.store.toggle_done(id);
                self.save_tasks();
            }
            Message::ToggleExpanded(id) => {
                if !self.collapsed.insert(id) {
                    self.collapsed.remove(&id);
                }
            }
            Message::SetActive(id) => {
                self.store.set_active(Some(id));
                self.save_tasks();
            }
            Message::ClearActive => {
                self.store.set_active(None);
                self.save_tasks();
            }
            Message::DeleteTask(id) => {
                self.store.delete(id);
                if self.editing == Some(id) {
                    self.editing = None;
                }
                if self.new_task_parent == Some(id) {
                    self.new_task_parent = None;
                }
                self.save_tasks();
            }

            Message::StartEdit(id) => {
                if let Some(task) = self.store.tasks.iter().find(|t| t.id == id) {
                    self.edit_title = task.title.clone();
                    self.edit_project = task.project_id;
                    self.editing = Some(id);
                }
            }
            Message::EditTitleChanged(v) => self.edit_title = v,
            Message::EditProjectSelected(index) => {
                self.edit_project = self.store.active_projects().get(index).map(|p| p.id);
            }
            Message::SaveEdit => {
                if let Some(id) = self.editing.take() {
                    self.store.edit(id, self.edit_title.trim().to_string(), self.edit_project);
                    self.save_tasks();
                }
            }
            Message::CancelEdit => self.editing = None,

            Message::NewProjectNameChanged(v) => self.new_project_name = v,
            Message::AddProject => {
                let name = self.new_project_name.trim().to_string();
                if !name.is_empty() {
                    self.store.add_project(name);
                    self.new_project_name.clear();
                    self.save_tasks();
                }
            }
            Message::ArchiveProject(id) => {
                self.store.set_project_archived(id, true);
                self.save_tasks();
            }
            Message::RestoreProject(id) => {
                self.store.set_project_archived(id, false);
                self.save_tasks();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        match self.nav_model.active_data::<Page>() {
            Some(Page::Projects) => self.projects_view(),
            Some(Page::Statistics) => self.statistics_view(),
            _ => self.tasks_view(),
        }
    }
}

fn icon_button<'a>(name: &'static str, on_press: Message) -> Element<'a, Message> {
    widget::button::icon(icon::from_name(name).size(16))
        .extra_small()
        .on_press(on_press)
        .into()
}

fn project_chip<'a>(name: &str) -> Element<'a, Message> {
    widget::container(text(name.to_string()).size(12))
        .class(theme::Container::Primary)
        .padding([2, 8])
        .into()
}

impl App {
    fn add_task_bar(&self) -> Element<'_, Message> {
        let project_names: Vec<String> = self.store.active_projects().iter().map(|p| p.name.clone()).collect();
        let selected = self.project_index(self.new_task_project);

        let mut bar = column(Vec::new()).spacing(6);

        if let Some(parent_id) = self.new_task_parent {
            if let Some(parent) = self.store.tasks.iter().find(|t| t.id == parent_id) {
                bar = bar.push(
                    row(Vec::new())
                        .spacing(6)
                        .align_y(Alignment::Center)
                        .push(text(format!("Adding a subtask under \"{}\"", parent.title)).size(12))
                        .push(widget::button::text("Cancel").on_press(Message::CancelAddSubtask)),
                );
            }
        }

        bar.push(
            row(Vec::new())
                .spacing(4)
                .push(
                    text_input("New task", &self.new_task_title)
                        .on_input(Message::NewTaskTitleChanged)
                        .width(Length::FillPortion(2)),
                )
                .push(
                    dropdown(project_names, selected, Message::NewTaskProjectSelected)
                        .width(Length::FillPortion(1)),
                )
                .push(widget::button::suggested("Add").on_press(Message::AddTask)),
        )
        .into()
    }

    fn task_row(&self, id: u64, depth: usize) -> Element<'_, Message> {
        let task = self.store.tasks.iter().find(|t| t.id == id).expect("row id came from the store");

        if self.editing == Some(id) {
            let project_names: Vec<String> = self.store.active_projects().iter().map(|p| p.name.clone()).collect();
            let selected = self.project_index(self.edit_project);
            return row(Vec::new())
                .spacing(4)
                .align_y(Alignment::Center)
                .push(Space::new().width(Length::Fixed(depth as f32 * INDENT_PX)))
                .push(
                    text_input("Title", &self.edit_title)
                        .on_input(Message::EditTitleChanged)
                        .width(Length::FillPortion(2)),
                )
                .push(dropdown(project_names, selected, Message::EditProjectSelected).width(Length::FillPortion(1)))
                .push(widget::button::suggested("Save").on_press(Message::SaveEdit))
                .push(widget::button::text("Cancel").on_press(Message::CancelEdit))
                .into();
        }

        let has_children = !self.store.children_of(id).is_empty();
        let is_active = self.store.active_task_id == Some(id);

        let mut children = vec![Space::new().width(Length::Fixed(depth as f32 * INDENT_PX)).into()];

        children.push(if has_children {
            let expanded = !self.collapsed.contains(&id);
            let name = if expanded { "pan-down-symbolic" } else { "pan-end-symbolic" };
            icon_button(name, Message::ToggleExpanded(id))
        } else {
            Space::new().width(Length::Fixed(24.0)).into()
        });

        children.push(checkbox(task.done).on_toggle(move |_| Message::ToggleDone(id)).into());

        let title = if task.done {
            format!("{} (done)", task.title)
        } else {
            task.title.clone()
        };
        children.push(text(title).width(Length::Fill).into());

        if let Some(project_id) = task.project_id {
            if let Some(name) = self.store.project_name(project_id) {
                children.push(project_chip(name));
            }
        }

        if has_children {
            let (done, total) = self.store.progress(id);
            children.push(text(format!("{done}/{total}")).size(12).into());
        }

        let star_icon = if is_active { "starred-symbolic" } else { "non-starred-symbolic" };
        children.push(icon_button(
            star_icon,
            if is_active { Message::ClearActive } else { Message::SetActive(id) },
        ));
        children.push(icon_button("list-add-symbolic", Message::StartAddSubtask(id)));
        children.push(icon_button("document-edit-symbolic", Message::StartEdit(id)));
        children.push(icon_button("user-trash-symbolic", Message::DeleteTask(id)));

        widget::settings::item_row(children).into()
    }

    fn tasks_view(&self) -> Element<'_, Message> {
        let rows = self.visible_rows();

        let body: Element<'_, Message> = if rows.is_empty() {
            widget::container(text("No tasks yet — add one below to get started."))
                .padding(16)
                .into()
        } else {
            let mut list = widget::list_column();
            for (id, depth) in rows {
                list = list.add(self.task_row(id, depth));
            }
            list.into()
        };

        column(Vec::new())
            .padding(16)
            .spacing(12)
            .push(text("Tasks").size(20))
            .push(self.add_task_bar())
            .push(widget::scrollable(body))
            .into()
    }

    fn projects_view(&self) -> Element<'_, Message> {
        let mut list = widget::list_column();
        for project in self.store.active_projects() {
            list = list.add(widget::settings::item_row(vec![
                text(project.name.clone()).width(Length::Fill).into(),
                icon_button("archive-symbolic", Message::ArchiveProject(project.id)),
            ]));
        }

        let mut view = column(Vec::new())
            .padding(16)
            .spacing(12)
            .push(text("Projects").size(20))
            .push(
                row(Vec::new())
                    .spacing(4)
                    .push(
                        text_input("New project", &self.new_project_name)
                            .on_input(Message::NewProjectNameChanged)
                            .width(Length::Fill),
                    )
                    .push(widget::button::suggested("Add").on_press(Message::AddProject)),
            )
            .push(list);

        let archived = self.store.archived_projects();
        if !archived.is_empty() {
            let mut archived_list = widget::list_column();
            for project in archived {
                archived_list = archived_list.add(widget::settings::item_row(vec![
                    text(project.name.clone()).width(Length::Fill).into(),
                    icon_button("edit-undo-symbolic", Message::RestoreProject(project.id)),
                ]));
            }
            view = view.push(text("Archived").size(14)).push(archived_list);
        }

        view.into()
    }

    fn statistics_view(&self) -> Element<'_, Message> {
        let summary = stats::summary(&self.sessions);
        let breakdown = stats::week_breakdown(&self.sessions);

        let tiles = row(Vec::new())
            .spacing(16)
            .push(stat_tile("Hours focused", format!("{:.1}", summary.hours_focused)))
            .push(stat_tile("Days accessed", summary.days_accessed.to_string()))
            .push(stat_tile("Day streak", summary.day_streak.to_string()));

        let mut table = widget::list_column();
        if breakdown.is_empty() {
            table = table.add(text("No focused time logged this week yet."));
        } else {
            for (project, seconds) in &breakdown {
                table = table.add(widget::settings::item_row(vec![
                    text(project.clone()).width(Length::Fill).into(),
                    text(format_hhmm(*seconds)).into(),
                ]));
            }
        }

        column(Vec::new())
            .padding(16)
            .spacing(16)
            .push(text("Activity Summary").size(20))
            .push(tiles)
            .push(text("Focus Hours This Week").size(16))
            .push(table)
            .into()
    }
}

fn stat_tile<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    widget::container(
        column(Vec::new())
            .align_x(Alignment::Center)
            .spacing(4)
            .push(text(value).size(28))
            .push(text(label)),
    )
    .class(theme::Container::Card)
    .padding(12)
    .width(Length::Fill)
    .into()
}

fn main() -> cosmic::iced::Result {
    let env = env_logger::Env::default().filter_or("PAWPAUSE_LOG", "warn");
    env_logger::init_from_env(env);
    let settings = Settings::default().size(Size::new(720.0, 560.0));
    cosmic::app::run::<App>(settings, ())
}
