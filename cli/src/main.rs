use std::env;
use std::process::ExitCode;

use kairo_interpreter::Interpreter;
use kairo_loader::{load_program, LoadError, ModuleSource};
use kairo_typecheck::TypeChecker;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    let Some(command) = args.get(1) else {
        print_usage();
        return ExitCode::FAILURE;
    };

    match command.as_str() {
        "run" => run_command(&args),
        "check" => check_command(&args),
        "build" | "fmt" => {
            eprintln!("`kairo {command}` is not implemented yet in v0.1.");
            ExitCode::FAILURE
        }
        other => {
            eprintln!("Unknown command: `{other}`\n");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  kairo run <file.kairo>     Run a Kairo program");
    eprintln!("  kairo check <file.kairo>   Check a Kairo program for errors");
}

fn run_command(args: &[String]) -> ExitCode {
    let Some(path) = args.get(2) else {
        eprintln!("Usage: kairo run <file.kairo>");
        return ExitCode::FAILURE;
    };

    let program = match compile(path) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };

    let mut sink = |s: &str| println!("{s}");
    let mut interpreter = Interpreter::new(&mut sink);

    if let Err(e) = interpreter.run(&program) {
        eprintln!("runtime error: {e:?}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn check_command(args: &[String]) -> ExitCode {
    let Some(path) = args.get(2) else {
        eprintln!("Usage: kairo check <file.kairo>");
        return ExitCode::FAILURE;
    };

    match compile(path) {
        Ok(_) => {
            println!("ok: no errors found");
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}

struct FsModuleSource {
    dir: std::path::PathBuf,
}

impl ModuleSource for FsModuleSource {
    fn read_module(&self, name: &str) -> Result<String, LoadError> {
        let path = self.dir.join(format!("{name}.kairo"));
        std::fs::read_to_string(&path)
            .map_err(|e| LoadError::Io(format!("{}: {}", path.display(), e)))
    }
}

fn compile(path: &str) -> Result<kairo_ast::Program, String> {
    let path_buf = std::path::Path::new(path);
    let dir = path_buf
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let entry_name = path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();

    let source = FsModuleSource { dir };
    let program = load_program(&entry_name, &source).map_err(|e| format!("load error: {e:?}"))?;

    TypeChecker::new().check_program(&program).map_err(|errors| {
        errors
            .iter()
            .map(|e| format!("type error: {e:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    Ok(program)
}