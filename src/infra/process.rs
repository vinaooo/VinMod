use std::error::Error;
use std::fmt::{Display, Formatter};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub enum ProcessError {
    Spawn(std::io::Error),
}

impl Display for ProcessError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "Process spawn failed: {error}"),
        }
    }
}

impl Error for ProcessError {}

pub trait ProcessExecutor: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> Result<ProcessOutput, ProcessError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemProcessExecutor;

impl ProcessExecutor for SystemProcessExecutor {
    fn run(&self, program: &str, args: &[&str]) -> Result<ProcessOutput, ProcessError> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(ProcessError::Spawn)?;

        Ok(ProcessOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}