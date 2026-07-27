use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::iced::core::window;
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Rectangle, Subscription};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::dropdown::popup_dropdown;
use cosmic::widget::{self, column, row, text, text_input};
use cosmic::Element;

use crate::config::{self, Config};
use crate::outputs;
use crate::overlay::{notify, Overlay};
use crate::pomodoro::{Phase, Pomodoro, RunState};

const ID: &str = "com.pawpause.Applet";
const MIN_MINUTES: f64 = 0.5;
const MINUTE_STEP: f64 = 1.0;

pub struct Window {
    core: Core,
    popup: Option<Id>,
    config: Config,
    pomodoro: Pomodoro,
    overlay: Overlay,
    settings_open: bool,
    available_outputs: Vec<String>,
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
    AdjustWorkMinutes(f64),
    AdjustShortBreakMinutes(f64),
    AdjustLongBreakMinutes(f64),
    AdjustSessions(i32),
    AdjustBlur(i32),
    VideoPathInput(String),
    ChooseVideo,
    VideoChosen(Option<String>),
    SleepVideoPathInput(String),
    ChooseSleepVideo,
    SleepVideoChosen(Option<String>),
    OutputSelected(usize),
    Surface(cosmic::surface::Action),
}

impl Window {
    fn handle_transition(&mut self, transition: Option<(Option<Phase>, Option<Phase>)>) {
        let Some((old_phase, new_phase)) = transition else {
            return;
        };

        if old_phase.is_some_and(Phase::is_break) {
            self.overlay.stop();
        }
        if new_phase.is_some_and(Phase::is_break) {
            self.overlay.start(
                &self.config.video_path,
                &self.config.video_sleep_path,
                &self.config.wayland_output,
                self.config.blur,
            );
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
            Message::AdjustWorkMinutes(delta) => {
                self.config.work_minutes = (self.config.work_minutes + delta).max(MIN_MINUTES);
                self.save_config();
            }
            Message::AdjustShortBreakMinutes(delta) => {
                self.config.short_break_minutes =
                    (self.config.short_break_minutes + delta).max(MIN_MINUTES);
                self.save_config();
            }
            Message::AdjustLongBreakMinutes(delta) => {
                self.config.long_break_minutes =
                    (self.config.long_break_minutes + delta).max(MIN_MINUTES);
                self.save_config();
            }
            Message::AdjustSessions(delta) => {
                let current = self.config.sessions_before_long_break as i32;
                self.config.sessions_before_long_break = (current + delta).max(1) as u32;
                self.save_config();
            }
            Message::AdjustBlur(delta) => {
                let current = self.config.blur as i32;
                self.config.blur = (current + delta).clamp(0, 100) as u32;
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
        let toggle_label = match self.pomodoro.state {
            RunState::Running => "Pause",
            RunState::Paused => "Resume",
            RunState::Idle => "Pause",
        };

        let mut list = column(Vec::new())
            .padding(10)
            .spacing(8)
            .push(text(self.pomodoro.status_text()).size(16));

        list = if self.pomodoro.state == RunState::Idle {
            list.push(widget::button::text("Start Pomodoro").on_press(Message::Start))
        } else {
            list.push(widget::button::text(toggle_label).on_press(Message::PauseResume))
                .push(widget::button::text("Skip").on_press(Message::Skip))
                .push(widget::button::text("Stop/Reset").on_press(Message::Stop))
        };

        list.push(widget::button::text("Settings").on_press(Message::ToggleSettings))
            .into()
    }

    fn settings_view(&self, popup_id: Id) -> Element<'_, Message> {
        let output_names = self.available_outputs.clone();
        let selected_output = self
            .available_outputs
            .iter()
            .position(|name| name == &self.config.wayland_output);

        column(Vec::new())
            .padding(10)
            .spacing(10)
            .push(text("Settings").size(16))
            .push(minute_stepper(
                "Work",
                self.config.work_minutes,
                Message::AdjustWorkMinutes(-MINUTE_STEP),
                Message::AdjustWorkMinutes(MINUTE_STEP),
            ))
            .push(minute_stepper(
                "Short break",
                self.config.short_break_minutes,
                Message::AdjustShortBreakMinutes(-MINUTE_STEP),
                Message::AdjustShortBreakMinutes(MINUTE_STEP),
            ))
            .push(minute_stepper(
                "Long break",
                self.config.long_break_minutes,
                Message::AdjustLongBreakMinutes(-MINUTE_STEP),
                Message::AdjustLongBreakMinutes(MINUTE_STEP),
            ))
            .push(count_stepper(
                "Sessions before long break",
                self.config.sessions_before_long_break,
                Message::AdjustSessions(-1),
                Message::AdjustSessions(1),
            ))
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
            .push(count_stepper(
                "Video blur",
                self.config.blur,
                Message::AdjustBlur(-5),
                Message::AdjustBlur(5),
            ))
            .push(text("Wayland output"))
            .push(popup_dropdown(
                output_names,
                selected_output,
                Message::OutputSelected,
                popup_id,
                Message::Surface,
                |m| m,
            ))
            .push(widget::button::text("Back").on_press(Message::ToggleSettings))
            .into()
    }
}

fn minute_stepper<'a>(
    label: &'a str,
    value: f64,
    dec: Message,
    inc: Message,
) -> Element<'a, Message> {
    row(Vec::new())
        .align_y(Alignment::Center)
        .spacing(8)
        .push(text(label).width(Length::Fill))
        .push(widget::button::text("-").on_press(dec))
        .push(text(format!("{value:.1} min")))
        .push(widget::button::text("+").on_press(inc))
        .into()
}

fn count_stepper<'a>(
    label: &'a str,
    value: u32,
    dec: Message,
    inc: Message,
) -> Element<'a, Message> {
    row(Vec::new())
        .align_y(Alignment::Center)
        .spacing(8)
        .push(text(label).width(Length::Fill))
        .push(widget::button::text("-").on_press(dec))
        .push(text(format!("{value}")))
        .push(widget::button::text("+").on_press(inc))
        .into()
}
