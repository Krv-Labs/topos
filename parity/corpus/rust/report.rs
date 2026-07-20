//! A branchy classifier plus a spawned shell command.
use std::process::Command;

pub struct Report {
    pub name: String,
    pub threshold: i32,
}

impl Report {
    pub fn classify(&self, score: i32, flags: &[&str]) -> String {
        let mut grade = match score {
            s if s > 90 => "A",
            s if s > 80 => "B",
            s if s > self.threshold => "C",
            _ => "F",
        }
        .to_string();
        if !flags.is_empty() && (flags.contains(&"urgent") || flags.contains(&"review")) {
            grade.push('!');
        }
        for extra in flags {
            if extra.starts_with("bonus") && grade != "F" {
                grade = "A+".to_string();
                break;
            }
        }
        grade
    }
}

pub fn run(cmd: &str) -> std::io::Result<std::process::Output> {
    Command::new("sh").arg("-c").arg(cmd).output()
}
