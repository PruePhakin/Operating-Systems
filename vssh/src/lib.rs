use std::env;
use nix::unistd::{fork, ForkResult, execvp};
use nix::libc::{self, _exit};
use nix::sys::wait::waitpid;
use std::ffi::CString;
use std::fs::File;
use std::os::unix::io::AsRawFd;

use std::process::{Command, Stdio};
use std::io::Error;

// Checks if a present ampersand is at the end of the command, also removes the ampersand from the arguments
// (-1 for invalid, 0 for no background process, 1 for background process)
pub fn background_process(arguments: &mut Vec<&str>) -> i32 {
    if arguments.len() == 1 && arguments[0] == "&" {
        eprintln!("Error: syntax error near unexpected token `&'");
        return -1;
    }
    for i in 0..arguments.len() - 1 {
        if arguments[i] == "&" {
            eprintln!("Error: syntax error near unexpected token `&'");
            return -1;
        }
    }
    let last_arg: &str = arguments.last().unwrap_or(&"");
    if last_arg == "&" {
        arguments.pop();
        1
    } else {
        0
    }
}

// Checks if there is a < or > argument at the second-to-first up to second-to-last index
// (-1 for invalid, 0 for no redirection, 1 for redirection)
pub fn verify_redirection(arguments: &[&str]) -> i32 {
    if arguments.first() == Some(&"<") || arguments.first() == Some(&">") {
        eprintln!("Error: syntax error near unexpected token `{}`", arguments.first().unwrap());
        return -1;
    }
    if arguments.last() == Some(&"<") || arguments.last() == Some(&">") {
        eprintln!("Error: syntax error near unexpected token `{}`", arguments.last().unwrap());
        return -1;
    }
    for i in 1..arguments.len() - 1 {
        if arguments[i] == "<" || arguments[i] == ">" {
            if i + 1 >= arguments.len() || arguments[i + 1] == "<" || arguments[i + 1] == ">" {
                eprintln!("Error: syntax error near unexpected token `{}`", arguments[i + 1]);
                return -1;
            }
            return 1;
        }
    }
    0
}

// Checks if the command has a pipeline and is valid
// (-1 for invalid, 0 for no pipeline, 1 for pipeline)
pub fn verify_pipeline(arguments: &[&str]) -> i32 {
    if arguments.first() == Some(&"|") || arguments.last() == Some(&"|") {
        eprintln!("Error: syntax error near unexpected token `|'");
        return -1;
    }
    for i in 0..arguments.len() - 1 {
        if arguments[i] == "|" && arguments[i + 1] == "|" {
            eprintln!("Error: syntax error near unexpected token `||'");
            return -1;
        }
    }
    if arguments.contains(&"|") {
        return 1;
    }
    0
}

// Executes the cd command
pub fn shell_command(arguments: Vec<&str>) {
    if let Some(directory) = arguments.get(1) {
        let directory = directory.to_string();
        if let Err(e) = env::set_current_dir(directory) {
            eprintln!("{}", e);
        }
    } else {
        eprintln!("cd needs an argument");
    }
}

// Executes an external command where it handles pipelines in a seperate function, else it forks the process and handles it here including redirection and background processing
pub fn external_command(mut arguments: Vec<&str>) -> Option<String> {
    let background_flag = background_process(&mut arguments);
    let redirection_flag = verify_redirection(&arguments);
    let pipeline_flag = verify_pipeline(&arguments);

    // Check if the command is invalid
    if background_flag == -1 || redirection_flag == -1 || pipeline_flag == -1 {
        return None;
    }

    // Go through the pipeline function to execute the pipeline
    if pipeline_flag == 1 {
        if let Err(e) = handle_pipeline(arguments){
            eprintln!("{}", e);
            
        }
        return None;
    }

    // Else just fork the process
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child, .. }) => {

            if background_flag == 0 {
                waitpid(child, None).expect("Failed to wait on child");
            } else {
                println!("Starting background process {}", child);
            }

        }
        Ok(ForkResult::Child) => {


            if redirection_flag == 1 {
                if let Err(e) = handle_redirection(&mut arguments) {
                    eprintln!("{}", e);
                    unsafe { _exit(1); }
                }
            }

            let args = externalize(arguments);
            match execvp(&args[0], &args) {
                Ok(_) => unsafe { _exit(0); },
                Err(_) => {
                    eprintln!("{} not found", args[0].to_str().unwrap());
                    unsafe { _exit(1); }
                }
            }
        }
        Err(e) => {
            eprintln!("Fork failed: {}", e);
            return None;
        }
    }
    None
}

