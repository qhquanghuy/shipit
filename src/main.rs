use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

include!(concat!(env!("OUT_DIR"), "/modules.rs"));

// The build script is compiled again here only so that its rules have unit tests.
#[cfg(test)]
#[path = "../build.rs"]
#[expect(
    dead_code,
    reason = "the build script's main is never called by the tests"
)]
mod build_script;

use crate::pure::dispatch::{
    Described, Invocation, command_help, dispatch, unknown_command, usage,
};
use crate::pure::failure::Failure;

type Run = fn(&[OsString], &mut dyn Write, &mut dyn Write) -> Result<(), Failure>;

struct Command {
    name: &'static str,
    hidden: bool,
    needs_setup: bool,
    usage: &'static str,
    summary: &'static str,
    exits: [&'static str; 4],
    run: Run,
}

impl Command {
    const fn described(&self) -> Described<'static> {
        Described {
            usage: self.usage,
            summary: self.summary,
            exits: self.exits,
            needs_setup: self.needs_setup,
        }
    }
}

fn execute(args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> Result<(), Failure> {
    let names: Vec<&str> = COMMANDS.iter().map(|command| command.name).collect();
    match dispatch(args, &names)? {
        Invocation::Help { command: None } => {
            let visible: Vec<Described<'_>> = COMMANDS
                .iter()
                .filter(|command| !command.hidden)
                .map(Command::described)
                .collect();
            write!(out, "{}", usage(&visible)).ok();
            Ok(())
        }
        Invocation::Help {
            command: Some(name),
        } => {
            let command = COMMANDS
                .iter()
                .find(|command| command.name == name)
                .ok_or_else(|| unknown_command(name))?;
            write!(out, "{}", command_help(&command.described())).ok();
            Ok(())
        }
        Invocation::Version => {
            writeln!(out, "shipit {}", env!("CARGO_PKG_VERSION")).ok();
            Ok(())
        }
        Invocation::Run { name, args } => {
            let command = COMMANDS
                .iter()
                .find(|command| command.name == name)
                .ok_or_else(|| unknown_command(name))?;
            (command.run)(args, out, err)
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "main is the only place that reads the arguments and opens the streams"
)]
fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    let result = execute(&args, &mut out, &mut err);
    let code = match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            writeln!(err, "shipit: {}", failure.message).ok();
            ExitCode::from(failure.exit.code())
        }
    };
    out.flush().ok();
    err.flush().ok();
    code
}
