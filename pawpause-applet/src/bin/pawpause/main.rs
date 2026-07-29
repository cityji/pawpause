use std::collections::HashSet;
use std::time::Duration;

mod chart;

use chrono::{Datelike, Local, NaiveDate};
use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{Alignment, Length, Size, Subscription};
use cosmic::widget::dropdown::dropdown;
use cosmic::widget::{self, checkbox, column, icon, nav_bar, row, text, text_input, Space};
use cosmic::{theme, Element};

use pawpause_applet::config::{self, Config};
use pawpause_applet::laptop_usage::{self, BootSession};
use pawpause_applet::stats;
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
    config: Config,
    boot_sessions: Vec<BootSession>,

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

    /// Reveal progress for the Statistics page's animated widgets, 0.0..=1.0.
    /// Canvas has no clock of its own, so this is advanced by a subscription
    /// tick and passed into each `Program`. It resets whenever the page is
    /// opened and stops ticking once it reaches 1.0 — an idle window must not
    /// redraw forever.
    anim: f32,
}

/// Reveal duration and tick rate. ~16ms ≈ 60fps; 420ms lands inside the
/// "responsive, not sluggish" window for a reveal of this size.
const ANIM_TICK: Duration = Duration::from_millis(16);
const ANIM_DURATION_SECS: f32 = 0.42;

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

    SetDailyGoal(u32),
    ExportCsv,
    ExportCsvDone,
    AnimTick,
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

        let (config, _created) = config::load_or_create();
        let app = App {
            core,
            nav_model,
            store: tasks::load(),
            sessions: stats::load_sessions(),
            config,
            boot_sessions: laptop_usage::list_boot_sessions(200),
            collapsed: HashSet::new(),
            new_task_title: String::new(),
            new_task_project: None,
            new_task_parent: None,
            editing: None,
            edit_title: String::new(),
            edit_project: None,
            new_project_name: String::new(),
            anim: 0.0,
        };

        (app, Task::none())
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav_model)
    }

    /// Ticks only while a reveal is actually in flight — `Subscription::none()`
    /// once it settles, so a window left open on the Statistics page costs
    /// nothing.
    fn subscription(&self) -> Subscription<Message> {
        let on_stats = self.nav_model.active_data::<Page>() == Some(&Page::Statistics);
        if on_stats && self.anim < 1.0 {
            cosmic::iced::time::every(ANIM_TICK).map(|_| Message::AnimTick)
        } else {
            Subscription::none()
        }
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Message> {
        self.nav_model.activate(id);
        match self.nav_model.active_data::<Page>() {
            Some(Page::Statistics) => {
                self.sessions = stats::load_sessions();
                self.boot_sessions = laptop_usage::list_boot_sessions(200);
                // Replay the reveal each time the page is entered.
                self.anim = 0.0;
            }
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

            Message::SetDailyGoal(value) => {
                self.config.daily_goal_minutes = value;
                config::save(&self.config);
            }
            Message::ExportCsv => {
                let sessions = self.sessions.clone();
                return Task::perform(
                    async move {
                        let Some(handle) = rfd::AsyncFileDialog::new()
                            .set_title("Export sessions to CSV")
                            .set_file_name("pawpause-sessions.csv")
                            .save_file()
                            .await
                        else {
                            return;
                        };

                        let mut csv = String::from("date,project,seconds,completed\n");
                        for s in &sessions {
                            let completed = match s.completed {
                                Some(true) => "true",
                                Some(false) => "false",
                                None => "",
                            };
                            csv.push_str(&format!("{},{},{},{}\n", s.date, s.project.replace(',', " "), s.seconds, completed));
                        }

                        if let Err(err) = std::fs::write(handle.path(), csv) {
                            pawpause_applet::overlay::notify("PawPause", &format!("Could not export CSV: {err}"));
                        } else {
                            pawpause_applet::overlay::notify("PawPause", "Exported sessions to CSV.");
                        }
                    },
                    |()| cosmic::Action::App(Message::ExportCsvDone),
                );
            }
            Message::ExportCsvDone => {}
            Message::AnimTick => {
                self.anim = (self.anim + ANIM_TICK.as_secs_f32() / ANIM_DURATION_SECS).min(1.0);
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

        // Project name and rolled-up progress share one compact chip instead
        // of two separate elements — every extra element eats into the
        // title's Length::Fill space, and rows get squeezed hard once
        // indentation stacks up with depth.
        let project_name = task.project_id.and_then(|id| self.store.project_name(id));
        if project_name.is_some() || has_children {
            let mut label = project_name.unwrap_or_default().to_string();
            if has_children {
                let (done, total) = self.store.progress(id);
                if !label.is_empty() {
                    label.push_str(" · ");
                }
                label.push_str(&format!("{done}/{total}"));
            }
            children.push(project_chip(&label));
        }

        let star_icon = if is_active { "starred-symbolic" } else { "non-starred-symbolic" };
        let actions = row(vec![
            icon_button(
                star_icon,
                if is_active { Message::ClearActive } else { Message::SetActive(id) },
            ),
            icon_button("list-add-symbolic", Message::StartAddSubtask(id)),
            icon_button("document-edit-symbolic", Message::StartEdit(id)),
            icon_button("user-trash-symbolic", Message::DeleteTask(id)),
        ])
        .spacing(2);
        children.push(actions.into());

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
        let theme = cosmic::theme::active();
        let cosmic_theme = theme.cosmic();
        // Accent, not destructive_color(): the old chart series was drawn in
        // the theme's *error* red, which read as an alarm rather than as
        // progress. Success green is reserved for "goal met".
        let focus_color: cosmic::iced::Color = cosmic_theme.accent_color().into();
        let done_color: cosmic::iced::Color = cosmic_theme.success_color().into();
        let neutral_color: cosmic::iced::Color = cosmic_theme.bg_component_color().into();
        let text_color: cosmic::iced::Color = cosmic_theme.on_bg_color().into();

        let summary = stats::summary(&self.sessions);
        let today = stats::today_seconds(&self.sessions);
        let goal_minutes = self.config.daily_goal_minutes;
        let goal_secs = goal_minutes as u64 * 60;
        let best = stats::best_day(&self.sessions);

        let hero = self.hero_card(today, goal_secs, best, &summary, focus_color, done_color, text_color);

        let body = column(Vec::new())
            .padding(20)
            .spacing(20)
            .push(
                row(Vec::new())
                    .align_y(Alignment::Center)
                    .push(text("Your progress").size(22).width(Length::Fill))
                    .push(widget::button::text("Export CSV").on_press(Message::ExportCsv)),
            )
            .push(hero)
            .push(self.goal_setting_row())
            .push(self.rhythm_section(focus_color, text_color))
            .push(self.this_week_section(goal_secs, focus_color, text_color))
            .push(self.consistency_section(focus_color, text_color))
            .push(self.uptime_section(neutral_color, text_color));

        widget::scrollable(body).into()
    }

    /// The one metric that matters right now — today against the daily goal —
    /// given the most visual weight on the page, flanked by streak and
    /// all-time context so a small "today" still sits inside a bigger story.
    fn hero_card(
        &self,
        today: u64,
        goal_secs: u64,
        best: Option<(NaiveDate, u64)>,
        summary: &stats::Summary,
        focus_color: cosmic::iced::Color,
        done_color: cosmic::iced::Color,
        text_color: cosmic::iced::Color,
    ) -> Element<'_, Message> {
        // With the goal switched off there's nothing to fill toward, and the
        // ring would render as an empty circle with a number floating in it —
        // the same dead look this redesign exists to remove. Fall back to the
        // personal best (or a 1-hour default) so the arc always means
        // something, and say which in the caption.
        let (ring_target, caption) = if goal_secs > 0 {
            (goal_secs, format!("of {} goal", stats::format_duration(goal_secs)))
        } else {
            match best.map(|(_, s)| s).filter(|s| *s > 0) {
                Some(best_secs) => (best_secs, format!("of best {}", stats::format_duration(best_secs))),
                None => (3600, "today".to_string()),
            }
        };

        let ring = widget::canvas(chart::GoalRing {
            today_seconds: today,
            goal_seconds: ring_target,
            celebrate: goal_secs > 0,
            headline: stats::format_duration(today),
            caption,
            color: focus_color,
            accent_done: done_color,
            text_color,
            anim: self.anim,
        })
        .width(Length::Fixed(150.0))
        .height(Length::Fixed(150.0));

        let (this_week, last_week) = stats::week_over_week(&self.sessions);
        let trend_line = match (this_week, last_week) {
            (0, 0) => "No focus logged this week yet.".to_string(),
            (t, 0) => format!("{} this week — your first week logging focus.", stats::format_duration(t)),
            (t, l) if t >= l => format!(
                "{} this week, up from {} at this point last week.",
                stats::format_duration(t),
                stats::format_duration(l)
            ),
            (t, l) => format!(
                "{} this week, {} behind last week's pace.",
                stats::format_duration(t),
                stats::format_duration(l - t)
            ),
        };

        let side = column(Vec::new())
            .spacing(10)
            .push(text(stats::encouragement(today, self.config.daily_goal_minutes, summary.day_streak)).size(15))
            .push(text(trend_line).size(12))
            .push(
                row(Vec::new())
                    .spacing(24)
                    .push(mini_metric("Streak", &format!("{} d", summary.day_streak)))
                    .push(mini_metric("Total focus", &stats::format_duration((summary.hours_focused * 3600.0) as u64)))
                    .push(mini_metric("Active days", &summary.days_accessed.to_string())),
            );

        widget::container(
            row(Vec::new())
                .spacing(20)
                .align_y(Alignment::Center)
                .push(ring)
                .push(side.width(Length::Fill)),
        )
        .class(theme::Container::Card)
        .padding(20)
        .width(Length::Fill)
        .into()
    }

    fn goal_setting_row(&self) -> Element<'_, Message> {
        widget::container(
            row(Vec::new())
                .spacing(8)
                .align_y(Alignment::Center)
                .push(
                    column(Vec::new())
                        .push(text("Daily focus goal"))
                        .push(text("Sets the target the ring above fills toward.").size(11))
                        .width(Length::Fill),
                )
                .push(widget::spin_button::spin_button(
                    if self.config.daily_goal_minutes == 0 {
                        "Off".to_string()
                    } else {
                        format!("{} min", self.config.daily_goal_minutes)
                    },
                    "Daily focus goal in minutes",
                    self.config.daily_goal_minutes,
                    15,
                    0,
                    600,
                    Message::SetDailyGoal,
                )),
        )
        .class(theme::Container::Card)
        .padding(16)
        .width(Length::Fill)
        .into()
    }

    /// "When do you actually focus" — a weekday profile that stays meaningful
    /// on a short history, unlike a 14-day line that's mostly zeros.
    fn rhythm_section(
        &self,
        focus_color: cosmic::iced::Color,
        text_color: cosmic::iced::Color,
    ) -> Element<'_, Message> {
        let today_index = Local::now().date_naive().weekday().num_days_from_monday() as usize;
        section(
            "Your rhythm",
            "Focused time by weekday, last 4 weeks. Today's column is highlighted.",
            chart_card(
                widget::canvas(chart::WeekdayProfile {
                    minutes: stats::weekday_profile(&self.sessions, 4),
                    today_index,
                    color: focus_color,
                    text_color,
                    anim: self.anim,
                })
                .height(Length::Fixed(150.0)),
            ),
        )
    }

    /// Per-project time this week. Bars are normalized against a real ceiling
    /// (personal best day, or the goal) so a single project no longer renders
    /// as a permanently full bar.
    fn this_week_section(
        &self,
        goal_secs: u64,
        focus_color: cosmic::iced::Color,
        text_color: cosmic::iced::Color,
    ) -> Element<'_, Message> {
        let breakdown = stats::week_breakdown(&self.sessions);
        let multi_project = breakdown.len() > 1;

        // With several projects, comparing them against each other is the
        // useful question, so the in-chart max is the honest ceiling. With a
        // single project that's vacuous — it would always fill the row, which
        // is the bug that made this chart look broken. Measure it against an
        // external ceiling instead, and crucially a *week*-scale one: these
        // bars are week totals, so a daily goal or a best *day* would be
        // outgrown by Wednesday and clamp back to full width.
        let days_elapsed = Local::now().date_naive().weekday().num_days_from_monday() as u64 + 1;
        let week_target = goal_secs * days_elapsed;
        let reference = if multi_project {
            None
        } else {
            Some(week_target.max(1))
        };

        let subtitle = if multi_project {
            "Time per project, Monday to today.".to_string()
        } else if week_target > 0 {
            format!(
                "Time per project, Monday to today — the track is your goal so far this week ({}).",
                stats::format_duration(week_target)
            )
        } else {
            "Time per project, Monday to today. Set a daily goal to see this as progress.".to_string()
        };

        let content: Element<'_, Message> = if breakdown.is_empty() {
            widget::container(text("Nothing logged this week yet — your first session will show up here.").size(13))
                .class(theme::Container::Card)
                .padding(16)
                .width(Length::Fill)
                .into()
        } else {
            let height = 34.0 * breakdown.len().min(8) as f32 + 20.0;
            chart_card(
                widget::canvas(chart::BarChart {
                    bars: breakdown,
                    color: focus_color,
                    text_color,
                    max_bars: 8,
                    reference,
                })
                .height(Length::Fixed(height)),
            )
        };

        section("This week", &subtitle, content)
    }

    /// Streak-shaped view: the heatmap plus an honest completion caption.
    fn consistency_section(
        &self,
        focus_color: cosmic::iced::Color,
        text_color: cosmic::iced::Color,
    ) -> Element<'_, Message> {
        let completion = stats::completion_summary(&self.sessions);
        let finished = completion.completed;
        let total_known = finished + completion.skipped_or_stopped;
        let caption = if total_known == 0 {
            "No finished sessions recorded yet.".to_string()
        } else {
            format!(
                "{finished} of {total_known} sessions run to completion ({}%).",
                (finished as f32 / total_known as f32 * 100.0).round() as u32
            )
        };

        let heatmap = chart_card(
            widget::canvas(chart::HeatmapCalendar {
                days: stats::daily_breakdown(&self.sessions, 84),
                base_color: focus_color,
                text_color,
                weeks: 12,
            })
            .height(Length::Fixed(130.0)),
        );

        section(
            "Consistency",
            "Every day of the last 12 weeks. Rows are weekdays, Monday at the top.",
            column(Vec::new()).spacing(8).push(heatmap).push(text(caption).size(12)).into(),
        )
    }

    /// Machine uptime. Deliberately *not* called "usage": a suspended laptop
    /// still counts as up, so this measures how long the machine ran, not how
    /// long it was worked on. Hidden entirely when `last`'s history is too
    /// short to fill the window — charting rotated-away days as flat zero
    /// implied "laptop was off", which is exactly the unreliability that made
    /// this card untrustworthy.
    fn uptime_section(
        &self,
        neutral_color: cosmic::iced::Color,
        text_color: cosmic::iced::Color,
    ) -> Element<'_, Message> {
        const WINDOW_DAYS: u32 = 14;
        let Some(history_start) = laptop_usage::history_starts_on(&self.boot_sessions) else {
            return Space::new().height(Length::Fixed(0.0)).into();
        };

        let today = Local::now().date_naive();
        let covered = (today - history_start).num_days().max(0) as u32 + 1;
        let days = covered.min(WINDOW_DAYS);
        if days < 2 {
            return Space::new().height(Length::Fixed(0.0)).into();
        }

        let points = zero_fill_minutes(&laptop_usage::daily_usage_minutes(&self.boot_sessions), days)
            .into_iter()
            .map(|(d, m)| (d, m * 60))
            .collect::<Vec<_>>();

        let subtitle = if covered < WINDOW_DAYS {
            format!("Hours the machine was powered on. Boot history only goes back {days} days.")
        } else {
            format!("Hours the machine was powered on, last {days} days. Includes time spent suspended.")
        };

        section(
            "Uptime",
            &subtitle,
            chart_card(
                widget::canvas(chart::TrendChart {
                    points,
                    color: neutral_color,
                    text_color,
                    fill: true,
                })
                .height(Length::Fixed(130.0)),
            ),
        )
    }
}

