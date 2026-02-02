use anyhow::Result;

/// Starts an executable file in a cross-platform way.
///
/// This is the Windows version.
#[cfg(windows)]
pub fn start_executable<I, S>(exe_path: &str, exe_arguments: I) -> Result<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    log::info!("start_executable called with path: {}", exe_path);

    // Check if the file is a batch file
    let is_batch_file = exe_path.to_lowercase().ends_with(".bat") || exe_path.to_lowercase().ends_with(".cmd");
    log::info!("Is batch file: {}", is_batch_file);

    if is_batch_file {
        // Convert batch file path to absolute path
        let current_dir = std::env::current_dir()?;
        let abs_bat_path = current_dir.join(exe_path);
        let abs_bat_path_str = abs_bat_path.to_str().unwrap_or(exe_path);

        log::info!("Batch file absolute path: {}", abs_bat_path_str);

        // For batch files, execute them directly WITHOUT elevation
        let exe_parameter = exe_arguments
            .into_iter()
            .fold(String::new(), |a: String, b| a + " " + b.as_ref());

        log::info!("Executing: {} {} (without UAC)", abs_bat_path_str, exe_parameter);
        windows::win32_spawn_process_open(abs_bat_path_str, &exe_parameter)
    } else {
        // For regular executables, use the original logic
        let exe_parameter = exe_arguments
            .into_iter()
            .fold(String::new(), |a: String, b| a + " " + b.as_ref() + "");
        log::info!("Executing: {} {}", exe_path, exe_parameter);
        windows::win32_spawn_process_runas(exe_path, &exe_parameter)
    }
}

/// Starts an executable file in a cross-platform way.
///
/// This is the non-Windows version.
#[cfg(not(windows))]
pub fn start_executable<I, S>(exe_path: &str, exe_arguments: I) -> Result<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    use std::process::Command;

    let exe_arguments: Vec<String> = exe_arguments
        .into_iter()
        .map(|e| e.as_ref().into())
        .collect();
    Command::new(exe_path)
        .args(exe_arguments)
        .spawn()
        .map(|_| Ok(true))?
}

// Note: Taken from the rustup project
#[cfg(windows)]
mod windows {
    use anyhow::{anyhow, Result};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn to_u16s<S: AsRef<OsStr>>(s: S) -> Result<Vec<u16>> {
        fn inner(s: &OsStr) -> Result<Vec<u16>> {
            let mut maybe_result: Vec<u16> = s.encode_wide().collect();
            if maybe_result.iter().any(|&u| u == 0) {
                return Err(anyhow!("strings passed to WinAPI cannot contain NULs"));
            }
            maybe_result.push(0);
            Ok(maybe_result)
        }
        inner(s.as_ref())
    }

    /// This function starts processes without elevation (normal execution).
    /// Used for batch files and scripts that don't need admin rights.
    pub fn win32_spawn_process_open<S>(path: S, parameter: S) -> Result<bool>
    where
        S: AsRef<OsStr>,
    {
        use std::ptr;
        use winapi::ctypes::c_int;
        use winapi::shared::minwindef::{BOOL, ULONG};
        use winapi::um::shellapi::SHELLEXECUTEINFOW;
        extern "system" {
            pub fn ShellExecuteExW(pExecInfo: *mut SHELLEXECUTEINFOW) -> BOOL;
        }
        const SEE_MASK_CLASSNAME: ULONG = 1;
        const SW_SHOW: c_int = 5;

        // For cmd.exe, use it directly from PATH
        let path_str = path.as_ref().to_str().unwrap_or("");
        let exe_path = if path_str.to_lowercase() == "cmd.exe" {
            to_u16s(path_str)?
        } else if path_str.contains("\\") || path_str.contains("/") {
            // For paths with directory separators, make them absolute
            let abs_path = std::env::current_dir()?.join(path.as_ref());
            to_u16s(abs_path.to_str().unwrap_or(""))?
        } else {
            // For relative paths without separators, make them absolute
            let abs_path = std::env::current_dir()?.join(path.as_ref());
            to_u16s(abs_path.to_str().unwrap_or(""))?
        };

        let parameter = to_u16s(parameter)?;
        let operation = to_u16s("open")?;  // Use "open" instead of "runas" - NO UAC prompt
        let class = to_u16s("exefile")?;
        let mut execute_info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_CLASSNAME,
            hwnd: ptr::null_mut(),
            lpVerb: operation.as_ptr(),
            lpFile: exe_path.as_ptr(),
            lpParameters: parameter.as_ptr(),
            lpDirectory: ptr::null_mut(),
            nShow: SW_SHOW,
            hInstApp: ptr::null_mut(),
            lpIDList: ptr::null_mut(),
            lpClass: class.as_ptr(),
            hkeyClass: ptr::null_mut(),
            dwHotKey: 0,
            hMonitor: ptr::null_mut(),
            hProcess: ptr::null_mut(),
        };

        let result = unsafe { ShellExecuteExW(&mut execute_info) };
        Ok(result != 0)
    }

    /// This function is required to start processes that require elevation, from
    /// a non-elevated process.
    pub fn win32_spawn_process_runas<S>(path: S, parameter: S) -> Result<bool>
    where
        S: AsRef<OsStr>,
    {
        use std::ptr;
        use winapi::ctypes::c_int;
        use winapi::shared::minwindef::{BOOL, ULONG};
        use winapi::um::shellapi::SHELLEXECUTEINFOW;
        extern "system" {
            pub fn ShellExecuteExW(pExecInfo: *mut SHELLEXECUTEINFOW) -> BOOL;
        }
        const SEE_MASK_CLASSNAME: ULONG = 1;
        const SW_SHOW: c_int = 5;

        // Check if the path is a system command (like cmd.exe)
        let path_str = path.as_ref().to_str().unwrap_or("");
        let exe_path = if path_str.to_lowercase() == "cmd.exe" || path_str.contains("\\") || path_str.contains("/") {
            // For system commands or paths with directory separators, use as-is or make absolute
            if path_str.to_lowercase() == "cmd.exe" {
                to_u16s(path_str)?
            } else {
                let abs_path = std::env::current_dir()?.join(path.as_ref());
                to_u16s(abs_path.to_str().unwrap_or(""))?
            }
        } else {
            // For relative paths without separators, make them absolute
            let abs_path = std::env::current_dir()?.join(path.as_ref());
            to_u16s(abs_path.to_str().unwrap_or(""))?
        };

        let parameter = to_u16s(parameter)?;
        let operation = to_u16s("runas")?;
        let class = to_u16s("exefile")?;
        let mut execute_info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_CLASSNAME,
            hwnd: ptr::null_mut(),
            lpVerb: operation.as_ptr(),
            lpFile: exe_path.as_ptr(),
            lpParameters: parameter.as_ptr(),
            lpDirectory: ptr::null_mut(),
            nShow: SW_SHOW,
            hInstApp: ptr::null_mut(),
            lpIDList: ptr::null_mut(),
            lpClass: class.as_ptr(),
            hkeyClass: ptr::null_mut(),
            dwHotKey: 0,
            hMonitor: ptr::null_mut(),
            hProcess: ptr::null_mut(),
        };

        let result = unsafe { ShellExecuteExW(&mut execute_info) };
        Ok(result != 0)
    }
}
