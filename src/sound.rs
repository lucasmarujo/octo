use std::f64::consts::TAU;
use std::path::{Path, PathBuf};

use crate::fsutil::write_if_changed;
use crate::model::AgentState;
use crate::notify::Alert;
use crate::paths::Home;

pub const SAMPLE_RATE: u32 = 44_100;
const PEAK: f64 = 0.45;
const ATTACK: f64 = 0.002;
const DECAY: f64 = 3.1;
const RELEASE: f64 = 0.2;
const MODULATOR_RATIO: f64 = 3.5;
const MODULATION_INDEX: f64 = 2.1;
const MODULATION_DECAY: f64 = 6.0;
const WAV_HEADER: usize = 44;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chime {
    Waiting,
    Done,
}

struct Strike {
    at: f64,
    midi: f64,
    gain: f64,
    length: f64,
}

impl Chime {
    pub fn for_alerts(alerts: &[Alert]) -> Option<Self> {
        if alerts.is_empty() {
            return None;
        }
        Some(if alerts.iter().any(|alert| alert.state == AgentState::Paused) { Chime::Waiting } else { Chime::Done })
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Chime::Waiting => "chime-waiting.wav",
            Chime::Done => "chime-done.wav",
        }
    }

    fn strikes(self) -> [Strike; 2] {
        match self {
            Chime::Waiting => [
                Strike { at: 0.0, midi: 79.0, gain: 1.0, length: 1.2 },
                Strike { at: 0.09, midi: 76.0, gain: 0.83, length: 1.2 },
            ],
            Chime::Done => [
                Strike { at: 0.0, midi: 84.0, gain: 1.0, length: 1.1 },
                Strike { at: 0.075, midi: 88.0, gain: 1.0, length: 1.1 },
            ],
        }
    }

    pub fn samples(self) -> Vec<i16> {
        let strikes = self.strikes();
        let rate = f64::from(SAMPLE_RATE);
        let length = strikes.iter().map(|strike| strike.at + strike.length).fold(0.0, f64::max);
        let mut mix = vec![0.0; (length * rate).ceil() as usize];
        for strike in &strikes {
            let frequency = 440.0 * 2f64.powf((strike.midi - 69.0) / 12.0);
            let start = (strike.at * rate).round() as usize;
            let count = ((strike.length * rate) as usize).min(mix.len() - start);
            for (i, sample) in mix[start..start + count].iter_mut().enumerate() {
                let t = i as f64 / rate;
                let envelope = (t / ATTACK).min(1.0) * (-DECAY * t).exp() * ((strike.length - t) / RELEASE).min(1.0);
                let index = MODULATION_INDEX * (-MODULATION_DECAY * t).exp();
                let modulator = (TAU * MODULATOR_RATIO * frequency * t).sin();
                *sample += strike.gain * envelope * (TAU * frequency * t + index * modulator).sin();
            }
        }
        let peak = mix.iter().fold(f64::EPSILON, |peak, sample| peak.max(sample.abs()));
        mix.iter().map(|sample| (sample / peak * PEAK * f64::from(i16::MAX)).round() as i16).collect()
    }

    pub fn wav(self) -> Vec<u8> {
        wav(&self.samples())
    }
}

