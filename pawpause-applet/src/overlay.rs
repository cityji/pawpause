use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

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

const FRAME_FOLDER_FPS: f64 = 15.0;

/// Duration of a clip in seconds: probed via ffprobe for a video file, or
/// (frame count / FRAME_FOLDER_FPS) for a PNG frame-sequence directory.
fn clip_duration_secs(path: &Path) -> Option<f64> {
    if path.is_dir() {
        let count = std::fs::read_dir(path)
            .ok()?
            .filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|ext| ext == "png"))
            })
            .count();
        return Some(count as f64 / FRAME_FOLDER_FPS);
    }

    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn mpv_input_and_opts(video_path: &str, blur: u32, loop_forever: bool) -> Option<(String, String)> {
    let expanded = expand_home(video_path);
    let path = Path::new(&expanded);

    // A directory holds a PNG frame sequence (e.g. produced via AI background
    // removal), played back through mpv's multi-file input. This sidesteps
    // WebM's alpha side-channel, which mpv's demuxer doesn't decode — real
    // per-pixel transparency in a single video file isn't reliably supported,
    // so a frame folder is the robust path.
    let (input, mut opts) = if path.is_dir() {
        (
            format!("mf://{expanded}/*.png"),
            format!("mf-fps={FRAME_FOLDER_FPS} "),
        )
    } else if path.is_file() {
        (expanded, String::new())
    } else {
        return None;
    };

    let loop_opt = if loop_forever { "inf" } else { "no" };
    opts.push_str(&format!("loop-file={loop_opt} no-audio-display hwdec=no alpha=yes"));
    if blur > 0 {
        opts.push_str(&format!(" vf=gblur=sigma={:.1}", blur as f64 / 10.0));
    }

    Some((input, opts))
}

fn spawn_mpvpaper(mpv_input: &str, opts: &str, output_name: &str) -> Option<Child> {
    match Command::new("mpvpaper")
        .args(["-l", "overlay", "-o", opts])
        .arg(output_name)
        .arg(mpv_input)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => Some(child),
        Err(err) => {
            notify("PawPause", &format!("Failed to launch mpvpaper: {err}"));
            None
        }
    }
}

/// Pending handoff from a one-shot "entry" clip to a looping "sleep" clip,
/// mirroring cat-gatekeeper's neko1 (walk in, plays once) -> neko2 (sleep,
/// loops) behavior.
struct Handoff {
    sleep_path: String,
    output_name: String,
    blur: u32,
    entry_started_at: Instant,
    entry_duration_secs: f64,
}

/// Starts/stops the mpvpaper break video as a layer-shell overlay, via
/// `mpvpaper -l overlay` (zwlr_layer_shell_v1's "overlay" layer) rather than
/// any custom Wayland protocol code.
pub struct Overlay {
    child: Option<Child>,
    handoff: Option<Handoff>,
    /// The superseded entry-clip process, kept alive one extra tick so the
    /// new sleep-clip surface has time to appear before the old one is
    /// killed — avoids a visible blank gap at the handoff.
    pending_kill: Option<Child>,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            child: None,
            handoff: None,
            pending_kill: None,
        }
    }

    /// `sleep_path`: if non-empty and different from `video_path`, `video_path`
    /// plays once then hands off to `sleep_path` looping forever (matching
    /// cat-gatekeeper). Otherwise `video_path` just loops by itself.
    /// `blur` is 0-100; 0 disables the blur filter entirely.
    pub fn start(&mut self, video_path: &str, sleep_path: &str, output_name: &str, blur: u32) {
        self.stop();

        if which("mpvpaper").is_none() {
            notify("PawPause", "mpvpaper is not installed — skipping video overlay.");
            return;
        }

        let has_sleep_clip = !sleep_path.trim().is_empty() && sleep_path.trim() != video_path.trim();

        let Some((mpv_input, opts)) = mpv_input_and_opts(video_path, blur, !has_sleep_clip) else {
            notify("PawPause", &format!("Break video not found: {video_path}"));
            return;
        };

        self.child = spawn_mpvpaper(&mpv_input, &opts, output_name);

        if has_sleep_clip {
            let entry_duration_secs = clip_duration_secs(Path::new(&expand_home(video_path))).unwrap_or(0.0);
            self.handoff = Some(Handoff {
                sleep_path: sleep_path.to_string(),
                output_name: output_name.to_string(),
                blur,
                entry_started_at: Instant::now(),
                entry_duration_secs,
            });
        }
    }

    /// Call once per second while a break overlay may be active; switches
    /// from the entry clip to the looping sleep clip once the entry clip's
    /// natural duration has elapsed.
    pub fn tick(&mut self) {
        // Kill whatever the previous handoff superseded, one tick after the
        // replacement was spawned so its surface has had time to appear.
        if let Some(mut child) = self.pending_kill.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        let Some(handoff) = &self.handoff else {
            return;
        };
        if handoff.entry_started_at.elapsed().as_secs_f64() < handoff.entry_duration_secs {
            return;
        }

        let handoff = self.handoff.take().unwrap();
        if let Some((mpv_input, opts)) = mpv_input_and_opts(&handoff.sleep_path, handoff.blur, true) {
            let new_child = spawn_mpvpaper(&mpv_input, &opts, &handoff.output_name);
            self.pending_kill = self.child.take();
            self.child = new_child;
        }
    }

    pub fn stop(&mut self) {
        self.handoff = None;
        if let Some(mut child) = self.pending_kill.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
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
