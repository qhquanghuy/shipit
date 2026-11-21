use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

include!(concat!(env!("OUT_DIR"), "/modules.rs"));

use crate::pure::dispatch::{Invocation, dispatch, unknown_command, usage};
use crate::pure::failure::Failure;

type Run = fn(&[OsString], &mut dyn Write, &mut dyn Write) -> Result<(), Failure>;

struct Command {
    name: &'static str,
    hidden: bool,
    run: Run,
}

fn execute(args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> Result<(), Failure> {
    let names: Vec<&str> = COMMANDS.iter().map(|command| command.name).collect();
    match dispatch(args, &names)? {
        Invocation::Help => {
            let visible: Vec<&str> = COMMANDS
                .iter()
                .filter(|command| !command.hidden)
                .map(|command| command.name)
                .collect();
            write!(out, "{}", usage(&visible)).ok();
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
