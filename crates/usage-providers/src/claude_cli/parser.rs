use std::time::{Duration, SystemTime, UNIX_EPOCH};
use usage_core::{Direction, FetchError, Measurement, NonNegative, QuotaWindow, WindowId};

const MAX_SCREEN_BYTES: usize = 256 * 1024;
const MAX_LINES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScreenState {
    Ready,
    Loading,
    Authentication,
    ApprovalRequired,
    Usage,
    Other,
}

pub(super) fn screen_state(screen: &str) -> ScreenState {
    if !bounded(screen) {
        return ScreenState::Other;
    }
    let lower = screen.to_lowercase();
    if [
        "not logged in",
        "please log in",
        "please login",
        "login required",
        "authentication required",
        "sign in to continue",
        "select login method",
        "select a login method",
        "invalid api key",
        "run /login",
        "/login to",
        "claude account with subscription",
        "console account",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
    {
        return ScreenState::Authentication;
    }
    if [
        "do you trust",
        "trust this folder",
        "trust this workspace",
        "accept the terms",
        "accept terms",
        "terms of service",
        "allow claude",
        "grant permission",
        "do you want to proceed",
        "approve",
        "consent",
        "choose the text style",
        "choose a theme",
        "select a theme",
        "security notes",
        "enter to confirm",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
    {
        return ScreenState::ApprovalRequired;
    }
    if ["loading", "fetching usage", "connecting", "starting claude"]
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        return ScreenState::Loading;
    }
    if screen.lines().any(|line| section(line).is_some()) {
        return ScreenState::Usage;
    }
    let lines: Vec<_> = screen
        .lines()
        .map(clean_line)
        .filter(|line| !line.is_empty())
        .collect();
    let has_banner = lower.contains("claude code");
    let ready = lines.iter().enumerate().any(|(index, line)| {
        matches!(*line, "❯" | ">")
            && lines[index + 1..].iter().all(|tail| {
                tail.chars()
                    .all(|c| matches!(c, '─' | '━' | ' ' | '│' | '╭' | '╰' | '╮' | '╯'))
                    || matches!(
                        *tail,
                        "? for shortcuts" | "! for bash mode · / for commands"
                    )
            })
    });
    if has_banner && ready {
        ScreenState::Ready
    } else {
        ScreenState::Other
    }
}

pub(super) fn parse_screen(screen: &str) -> Result<Vec<QuotaWindow>, FetchError> {
    if !bounded(screen) {
        return Err(FetchError::InvalidResponse);
    }
    match screen_state(screen) {
        ScreenState::Authentication => return Err(FetchError::NeedsAuthentication),
        ScreenState::ApprovalRequired => return Err(FetchError::PermissionDenied),
        ScreenState::Loading => return Err(FetchError::TemporarilyUnavailable),
        _ => {}
    }
    let mut windows: Vec<QuotaWindow> = Vec::new();
    let mut active = None;
    for line in screen.lines().map(clean_line) {
        if let Some((window, rest)) = section(line) {
            if windows.iter().any(|existing| existing.id == window.id) {
                return Err(FetchError::InvalidResponse);
            }
            windows.push(window);
            active = Some(windows.len() - 1);
            apply_line(windows.last_mut().unwrap(), rest)?;
            if !rest.is_empty()
                && !rest.chars().all(is_bar_character)
                && measurement(rest)?.is_none()
            {
                active = None;
            }
        } else if let Some(index) = active {
            if !line.is_empty()
                && !line.starts_with("Resets ")
                && !line.starts_with("Reset ")
                && !line.chars().all(is_bar_character)
                && measurement(line)?.is_none()
            {
                active = None;
            } else {
                apply_line(&mut windows[index], line)?;
            }
        }
    }
    if windows
        .iter()
        .any(|window| matches!(window.measurement, Measurement::Percent { .. }))
    {
        Ok(windows)
    } else {
        Err(FetchError::InvalidResponse)
    }
}

fn bounded(screen: &str) -> bool {
    screen.len() <= MAX_SCREEN_BYTES
        && screen.lines().count() <= MAX_LINES
        && !screen.chars().any(|c| c == '\u{1b}' || c == '\u{7f}')
}

fn clean_line(line: &str) -> &str {
    line.trim().trim_matches('│').trim()
}

fn section(line: &str) -> Option<(QuotaWindow, &str)> {
    let line = clean_line(line);
    let (label, rest, id, model, seconds) = if let Some(rest) = line.strip_prefix("Current session")
    {
        ("Current session", rest, "five_hour".to_owned(), None, 18000)
    } else {
        let rest = line.strip_prefix("Current week (")?;
        let end = rest.find(')')?;
        let name = &rest[..end];
        if name.is_empty()
            || name.len() > 80
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '.'))
        {
            return None;
        }
        let label = &line[.."Current week (".len() + end + 1];
        let model = if name == "all models" {
            None
        } else {
            Some(name.strip_suffix(" only").unwrap_or(name).to_owned())
        };
        let id = model
            .as_ref()
            .map(|name| format!("seven_day_{}", name.to_ascii_lowercase().replace(' ', "_")))
            .unwrap_or_else(|| "seven_day".to_owned());
        (label, &rest[end + 1..], id, model, 604800)
    };
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) && !rest.starts_with(':') {
        return None;
    }
    let rest = rest.trim_start_matches(':').trim();
    if !rest.is_empty()
        && !rest.starts_with(|c: char| {
            c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | '█' | '░' | '▒' | '▓')
        })
    {
        return None;
    }
    Some((
        QuotaWindow {
            id: WindowId::new(id).ok(),
            label: Some(label.to_owned()),
            measurement: Measurement::Unknown,
            model,
            period: Some(Duration::from_secs(seconds)),
            period_started_at: None,
            resets_at: None,
            trial_expires_at: None,
        },
        rest,
    ))
}

