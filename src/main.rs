//! CLI 入口：只负责把命令行参数交给库里的 run()，逻辑与 GUI 共用同一套引擎。

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match obsidian_sync::run(&args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("\n错误：{msg}");
            ExitCode::from(2)
        }
    }
}
