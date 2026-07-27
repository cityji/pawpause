use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{Alignment, Length, Size};
use cosmic::widget::{self, checkbox, column, nav_bar, row, text, text_input};
use cosmic::Element;

use pawpause_applet::stats::{self, format_hhmm};
use pawpause_applet::tasks::{self, TasksStore};

const APP_ID: &str = "com.pawpause.App";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Tasks,
    Statistics,
}

struct App {
    core: Core,
    nav_model: nav_bar::Model,
    store: TasksStore,
    new_title: String,
    new_project: String,
    editing: Option<u64>,
    edit_title: String,
    edit_project: String,
    sessions: Vec<stats::SessionRecord>,
}

#[derive(Clone, Debug)]
enum Message {
    NewTitleChanged(String),
    NewProjectChanged(String),
    AddTask,
    ToggleDone(u64),
    DeleteTask(u64),
    SetActive(u64),
    ClearActive,
    StartEdit(u64),
    EditTitleChanged(String),
    EditProjectChanged(String),
    SaveEdit,
    CancelEdit,
}

impl App {
    fn save_tasks(&self) {
        tasks::save(&self.store);
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
        nav_model.insert().text("Statistics").data(Page::Statistics);
        nav_model.activate_position(0);

        let app = App {
            core,
            nav_model,
            store: tasks::load(),
            new_title: String::new(),
            new_project: String::new(),
            editing: None,
            edit_title: String::new(),
            edit_project: String::new(),
            sessions: stats::load_sessions(),
        };

        (app, Task::none())
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav_model)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Message> {
        self.nav_model.activate(id);
        if self.nav_model.active_data::<Page>() == Some(&Page::Statistics) {
            self.sessions = stats::load_sessions();
        } else {
            self.store = tasks::load();
        }
        Task::none()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NewTitleChanged(v) => self.new_title = v,
            Message::NewProjectChanged(v) => self.new_project = v,
            Message::AddTask => {
                let title = self.new_title.trim().to_string();
                if !title.is_empty() {
                    let project = self.new_project.trim().to_string();
                    self.store.add(title, project);
                    self.new_title.clear();
                    self.new_project.clear();
                    self.save_tasks();
                }
            }
            Message::ToggleDone(id) => {
                self.store.toggle_done(id);
                self.save_tasks();
            }
            Message::DeleteTask(id) => {
                self.store.delete(id);
                if self.editing == Some(id) {
                    self.editing = None;
                }
                self.save_tasks();
            }
            Message::SetActive(id) => {
                self.store.set_active(Some(id));
                self.save_tasks();
            }
            Message::ClearActive => {
                self.store.set_active(None);
                self.save_tasks();
            }
            Message::StartEdit(id) => {
                if let Some(task) = self.store.tasks.iter().find(|t| t.id == id) {
                    self.edit_title = task.title.clone();
                    self.edit_project = task.project.clone();
                    self.editing = Some(id);
                }
            }
            Message::EditTitleChanged(v) => self.edit_title = v,
            Message::EditProjectChanged(v) => self.edit_project = v,
            Message::SaveEdit => {
                if let Some(id) = self.editing.take() {
                    self.store
                        .edit(id, self.edit_title.trim().to_string(), self.edit_project.trim().to_string());
                    self.save_tasks();
                }
            }
            Message::CancelEdit => {
                self.editing = None;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        match self.nav_model.active_data::<Page>() {
            Some(Page::Statistics) => self.statistics_view(),
            _ => self.tasks_view(),
        }
    }
}

impl App {
    fn tasks_view(&self) -> Element<'_, Message> {
        let mut list = column(Vec::new()).spacing(8);

        for task in &self.store.tasks {
            let is_active = self.store.active_task_id == Some(task.id);

            if self.editing == Some(task.id) {
                list = list.push(
                    row(Vec::new())
                        .spacing(4)
                        .align_y(Alignment::Center)
                        .push(text_input("Title", &self.edit_title).on_input(Message::EditTitleChanged).width(Length::FillPortion(2)))
                        .push(text_input("Project", &self.edit_project).on_input(Message::EditProjectChanged).width(Length::FillPortion(1)))
                        .push(widget::button::text("Save").on_press(Message::SaveEdit))
                        .push(widget::button::text("Cancel").on_press(Message::CancelEdit)),
                );
                continue;
            }

            let title_text = if task.done {
                format!("{} (done)", task.title)
            } else {
                task.title.clone()
            };

            let mut task_row = row(Vec::new())
                .spacing(8)
                .align_y(Alignment::Center)
                .push(checkbox("", task.done).on_toggle(move |_| Message::ToggleDone(task.id)))
                .push(text(title_text).width(Length::Fill));

            if !task.project.is_empty() {
                task_row = task_row.push(text(format!("[{}]", task.project)));
            }

            task_row = task_row.push(widget::button::text(if is_active { "Active" } else { "Set active" }).on_press(
                if is_active {
                    Message::ClearActive
                } else {
                    Message::SetActive(task.id)
                },
            ));
            task_row = task_row
                .push(widget::button::text("Edit").on_press(Message::StartEdit(task.id)))
                .push(widget::button::text("Delete").on_press(Message::DeleteTask(task.id)));

            list = list.push(task_row);
        }

        column(Vec::new())
            .padding(16)
            .spacing(12)
            .push(text("Tasks").size(20))
            .push(
                row(Vec::new())
                    .spacing(4)
                    .push(text_input("New task", &self.new_title).on_input(Message::NewTitleChanged).width(Length::FillPortion(2)))
                    .push(text_input("Project (optional)", &self.new_project).on_input(Message::NewProjectChanged).width(Length::FillPortion(1)))
                    .push(widget::button::suggested("Add").on_press(Message::AddTask)),
            )
            .push(widget::scrollable(list))
            .into()
    }

    fn statistics_view(&self) -> Element<'_, Message> {
        let summary = stats::summary(&self.sessions);
        let breakdown = stats::week_breakdown(&self.sessions);

        let tiles = row(Vec::new())
            .spacing(16)
            .push(stat_tile("Hours focused", format!("{:.1}", summary.hours_focused)))
            .push(stat_tile("Days accessed", summary.days_accessed.to_string()))
            .push(stat_tile("Day streak", summary.day_streak.to_string()));

        let mut table = column(Vec::new()).spacing(6).push(
            row(Vec::new())
                .push(text("PROJECT").width(Length::Fill))
                .push(text("TIME (HH:MM)")),
        );

        if breakdown.is_empty() {
            table = table.push(text("No focused time logged this week yet."));
        } else {
            for (project, seconds) in &breakdown {
                table = table.push(
                    row(Vec::new())
                        .push(text(project.clone()).width(Length::Fill))
                        .push(text(format_hhmm(*seconds))),
                );
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
    .padding(12)
    .width(Length::Fill)
    .into()
}

fn main() -> cosmic::iced::Result {
    let env = env_logger::Env::default().filter_or("PAWPAUSE_LOG", "warn");
    env_logger::init_from_env(env);
    let settings = Settings::default().size(Size::new(640.0, 520.0));
    cosmic::app::run::<App>(settings, ())
}
