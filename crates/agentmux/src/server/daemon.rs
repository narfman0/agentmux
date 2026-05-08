use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const DETACHED_PROCESS: u32 = 0x00000008;
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn spawn_server_with_agent(session: &str, agent: &str) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;

    #[cfg(windows)]
    {
        Command::new(&exe)
            .arg("__server")
            .arg(session)
            .arg(agent)
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()?;
    }

    #[cfg(not(windows))]
    {
        Command::new(&exe).arg("__server").arg(session).arg(agent).spawn()?;
    }

    Ok(())
}
