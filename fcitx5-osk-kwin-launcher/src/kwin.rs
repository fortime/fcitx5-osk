use std::{path::PathBuf, process::Stdio, time::Duration};

use anyhow::{Context, Result};
use tokio::process::Command;

async fn set_input_method(
    kwriteconfig: Option<&PathBuf>,
    input_method: Option<&str>,
) -> Result<()> {
    let mut command = if let Some(exec) = kwriteconfig {
        Command::new(exec)
    } else {
        Command::new("kwriteconfig6")
    };
    command.args([
        "--file",
        "kwinrc",
        "--group",
        "Wayland",
        "--key",
        "InputMethod",
        "--notify",
    ]);
    if let Some(input_method) = input_method {
        command.arg(input_method);
    } else {
        command.arg("--delete");
    }
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = child
        .wait_with_output()
        .await
        .context("Unable to run a process to read current input method")?;
    if output.status.success() {
        tracing::info!(
            "`InputMethod` is updated, stdout: {:?}, stderr: {:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    } else {
        anyhow::bail!(
            "Running the process failed with code[{:?}], stdout: {:?}, stderr: {:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub async fn cur_input_method(kreadconfig: Option<&PathBuf>) -> Result<String> {
    let mut command = if let Some(exec) = kreadconfig {
        Command::new(exec)
    } else {
        Command::new("kreadconfig6")
    };
    let child = command
        .args([
            "--file",
            "kwinrc",
            "--group",
            "Wayland",
            "--key",
            "InputMethod",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = child
        .wait_with_output()
        .await
        .context("Unable to run a process to read current input method")?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map(|s| s.trim().to_string())
            .context("The path of InputMethod is not a valid utf8 string")
    } else {
        anyhow::bail!(
            "Running the process failed with code[{:?}], stdout: {:?}, stderr: {:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub async fn restart_input_method(
    kreadconfig: Option<&PathBuf>,
    kwriteconfig: Option<&PathBuf>,
    input_method: Option<&str>,
    next: Option<&PathBuf>,
) -> Result<()> {
    tracing::info!("Next InputMethod: {:?}", next);
    let input_method = if let Some(input_method) = input_method {
        let cur_input_method = cur_input_method(kreadconfig).await?;
        if cur_input_method != input_method {
            tracing::info!(
                "InputMethod has been switched to [{}] from [{}], don't restart",
                cur_input_method,
                input_method,
            );
            return Ok(());
        }
        input_method
    } else {
        ""
    };
    if let Some(next) = next.filter(|p| p.is_file())
        && let Some(next) = next.to_str()
    {
        set_input_method(kwriteconfig, Some(next)).await?;
    } else if !input_method.is_empty() {
        set_input_method(kwriteconfig, None).await?;
        // wait a moment
        tokio::time::sleep(Duration::from_secs(1)).await;
        set_input_method(kwriteconfig, Some(input_method)).await?;
    }
    Ok(())
}
