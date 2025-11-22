use ashmaize::*;
use chrono::{DateTime, Utc};

pub fn create_rom(no_pre_mine: &str) -> Rom {
    const MB: usize = 1024 * 1024;
    const GB: usize = 1024 * MB;

    let rom = Rom::new(
        no_pre_mine.as_bytes(),
        RomGenerationType::TwoStep {
            pre_size: 16 * MB,
            mixing_numbers: 4,
        },
        1 * GB,
    );
    rom
}

pub fn format_duration(mut seconds: i32) -> String {
    let hours = seconds / 3600;
    seconds %= 3600;
    let minutes = seconds / 60;
    seconds %= 60;

    let mut result = String::new();
    if hours > 0 {
        result.push_str(&format!("{}h", hours));
    }
    if minutes > 0 {
        result.push_str(&format!("{}m", minutes));
    }
    if seconds > 0 || result.is_empty() {
        result.push_str(&format!("{}s", seconds));
    }

    result
}

pub fn shorten_address(addr: &String) -> String {
    if addr.len() <= 24 {
        return addr.clone();
    }

    let prefix_len = 10;
    let suffix_len = 5;
    let start = &addr[..prefix_len];
    let end = &addr[addr.len() - suffix_len..];
    format!("{}...{}", start, end)
}

pub fn time_to_string(t: &DateTime<Utc>) -> String {
    return t.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
}

pub fn handle_submit_error(err: &anyhow::Error) -> String {
    let msg = err.to_string();

    if msg.contains("deadline has elapsed") {
        return "failed_to_submit_deadline_exceeded".into();
    }

    if msg.contains("timed out") || msg.contains("timeout") {
        return "failed_to_submit_timeout".into();
    }

    if msg.contains("Solution already exists") {
        return "failed_to_submit_solution_exists".into();
    }

    if msg.contains("Challenge window closed") {
        return "failed_to_submit_submission_window_closed".into();
    }

    "failed_to_submit_general".into()
}
