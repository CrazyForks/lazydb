use std::{env, io, process::Stdio, time::Duration};

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
    use super::user_var_sequence;

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
