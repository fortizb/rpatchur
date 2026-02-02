use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::patcher::{get_patcher_name, PatcherCommand, PatcherConfiguration};
use crate::process::start_executable;
use serde::Deserialize;
use serde_json::Value;
use tinyfiledialogs as tfd;
use wry::application::event_loop::EventLoop;
use wry::application::window::WindowBuilder;
use wry::webview::WebViewBuilder;
use wry::application::window::Icon;
use image::io::Reader as ImageReader;
use std::io::Cursor;

pub enum UiCommand {
    EvaluateScript(String),
}

pub struct UiController {
    ui_tx: flume::Sender<UiCommand>,
}

impl UiController {
    pub fn new(ui_tx: flume::Sender<UiCommand>) -> UiController {
        UiController { ui_tx }
    }

    pub fn dispatch_patching_status(&self, status: PatchingStatus) {
        let script = match status {
            PatchingStatus::Ready => "patchingStatusReady()".to_string(),
            PatchingStatus::Error(msg) => format!("patchingStatusError(\"{}\")", msg),
            PatchingStatus::DownloadInProgress(nb_downloaded, nb_total, bytes_per_sec) => {
                format!(
                    "patchingStatusDownloading({}, {}, {})",
                    nb_downloaded, nb_total, bytes_per_sec
                )
            }
            PatchingStatus::InstallationInProgress(nb_installed, nb_total) => {
                format!("patchingStatusInstalling({}, {})", nb_installed, nb_total)
            }
            PatchingStatus::ManualPatchApplied(name) => {
                format!("patchingStatusPatchApplied(\"{}\")", name)
            }
        };

        let _ = self.ui_tx.send(UiCommand::EvaluateScript(script));
    }

    pub fn set_patch_in_progress(&self, _value: bool) {
    }
}

pub enum PatchingStatus {
    Ready,
    Error(String),
    DownloadInProgress(usize, usize, u64),
    InstallationInProgress(usize, usize),
    ManualPatchApplied(String),
}

pub struct WebViewUserData {
    pub patcher_config: PatcherConfiguration,
    pub patching_thread_tx: flume::Sender<PatcherCommand>,
    pub patching_in_progress: bool,
}

impl WebViewUserData {
    pub fn new(
        patcher_config: PatcherConfiguration,
        patching_thread_tx: flume::Sender<PatcherCommand>,
    ) -> WebViewUserData {
        WebViewUserData {
            patcher_config,
            patching_thread_tx,
            patching_in_progress: false,
        }
    }
}

impl Drop for WebViewUserData {
    fn drop(&mut self) {
        let _res = self.patching_thread_tx.try_send(PatcherCommand::Quit);
    }
}

pub fn build_webview(
    title: &str,
    user_data: WebViewUserData,
) -> anyhow::Result<(EventLoop<()>, Arc<wry::webview::WebView>, Arc<Mutex<WebViewUserData>>, flume::Sender<UiCommand>, flume::Receiver<UiCommand>)> {
    let event_loop = EventLoop::new();
    
    let mut window_builder = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(wry::application::dpi::LogicalSize::new(
            user_data.patcher_config.window.width as f64,
            user_data.patcher_config.window.height as f64,
        ))
        .with_resizable(user_data.patcher_config.window.resizable);
    
    if let Some(icon) = load_window_icon() {
        window_builder = window_builder.with_window_icon(Some(icon));
    }
    
    let window = window_builder.build(&event_loop)?;

    let url = user_data.patcher_config.web.index_url.clone();
    let user_data = Arc::new(Mutex::new(user_data));
    let user_data_clone = Arc::clone(&user_data);

    let (ui_tx, ui_rx) = flume::unbounded();

    let webview = WebViewBuilder::new(window)?
        .with_url(&url)?
        .with_ipc_handler(move |_window, message| {
            handle_message(&message, &user_data_clone);
        })
        .build()?;

    Ok((event_loop, Arc::new(webview), user_data, ui_tx, ui_rx))
}

