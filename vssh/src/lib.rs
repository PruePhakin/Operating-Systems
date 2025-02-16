use std::env;
use nix::unistd::{fork, pipe, dup2, close, ForkResult, execvp};
use nix::libc::{STDIN_FILENO, STDOUT_FILENO};
use nix::sys::wait::waitpid;
use nix::libc::{self, _exit};
use std::ffi::CString;
use std::fs::File;
use std::os::unix::io::{AsRawFd, RawFd};



// Checks if a present ampersand is at the end of the command, also removes the ampersand from the arguments
// (-1 for invalid, 0 for no background process, 1 for background process)
pub fn background_process(arguments: &mut Vec<&str>) -> i32 {

    // Check if ampersand is the only argument
    if arguments.len() == 1 && arguments[0] == "&" {
        eprintln!("Error: syntax error near unexpected token `&'");
        return -1;
    }
    // Check if there is an ampersand at the start until the second-to-last index
    for i in 0..arguments.len() - 1 {
        if arguments[i] == "&" {
            eprintln!("Error: syntax error near unexpected token `&'");
            return -1;
        }
    }
    // Check if there is an ampersand at the end
    let last_arg: &str = arguments.last().unwrap_or(&"");
    // Remove the ampersand if it is at the end
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
    // Check if there is a '<' or '>' at the start or end
    if arguments.first() == Some(&"<") || arguments.first() == Some(&">") {
        eprintln!("Error: syntax error near unexpected token `{}`", arguments.first().unwrap());
        return -1;
    }
    // Check if there is a '<' or '>' at the start or end
    if arguments.last() == Some(&"<") || arguments.last() == Some(&">") {
        eprintln!("Error: syntax error near unexpected token `{}`", arguments.last().unwrap());
        return -1;
    }

    // Check for consecutive '<' or '>' symbols
    for i in 1..arguments.len() - 1 {
        if arguments[i] == "<" || arguments[i] == ">" {
            // Check if there is a valid file argument after the redirection symbol
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


    // Check if there is a '|' at the start or end
    if arguments.first() == Some(&"|") || arguments.last() == Some(&"|") {
        eprintln!("Error: syntax error near unexpected token `|'");
        return -1;
    }


    // Check for consecutive '|' symbols
    for i in 0..arguments.len() - 1 {
        if arguments[i] == "|" && arguments[i + 1] == "|" {
            eprintln!("Error: syntax error near unexpected token `||'");
            return -1;
        }
    }
 
    // Check if there is any '|' in the arguments
    if arguments.contains(&"|") {
        return 1;
    }

    // Return 0 if no pipeline
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

// Executes an external command with support for pipelines and background execution
pub fn external_command(mut arguments: Vec<&str>) -> Result<(), String> {

    let background_flag = background_process(&mut arguments);
    let redirection_flag = verify_redirection(&arguments);
    let pipeline_flag = verify_pipeline(&arguments);

    if background_flag == -1 || redirection_flag == -1 || pipeline_flag == -1{
        return Ok(());
    }

    match unsafe { fork() } {
        Ok(ForkResult::Parent { child, .. }) => {

            // Only wait if background flag is set to 0
            if background_flag == 0 {
                waitpid(child, None).map_err(|e| format!("Failed to wait on child process: {}", e))?;
            } else {
                println!("Starting background process {}", child);
            }
        }
        Ok(ForkResult::Child) => {

            // Handle redirection if needed
            if redirection_flag == 1 {
                if let Err(e) = handle_redirection(&mut arguments) {
                    eprintln!("{}", e);
                    unsafe { _exit(1); }
                }
            }

            // Execute the command
            let args = externalize(arguments);
            match execvp(&args[0], &args) {
                Ok(_) => unsafe { _exit(0); },
                Err(e) => {
                    eprintln!("execvp failed: {}", e);
                    unsafe { _exit(1); }
                }
            }
        }
        Err(e) => {
            return Err(format!("Fork failed: {}", e));
        }
    }
    Ok(())
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

    // Loop through the arguments and handle redirection
    while i < arguments.len() {
        if arguments[i] == "<" || arguments[i] == ">" {
            // Check if there is a valid file argument after the redirection symbol
            let file_path = arguments.get(i + 1).ok_or(format!("Error: missing file path for redirection `{}`", arguments[i]))?;
            let file = match arguments[i] {
                ">" => File::create(file_path).map_err(|e| format!("Error: {}", e)),
                "<" => File::open(file_path).map_err(|e| format!("Error: {}", e)),
                _ => {
                    eprintln!("Error: unknown redirection symbol `{}`", arguments[i]);
                    return Ok(false);
                }
            }?;

            // Check if the file was successfully opened
            let fd = file.as_raw_fd();
            let std_fd = match arguments[i] {
                ">" => 1, // STDOUT
                "<" => 0, // STDIN
                _ => return Err(format!("Error: unknown redirection symbol `{}`", arguments[i])),
            };

            // Duplicate the file descriptor
            unsafe {
                if libc::dup2(fd, std_fd) == -1 {
                    return Err(format!("Error: failed to duplicate file descriptor"));
                }
            }

            // Remove the redirection symbol and file path from arguments
            arguments.drain(i..=i + 1);
        } else {
            i += 1;
        } 
    }

    Ok(true)
}

// Handles the pipeline logic
pub fn handle_pipeline(arguments: Vec<&str>) -> Result<(), String> {
    let mut commands: Vec<Vec<&str>> = Vec::new();
    let mut current_command = Vec::new();

    // Split into separate commands at pipe symbols
    for &arg in arguments.iter() {
        if arg == "|" {
            if !current_command.is_empty() {
                commands.push(current_command);
                current_command = Vec::new();
            }
        } else {
            current_command.push(arg);
        }
    }
    // Add the last command
    if !current_command.is_empty() {
        commands.push(current_command);
    }

 
    Ok(())
}