use std::env;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

fn main() {
    println!("Starting container initialization");

    let args: Vec<String> = env::args().collect();
    let claude_args: Vec<String> = if args.is_empty() {
        vec![]
    } else {
        args[1..].to_vec()
    };

    println!("Launching Claude with args: {:?}", claude_args);

    // Run claude directly
    let mut cmd = Command::new("claude");
    cmd.args(&claude_args);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let err = cmd.exec();

    eprintln!("Failed to exec claude: {}", err);
    std::process::exit(1);
}