fn apply_line(window: &mut QuotaWindow, line: &str) -> Result<(), FetchError> {
    if let Some(measurement) = measurement(line)? {
        if window.measurement != Measurement::Unknown && window.measurement != measurement {
            return Err(FetchError::InvalidResponse);
        }
        window.measurement = measurement;
    }
    if let Some(reset) = line
        .strip_prefix("Resets ")
        .or_else(|| line.strip_prefix("Reset "))
    {
        window.resets_at = absolute_reset(reset);
    }
    Ok(())
}

fn is_bar_character(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '\u{2580}'..='\u{259f}' | '▰' | '▱' | '■' | '□' | '─' | '━'
        )
}

fn measurement(line: &str) -> Result<Option<Measurement>, FetchError> {
    let Some((before, after)) = line.split_once('%') else {
        return Ok(None);
    };
    let after = after.trim();
    let word = after
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(['.', ',']);
    let direction = match word {
        "used" => Direction::Used,
        "remaining" | "left" | "available" => Direction::Remaining,
        _ => return Ok(None),
    };
    let number = before
        .trim_end()
        .rsplit(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .next()
        .unwrap_or("");
    let prefix = before.trim_end().strip_suffix(number).unwrap_or(before);
    if !prefix.chars().all(is_bar_character) {
        return Ok(None);
    }
    if after.split_whitespace().count() != 1 {
        return Ok(None);
    }
    let value = number
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
        .and_then(|n| NonNegative::new(n).ok())
        .ok_or(FetchError::InvalidResponse)?;
    if after.contains('%') {
        return Err(FetchError::InvalidResponse);
    }
    Ok(Some(Measurement::Percent { value, direction }))
}

fn absolute_reset(reset: &str) -> Option<SystemTime> {
    let reset = reset.trim();
    let date = chrono::DateTime::parse_from_rfc3339(reset)
        .ok()
        .or_else(|| chrono::DateTime::parse_from_str(reset, "%Y-%m-%d %H:%M:%S %:z").ok())
        .or_else(|| chrono::DateTime::parse_from_str(reset, "%Y-%m-%d %H:%M %:z").ok())?;
    let millis = u64::try_from(date.timestamp_millis()).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUOTAS: &str = "Settings  Usage\nCurrent session\n████░░ 24% used\nResets in 2h 15m\nCurrent week (all models)\n72.5% remaining\nResets 2026-11-02T09:00:00+09:00\nCurrent week (Sonnet only)\n8% used\nResets Nov 2 at 9am (Asia/Seoul)\nCurrent week (Opus)\nUsage unavailable\nEsc to cancel";

    #[test]
    fn preserves_subscription_windows_directions_and_unknown_values() {
        let windows = parse_screen(QUOTAS).unwrap();
        assert_eq!(windows.len(), 4);
        assert_eq!(windows[0].id.as_ref().unwrap().as_str(), "five_hour");
        assert_eq!(windows[0].period, Some(Duration::from_secs(18000)));
        assert_eq!(
            windows[0].measurement,
            Measurement::Percent {
                value: NonNegative::new(24.0).unwrap(),
                direction: Direction::Used
            }
        );
        assert_eq!(
            windows[1].measurement,
            Measurement::Percent {
                value: NonNegative::new(72.5).unwrap(),
                direction: Direction::Remaining
            }
        );
        assert_eq!(windows[1].period, Some(Duration::from_secs(604800)));
        assert_eq!(
            windows[1].resets_at,
            Some(UNIX_EPOCH + Duration::from_secs(1793577600))
        );
        assert_eq!(windows[2].model.as_deref(), Some("Sonnet"));
        assert_eq!(windows[3].measurement, Measurement::Unknown);
        assert!(windows[0].resets_at.is_none());
        assert!(windows[2].resets_at.is_none());
    }

    #[test]
    fn missing_windows_are_not_invented() {
        let windows = parse_screen("Current session 0% used").unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(
            windows[0].measurement,
            Measurement::Percent {
                value: NonNegative::new(0.0).unwrap(),
                direction: Direction::Used
            }
        );
        assert_eq!(
            parse_screen("Current week (Haiku)\n100% left").unwrap()[0]
                .model
                .as_deref(),
            Some("Haiku")
        );
    }

    #[test]
    fn unrelated_or_ambiguous_percentages_do_not_establish_subscription_usage() {
        for screen in [
            "API billing\n10% used",
            "0% context left",
            "Current session\nUsage unavailable",
            "Current session\n10%",
            "Current session\nExtra usage\n20% used",
            "Current session spoof\n20% used",
            "Current session\nContext 20% used",
            "Current session\nAPI cost 20% used",
            "Current session 25% used for API billing\n25% used",
        ] {
            assert_eq!(
                parse_screen(screen),
                Err(FetchError::InvalidResponse),
                "{screen}"
            );
        }
    }

    #[test]
    fn unrecognized_rows_end_quota_association() {
        for boundary in [
            "Usage unavailable",
            "USAGE UNAVAILABLE",
            "Failed to load usage",
            "API Usage Billing",
            "Extra Usage",
            "usage-based billing",
            "Current session spoof",
            "Current week (invalid!)",
            "Context usage",
            "25% used for API billing",
        ] {
            let screen = format!("Current session\n{boundary}\n25% used");
            assert_eq!(
                parse_screen(&screen),
                Err(FetchError::InvalidResponse),
                "{screen}"
            );
            let screen = format!(
                "Current session\n10% used\n{boundary}\n25% used\nCurrent week (all models)\n20% used"
            );
            let windows = parse_screen(&screen).unwrap();
            assert_eq!(windows.len(), 2);
            assert_eq!(
                windows[0].measurement,
                Measurement::Percent {
                    value: NonNegative::new(10.0).unwrap(),
                    direction: Direction::Used,
                }
            );
        }
    }

    #[test]
    fn rejects_invalid_and_conflicting_measurements() {
        for value in ["-1", "101", "999", "1.2.3", "+101"] {
            assert_eq!(
                parse_screen(&format!("Current session\n{value}% used")),
                Err(FetchError::InvalidResponse)
            );
        }
        assert_eq!(
            parse_screen("Current session\n5% used\n6% used"),
            Err(FetchError::InvalidResponse)
        );
        assert_eq!(
            parse_screen("Current session 5% used\nCurrent session 5% used"),
            Err(FetchError::InvalidResponse)
        );
    }

    #[test]
    fn authentication_and_approval_override_old_usage_and_empty_prompt() {
        for notice in [
            "Not logged in · Please run /login",
            "Select login method",
            "Invalid API key",
        ] {
            let screen = format!("{QUOTAS}\n{notice}\n❯");
            assert_eq!(screen_state(&screen), ScreenState::Authentication);
            assert_eq!(parse_screen(&screen), Err(FetchError::NeedsAuthentication));
        }
        for notice in [
            "Do you trust the files in this folder?",
            "Accept the terms of service",
            "Choose a theme",
            "Do you want to proceed?",
        ] {
            let screen = format!("{QUOTAS}\n{notice}\n❯");
            assert_eq!(screen_state(&screen), ScreenState::ApprovalRequired);
            assert_eq!(parse_screen(&screen), Err(FetchError::PermissionDenied));
        }
    }

    #[test]
    fn only_recognized_idle_prompt_is_ready() {
        assert_eq!(
            screen_state(
                "Claude Code v2.1\n────────────────\n❯\n────────────────\n? for shortcuts"
            ),
            ScreenState::Ready
        );
        for screen in [
            "❯",
            "Claude Code\n❯ /usage",
            "Claude Code\n❯\nUnrecognized question?",
            "Claude Code\nLoading usage...\n❯",
        ] {
            assert_ne!(screen_state(screen), ScreenState::Ready);
        }
        assert_eq!(
            screen_state("Loading usage...\nCurrent session\n5% used"),
            ScreenState::Loading
        );
        assert_eq!(
            parse_screen("Loading usage...\nCurrent session\n5% used"),
            Err(FetchError::TemporarilyUnavailable)
        );
        assert_eq!(screen_state(QUOTAS), ScreenState::Usage);
    }

    #[test]
    fn limits_and_terminal_control_bytes_are_rejected() {
        assert_eq!(
            parse_screen(&"x".repeat(MAX_SCREEN_BYTES + 1)),
            Err(FetchError::InvalidResponse)
        );
        assert_eq!(
            parse_screen(&"\n".repeat(MAX_LINES + 1)),
            Err(FetchError::InvalidResponse)
        );
        assert_eq!(
            parse_screen("\x1b[2JCurrent session 5% used"),
            Err(FetchError::InvalidResponse)
        );
    }

    #[test]
    fn reset_requires_absolute_date_and_numeric_timezone() {
        for reset in [
            "in 2h",
            "9am",
            "Nov 2 at 9am (Asia/Seoul)",
            "2026-11-02T09:00:00",
            "2026-11-02 09:00 PST",
        ] {
            let windows =
                parse_screen(&format!("Current session\n5% used\nResets {reset}")).unwrap();
            assert!(windows[0].resets_at.is_none(), "{reset}");
        }
        let windows =
            parse_screen("Current session\n5% used\nResets 2026-11-02 00:00 +00:00").unwrap();
        assert_eq!(
            windows[0].resets_at,
            Some(UNIX_EPOCH + Duration::from_secs(1793577600))
        );
    }
}
