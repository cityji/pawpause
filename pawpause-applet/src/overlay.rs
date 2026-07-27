use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::outputs;

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

/// Ascending-sorted PNG paths in a frame folder, or empty if `path` isn't a
/// directory / has no PNGs.
fn frame_paths(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
        .collect();
    paths.sort();
    paths
}

/// Duration of a clip in seconds: probed via ffprobe for a video file, or
/// (frame count / FRAME_FOLDER_FPS) for a PNG frame-sequence directory.
fn clip_duration_secs(path: &Path) -> Option<f64> {
    if path.is_dir() {
        let count = frame_paths(path).len();
        return Some(count as f64 / FRAME_FOLDER_FPS);
    }

    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Writes `paths`, one per line, to `out_path` — the format mpv's
/// `mf://@listfile` protocol reads, in exactly the given order. Used to play
/// a frame folder in reverse without duplicating any image data: mpv just
/// reads the same files in a different order, so there's no extra disk or
/// memory cost over forward playback.
fn write_frame_listfile(paths: &[PathBuf], out_path: &Path) -> bool {
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut content = String::new();
    for p in paths {
        content.push_str(&p.to_string_lossy());
        content.push('\n');
    }
    std::fs::write(out_path, content).is_ok()
}

fn exit_listfile_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pawpause")
        .join("exit-frames.txt")
}

/// Fixed path for mpv's JSON IPC control socket, used to drive entry → rest
/// → exit handoffs within a single long-lived mpv process (`loadfile ...
/// replace`) instead of killing and respawning `mpvpaper` for each phase —
/// the latter briefly runs two competing Wayland layer-shell surfaces on the
/// same output and re-pays mpv's ~250-300ms cold-start cost at every
/// handoff, which is what read as a "glitch" at each transition.
fn ipc_socket_path() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pawpause")
        .join("overlay.sock")
}

/// Sends one JSON IPC command to the mpv instance listening on
/// `socket_path` and returns whether the write succeeded. Doesn't wait for
/// or read mpv's reply — the same fire-and-forget pattern mpv's own manual
/// demonstrates (`echo '...' | socat - /path/socket`).
fn send_ipc_command(socket_path: &Path, command: &serde_json::Value) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket_path) else {
        return false;
    };
    let mut payload = command.to_string();
    payload.push('\n');
    stream.write_all(payload.as_bytes()).is_ok()
}

/// The mpv input URL for `video_path`, and whether it's a frame folder
/// (real per-pixel alpha) rather than a single video file (needs
/// chroma-keying). `reversed_listfile`, when given, plays that frame listing
/// via `mf://@listfile` instead of globbing `video_path` directly — used for
/// the reversed-entry exit clip.
fn mf_input(video_path: &str, reversed_listfile: Option<&Path>) -> Option<(String, bool)> {
    if let Some(listfile) = reversed_listfile {
        return Some((format!("mf://@{}", listfile.display()), true));
    }
    let expanded = expand_home(video_path);
    let path = Path::new(&expanded);

    // A directory holds a PNG frame sequence (e.g. produced via AI
    // background removal), played back through mpv's multi-file input. This
    // sidesteps WebM's alpha side-channel, which mpv's demuxer doesn't
    // decode — real per-pixel transparency in a single video file isn't
    // reliably supported, so a frame folder is the robust path.
    if path.is_dir() {
        Some((format!("mf://{expanded}/*.png"), true))
    } else if path.is_file() {
        Some((expanded, false))
    } else {
        None
    }
}

const DEFAULT_OUT_SIZE: (u32, u32) = (1920, 1080);
/// Matches cat-gatekeeper's CSS `slide-in` keyframe duration.
const SLIDE_IN_SECS: f64 = 3.0;

/// Where the cat sits on screen over a clip's playback (`t` = seconds since
/// that clip started playing).
enum Motion {
    /// Held centered for the whole clip — the looping rest pose.
    Static,
    /// Slides from off-screen-right to centered over the first
    /// `SLIDE_IN_SECS`, then holds — the walk-in entry.
    SlideIn,
    /// Holds centered, then slides from centered to off-screen-right over
    /// the last `SLIDE_IN_SECS` of `duration_secs` — the walk-out exit,
    /// timed so the cat is off-screen right as its own clip ends.
    SlideOut { duration_secs: f64 },
}

