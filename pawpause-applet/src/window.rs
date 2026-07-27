use std::process::Command;
use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::iced::core::window;
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Rectangle, Subscription};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::{self, column, layer_container, row, text};
use cosmic::Element;

use crate::config::{self, Config};
use crate::overlay::{notify, Overlay};
use crate::pomodoro::{Phase, Pomodoro, RunState};

const ID: &str = "com.pawpause.Applet";

pub struct Window {
    core: Core,
    popup: Option<Id>,
    config: Config,
    pomodoro: Pomodoro,
    overlay: Overlay,
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
    OpenSettings,
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
            self.overlay.start(&self.config.video_path, &self.config.wayland_output);
        }

        let (title, body) = match new_phase {
            Some(Phase::Work) => ("Back to work", "Focus time started."),
            Some(Phase::ShortBreak) => ("Break time", "Step away for a short break."),
            Some(Phase::LongBreak) => ("Long break", "Nice work — take a longer break."),
            None => ("PawPause", "Stopped."),
        };
        notify(title, body);
    }

    fn phase_icon_name(&self) -> &'static str {
        if self.pomodoro.state == RunState::Paused {
            return "media-playback-pause-symbolic";
        }
        match self.pomodoro.phase {
            None => "alarm-symbolic",
            Some(Phase::Work) => "media-playback-start-symbolic",
            Some(Phase::ShortBreak) => "weather-clear-symbolic",
            Some(Phase::LongBreak) => "weather-few-clouds-symbolic",
        }
    }

    fn pill_label(&self) -> String {
        match self.pomodoro.phase {
            None => "PawPause".to_string(),
            Some(_) => self.pomodoro.short_time_text(),
        }
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
        let window = Window {
            core,
            ..Default::default()
        };
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
                }
            }
            Message::Tick => {
                let transition = self.pomodoro.tick(&self.config);
                self.handle_transition(transition);
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
            Message::OpenSettings => {
                let path = config::config_path();
                for editor in ["cosmic-edit", "gnome-text-editor", "gedit", "xdg-open"] {
                    if Command::new(editor).arg(&path).spawn().is_ok() {
                        break;
                    }
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
        let icon = widget::icon::from_name(self.phase_icon_name())
            .symbolic(true)
            .size(self.core.applet.suggested_size(true).0)
            .into();

        let content = row(Vec::new())
            .align_y(Alignment::Center)
            .spacing(4)
            .push(widget::icon(icon))
            .push(text(self.pill_label()));

        let (major, minor) = self.core.applet.suggested_padding(true);
        let (h_pad, v_pad) = if self.core.applet.is_horizontal() {
            (major, minor)
        } else {
            (minor, major)
        };
        let suggested_height = self.core.applet.suggested_size(true).1;

        let btn = cosmic::widget::button::custom(
            layer_container(content).center_y(Length::Fixed(f32::from(suggested_height + 2 * v_pad))),
        )
        .padding([0, h_pad])
        .class(cosmic::theme::Button::AppletIcon)
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
                        let toggle_label = match state.pomodoro.state {
                            RunState::Running => "Pause",
                            RunState::Paused => "Resume",
                            RunState::Idle => "Pause",
                        };

                        let mut content_list = column(Vec::new()).padding(10).spacing(8).push(
                            text(state.pomodoro.status_text()).size(16),
                        );

                        content_list = if state.pomodoro.state == RunState::Idle {
                            content_list.push(
                                widget::button::text("Start Pomodoro").on_press(Message::Start),
                            )
                        } else {
                            content_list
                                .push(widget::button::text(toggle_label).on_press(Message::PauseResume))
                                .push(widget::button::text("Skip").on_press(Message::Skip))
                                .push(widget::button::text("Stop/Reset").on_press(Message::Stop))
                        };

                        content_list = content_list.push(
                            widget::button::text("Settings").on_press(Message::OpenSettings),
                        );

                        Element::from(state.core.applet.popup_container(content_list))
                            .map(cosmic::Action::App)
                    })),
                ))
            }
        });

        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            self.pomodoro.status_text(),
            self.popup.is_some(),
            |a| Message::Surface(a),
            None,
        ))
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        "".into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