pub fn wav(samples: &[i16]) -> Vec<u8> {
    let data = u32::try_from(samples.len() * 2).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(WAV_HEADER + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(data + 36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

pub fn plays_locally(os: &str, var: impl Fn(&str) -> Option<String>) -> bool {
    let set = |key: &str| var(key).is_some_and(|value| !value.is_empty());
    if set("SSH_CONNECTION") || set("SSH_TTY") {
        return false;
    }
    match os {
        "windows" | "macos" => true,
        "linux" => set("DISPLAY") || set("WAYLAND_DISPLAY"),
        _ => false,
    }
}

pub struct Player {
    dir: PathBuf,
}

impl Player {
    pub fn new(home: &Home) -> Option<Self> {
        plays_locally(std::env::consts::OS, |key| std::env::var(key).ok())
            .then(|| Self { dir: home.root().to_path_buf() })
    }

    pub fn play(&self, chime: Chime) -> bool {
        play(chime, &self.dir)
    }
}

pub fn prepare(chime: Chime, dir: &Path) -> std::io::Result<PathBuf> {
    let file = dir.join(chime.file_name());
    write_if_changed(&file, &chime.wav())?;
    Ok(file)
}

#[cfg(windows)]
fn play(chime: Chime, _dir: &Path) -> bool {
    use std::sync::LazyLock;

    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_MEMORY: u32 = 0x0004;

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn PlaySoundW(sound: *const u16, module: isize, flags: u32) -> i32;
    }

    static WAITING: LazyLock<Vec<u8>> = LazyLock::new(|| Chime::Waiting.wav());
    static DONE: LazyLock<Vec<u8>> = LazyLock::new(|| Chime::Done.wav());
    let wav: &'static [u8] = match chime {
        Chime::Waiting => &WAITING,
        Chime::Done => &DONE,
    };
    unsafe { PlaySoundW(wav.as_ptr().cast(), 0, SND_ASYNC | SND_MEMORY | SND_NODEFAULT) != 0 }
}

#[cfg(target_os = "macos")]
const PLAYERS: &[(&str, &[&str])] = &[("afplay", &[])];

#[cfg(all(unix, not(target_os = "macos")))]
const PLAYERS: &[(&str, &[&str])] = &[("paplay", &[]), ("pw-play", &[]), ("aplay", &["-q"])];

#[cfg(unix)]
fn play(chime: Chime, dir: &Path) -> bool {
    use std::process::{Command, Stdio};

    let Some((program, args)) =
        PLAYERS.iter().find_map(|(name, args)| which::which(name).ok().map(|path| (path, *args)))
    else {
        return false;
    };
    let Ok(file) = prepare(chime, dir) else { return false };
    let spawned = Command::new(program)
        .args(args)
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(any(windows, unix)))]
fn play(_chime: Chime, _dir: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentKind;
    use std::collections::HashMap;

    fn alert(state: AgentState) -> Alert {
        Alert {
            id: 1,
            name: "fix-login".into(),
            kind: AgentKind::Claude,
            state,
            session: "default".into(),
            cwd: PathBuf::from("/work"),
            activity: String::new(),
        }
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn waiting_wins_over_done_when_alerts_arrive_together() {
        assert_eq!(Chime::for_alerts(&[]), None);
        assert_eq!(Chime::for_alerts(&[alert(AgentState::Done)]), Some(Chime::Done));
        assert_eq!(Chime::for_alerts(&[alert(AgentState::Paused)]), Some(Chime::Waiting));
        assert_eq!(Chime::for_alerts(&[alert(AgentState::Done), alert(AgentState::Paused)]), Some(Chime::Waiting));
    }

    #[test]
    fn chimes_are_short_soft_and_end_in_silence() {
        for chime in [Chime::Waiting, Chime::Done] {
            let samples = chime.samples();
            let seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
            assert!((1.0..1.5).contains(&seconds), "{chime:?} dura {seconds}s");
            let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
            assert!((f64::from(peak) - PEAK * f64::from(i16::MAX)).abs() <= 1.0, "{chime:?} pico {peak}");
            assert_eq!(samples[0], 0, "{chime:?} começa do silêncio");
            let tail = &samples[samples.len() - 200..];
            assert!(tail.iter().all(|s| s.unsigned_abs() < 100), "{chime:?} termina em silêncio");
        }
        assert_ne!(Chime::Waiting.samples(), Chime::Done.samples());
        assert_eq!(Chime::Done.samples(), Chime::Done.samples());
    }

    #[test]
    fn waiting_steps_down_and_done_steps_up() {
        let waiting = Chime::Waiting.strikes();
        let done = Chime::Done.strikes();
        assert!(waiting[1].midi < waiting[0].midi && waiting[1].at > waiting[0].at);
        assert!(done[1].midi > done[0].midi && done[1].at > done[0].at);
    }

    #[test]
    fn wav_is_16_bit_mono_pcm() {
        let samples = Chime::Waiting.samples();
        let bytes = wav(&samples);
        let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let u32_at = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16_at(20), 1);
        assert_eq!(u16_at(22), 1);
        assert_eq!(u32_at(24), SAMPLE_RATE);
        assert_eq!(u16_at(34), 16);
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32_at(40) as usize, samples.len() * 2);
        assert_eq!(u32_at(4) as usize, bytes.len() - 8);
        assert_eq!(i16::from_le_bytes([bytes[44], bytes[45]]), samples[0]);
    }

    #[test]
    fn sound_plays_only_on_the_machine_in_front_of_the_user() {
        assert!(plays_locally("windows", env(&[])));
        assert!(plays_locally("macos", env(&[])));
        assert!(plays_locally("linux", env(&[("DISPLAY", ":0")])));
        assert!(plays_locally("linux", env(&[("WAYLAND_DISPLAY", "wayland-0")])));
        assert!(!plays_locally("linux", env(&[])));
        assert!(!plays_locally("macos", env(&[("SSH_CONNECTION", "10.0.0.2 51000 10.0.0.1 22")])));
        assert!(!plays_locally("windows", env(&[("SSH_TTY", "/dev/pts/1")])));
        assert!(plays_locally("windows", env(&[("SSH_TTY", "")])));
        assert!(!plays_locally("freebsd", env(&[("DISPLAY", ":0")])));
    }

    #[test]
    fn prepared_files_hold_each_chime() {
        let dir = std::env::temp_dir().join(format!("orkai-sound-{}-{}", std::process::id(), crate::util::now_ms()));
        let waiting = prepare(Chime::Waiting, &dir).unwrap();
        let done = prepare(Chime::Done, &dir).unwrap();
        assert_ne!(waiting, done);
        assert_eq!(std::fs::read(&waiting).unwrap(), Chime::Waiting.wav());
        assert_eq!(std::fs::read(&done).unwrap(), Chime::Done.wav());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
