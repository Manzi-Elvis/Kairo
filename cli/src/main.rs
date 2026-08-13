use std::env;
use std::fs;
use std::process::ExitCode;

use kairo_interpreter::Interpreter;
use kairo_lexer::Lexer;
use kairo_parser::Parser;

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

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read `{path}`: {e}");
            return ExitCode::FAILURE;
        }
    };

    let program = match compile(&source) {
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

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read `{path}`: {e}");
            return ExitCode::FAILURE;
        }
    };

    match compile(&source) {
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

/// Lex + parse a source string into an AST, formatting any failure
/// as a single human-readable error message.
fn compile(source: &str) -> Result<kairo_ast::Program, String> {
    let tokens = Lexer::new(source)
        .tokenize()
        .map_err(|e| format!("lex error: {e:?}"))?;

    let program = Parser::new(tokens)
        .parse_program()
        .map_err(|e| format!("parse error: {e:?}"))?;

    Ok(program)
}