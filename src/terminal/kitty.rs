use std::{env, io, process::Stdio, time::Duration};

use serde::Deserialize;
use tokio::{process::Command, time::timeout};

use crate::model::pane_navigation::PaneDirection;

pub fn available() -> bool {
    env::var_os("KITTY_LISTEN_ON").is_some_and(|v| !v.is_empty())
        && env::var_os("KITTY_WINDOW_ID").is_some_and(|v| !v.is_empty())
}

pub async fn neighboring_window(direction: PaneDirection) -> io::Result<()> {
    let socket = env::var_os("KITTY_LISTEN_ON")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "KITTY_LISTEN_ON is unset"))?;
    let window = env::var_os("KITTY_WINDOW_ID")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "KITTY_WINDOW_ID is unset"))?;
    let direction = match direction {
        PaneDirection::Left => "left",
        PaneDirection::Right => "right",
        PaneDirection::Up => "top",
        PaneDirection::Down => "bottom",
    };
    let mut child = Command::new("kitten")
        .arg("@")
        .arg("--to")
        .arg(socket)
        .arg("action")
        .arg("--match")
        .arg(format!("id:{}", window.to_string_lossy()))
        .arg(format!("neighboring_window {direction}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    match timeout(Duration::from_secs(1), child.wait()).await {
        Ok(result) => result.and_then(|status| {
            status
                .success()
                .then_some(())
                .ok_or_else(|| io::Error::other("kitty action failed"))
        }),
        Err(_) => {
            let _ = child.kill().await;
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "kitty action timed out",
            ))
        }
    }
}

pub async fn resize_window(direction: PaneDirection, amount: u16) -> io::Result<()> {
    let socket = env::var_os("KITTY_LISTEN_ON")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "KITTY_LISTEN_ON is unset"))?;
    let window = env::var_os("KITTY_WINDOW_ID")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "KITTY_WINDOW_ID is unset"))?;
    let direction = match direction {
        PaneDirection::Left => "left",
        PaneDirection::Right => "right",
        PaneDirection::Up => "up",
        PaneDirection::Down => "down",
    };
    let mut child = Command::new("kitten")
        .arg("@")
        .arg("--to")
        .arg(socket)
        .arg("kitten")
        .arg("--match")
        .arg(format!("id:{}", window.to_string_lossy()))
        .arg("lazydb_resize.py")
        .arg(direction)
        .arg(amount.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    match timeout(Duration::from_secs(1), child.wait()).await {
        Ok(result) => result.and_then(|status| {
            status
                .success()
                .then_some(())
                .ok_or_else(|| io::Error::other("kitty resize action failed"))
        }),
        Err(_) => {
            let _ = child.kill().await;
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "kitty resize timed out",
            ))
        }
    }
}

#[derive(Deserialize)]
struct KittyOsWindow {
    tabs: Vec<KittyTab>,
}

