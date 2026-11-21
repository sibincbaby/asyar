//! Cross-platform media-session abstraction.
//!
//! Linux: MPRIS over the session D-Bus (`linux::media`).

use crate::error::PlatformError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaTransportAction {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSessionInfo {
    /// Player identifier (MPRIS bus name).
    pub player_id: String,
    /// Display name, e.g. `Music`.
    pub player_name: String,
    pub status: PlaybackStatus,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_seconds: Option<f64>,
    pub position_seconds: Option<f64>,
}

/// Now-playing info for `player_id`, or the best running player (playing
/// preferred over paused) when `None`. `player_id` is a bus name or its
/// suffix (Linux MPRIS).
pub fn get_now_playing(player_id: Option<&str>) -> Result<Option<MediaSessionInfo>, PlatformError> {
    crate::linux::media::get_now_playing(player_id)
}

/// All running, supported players that currently have a track loaded.
pub fn list_sessions() -> Result<Vec<MediaSessionInfo>, PlatformError> {
    crate::linux::media::list_sessions()
}

/// Sends a transport action. With `player_id == None` it goes to the active
/// player.
pub fn send_transport(
    action: MediaTransportAction,
    player_id: Option<&str>,
) -> Result<(), PlatformError> {
    crate::linux::media::send_transport(action, player_id)
}

/// Prefers a playing session over a paused one; stopped sessions never win.
/// Ties keep the earliest session.
pub(crate) fn pick_best(sessions: Vec<MediaSessionInfo>) -> Option<MediaSessionInfo> {
    let mut best: Option<MediaSessionInfo> = None;
    for s in sessions {
        match (&best, s.status) {
            (_, PlaybackStatus::Stopped) => {}
            (None, _) => best = Some(s),
            (Some(b), PlaybackStatus::Playing) if b.status != PlaybackStatus::Playing => {
                best = Some(s)
            }
            _ => {}
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_is_camel_case_with_lowercase_status() {
        let info = MediaSessionInfo {
            player_id: "com.apple.Music".into(),
            player_name: "Music".into(),
            status: PlaybackStatus::Playing,
            title: Some("T".into()),
            artist: None,
            album: None,
            duration_seconds: Some(1.5),
            position_seconds: None,
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["playerId"], "com.apple.Music");
        assert_eq!(v["status"], "playing");
        assert_eq!(v["durationSeconds"], 1.5);
        assert_eq!(
            serde_json::to_value(MediaTransportAction::PlayPause).unwrap(),
            "playPause"
        );
    }

    fn session(id: &str, status: PlaybackStatus) -> MediaSessionInfo {
        MediaSessionInfo {
            player_id: id.into(),
            player_name: id.into(),
            status,
            title: None,
            artist: None,
            album: None,
            duration_seconds: None,
            position_seconds: None,
        }
    }

    #[test]
    fn pick_best_prefers_playing_over_paused() {
        let best = pick_best(vec![
            session("a", PlaybackStatus::Paused),
            session("b", PlaybackStatus::Playing),
            session("c", PlaybackStatus::Stopped),
        ])
        .unwrap();
        assert_eq!(best.player_id, "b");
        assert!(pick_best(vec![session("c", PlaybackStatus::Stopped)]).is_none());
        assert!(pick_best(vec![]).is_none());
    }
}