// Convert a vector of string slices into a vector of CStrings
pub fn externalize(arguments: Vec<&str>) -> Vec<CString> {
    arguments.into_iter()
        .map(|s| CString::new(s).unwrap())
        .collect()
}

// Performs the redirection logic
pub fn handle_redirection(arguments: &mut Vec<&str>) -> Result<bool, String> {
    let mut i = 0;
    while i < arguments.len() {
        if arguments[i] == "<" || arguments[i] == ">" {
            let file_path = arguments.get(i + 1).ok_or(format!("Error: missing file path for redirection `{}`", arguments[i]))?;
            let file = match arguments[i] {
                ">" => File::create(file_path).map_err(|e| format!("Error: {}", e)),
                "<" => File::open(file_path).map_err(|e| format!("Error: {}", e)),
                _ => {
                    eprintln!("Error: unknown redirection symbol `{}`", arguments[i]);
                    return Ok(false);
                }
            }?;

            let fd = file.as_raw_fd();
            let std_fd = match arguments[i] {
                ">" => 1,
                "<" => 0,
                _ => return Err(format!("Error: unknown redirection symbol `{}`", arguments[i])),
            };

            unsafe {
                if libc::dup2(fd, std_fd) == -1 {
                    return Err(format!("Error: failed to duplicate file descriptor"));
                }
            }

            arguments.drain(i..=i + 1);
        } else {
            i += 1;
        }
    }
    Ok(true)
}


fn handle_pipeline(args: Vec<&str>) -> Result<(), Error> {
    // First, separate redirection from the command arguments
    let mut commands: Vec<Vec<&str>> = Vec::new();
    let mut current_command: Vec<&str> = Vec::new();
    let mut output_file: Option<&str> = None;
    
    let mut i = 0;
    // Iterate over the arguments
    while i < args.len() {
        match args[i] {
            // Parses out the commands seperated by pipes
            "|" => {
                if !current_command.is_empty() {
                    commands.push(current_command);
                    current_command = Vec::new();
                }
            }
            // Assigns the output file after redirection
            ">" => {
                if i + 1 < args.len() {
                    output_file = Some(args[i + 1]);
                    i += 1;
                }
            }
            _ => {
                current_command.push(args[i]);
            }
        }
        i += 1;
    }

    // Pushes out the last command in the pipeline
    if !current_command.is_empty() {
        commands.push(current_command);
    }

    let mut previous_stdout: Option<std::process::ChildStdout> = None;
    let commands_len = commands.len();

    // Iterate over the commands
    for (i, command) in commands.iter().enumerate() {
        if command.is_empty() {
            continue;
        }

        // Create the command
        let mut cmd = Command::new(command[0]);
        if command.len() > 1 {
            cmd.args(&command[1..]);
        }

        // Set up stdin from previous command's output if it exists
        if let Some(prev_stdout) = previous_stdout.take() {
            cmd.stdin(Stdio::from(prev_stdout));
        }

        // Set up stdout
        if i == commands_len - 1 {
            // Last command: handle file redirection if specified
            if let Some(outfile) = output_file {
                let file = File::create(outfile)?;
                cmd.stdout(Stdio::from(file));
            } else {
                cmd.stdout(Stdio::inherit());
            }
        } else {
            // Not the last command: pipe to next command
            cmd.stdout(Stdio::piped());
        }

        // Spawn the child command
        let mut child = cmd.spawn()?;

        // Store stdout for the next command if this isn't the last command
        if i != commands_len - 1 {
            previous_stdout = child.stdout.take();
        } else {
            // Wait for the last command to complete
            child.wait()?;
        }
    }

    Ok(())
}
