//! Application resource path resolution helpers.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::path::Path;

static APP_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Returns the application root directory as a string slice.
///
/// In debug builds, this is the server workspace directory. In release builds,
/// this is the directory containing the current executable.
///
/// # Returns
///
/// A cached, process-wide application root path.
///
/// # Panics
///
/// Panics if the cached application root cannot be represented as valid UTF-8.
///
/// # Examples
///
/// ```
/// let root = arachnea_core::applicatio::get_application_root();
/// println!("Application root: {}", root);
/// ```
pub fn get_application_root() -> &'static str {
    APP_ROOT.get_or_init(init_app_root).to_str().unwrap()
}

/// Builds an absolute path from the application root and a relative path.
///
/// # Arguments
///
/// * `path` - Path segment to append to the application root.
///
/// # Returns
///
/// The joined path as an owned string, using lossy UTF-8 conversion if needed.
///
/// # Examples
///
/// ```
/// let data_path = arachnea_core::application::get_application_path("data/config.json");
/// println!("Config path: {}", data_path);
/// ```
pub fn get_application_path(path: &str) -> String {
    APP_ROOT
        .get_or_init(init_app_root)
        .join(path)
        .to_string_lossy()
        .into_owned()
}

/// Resolves the application root path for the current build profile.
///
/// # Returns
///
/// The server workspace directory in debug builds, or the executable directory in
/// release builds.
///
/// # Panics
///
/// Panics in release builds if the current executable path cannot be read.
/// Panics in debug builds if the server application root cannot be resolved.
fn init_app_root() -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("Failed to resolve the server application root.")
            .to_path_buf()
    } else {
        let mut path = std::env::current_exe().expect("Failed to get current executable path");

        path.pop();
        path
    }
}


/// Returns the executable name used in help and error messages.
///
/// Derives the name from the first command-line argument (`args_os(0)`),
/// extracting the file name component when available.
///
/// # Returns
///
/// The executable name as an owned `String`.
pub fn program_name() -> String {
    std::env::args_os()
        .next()
        .and_then(|arg0| {
            let path = Path::new(&arg0);

            path.file_name()
                .map(|file_name| file_name.to_string_lossy().into_owned())
                .or_else(|| {
                    let raw = arg0.to_string_lossy().into_owned();
                    (!raw.is_empty()).then_some(raw)
                })
        })
        .unwrap_or_else(|| String::from("?"))
}

/// Performs application-level initialization before the main entry point logic.
///
/// On Windows, when the process is launched from a console (e.g. `cmd.exe` or
/// PowerShell), this attaches the process to the parent console and redirects
/// the standard input, output, and error handles to it. When the process is
/// launched without a parent console (double-click, shortcut, service), this
/// is a no-op and no console window is created.
///
/// On other platforms this function does nothing.
pub fn application_init() {
    #[cfg(target_os = "windows")]
    attach_parent_console_if_any();
}

/// Attaches the process to the parent console on Windows and redirects the
/// standard handles when a parent console exists.
///
/// This is a no-op when the process was not launched from a console.
#[cfg(target_os = "windows")]
fn attach_parent_console_if_any() {
    use windows_sys::Win32::Foundation::{
        GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        AttachConsole, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE,
    };
    use windows_sys::core::w;

    // Attach to the parent console if one exists. Fails with
    // ERROR_INVALID_HANDLE when there is no parent console (e.g. double-click).
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        // No parent console: keep the GUI behavior (no console window).
        return;
    }

    // Redirect the standard output and error handles to the attached console.
    let output: HANDLE = unsafe {
        CreateFileW(
            w!("CONOUT$"),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if output != INVALID_HANDLE_VALUE {
        unsafe { SetStdHandle(STD_OUTPUT_HANDLE, output) };
        unsafe { SetStdHandle(STD_ERROR_HANDLE, output) };
    }

    // Redirect the standard input handle to the attached console.
    let input: HANDLE = unsafe {
        CreateFileW(
            w!("CONIN$"),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if input != INVALID_HANDLE_VALUE {
        unsafe { SetStdHandle(STD_INPUT_HANDLE, input) };
    }
}