fn handle_message(message: &str, user_data: &Arc<Mutex<WebViewUserData>>) {
    println!("========================================");
    println!("IPC MESSAGE RECEIVED: '{}'", message);
    println!("========================================");
    
    match message {
        "play" => {
            println!("Handling: play");
            handle_play(user_data)
        },
        "setup" => {
            println!("Handling: setup");
            handle_setup(user_data)
        },
        "repair" => {
            println!("Handling: repair");
            handle_repair(user_data)
        },
        "exit" => {
            println!("Handling: exit");
            std::process::exit(0)
        },
        "start_update" => {
            println!("Handling: start_update");
            handle_start_update(user_data)
        },
        "cancel_update" => {
            println!("Handling: cancel_update");
            handle_cancel_update(user_data)
        },
        "reset_cache" => {
            println!("Handling: reset_cache");
            handle_reset_cache()
        },
        "manual_patch" => {
            println!("Handling: manual_patch");
            handle_manual_patch(user_data)
        },
        request => {
            println!("Handling: JSON request");
            handle_json_request(user_data, request)
        },
    }
}

fn handle_play(user_data: &Arc<Mutex<WebViewUserData>>) {
    let client_arguments = user_data
        .lock()
        .unwrap()
        .patcher_config
        .play
        .arguments
        .clone();
    start_game_client(user_data, &client_arguments);
}

