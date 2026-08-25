use std::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn command(program: &str) -> Command {
    let mut command = Command::new(program);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

#[cfg(test)]
mod tests {
    use super::command;

    #[test]
    fn background_commands_still_capture_output() {
        #[cfg(target_os = "windows")]
        let output = command("cmd")
            .args(["/C", "echo", "lantern-background-test"])
            .output()
            .expect("background command should start");

        #[cfg(not(target_os = "windows"))]
        let output = command("sh")
            .args(["-c", "printf lantern-background-test"])
            .output()
            .expect("background command should start");

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "lantern-background-test"
        );
    }
}
