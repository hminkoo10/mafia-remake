use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    #[serde(default = "default_true")]
    pub game_enabled: bool,
    pub participant_role: String,
    pub manager_role: String,
    pub default_mafia_count: u32,
    pub default_doctor_count: u32,
    pub default_police_count: u32,
    #[serde(default = "default_joker_count")]
    pub default_joker_count: u32,
    #[serde(default)]
    pub max_player_count: u32,
    pub night_seconds: u64,
    #[serde(default = "default_discussion_seconds")]
    pub discussion_seconds: u64,
    pub vote_seconds: u64,
    #[serde(default = "default_chat_slowmode_seconds")]
    pub chat_slowmode_seconds: u64,
    #[serde(default)]
    pub reveal_death_roles: bool,
    #[serde(default = "default_true")]
    pub reveal_public_police_status: bool,
    #[serde(default = "default_true")]
    pub reveal_morning_mafia_count: bool,
    #[serde(default)]
    pub citizen_special_count: u32,
    #[serde(default)]
    pub mafia_special_count: u32,
    #[serde(default = "default_neutral_special_count")]
    pub neutral_special_count: u32,
    #[serde(default = "default_true")]
    pub enable_detective: bool,
    #[serde(default = "default_true")]
    pub enable_graverobber: bool,
    #[serde(default = "default_true")]
    pub enable_spy: bool,
    #[serde(default = "default_true")]
    pub enable_contractor: bool,
    #[serde(default = "default_true")]
    pub enable_witch: bool,
    #[serde(default = "default_true")]
    pub enable_scientist: bool,
    #[serde(default = "default_true")]
    pub enable_madam: bool,
    #[serde(default = "default_true")]
    pub enable_godfather: bool,
    #[serde(default = "default_true")]
    pub enable_joker: bool,
    #[serde(default = "default_true")]
    pub enable_politician: bool,
    #[serde(default = "default_true")]
    pub enable_judge: bool,
    #[serde(default = "default_true")]
    pub enable_reporter: bool,
    #[serde(default = "default_true")]
    pub enable_hacker: bool,
    #[serde(default = "default_true")]
    pub enable_terrorist: bool,
    #[serde(default = "default_true")]
    pub enable_lover: bool,
    #[serde(default = "default_true")]
    pub enable_shaman: bool,
    #[serde(default = "default_true")]
    pub enable_priest: bool,
    #[serde(default = "default_true")]
    pub enable_soldier: bool,
    #[serde(default = "default_true")]
    pub enable_nurse: bool,
    #[serde(default = "default_true")]
    pub enable_gangster: bool,
    #[serde(default = "default_true")]
    pub enable_prophet: bool,
    #[serde(default = "default_true")]
    pub enable_psychologist: bool,
    #[serde(default = "default_true")]
    pub enable_mercenary: bool,
    #[serde(default = "default_true")]
    pub enable_thief: bool,
    #[serde(default)]
    pub enable_cult_team: bool,
    #[serde(default)]
    pub use_agent: bool,
    #[serde(default)]
    pub use_vigilante: bool,
    #[serde(default)]
    pub anonymous_mode: bool,
    #[serde(default = "default_anonymous_name_mode")]
    pub anonymous_name_mode: String,
    #[serde(default)]
    pub blacklist_user_ids: Vec<u64>,
}

pub fn load_config(path: impl AsRef<Path>) -> Result<BotConfig> {
    let path = path.as_ref();
    if !path.exists() {
        let example_path = path.with_file_name("config.example.json");
        fs::copy(&example_path, path).with_context(|| {
            format!(
                "config.json이 없어 config.example.json을 복사하지 못했습니다: {}",
                example_path.display()
            )
        })?;
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("config 파일을 읽지 못했습니다: {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("config JSON을 파싱하지 못했습니다: {}", path.display()))
}

pub fn save_config(path: impl AsRef<Path>, config: &BotConfig) -> Result<()> {
    let path = path.as_ref();
    let text = serde_json::to_string_pretty(config).context("config JSON 직렬화 실패")?;
    let temp_path = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.json")
    ));
    fs::write(&temp_path, format!("{text}\n")).with_context(|| {
        format!(
            "config 임시 파일을 쓰지 못했습니다: {}",
            temp_path.display()
        )
    })?;
    if path.exists() {
        fs::remove_file(path).with_context(|| {
            format!("기존 config 파일을 교체하지 못했습니다: {}", path.display())
        })?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("config 파일을 교체하지 못했습니다: {}", path.display()))?;
    Ok(())
}

const fn default_true() -> bool {
    true
}

const fn default_joker_count() -> u32 {
    1
}

const fn default_discussion_seconds() -> u64 {
    60
}

const fn default_chat_slowmode_seconds() -> u64 {
    3
}

const fn default_neutral_special_count() -> u32 {
    1
}

fn default_anonymous_name_mode() -> String {
    "animal".to_string()
}
