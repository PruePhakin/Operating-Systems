use std::env;
use std::io::{self, Write};
use cmd;


fn main()
{

    loop 
    {
        // Print the current directory
        let current_dir = env::current_dir().expect("Failed to get current directory");
        print!("{}$ ", current_dir.display());
        io::stdout().flush().expect("Failed to flush stdout");

        // Get user command and clean it up
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Failed to read line");

        // Trim whitespaces, then parse words seperated by whitespaces and then store into arguments
        let arguments: Vec<&str> = input.trim().split_whitespace().collect();

        // Get the command
        let command = arguments.first().unwrap_or(&"");
        //3 cases background process, pipelines and redirection
        match *command
        {
            "" => {
                continue;
            }
            "exit" => {
                break;
            }
            "cd" => {   
                cmd::shell_command(arguments);
            }
            _ => {
                cmd::external_command(arguments);
            }
        }

    }
}