fn handle_setup(user_data: &Arc<Mutex<WebViewUserData>>) {
    let (setup_exe, setup_arguments, exit_on_success) = {
        let data = user_data.lock().unwrap();
        (
            data.patcher_config.setup.path.clone(),
            data.patcher_config.setup.arguments.clone(),
            data.patcher_config.setup.exit_on_success.unwrap_or(false),
        )
    };

    match start_executable(&setup_exe, &setup_arguments) {
        Ok(success) => {
            if success {
                log::trace!("Setup software started");
                if exit_on_success {
                    std::process::exit(0);
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to start setup software: {}", e);
        }
    }
}


fn handle_repair(user_data: &Arc<Mutex<WebViewUserData>>) {
    println!("========================================");
    println!("=== REPAIR BUTTON CLICKED ===");
    println!("========================================");
    log::info!("=== REPAIR BUTTON CLICKED ===");

    let repair_config = {
        let data = user_data.lock().unwrap();
        println!("Locked user_data successfully");
        log::info!("Locked user_data successfully");
        data.patcher_config.repair.clone()
    };

    if let Some(repair) = repair_config {
        let repair_exe = repair.path.clone();
        let repair_arguments = repair.arguments.clone();
        let exit_on_success = repair.exit_on_success.unwrap_or(false);

        println!("Repair configuration found:");
        println!("  - Path: {}", repair_exe);
        println!("  - Arguments: {:?}", repair_arguments);
        println!("  - Exit on success: {}", exit_on_success);
        
        log::info!("Repair configuration found:");
        log::info!("  - Path: {}", repair_exe);
        log::info!("  - Arguments: {:?}", repair_arguments);
        log::info!("  - Exit on success: {}", exit_on_success);

        let current_dir = std::env::current_dir().unwrap_or_default();
        println!("  - Current directory: {:?}", current_dir);
        log::info!("  - Current directory: {:?}", current_dir);

        println!("Attempting to start repair tool...");
        log::info!("Attempting to start repair tool...");
        match start_executable(&repair_exe, &repair_arguments) {
            Ok(success) => {
                println!("start_executable returned: {}", success);
                log::info!("start_executable returned: {}", success);
                if success {
                    println!("Repair tool started successfully");
                    log::info!("Repair tool started successfully");
                    if exit_on_success {
                        println!("Exiting application as configured");
                        log::info!("Exiting application as configured");
                        std::process::exit(0);
                    }
                } else {
                    println!("WARNING: start_executable returned false");
                    log::warn!("start_executable returned false - repair tool may not have started");
                }
            }
            Err(e) => {
                println!("ERROR: Failed to start repair tool: {}", e);
                println!("Error details: {:?}", e);
                log::error!("Failed to start repair tool: {}", e);
                log::error!("Error details: {:?}", e);
            }
        }
    } else {
        println!("ERROR: Repair configuration not found in rpatchur.yml");
        log::error!("Repair configuration not found in rpatchur.yml");
        log::error!("Please add a 'repair:' section to your configuration file");
    }

    println!("=== REPAIR HANDLER FINISHED ===");
    println!("========================================");
    log::info!("=== REPAIR HANDLER FINISHED ===");
}

fn handle_start_update(user_data: &Arc<Mutex<WebViewUserData>>) {
    let data = user_data.lock().unwrap();
    if data.patching_in_progress {
        log::warn!("Patching already in progress");
        return;
    }

    let send_res = data.patching_thread_tx.send(PatcherCommand::StartUpdate);
    if send_res.is_ok() {
        log::trace!("Sent StartUpdate command to patching thread");
    }
}

fn handle_cancel_update(user_data: &Arc<Mutex<WebViewUserData>>) {
    let data = user_data.lock().unwrap();
    if data
        .patching_thread_tx
        .send(PatcherCommand::CancelUpdate)
        .is_ok()
    {
        log::trace!("Sent CancelUpdate command to patching thread");
    }
}

fn handle_reset_cache() {
    if let Ok(patcher_name) = get_patcher_name() {
        let cache_file_path = PathBuf::from(patcher_name).with_extension("dat");
        if let Err(e) = fs::remove_file(cache_file_path) {
            log::warn!("Failed to remove the cache file: {}", e);
        }
    }
}

fn handle_manual_patch(user_data: &Arc<Mutex<WebViewUserData>>) {
    let data = user_data.lock().unwrap();
    if data.patching_in_progress {
        log::warn!("Patching already in progress");
        return;
    }

    let opt_path = tfd::open_file_dialog(
        "Select a file",
        "",
        Some((&["*.thor"], "Patch Files (*.thor)")),
    );
    if let Some(path) = opt_path {
        log::info!("Requesting manual patch '{}'", path);
        if data
            .patching_thread_tx
            .send(PatcherCommand::ApplyPatch(PathBuf::from(path)))
            .is_ok()
        {
            log::trace!("Sent ApplyPatch command to patching thread");
        }
    }
}

fn handle_json_request(user_data: &Arc<Mutex<WebViewUserData>>, request: &str) {
    let result: serde_json::Result<Value> = serde_json::from_str(request);
    match result {
        Err(e) => {
            log::error!("Invalid JSON request: {}", e);
        }
        Ok(json_req) => {
            let function_name = json_req["function"].as_str();
            if let Some(function_name) = function_name {
                let function_params = json_req["parameters"].clone();
                match function_name {
                    "login" => handle_login(user_data, function_params),
                    "open_url" => handle_open_url(function_params),
                    _ => {
                        log::error!("Unknown function '{}'", function_name);
                    }
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct LoginParameters {
    login: String,
    password: String,
}

fn handle_login(user_data: &Arc<Mutex<WebViewUserData>>, parameters: Value) {
    let result: serde_json::Result<LoginParameters> = serde_json::from_value(parameters);
    match result {
        Err(e) => log::error!("Invalid arguments given for 'login': {}", e),
        Ok(login_params) => {
            let mut play_arguments: Vec<String> = vec![
                format!("-t:{}", login_params.password),
                login_params.login,
                "server".to_string(),
            ];
            let data = user_data.lock().unwrap();
            play_arguments.extend(data.patcher_config.play.arguments.iter().cloned());
            drop(data);
            start_game_client(user_data, &play_arguments);
        }
    }
}

#[derive(Deserialize)]
struct OpenUrlParameters {
    url: String,
}

fn handle_open_url(parameters: Value) {
    let result: serde_json::Result<OpenUrlParameters> = serde_json::from_value(parameters);
    match result {
        Err(e) => log::error!("Invalid arguments given for 'open_url': {}", e),
        Ok(params) => match open::that(params.url) {
            Ok(exit_status) => {
                if !exit_status.success() {
                    if let Some(code) = exit_status.code() {
                        log::error!("Command returned non-zero exit status {}!", code);
                    }
                }
            }
            Err(why) => {
                log::error!("Error open_url function: '{}'", why);
            }
        },
    }
}

fn start_game_client(user_data: &Arc<Mutex<WebViewUserData>>, client_arguments: &[String]) {
    let (client_exe, exit_on_success) = {
        let data = user_data.lock().unwrap();
        (
            data.patcher_config.play.path.clone(),
            data.patcher_config.play.exit_on_success.unwrap_or(true),
        )
    };

    match start_executable(&client_exe, client_arguments) {
        Ok(success) => {
            if success {
                log::trace!("Client started");
                if exit_on_success {
                    std::process::exit(0);
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to start client: {}", e);
        }
    }
}


fn load_window_icon() -> Option<Icon> {
    const ICON_BYTES: &[u8] = include_bytes!("../resources/rpatchur.ico");
    
    let img = ImageReader::new(Cursor::new(ICON_BYTES))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let rgba_data = rgba.into_raw();
    
    Icon::from_rgba(rgba_data, width, height).ok()
}