#[derive(Deserialize)]
struct KittyTab {
    id: u64,
    layout: String,
    windows: Vec<KittyWindow>,
    // An overlay shares its underlying window's layout group.
    groups: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct KittyWindow {
    id: u64,
}

#[derive(Debug, PartialEq)]
struct MaximizeTarget {
    tab_id: u64,
    maximized: bool,
}

fn smart_maximize_target(output: &[u8], window_id: u64) -> io::Result<Option<MaximizeTarget>> {
    let windows: Vec<KittyOsWindow> = serde_json::from_slice(output)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let tab = windows
        .into_iter()
        .flat_map(|window| window.tabs)
        .find(|tab| tab.windows.iter().any(|window| window.id == window_id))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Kitty window was not found"))?;
    let pane_count = tab.groups.as_ref().map_or(tab.windows.len(), Vec::len);
    if pane_count < 2 {
        return Ok(None);
    }
    Ok(Some(MaximizeTarget {
        tab_id: tab.id,
        maximized: tab.layout != "stack",
    }))
}

async fn zoom_command(socket: &std::ffi::OsStr, args: &[&str]) -> io::Result<Vec<u8>> {
    let output = timeout(
        Duration::from_secs(1),
        Command::new("kitten")
            .arg("@")
            .arg("--to")
            .arg(socket)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Kitty zoom action timed out"))??;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Kitty zoom action failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

/// Returns the explicit synchronized state, or None for a single layout group.
pub async fn toggle_maximized() -> io::Result<Option<bool>> {
    let socket = env::var_os("KITTY_LISTEN_ON")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "KITTY_LISTEN_ON is unset"))?;
    let window_id = env::var("KITTY_WINDOW_ID")
        .ok()
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "KITTY_WINDOW_ID is invalid"))?;
    let output = zoom_command(&socket, &["ls"]).await?;
    let Some(target) = smart_maximize_target(&output, window_id)? else {
        return Ok(None);
    };
    let tab_match = format!("id:{}", target.tab_id);
    if target.maximized {
        zoom_command(&socket, &["goto-layout", "--match", &tab_match, "stack"]).await?;
    } else {
        zoom_command(&socket, &["last-used-layout", "--match", &tab_match]).await?;
    }
    // Verify the acknowledged target, including tabs initially created in stack
    // with no previous layout. Never report a restore that did not actually occur.
    let output = zoom_command(&socket, &["ls"]).await?;
    match smart_maximize_target(&output, window_id)? {
        Some(next) if next.tab_id == target.tab_id && next.maximized != target.maximized => {
            Ok(Some(target.maximized))
        }
        None => Ok(None),
        _ => Err(io::Error::other("Kitty did not reach the requested layout")),
    }
}

pub fn set_user_var(writer: &mut impl io::Write, enabled: bool) -> io::Result<()> {
    if !available() {
        return Ok(());
    }
    writer
        .write_all(user_var_sequence(enabled))
        .and_then(|_| writer.flush())
}

fn user_var_sequence(enabled: bool) -> &'static [u8] {
    if enabled {
        b"\x1b]1337;SetUserVar=IS_LAZYDB=MQ==\x07"
    } else {
        b"\x1b]1337;SetUserVar=IS_LAZYDB\x07"
    }
}

#[cfg(test)]
mod tests {
    use super::{MaximizeTarget, smart_maximize_target, user_var_sequence};

    #[test]
    fn smart_maximize_targets_owning_tab_across_os_windows() {
        let windows = serde_json::json!([
            {"tabs": [{"id": 10, "layout": "stack", "windows": [{"id": 1}, {"id": 2}]}]},
            {"tabs": [{"id": 20, "layout": "splits", "is_active": false,
                "windows": [{"id": 3}, {"id": 4}]}]}
        ]);
        let output = serde_json::to_vec(&windows).unwrap();
        assert_eq!(
            smart_maximize_target(&output, 4).unwrap(),
            Some(MaximizeTarget {
                tab_id: 20,
                maximized: true
            })
        );
        assert_eq!(
            smart_maximize_target(&output, 2).unwrap(),
            Some(MaximizeTarget {
                tab_id: 10,
                maximized: false
            })
        );
        assert!(smart_maximize_target(&output, 99).is_err());
    }

    #[test]
    fn smart_maximize_counts_overlay_groups_instead_of_process_windows() {
        let windows = serde_json::json!([{"tabs": [{"id": 10, "layout": "splits",
            "windows": [{"id": 1}, {"id": 2}],
            "groups": [{"id": 1, "windows": [1, 2]}]}]}]);
        assert_eq!(
            smart_maximize_target(&serde_json::to_vec(&windows).unwrap(), 2).unwrap(),
            None
        );
        let legacy_single = br#"[{"tabs":[{"id":10,"layout":"stack","windows":[{"id":1}]}]}]"#;
        assert_eq!(smart_maximize_target(legacy_single, 1).unwrap(), None);
        assert!(smart_maximize_target(b"invalid JSON", 1).is_err());
        assert!(smart_maximize_target(br#"[{"tabs":[{"id":10}]}]"#, 1).is_err());
    }

    #[test]
    fn lazydb_user_var_sequences_are_stable() {
        assert_eq!(
            user_var_sequence(true),
            b"\x1b]1337;SetUserVar=IS_LAZYDB=MQ==\x07"
        );
        assert_eq!(
            user_var_sequence(false),
            b"\x1b]1337;SetUserVar=IS_LAZYDB\x07"
        );
    }
}
