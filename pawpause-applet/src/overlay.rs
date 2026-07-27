use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub fn notify(title: &str, body: &str) {
    let _ = Command::new("notify-send").arg(title).arg(body).spawn();
}

fn which(bin: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).find_map(|dir| {
        let full = dir.join(bin);
        full.is_file().then_some(full)
    })
}

fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

/// Starts/stops the mpvpaper break video as a layer-shell overlay, via
/// `mpvpaper -l overlay` (zwlr_layer_shell_v1's "overlay" layer) rather than
/// any custom Wayland protocol code.
pub struct Overlay {
    child: Option<Child>,
}

impl Overlay {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub fn start(&mut self, video_path: &str, output_name: &str) {
        self.stop();

        let video_path = expand_home(video_path);
        if !Path::new(&video_path).is_file() {
            notify("PawPause", &format!("Break video not found: {video_path}"));
            return;
        }
        if which("mpvpaper").is_none() {
            notify("PawPause", "mpvpaper is not installed — skipping video overlay.");
            return;
        }

        match Command::new("mpvpaper")
            .args(["-l", "overlay", "-o", "loop-file=inf no-audio-display"])
            .arg(output_name)
            .arg(&video_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => self.child = Some(child),
            Err(err) => notify("PawPause", &format!("Failed to launch mpvpaper: {err}")),
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        self.stop();
    }
}
