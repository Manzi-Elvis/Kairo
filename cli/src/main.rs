use std::env;
use std::process::ExitCode;
use kairo_hir::lower_program;
use kairo_interpreter::Interpreter;
use kairo_loader::{load_program, LoadError, ModuleSource};
use kairo_typecheck::TypeChecker;
use kairo_codegen::generate;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    let Some(command) = args.get(1) else {
        print_usage();
        return ExitCode::FAILURE;
    };

    match command.as_str() {
        "run" => run_command(&args),
        "check" => check_command(&args),
        "build" => build_command(&args),
        "fmt" => {
            eprintln!("`kairo fmt` is not implemented yet.");
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
    eprintln!("  kairo build <file.kairo>   Compile a Kairo program to a native executable");
}

fn run_command(args: &[String]) -> ExitCode {
    let Some(path) = args.get(2) else {
        eprintln!("Usage: kairo run <file.kairo>");
        return ExitCode::FAILURE;
    };

    let ast_program = match compile(path) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    let program = lower_program(&ast_program);

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

fn build_command(args: &[String]) -> ExitCode {
    let Some(path) = args.get(2) else {
        eprintln!("Usage: kairo build <file.kairo>");
        return ExitCode::FAILURE;
    };

    let ast_program = match compile(path) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    let hir_program = lower_program(&ast_program);

    let ir = match generate(&hir_program) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("codegen error: {e:?}");
            eprintln!(
                "note: native compilation currently only supports the Int/Bool subset \
                 (arithmetic, comparisons, functions, if/while, print) — see docs/language/syntax.md"
            );
            return ExitCode::FAILURE;
        }
    };

    let path_buf = std::path::Path::new(path);
    let ll_path = path_buf.with_extension("ll");
    let exe_path = path_buf.with_extension("exe");

    if let Err(e) = std::fs::write(&ll_path, &ir) {
        eprintln!("error: could not write `{}`: {e}", ll_path.display());
        return ExitCode::FAILURE;
    }

    let status = std::process::Command::new("clang")
        .arg(&ll_path)
        .arg("-o")
        .arg(&exe_path)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("built {}", exe_path.display());
            ExitCode::SUCCESS
        }
        Ok(s) => {
            eprintln!("clang exited with status {s}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: could not run clang: {e}");
            eprintln!("note: install LLVM/clang and ensure it's on PATH");
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