/// A titled section: heading, one explanatory line, then the content. The
/// subtitle is load-bearing — every chart on this page now states its own
/// scope, since unlabeled mixed-scope metrics were a large part of why the
/// old page couldn't be trusted.
fn section<'a>(title: &'a str, subtitle: &str, content: Element<'a, Message>) -> Element<'a, Message> {
    column(Vec::new())
        .spacing(8)
        .push(text(title).size(16))
        .push(text(subtitle.to_string()).size(11))
        .push(content)
        .into()
}

fn mini_metric<'a>(label: &str, value: &str) -> Element<'a, Message> {
    column(Vec::new())
        .spacing(2)
        .push(text(value.to_string()).size(18))
        .push(text(label.to_string()).size(11))
        .into()
}

fn chart_card<'a>(
    canvas: widget::Canvas<impl cosmic::iced::widget::canvas::Program<Message, cosmic::Theme> + 'a, Message, cosmic::Theme>,
) -> Element<'a, Message> {
    widget::container(canvas.width(Length::Fill))
        .class(theme::Container::Card)
        .padding(12)
        .width(Length::Fill)
        .into()
}

/// Zero-filled, ascending-date minute totals for the last `days` calendar
/// days ending today — mirrors `stats::daily_breakdown`'s shape so the
/// laptop-usage trend chart reads consistently with the pomodoro one.
fn zero_fill_minutes(daily: &[(NaiveDate, u64)], days: u32) -> Vec<(NaiveDate, u64)> {
    let today = Local::now().date_naive();
    let totals: std::collections::BTreeMap<NaiveDate, u64> = daily.iter().copied().collect();
    (0..days)
        .rev()
        .map(|offset| {
            let date = today - chrono::Duration::days(offset as i64);
            (date, totals.get(&date).copied().unwrap_or(0))
        })
        .collect()
}

fn main() -> cosmic::iced::Result {
    let env = env_logger::Env::default().filter_or("PAWPAUSE_LOG", "warn");
    env_logger::init_from_env(env);
    let settings = Settings::default().size(Size::new(920.0, 640.0));
    cosmic::app::run::<App>(settings, ())
}
