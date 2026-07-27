use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::iced::core::window;
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Background, Border, Color, Length, Rectangle, Subscription};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::dropdown::popup_dropdown;
use cosmic::widget::{self, column, container, row, text, text_input, Space};
use cosmic::{Element, Theme};

use crate::config::{self, Config};
use crate::outputs;
use crate::overlay::{notify, Overlay};
use crate::pomodoro::{Phase, Pomodoro, RunState, Transition};
use crate::stats;
use crate::tasks;
use crate::wallpaper_blur::{self, WallpaperBackup};

const ID: &str = "com.pawpause.Applet";
const MIN_MINUTES: f64 = 0.5;
const MINUTE_STEP: f64 = 1.0;

/// Launches the standalone `pawpause` window (Tasks + Statistics), preferring
/// the copy installed alongside this applet binary and falling back to PATH.
fn open_companion_app() {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("pawpause")));

    let mut command = match sibling.filter(|path| path.is_file()) {
        Some(path) => std::process::Command::new(path),
        None => std::process::Command::new("pawpause"),
    };

    if let Err(err) = command.spawn() {
        notify("PawPause", &format!("Could not open PawPause: {err}"));
    }
}

pub struct Window {
    core: Core,
    popup: Option<Id>,
    config: Config,
    pomodoro: Pomodoro,
    overlay: Overlay,
    settings_open: bool,
    available_outputs: Vec<String>,
    /// Set while a break's wallpaper blur is active, so it can be restored
    /// when the break ends. In-memory only: a crash mid-break leaves the
    /// wallpaper blurred until the next full break cycle completes.
    wallpaper_backup: Option<WallpaperBackup>,
}

