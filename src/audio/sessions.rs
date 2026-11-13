//! Pure helpers for audio sessions: identifying the app and volume maths.

use crate::input::Direction;

/// Below this the session counts as silent.
const SILENCE: f32 = 0.005;
/// Scrolling up from silence starts here (1%), since a multiplicative step
/// can never climb away from zero on its own.
const FLOOR: f32 = 0.01;

/// Gets the executable file name from a session instance identifier.
///
/// Windows formats these as
/// `{device}|\Device\HarddiskVolume3\...\Spotify.exe%b{guid}|1%b1234`.
/// The exe path is read from this string, so Scrollify never opens a handle
/// to the process (or to any game) to find out what it is.
pub fn exe_from_instance_id(id: &str) -> Option<&str> {
    id.split('|').find_map(|segment| {
        let path = segment.split("%b").next()?;
        let name = path.rsplit(['\\', '/']).next()?;
        let is_exe = name.len() > 4 && name.to_ascii_lowercase().ends_with(".exe");
        is_exe.then_some(name)
    })
}

pub fn is_target(exe: &str, targets: &[String]) -> bool {
    targets.iter().any(|t| t.eq_ignore_ascii_case(exe))
}

/// Applies one dB step to a 0..=1 session volume.
pub fn step_volume(current: f32, step_db: f32, direction: Direction) -> f32 {
    let ratio = 10f32.powf(step_db / 20.0);
    match direction {
        Direction::Up if current < SILENCE => FLOOR,
        Direction::Up => (current * ratio).min(1.0),
        Direction::Down => {
            let next = current / ratio;
            if next < SILENCE {
                0.0
            } else {
                next
            }
        }
    }
}

/// `Spotify.exe` → `Spotify`.
pub fn display_name(exe: &str) -> &str {
    if exe.to_ascii_lowercase().ends_with(".exe") {
        &exe[..exe.len() - 4]
    } else {
        exe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exe_from_instance_id() {
        let id = r"{0.0.0.00000000}.{a1b2}|\Device\HarddiskVolume3\Users\me\AppData\Roaming\Spotify\Spotify.exe%b{00000000-0000-0000-0000-000000000000}|1%b9876";
        assert_eq!(exe_from_instance_id(id), Some("Spotify.exe"));
    }

    #[test]
    fn system_sounds_have_no_exe() {
        let id = "{0.0.0.00000000}.{a1b2}|#%b{A9EF3FD9-4240-455E-A4D5-F2B3301887B2}|1%b#";
        assert_eq!(exe_from_instance_id(id), None);
    }

    #[test]
    fn target_match_ignores_case() {
        let targets = vec!["Spotify.exe".to_string()];
        assert!(is_target("SPOTIFY.EXE", &targets));
        assert!(!is_target("firefox.exe", &targets));
    }

    #[test]
    fn six_db_roughly_doubles() {
        let up = step_volume(0.25, 6.0, Direction::Up);
        assert!((up - 0.499).abs() < 0.01);
        let down = step_volume(0.5, 6.0, Direction::Down);
        assert!((down - 0.2506).abs() < 0.01);
    }

    #[test]
    fn volume_stays_in_range_and_leaves_zero() {
        assert_eq!(step_volume(0.99, 12.0, Direction::Up), 1.0);
        assert_eq!(step_volume(0.006, 3.0, Direction::Down), 0.0);
        assert_eq!(step_volume(0.0, 1.0, Direction::Up), 0.01);
    }

    #[test]
    fn display_name_drops_extension() {
        assert_eq!(display_name("Spotify.exe"), "Spotify");
        assert_eq!(display_name("thing"), "thing");
    }
}
