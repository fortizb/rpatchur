#![windows_subsystem = "windows"]

mod patcher;
mod process;
mod ui;

use log::LevelFilter;
use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use simple_logger::SimpleLogger;
use structopt::StructOpt;
use tinyfiledialogs as tfd;
use tokio::runtime;
use wry::application::event::{Event, WindowEvent};
use wry::application::event_loop::ControlFlow;

use patcher::{
    patcher_thread_routine, retrieve_patcher_configuration, PatcherCommand, PatcherConfiguration,
};
use ui::{UiController, WebViewUserData};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const PKG_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const PKG_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

#[derive(Debug, StructOpt)]
#[structopt(name = PKG_NAME, version = PKG_VERSION, author = PKG_AUTHORS, about = PKG_DESCRIPTION)]
struct Opt {
    #[structopt(short, long, parse(from_os_str))]
    working_directory: Option<PathBuf>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Off)
        .with_module_level(PKG_NAME, LevelFilter::Info)
        .init()
        .with_context(|| "Failed to initalize the logger")?;

    let cli_args = Opt::from_args();

    if let Some(working_directory) = cli_args.working_directory {
        env::set_current_dir(working_directory)
            .with_context(|| "Specified working directory is invalid or inaccessible")?;
    };

    let config = match retrieve_patcher_configuration(None) {
        Err(e) => {
            let err_msg = "Failed to retrieve the patcher's configuration";
            tfd::message_box_ok(
                "Error",
                format!("Error: {}: {:#}.", err_msg, e).as_str(),
                tfd::MessageBoxIcon::Error,
            );
            return Err(e);
        }
        Ok(v) => v,
    };

    let (tx, rx) = flume::bounded(32);
    let window_title = config.window.title.clone();
    let (event_loop, webview, user_data, ui_tx, ui_rx) = ui::build_webview(
        window_title.as_str(),
        WebViewUserData::new(config.clone(), tx),
    )
    .with_context(|| "Failed to build a web view")?;

    let _patching_thread = new_patching_thread(rx, UiController::new(ui_tx), config);

    event_loop.run(move |event, _, control_flow| {


        while let Ok(ui_cmd) = ui_rx.try_recv() {
            match ui_cmd {
                ui::UiCommand::EvaluateScript(script) => {
                    let _ = webview.evaluate_script(&script);
                }
            }
        }

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                let _res = user_data.lock().unwrap().patching_thread_tx.send(PatcherCommand::Quit);
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

fn new_patching_thread(
    rx: flume::Receiver<PatcherCommand>,
    ui_ctrl: UiController,
    config: PatcherConfiguration,
) -> std::thread::JoinHandle<Result<()>> {
    std::thread::spawn(move || {
        let tokio_rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .with_context(|| "Failed to build a tokio runtime")?;
        tokio_rt.block_on(patcher_thread_routine(ui_ctrl, config, rx));

        Ok(())
    })
}