impl Default for Window {
    fn default() -> Self {
        let (config, created) = config::load_or_create();
        if created {
            notify(
                "PawPause",
                &format!("Created default config at {}", config::config_path().display()),
            );
        }
        Self {
            core: Core::default(),
            popup: None,
            config,
            pomodoro: Pomodoro::new(),
            overlay: Overlay::new(),
            settings_open: false,
            available_outputs: Vec::new(),
            wallpaper_backup: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    PopupClosed(Id),
    Tick,
    Start,
    PauseResume,
    Skip,
    Stop,
    ToggleSettings,
    SetWorkMinutes(f64),
    SetShortBreakMinutes(f64),
    SetLongBreakMinutes(f64),
    SetSessions(u32),
    SetBlur(u32),
    VideoPathInput(String),
    ChooseVideo,
    VideoChosen(Option<String>),
    SleepVideoPathInput(String),
    ChooseSleepVideo,
    SleepVideoChosen(Option<String>),
    OutputSelected(usize),
    Surface(cosmic::surface::Action),
    OpenApp,
}

impl Window {
    fn handle_transition(&mut self, transition: Option<Transition>) {
        let Some(Transition {
            old_phase,
            new_phase,
            old_phase_elapsed_secs,
            completed,
        }) = transition
        else {
            return;
        };

        if old_phase == Some(Phase::Work) && old_phase_elapsed_secs > 0 {
            let project = tasks::load().active_project_name();
            stats::log_session(&project, old_phase_elapsed_secs as u64, completed);
        }

        if old_phase.is_some_and(Phase::is_break) {
            self.overlay.stop();
            if let Some(backup) = self.wallpaper_backup.take() {
                wallpaper_blur::restore(backup);
            }
        }
        if new_phase.is_some_and(Phase::is_break) {
            self.overlay.start(
                &self.config.video_path,
                &self.config.video_sleep_path,
                &self.config.wayland_output,
                self.pomodoro.current_phase_duration_secs() as f64,
            );
            self.wallpaper_backup = wallpaper_blur::apply(&self.config.wayland_output, self.config.blur);
        }

        let (title, body) = match new_phase {
            Some(Phase::Work) => ("Back to work", "Focus time started."),
            Some(Phase::ShortBreak) => ("Break time", "Step away for a short break."),
            Some(Phase::LongBreak) => ("Long break", "Nice work — take a longer break."),
            None => ("PawPause", "Stopped."),
        };
        notify(title, body);
    }

    fn pill_label(&self) -> String {
        match self.pomodoro.phase {
            None => "PawPause".to_string(),
            Some(_) => self.pomodoro.short_time_text(),
        }
    }

    fn phase_dot(&self) -> &'static str {
        if self.pomodoro.state == RunState::Paused {
            return "🟡";
        }
        match self.pomodoro.phase {
            None => "⚪",
            Some(Phase::Work) => "🔴",
            Some(Phase::ShortBreak) => "🟢",
            Some(Phase::LongBreak) => "🔵",
        }
    }

    fn save_config(&self) {
        config::save(&self.config);
    }

    /// Progress through the current phase, 0.0-1.0. 0.0 while idle.
    fn progress(&self) -> f32 {
        let total = self.pomodoro.current_phase_duration_secs();
        if total <= 0 {
            return 0.0;
        }
        (1.0 - (self.pomodoro.remaining.max(0) as f32 / total as f32)).clamp(0.0, 1.0)
    }

    /// How many dots in the current `goal`-sized cycle should read as
    /// "filled" — mirrors the reference applet's dot semantics: the
    /// in-progress Work session's dot counts as filled too, not just fully
    /// completed ones, so the row reads "session N of goal" while working.
    fn completed_in_cycle(&self, goal: u32) -> u32 {
        match self.pomodoro.phase {
            Some(Phase::Work) => (self.pomodoro.session_count % goal) + 1,
            Some(Phase::LongBreak) => goal,
            _ => self.pomodoro.session_count % goal,
        }
    }
}

fn phase_color(theme: &Theme, phase: Option<Phase>) -> Color {
    let c = theme.cosmic();
    match phase {
        None => c.bg_component_color().into(),
        Some(Phase::Work) => c.destructive_color().into(),
        Some(Phase::ShortBreak) => c.success_color().into(),
        Some(Phase::LongBreak) => c.accent_color().into(),
    }
}

fn muted(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

fn colored_bg(color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| container::Style {
        background: Some(Background::Color(color)),
        border: Border {
            radius: radius.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn dot(filled: bool, color: Color) -> Element<'static, Message> {
    container(Space::new().width(Length::Fixed(8.0)).height(Length::Fixed(8.0)))
        .style(colored_bg(if filled { color } else { muted(color, 0.25) }, 4.0))
        .into()
}

impl cosmic::Application for Window {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let mut window = Window {
            core,
            ..Default::default()
        };
        window.available_outputs = outputs::list_outputs();
        (window, Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        cosmic::iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                    self.settings_open = false;
                }
            }
            Message::Tick => {
                let transition = self.pomodoro.tick(&self.config);
                self.handle_transition(transition);
                self.overlay.tick();
            }
            Message::Start => {
                let transition = self.pomodoro.start(&self.config);
                self.handle_transition(transition);
            }
            Message::PauseResume => match self.pomodoro.state {
                RunState::Running => self.pomodoro.pause(),
                RunState::Paused => self.pomodoro.resume(),
                RunState::Idle => {}
            },
            Message::Skip => {
                let transition = self.pomodoro.skip(&self.config);
                self.handle_transition(transition);
            }
            Message::Stop => {
                let transition = self.pomodoro.stop();
                self.handle_transition(transition);
            }
            Message::ToggleSettings => {
                self.settings_open = !self.settings_open;
                if self.settings_open {
                    self.available_outputs = outputs::list_outputs();
                }
            }
            Message::SetWorkMinutes(value) => {
                self.config.work_minutes = value.max(MIN_MINUTES);
                self.save_config();
            }
            Message::SetShortBreakMinutes(value) => {
                self.config.short_break_minutes = value.max(MIN_MINUTES);
                self.save_config();
            }
            Message::SetLongBreakMinutes(value) => {
                self.config.long_break_minutes = value.max(MIN_MINUTES);
                self.save_config();
            }
            Message::SetSessions(value) => {
                self.config.sessions_before_long_break = value.max(1);
                self.save_config();
            }
            Message::SetBlur(value) => {
                self.config.blur = value.min(100);
                self.save_config();
            }
            Message::VideoPathInput(value) => {
                self.config.video_path = value;
                self.save_config();
            }
            Message::ChooseVideo => {
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose break video")
                            .add_filter("Video", &["mp4", "webm", "mkv", "mov", "gif"])
                            .pick_file()
                            .await
                            .map(|f| f.path().to_string_lossy().into_owned())
                    },
                    |path| cosmic::Action::App(Message::VideoChosen(path)),
                );
            }
            Message::VideoChosen(Some(path)) => {
                self.config.video_path = path;
                self.save_config();
            }
            Message::VideoChosen(None) => {}
            Message::SleepVideoPathInput(value) => {
                self.config.video_sleep_path = value;
                self.save_config();
            }
            Message::ChooseSleepVideo => {
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose sleep/idle video (looped after the entry clip)")
                            .add_filter("Video", &["mp4", "webm", "mkv", "mov", "gif"])
                            .pick_file()
                            .await
                            .map(|f| f.path().to_string_lossy().into_owned())
                    },
                    |path| cosmic::Action::App(Message::SleepVideoChosen(path)),
                );
            }
            Message::SleepVideoChosen(Some(path)) => {
                self.config.video_sleep_path = path;
                self.save_config();
            }
            Message::SleepVideoChosen(None) => {}
            Message::OutputSelected(index) => {
                if let Some(name) = self.available_outputs.get(index) {
                    self.config.wayland_output = name.clone();
                    self.save_config();
                }
            }
            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
                    action,
                )));
            }
            Message::OpenApp => {
                open_companion_app();
            }
        };
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let have_popup = self.popup;
        let label = format!("{} {}", self.phase_dot(), self.pill_label());

        let btn = self
            .core
            .applet
            .text_button(text(label), Message::PopupClosed(Id::NONE))
            .on_press_with_rectangle(move |offset, bounds| {
            if let Some(id) = have_popup {
                Message::Surface(destroy_popup(id))
            } else {
                Message::Surface(app_popup::<Window>(
                    |_| Default::default(),
                    move |state: &mut Window| {
                        let new_id = Id::unique();
                        state.popup = Some(new_id);
                        let mut popup_settings = state.core.applet.get_popup_settings(
                            state.core.main_window_id().unwrap(),
                            new_id,
                            None,
                            None,
                            None,
                        );
                        popup_settings.positioner.anchor_rect = Rectangle {
                            x: (bounds.x - offset.x) as i32,
                            y: (bounds.y - offset.y) as i32,
                            width: bounds.width as i32,
                            height: bounds.height as i32,
                        };
                        popup_settings
                    },
                    Some(Box::new(move |state: &Window| {
                        let popup_id = state.popup.unwrap_or(Id::NONE);
                        let content_list = if state.settings_open {
                            state.settings_view(popup_id)
                        } else {
                            state.controls_view()
                        };
                        Element::from(state.core.applet.popup_container(content_list))
                            .map(cosmic::Action::App)
                    })),
                ))
            }
        });

        let tooltip = self.core.applet.applet_tooltip::<Message>(
            btn,
            self.pomodoro.status_text(),
            self.popup.is_some(),
            |a| Message::Surface(a),
            None,
        );

        static AUTOSIZE_ID: std::sync::LazyLock<cosmic::widget::Id> =
            std::sync::LazyLock::new(|| cosmic::widget::Id::new("pawpause-autosize"));

        cosmic::widget::autosize::autosize(Element::from(tooltip), AUTOSIZE_ID.clone()).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        "".into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl Window {
    fn controls_view(&self) -> Element<'_, Message> {
        let theme = cosmic::theme::active();
        let phase = self.pomodoro.phase;
        let accent = phase_color(&theme, phase);

        let phase_label = match (phase, self.pomodoro.state) {
            (None, _) => "Idle".to_string(),
            (Some(p), RunState::Paused) => format!("{} (paused)", p.label()),
            (Some(p), _) => p.label().to_string(),
        };

        let timer_card = container(
            column(Vec::new())
                .align_x(Alignment::Center)
                .spacing(4)
                .push(text::heading(phase_label))
                .push(text::title1(self.pomodoro.short_time_text())),
        )
        .width(Length::Fill)
        .padding([16, 20])
        .align_x(Alignment::Center)
        .style(colored_bg(muted(accent, 0.3), 12.0));

        let progress_bar = widget::progress_bar::determinate_linear(self.progress())
            .girth(Length::Fixed(6.0))
            .width(Length::Fill);

        let goal = self.config.sessions_before_long_break.max(1);
        let filled = self.completed_in_cycle(goal);
        let mut dots = row(Vec::new()).spacing(6).align_y(Alignment::Center);
        for i in 0..goal {
            dots = dots.push(dot(i < filled, accent));
        }

        let toggle_label = match self.pomodoro.state {
            RunState::Running => "Pause",
            RunState::Paused => "Resume",
            RunState::Idle => "Pause",
        };
        let actions: Element<'_, Message> = if self.pomodoro.state == RunState::Idle {
            row(Vec::new())
                .push(widget::button::suggested("Start Pomodoro").on_press(Message::Start))
                .into()
        } else {
            row(Vec::new())
                .spacing(8)
                .push(widget::button::standard(toggle_label).on_press(Message::PauseResume))
                .push(widget::button::standard("Skip").on_press(Message::Skip))
                .push(widget::button::destructive("Stop").on_press(Message::Stop))
                .into()
        };

        column(Vec::new())
            .padding(10)
            .spacing(10)
            .align_x(Alignment::Center)
            .push(timer_card)
            .push(progress_bar)
            .push(dots)
            .push(actions)
            .push(widget::divider::horizontal::default())
            .push(
                row(Vec::new())
                    .spacing(8)
                    .push(widget::button::text("Settings").on_press(Message::ToggleSettings))
                    .push(widget::button::text("Open PawPause").on_press(Message::OpenApp)),
            )
            .into()
    }

    fn settings_view(&self, popup_id: Id) -> Element<'_, Message> {
        let output_names = self.available_outputs.clone();
        let selected_output = self
            .available_outputs
            .iter()
            .position(|name| name == &self.config.wayland_output);

        let timing = widget::list_column()
            .add(minute_spin_button(
                "Work minutes",
                self.config.work_minutes,
                Message::SetWorkMinutes,
            ))
            .add(minute_spin_button(
                "Short break minutes",
                self.config.short_break_minutes,
                Message::SetShortBreakMinutes,
            ))
            .add(minute_spin_button(
                "Long break minutes",
                self.config.long_break_minutes,
                Message::SetLongBreakMinutes,
            ))
            .add(widget::settings::item_row(vec![
                text("Sessions before long break").width(Length::Fill).into(),
                widget::spin_button::spin_button(
                    format!("{}", self.config.sessions_before_long_break),
                    "Sessions before long break",
                    self.config.sessions_before_long_break,
                    1,
                    1,
                    20,
                    Message::SetSessions,
                )
                .into(),
            ]))
            .add(widget::settings::item_row(vec![
                text("Background blur").width(Length::Fill).into(),
                widget::spin_button::spin_button(
                    format!("{}%", self.config.blur),
                    "Background blur",
                    self.config.blur,
                    5,
                    0,
                    100,
                    Message::SetBlur,
                )
                .into(),
            ]));

        let body = column(Vec::new())
            .padding(10)
            .spacing(10)
            .push(text("Settings").size(16))
            .push(timing)
            .push(widget::divider::horizontal::default())
            .push(text("Break video"))
            .push(
                row(Vec::new())
                    .spacing(4)
                    .push(
                        text_input("~/Videos/break.mp4", &self.config.video_path)
                            .on_input(Message::VideoPathInput)
                            .width(Length::Fill),
                    )
                    .push(widget::button::text("Choose…").on_press(Message::ChooseVideo)),
            )
            .push(text("Sleep video (looped after entry clip finishes once)"))
            .push(
                row(Vec::new())
                    .spacing(4)
                    .push(
                        text_input("(optional — leave empty to just loop the entry clip)", &self.config.video_sleep_path)
                            .on_input(Message::SleepVideoPathInput)
                            .width(Length::Fill),
                    )
                    .push(widget::button::text("Choose…").on_press(Message::ChooseSleepVideo)),
            )
            .push(widget::divider::horizontal::default())
            .push(text("Wayland output"))
            .push(popup_dropdown(
                output_names,
                selected_output,
                Message::OutputSelected,
                popup_id,
                Message::Surface,
                |m| m,
            ))
            .push(widget::button::text("Back").on_press(Message::ToggleSettings));

        widget::scrollable(body).height(Length::Fixed(340.0)).into()
    }
}

fn minute_spin_button<'a>(
    label: &'a str,
    value: f64,
    on_change: fn(f64) -> Message,
) -> Element<'a, Message> {
    widget::settings::item_row(vec![
        text(label).width(Length::Fill).into(),
        widget::spin_button::spin_button(format!("{value:.1} min"), label, value, MINUTE_STEP, MIN_MINUTES, 180.0, on_change)
            .into(),
    ])
    .into()
}