/// Builds an mpv `--lavfi-complex` graph that: chroma-keys out the clip's
/// solid black matte (single-file inputs only — a frame-folder input already
/// has real per-pixel alpha from AI background removal), scales the cat to
/// the output height, positions it per `motion`, and terminates the output
/// (`eof_action=endall`) the moment the cat's own (short) stream ends rather
/// than the instant the background's synthetic `d=3600` runs out — without
/// this, `--loop-file=inf` never sees a real EOF to loop on, and the overlay
/// filter's default `eof_action=repeat` just freezes on the cat's last frame
/// for up to an hour instead of looping.
fn build_filter_graph(is_frame_folder: bool, motion: &Motion, out_w: u32, out_h: u32) -> String {
    let colorkey = if is_frame_folder {
        String::new()
    } else {
        "colorkey=0x000000:0.12:0.08,".to_string()
    };
    let center = format!("({out_w}-w)/2");
    let x_expr = match motion {
        Motion::Static => center,
        Motion::SlideIn => {
            format!("if(lt(t,{SLIDE_IN_SECS}),{out_w}-(t/{SLIDE_IN_SECS})*({out_w}-{center}),{center})")
        }
        Motion::SlideOut { duration_secs } => {
            let hold_until = (duration_secs - SLIDE_IN_SECS).max(0.0);
            format!(
                "if(lt(t,{hold_until}),{center},{center}+((t-{hold_until})/{SLIDE_IN_SECS})*({out_w}-{center}))"
            )
        }
    };
    format!(
        "color=c=black@0.0:s={out_w}x{out_h}:d=3600[bg];\
         [vid1]{colorkey}scale=-2:{out_h}[cat];\
         [bg][cat]overlay=x='{x_expr}':y='({out_h}-h)/2':format=auto:eof_action=endall[vo]"
    )
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

/// What clip should play next, and when to switch to it.
#[derive(Clone, Copy, PartialEq)]
enum NextClip {
    /// Entry clip finished its one-shot walk-in; hand off to the looping
    /// rest clip.
    Sleep,
    /// Enough of the break has elapsed that the rest clip should give way to
    /// the reversed-entry walk-out clip.
    Exit,
}

struct Pending {
    at: Instant,
    next: NextClip,
}

/// Everything remembered from `start()` so `tick()` can drive the sleep and
/// exit handoffs later without the caller re-supplying config every tick.
struct PlaybackContext {
    entry_path: String,
    sleep_path: String,
    out_w: u32,
    out_h: u32,
    /// Same duration used for the reversed exit clip — it's the same frames
    /// at the same fps, just reordered.
    entry_duration_secs: f64,
    /// Listing of the entry clip's frames in reverse order, for the exit
    /// clip's `mf://@listfile` input. `None` when the entry clip isn't a
    /// frame folder (a single video file has no cheap reverse-playback
    /// trick available, so the walk-out exit is skipped for it) or has no
    /// frames.
    reversed_listfile: Option<PathBuf>,
    /// When to hand off from the rest clip to the exit clip, if there's
    /// enough break left to make that worthwhile.
    exit_at: Option<Instant>,
}

/// Starts/stops the mpvpaper break video as a layer-shell overlay, via
/// `mpvpaper -l overlay` (zwlr_layer_shell_v1's "overlay" layer) rather than
/// any custom Wayland protocol code. A single mpvpaper process (and Wayland
/// surface) is kept alive for the whole break; entry → rest → exit handoffs
/// are driven by mpv's JSON IPC (`loadfile ... replace`) rather than killing
/// and respawning the process, so there's no moment where two overlay
/// surfaces briefly coexist or the surface is torn down and recreated.
pub struct Overlay {
    child: Option<Child>,
    ipc_socket: Option<PathBuf>,
    pending: Option<Pending>,
    ctx: Option<PlaybackContext>,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            child: None,
            ipc_socket: None,
            pending: None,
            ctx: None,
        }
    }

    /// `sleep_path`: if non-empty and different from `video_path`,
    /// `video_path` plays once (sliding in from off-screen-right) then hands
    /// off to `sleep_path` looping forever, settled centered. If
    /// `break_duration_secs` leaves enough room, it later hands off once
    /// more — near the end of the break — to `video_path`'s own frames
    /// played in reverse, sliding back out, timed so the cat is gone right
    /// as the break ends. Otherwise `video_path` just slides in once then
    /// loops by itself for the whole break.
    pub fn start(&mut self, video_path: &str, sleep_path: &str, output_name: &str, break_duration_secs: f64) {
        self.stop();

        if which("mpvpaper").is_none() {
            notify("PawPause", "mpvpaper is not installed — skipping video overlay.");
            return;
        }

        let (out_w, out_h) = outputs::output_resolution(output_name).unwrap_or(DEFAULT_OUT_SIZE);
        let has_sleep_clip = !sleep_path.trim().is_empty() && sleep_path.trim() != video_path.trim();

        let Some((mpv_input, is_frame_folder)) = mf_input(video_path, None) else {
            notify("PawPause", &format!("Break video not found: {video_path}"));
            return;
        };

        let socket_path = ipc_socket_path();
        if let Some(parent) = socket_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // A killed (not gracefully quit) mpv doesn't unlink its own socket
        // file, which would otherwise make the new process fail to bind it.
        let _ = std::fs::remove_file(&socket_path);

        let loop_opt = if has_sleep_clip { "no" } else { "inf" };
        let graph = build_filter_graph(is_frame_folder, &Motion::SlideIn, out_w, out_h);
        let opts = format!(
            "mf-fps={FRAME_FOLDER_FPS} loop-file={loop_opt} no-audio-display hwdec=no alpha=yes \
             input-ipc-server={} lavfi-complex={graph}",
            socket_path.display(),
        );

        self.child = spawn_mpvpaper(&mpv_input, &opts, output_name);
        if self.child.is_none() {
            return;
        }
        self.ipc_socket = Some(socket_path);

        if !has_sleep_clip {
            return;
        }

        let expanded_entry = expand_home(video_path);
        let entry_dir = Path::new(&expanded_entry);
        let entry_duration_secs = clip_duration_secs(entry_dir).unwrap_or(0.0);

        let reversed_listfile = if entry_dir.is_dir() {
            let mut frames = frame_paths(entry_dir);
            frames.reverse();
            (!frames.is_empty())
                .then(exit_listfile_path)
                .filter(|listfile| write_frame_listfile(&frames, listfile))
        } else {
            None
        };

        // Same frames, same fps, just reordered.
        let exit_duration_secs = entry_duration_secs;
        let exit_at = reversed_listfile.as_ref().and_then(|_| {
            let enough_room = break_duration_secs > entry_duration_secs + exit_duration_secs;
            enough_room.then(|| Instant::now() + Duration::from_secs_f64(break_duration_secs - exit_duration_secs))
        });

        self.ctx = Some(PlaybackContext {
            entry_path: video_path.to_string(),
            sleep_path: sleep_path.to_string(),
            out_w,
            out_h,
            entry_duration_secs,
            reversed_listfile,
            exit_at,
        });
        self.pending = Some(Pending {
            at: Instant::now() + Duration::from_secs_f64(entry_duration_secs.max(0.0)),
            next: NextClip::Sleep,
        });
    }

    /// Call once per second while a break overlay may be active; advances
    /// through the entry → rest → exit handoffs as their scheduled times
    /// arrive, each via an in-place `loadfile ... replace` over IPC rather
    /// than a process restart.
    pub fn tick(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        if Instant::now() < pending.at {
            return;
        }
        let next = self.pending.take().unwrap().next;
        let (Some(ctx), Some(socket)) = (&self.ctx, &self.ipc_socket) else {
            return;
        };

        let (mpv_input, is_frame_folder, motion, loop_forever) = match next {
            NextClip::Sleep => {
                let Some((mpv_input, is_frame_folder)) = mf_input(&ctx.sleep_path, None) else {
                    return;
                };
                (mpv_input, is_frame_folder, Motion::Static, true)
            }
            NextClip::Exit => {
                let Some(listfile) = ctx.reversed_listfile.as_deref() else {
                    return;
                };
                let Some((mpv_input, is_frame_folder)) = mf_input(&ctx.entry_path, Some(listfile)) else {
                    return;
                };
                (
                    mpv_input,
                    is_frame_folder,
                    Motion::SlideOut { duration_secs: ctx.entry_duration_secs },
                    false,
                )
            }
        };

        let graph = build_filter_graph(is_frame_folder, &motion, ctx.out_w, ctx.out_h);
        let loop_opt = if loop_forever { "inf" } else { "no" };
        let per_file_options = format!("mf-fps={FRAME_FOLDER_FPS},loop-file={loop_opt},lavfi-complex={graph}");
        let command = json!({"command": ["loadfile", mpv_input, "replace", per_file_options]});
        if !send_ipc_command(socket, &command) {
            notify("PawPause", "Lost the break video overlay's control connection.");
            return;
        }

        if next == NextClip::Sleep {
            if let Some(exit_at) = ctx.exit_at {
                self.pending = Some(Pending { at: exit_at, next: NextClip::Exit });
            }
        }
    }

    pub fn stop(&mut self) {
        self.pending = None;
        self.ctx = None;
        self.ipc_socket = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_graph_has_no_whitespace_and_keys_out_black_for_files() {
        let graph = build_filter_graph(false, &Motion::SlideIn, 1920, 1080);
        assert!(!graph.contains(' '), "graph must not contain spaces: {graph}");
        assert!(graph.contains("colorkey=0x000000"));
        assert!(graph.starts_with("color=c=black@0.0:s=1920x1080:d=3600[bg];"));
    }

    #[test]
    fn frame_folder_input_skips_colorkey() {
        let graph = build_filter_graph(true, &Motion::SlideIn, 1920, 1080);
        assert!(!graph.contains("colorkey"));
    }

    #[test]
    fn sleep_pose_is_statically_centered() {
        let graph = build_filter_graph(false, &Motion::Static, 1920, 1080);
        assert!(graph.contains("overlay=x='(1920-w)/2'"));
        assert!(!graph.contains("slide") && !graph.contains("lt(t,"));
    }

    #[test]
    fn every_motion_terminates_on_the_clips_own_eof_not_the_background() {
        for motion in [Motion::Static, Motion::SlideIn, Motion::SlideOut { duration_secs: 11.0 }] {
            let graph = build_filter_graph(false, &motion, 1920, 1080);
            assert!(
                graph.contains("eof_action=endall"),
                "without this, loop-file=inf never sees a real EOF and the clip just freezes: {graph}"
            );
        }
    }

    #[test]
    fn slide_out_holds_centered_then_exits_during_the_final_window() {
        let graph = build_filter_graph(false, &Motion::SlideOut { duration_secs: 11.0 }, 1920, 1080);
        // hold_until = duration_secs - SLIDE_IN_SECS = 11 - 3 = 8.
        assert!(graph.contains("if(lt(t,8)"), "should hold until 8s: {graph}");
        assert!(graph.contains("(1920-w)/2"), "held position should be centered: {graph}");
    }

    #[test]
    fn slide_out_never_produces_a_negative_hold_window() {
        // A clip shorter than SLIDE_IN_SECS should clamp hold_until to 0
        // rather than emit a negative time bound ffmpeg can't parse.
        let graph = build_filter_graph(false, &Motion::SlideOut { duration_secs: 1.0 }, 1920, 1080);
        assert!(graph.contains("if(lt(t,0)"), "hold_until should clamp to 0: {graph}");
        assert!(!graph.contains("lt(t,-"));
    }

    #[test]
    fn write_frame_listfile_preserves_given_order() {
        let dir = std::env::temp_dir().join("pawpause-overlay-test-listfile");
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("list.txt");
        let paths = vec![PathBuf::from("/tmp/b.png"), PathBuf::from("/tmp/a.png")];
        assert!(write_frame_listfile(&paths, &out));
        let content = std::fs::read_to_string(&out).unwrap();
        assert_eq!(content, "/tmp/b.png\n/tmp/a.png\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mf_input_prefers_the_reversed_listfile_when_given() {
        let listfile = Path::new("/tmp/pawpause-exit-list.txt");
        let (input, is_frame_folder) = mf_input("/some/entry/dir", Some(listfile)).unwrap();
        assert_eq!(input, "mf://@/tmp/pawpause-exit-list.txt");
        assert!(is_frame_folder);
    }

    #[test]
    fn mf_input_globs_a_directory_without_a_listfile() {
        let dir = std::env::temp_dir().join("pawpause-overlay-test-mf-input");
        let _ = std::fs::create_dir_all(&dir);
        let (input, is_frame_folder) = mf_input(dir.to_str().unwrap(), None).unwrap();
        assert!(input.starts_with("mf://") && input.ends_with("/*.png"));
        assert!(is_frame_folder);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mf_input_is_none_for_a_missing_path() {
        assert!(mf_input("/definitely/does/not/exist", None).is_none());
    }
}
