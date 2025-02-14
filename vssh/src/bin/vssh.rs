use nix::unistd::{fork, ForkResult, execvp};
use nix::sys::wait::waitpid;
use std::env;
use std::fs::File;
use std::path::Path;
use std::io::{self, Write, Read};

fn main()
{

    while true
    {
        // Print the current directory
        let current_dir = env::current_dir().expect("Failed to get current directory");
        print!("{}$ ", current_dir.display());
        io::stdout().flush().expect("Failed to flush stdout");

        // Get user command and clean it up
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Failed to read line");

        // Trim whitespaces, then split words seperated by whitespaces and then store into arguments
        let arguments: Vec<&str> = input.trim().split_whitespace().collect();
        let command = arguments.first().unwrap_or(&"");

        //3 cases background process, pipelines and redirection

        match *command
        {
            "exit" => 
            {
                break;
            }
            "cd" =>
            {   
                // If there is second argument, then use that as directory path, else fail
                let directory = arguments.get(1).expect("cd needs an argument");
                env::set_current_dir(directory).expect("Failed to change directory");
            }
            "ls" =>
            {
                // If there is second argument, then use that as directory path, else use current directory
                let dir_path = if let Some(dir) = arguments.get(1)
                {
                    Path::new(dir).to_path_buf()
                } else
                {
                    env::current_dir().expect("Failed to get current directory")
                };

                // Read the directory and print the entries
                let entries: Vec<String> = dir_path.read_dir()
                    .expect("Failed to read directory")
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect();
                println!("{}", entries.join("  "));
            }
            "cat" =>
            {
                // If there is second argument, then use that as file path, else fail
                let file_path = arguments.get(1).expect("cat needs a file path");

                should_fork(&arguments);

                let mut file = File::open(file_path).expect("Failed to open file");
                let mut contents = String::new();
                file.read_to_string(&mut contents).expect("Failed to read file");
                println!("{}", contents);
            }


            "" =>
            {
                continue;
            }
            _ =>
            {
                println!("Unknown command: {}", command);
            }
        }

    }

}

// Function that checks the last argument to see if the process needs to be forked
fn should_fork(arguments: &[&str]) -> bool {
    let last_arg: &str = arguments.last().unwrap_or(&"");
    last_arg == "&"
}