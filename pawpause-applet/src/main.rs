mod config;
mod outputs;
mod overlay;
mod pomodoro;
mod window;

use crate::window::Window;

fn main() -> cosmic::iced::Result {
    let env = env_logger::Env::default().filter_or("PAWPAUSE_LOG", "warn");
    env_logger::init_from_env(env);
    cosmic::applet::run::<Window>(())
}
