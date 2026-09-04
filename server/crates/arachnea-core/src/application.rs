//! Application path resolution helpers.
//!
//! Three process-wide roots are resolved once and cached:
//! - the **application root** (legacy, executable-relative): superseded by the
//!   resource and data roots below,
//! - the **resource root** (read-only `services/` data): probed at first call
//!   across the layouts the release artifacts produce (portable folder, macOS
//!   bundle `Contents/Resources`, Linux system resource dir),
//! - the **data root** (writable `data/`, credentials): executable directory
//!   for portable layouts, per-OS standard directory for packaged installs.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::{env, fs};

use anyhow::{bail, Context};

use crate::controler::options::SettingSource;
use crate::persistence::{JsonPersistenceFileCodec, PersistenceFileCodec};

static APP_ROOT: OnceLock<PathBuf> = OnceLock::new();
static APP_RESOURCE_ROOT: OnceLock<PathBuf> = OnceLock::new();
static APP_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();
static APPLICATION_DATA_DIR_NAME: OnceLock<String> = OnceLock::new();

/// Configures the application identifier used as the per-user data directory
/// name on packaged installs.
///
/// The application crate must call this before any data path is requested. It
/// should supply its own application identifier, such as the Tauri
/// `identifier` value.
///
/// # Errors
///
/// Returns an error when the identifier is empty, contains a path separator,
/// has already been configured, or the data root was already resolved.
pub fn configure_application_data_dir_name(identifier: impl Into<String>) -> Result<(), String> {
    if APP_DATA_ROOT.get().is_some() {
        return Err(String::from(
            "application data root was already resolved; configure its identifier before requesting a data path",
        ));
    }

    let identifier = identifier.into();
    if identifier.trim().is_empty()
        || identifier == "."
        || identifier == ".."
        || identifier.contains(['/', '\\'])
    {
        return Err(String::from(
            "application data directory identifier must be a non-empty directory name",
        ));
    }

    APPLICATION_DATA_DIR_NAME
        .set(identifier)
        .map_err(|_| String::from("application data directory identifier was already configured"))
}

/// Returns the configured application data directory name.
///
/// # Panics
///
/// Panics when the application crate did not configure its identifier before a
/// data path was requested.
fn application_data_dir_name() -> &'static str {
    APPLICATION_DATA_DIR_NAME
        .get()
        .map(String::as_str)
        .expect("application data directory identifier is not configured")
}

/// Returns the application root directory as a string slice.
///
/// In debug builds, this is the server workspace directory. In release builds,
/// this is the directory containing the current executable.
///
/// Superseded by [`get_application_resource_root`] (read-only resources) and
/// [`get_application_data_root`] (writable data); kept for compatibility.
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
/// let root = arachnea_core::application::get_application_root();
/// println!("Application root: {}", root);
/// ```
pub fn get_application_root() -> &'static str {
    APP_ROOT.get_or_init(init_app_root).to_str().unwrap()
}

/// Builds an absolute path from the application root and a relative path.
///
/// Superseded by [`get_application_resource_path`] (read-only resources) and
/// [`get_application_data_path`] (writable data); kept for compatibility.
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
        debug_workspace_root()
    } else {
        release_executable_dir()
    }
}

/// Returns the application resource root directory as a string slice.
///
/// Read-only resources (`services/` scraper manifests and YAML sources) live
/// next to the executable in portable layouts, but inside the bundle on
/// packaged installs. The root is probed once, at first call, and the first
/// candidate containing a `services` directory wins:
/// - macOS bundles: `Contents/Resources/` (Tauri bundler layout),
/// - Linux system packages (deb/rpm/AppImage): `../lib/<product name>/`,
/// - fallback: the executable directory.
///
/// # Returns
///
/// A cached, process-wide application resource root path.
///
/// # Panics
///
/// Panics if the cached root cannot be represented as valid UTF-8.
pub fn get_application_resource_root() -> &'static str {
    APP_RESOURCE_ROOT
        .get_or_init(init_app_resource_root)
        .to_str()
        .unwrap()
}

/// Builds an absolute path from the application resource root and a relative
/// path (see [`get_application_resource_root`]).
///
/// # Arguments
///
/// * `path` - Path segment to append to the resource root.
///
/// # Returns
///
/// The joined path as an owned string, using lossy UTF-8 conversion if needed.
///
/// # Examples
///
/// ```
/// let manifest = arachnea_core::application::get_application_resource_path("services/a/services.json");
/// println!("Manifest path: {}", manifest);
/// ```
pub fn get_application_resource_path(path: &str) -> String {
    APP_RESOURCE_ROOT
        .get_or_init(init_app_resource_root)
        .join(path)
        .to_string_lossy()
        .into_owned()
}

/// Resolves the application resource root (see [`get_application_resource_root`]).
///
/// # Panics
///
/// Panics in release builds if the current executable path cannot be read.
/// Panics in debug builds if the server application root cannot be resolved.
fn init_app_resource_root() -> PathBuf {
    if cfg!(debug_assertions) {
        return debug_workspace_root();
    }

    let exe_dir = release_executable_dir();

    // Portable layout: services next to the executable (portable archives,
    // NSIS installs, and pre-existing archives staged the legacy way).
    if exe_dir.join("services").is_dir() {
        return exe_dir;
    }

    // macOS bundles: the Tauri bundler installs `bundle.resources` under
    // `Contents/Resources/` (exe dir is `Contents/MacOS`).
    #[cfg(target_os = "macos")]
    if let Some(contents_dir) = exe_dir.parent() {
        let resources_dir = contents_dir.join("Resources");
        if resources_dir.join("services").is_dir() {
            return resources_dir;
        }
    }

    // Linux system packages (deb/rpm/AppImage): the Tauri bundler installs
    // `bundle.resources` under `/usr/lib/<product name>/` while the binary
    // sits in `/usr/bin`. The product name is not embedded in the binary, so
    // the first `lib/` entry shipping a `services` directory wins.
    #[cfg(target_os = "linux")]
    if let Some(prefix_dir) = exe_dir.parent() {
        if let Some(lib_dir) = prefix_dir.parent().map(|prefix| prefix.join("lib")) {
            if let Ok(entries) = fs::read_dir(lib_dir) {
                let mut candidates: Vec<PathBuf> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|dir| dir.join("services").is_dir())
                    .collect();
                candidates.sort();
                if let Some(candidate) = candidates.first() {
                    return candidate.clone();
                }
            }
        }
    }

    exe_dir
}

/// Returns the application data root directory as a string slice.
///
/// Writable data (`data/`, credentials) belongs next to the executable in
/// portable layouts, and in the per-user standard directory of the OS on
/// packaged installs (`~/Library/Application Support/<id>` on macOS,
/// `$XDG_DATA_HOME/<id>` on Linux, `%APPDATA%\<id>` on Windows). The root is
/// resolved once, at first call: the executable directory is used when the
/// install is not packaged and the directory is writable, otherwise the
/// platform directory.
///
/// # Returns
///
/// A cached, process-wide application data root path.
///
/// # Panics
///
/// Panics if the cached root cannot be represented as valid UTF-8.
pub fn get_application_data_root() -> &'static str {
    APP_DATA_ROOT
        .get_or_init(init_app_data_root)
        .to_str()
        .unwrap()
}

/// Builds an absolute path from the application data root and a relative path
/// (see [`get_application_data_root`]).
///
/// # Arguments
///
/// * `path` - Path segment to append to the data root.
///
/// # Returns
///
/// The joined path as an owned string, using lossy UTF-8 conversion if needed.
///
/// # Examples
///
/// ```
/// let data_path = arachnea_core::application::get_application_data_path("data/cache");
/// println!("Data path: {}", data_path);
/// ```
pub fn get_application_data_path(path: &str) -> String {
    APP_DATA_ROOT
        .get_or_init(init_app_data_root)
        .join(path)
        .to_string_lossy()
        .into_owned()
}

/// Resolves the application data root (see [`get_application_data_root`]).
///
/// # Panics
///
/// Panics in release builds if the current executable path cannot be read.
/// Panics in debug builds if the server application root cannot be resolved.
fn init_app_data_root() -> PathBuf {
    let application_data_dir_name = application_data_dir_name();
    let data_root = if cfg!(debug_assertions) {
        debug_workspace_root()
    } else {
        let exe_dir = release_executable_dir();
        if !is_packaged_install(&exe_dir) && is_writable_dir(&exe_dir) {
            exe_dir.clone()
        } else {
            let data_root =
                platform_data_root(application_data_dir_name).unwrap_or_else(|| exe_dir.clone());
            // Ensure the per-user directory exists so first launch can write into it.
            let _ = fs::create_dir_all(&data_root);
            data_root
        }
    };

    tracing::info!(path = %data_root.display(), "application data directory initialized");
    data_root
}

/// Returns `true` when the executable runs from a system-managed install
/// whose directory must not receive writable data.
#[allow(unused_variables)]
fn is_packaged_install(exe_dir: &Path) -> bool {
    #[cfg(target_os = "macos")]
    if exe_dir.file_name().is_some_and(|name| name == "MacOS") {
        if let Some(bundle_dir) = exe_dir.parent().and_then(Path::parent) {
            if bundle_dir
                .extension()
                .is_some_and(|extension| extension == "app")
            {
                return true;
            }
        }
    }

    #[cfg(target_os = "linux")]
    if let Some(dir) = exe_dir.to_str() {
        // /usr/bin (deb/rpm), /opt & /snap installs, and AppImage mount points.
        const PACKAGED_PREFIXES: [&str; 4] = ["/usr/", "/opt/", "/snap/", "/tmp/.mount_"];
        if PACKAGED_PREFIXES
            .iter()
            .any(|prefix| dir.starts_with(prefix))
        {
            return true;
        }
    }

    false
}

/// Returns `true` when a file can be created inside the directory (cheap
/// create-and-remove probe, more reliable than permission bits).
fn is_writable_dir(dir: &Path) -> bool {
    let probe = dir.join(".arachnea-write-probe");
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Returns the per-OS standard application data directory, or `None` when it
/// cannot be determined (missing `HOME`/`APPDATA`).
fn platform_data_root(application_data_dir_name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(application_data_dir_name),
        )
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(xdg_data_home) = env::var_os("XDG_DATA_HOME") {
            if !xdg_data_home.is_empty() {
                return Some(PathBuf::from(xdg_data_home).join(application_data_dir_name));
            }
        }
        let home = env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(application_data_dir_name),
        )
    }

    #[cfg(target_os = "windows")]
    {
        let app_data = env::var_os("APPDATA")?;
        Some(PathBuf::from(app_data).join(application_data_dir_name))
    }
}

/// Returns the directory containing the current executable (release builds).
///
/// # Panics
///
/// Panics if the current executable path cannot be read.
fn release_executable_dir() -> PathBuf {
    let mut path = env::current_exe().expect("Failed to get current executable path");
    path.pop();
    path
}

/// Returns the server workspace directory (debug builds).
///
/// # Panics
///
/// Panics if the server application root cannot be resolved.
fn debug_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("Failed to resolve the server application root.")
        .to_path_buf()
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

/// Returns whether the process runs with elevated (root/administrator) privileges.
///
/// On Unix this checks for the superuser UID (`0`). On Windows this checks the
/// process token elevation, which is the closest equivalent to running as root.
///
/// # Returns
///
/// `true` when the process can be considered privileged on its platform.
pub fn is_running_elevated() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::getuid() == 0 }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut returned_size: u32 = 0;
            let succeeded = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned_size,
            );
            windows_sys::Win32::Foundation::CloseHandle(token);
            succeeded != 0 && elevation.TokenIsElevated != 0
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        false
    }
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
    use windows_sys::core::w;
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

/// Declares a single CLI option exposed by an application.
///
/// Used by [`ApplicationOptionsProvider::get_options`] to describe the
/// supported options so callers can build help messages or documentation.
pub struct ApplicationOptionDefinition {
    /// Option name without the leading dashes (e.g. `server-port`).
    pub name: String,
    /// Human-readable description of what the option does.
    pub description: String,
}

impl ApplicationOptionDefinition {
    /// Creates a new option definition.
    ///
    /// # Arguments
    ///
    /// * `name` - Option name without the leading dashes.
    /// * `description` - Human-readable description of the option.
    ///
    /// # Returns
    ///
    /// The newly created option definition.
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
        }
    }
}

/// Contract implemented by application option holders parsed from command-line
/// arguments and persisted configuration files.
///
/// Implementors declare the options they support through
/// [`ApplicationOptionsProvider::get_options`], define how raw arguments are
/// applied to an existing instance through [`ApplicationOptionsProvider::parse_vect`],
/// and export persisted values through [`ApplicationOptionsProvider::export`].
pub trait ApplicationOptionsProvider {
    /// Lists the CLI options supported by this application.
    ///
    /// # Returns
    ///
    /// One [`ApplicationOptionDefinition`] per supported option.
    fn get_options(&self) -> Vec<ApplicationOptionDefinition>;

    /// Exports persisted options using names without their leading `--` prefix.
    ///
    /// # Arguments
    ///
    /// * `output` - Map receiving option names and their optional values.
    fn export(&self, output: &mut BTreeMap<String, Option<String>>);

    /// Applies raw option strings to this instance.
    ///
    /// # Arguments
    ///
    /// * `args` - CLI-style arguments, with each option prefixed by `--`.
    /// * `source` - Provenance assigned to values supplied by `args`.
    ///
    /// # Errors
    ///
    /// Returns an error when an option is missing its value or when a
    /// provided value is invalid.
    fn parse_vect(&mut self, args: Vec<String>, source: SettingSource) -> anyhow::Result<()>;

    /// Applies the process command-line arguments to this instance.
    ///
    /// Collects `std::env::args` without the program name and delegates to
    /// [`ApplicationOptionsProvider::parse_vect`].
    ///
    /// # Returns
    ///
    /// This instance with command-line values applied.
    ///
    /// # Errors
    ///
    /// Returns the errors reported by
    /// [`ApplicationOptionsProvider::parse_vect`].
    fn load_args(mut self) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        self.parse_vect(
            std::env::args().skip(1).collect(),
            SettingSource::CommandLine,
        )?;
        Ok(self)
    }

    /// Applies persisted options from a JSON configuration file to this instance.
    ///
    /// The document must be a JSON object whose keys are option names without
    /// their `--` prefix. Values are strings, numbers, booleans, or `null` for
    /// flags without a value. A missing file leaves this instance unchanged.
    ///
    /// # Arguments
    ///
    /// * `path` - Configuration file path.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, is not a supported JSON
    /// option map, or contains invalid option values.
    fn load_file(mut self, path: impl AsRef<Path>) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let path = path.as_ref();
        let payload = match fs::read(path) {
            Ok(payload) => payload,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(self),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to read application configuration {}.",
                        path.display()
                    )
                })
            }
        };
        let entries: BTreeMap<String, Option<serde_json::Value>> = JsonPersistenceFileCodec
            .deserialize(&payload)
            .with_context(|| {
                format!(
                    "Failed to parse application configuration {}.",
                    path.display()
                )
            })?;
        let mut args = Vec::new();
        for (name, value) in entries {
            if name.is_empty() || name.starts_with("--") {
                bail!("Invalid application configuration option name `{name}`.");
            }
            let name = match name.as_str() {
                "server_port" => "server-port",
                "network_mode" => "network",
                "entrypoint_root" => "entrypoint-root",
                name => name,
            };
            args.push(format!("--{name}"));
            if let Some(value) = value {
                let value = match value {
                    serde_json::Value::String(value) => value,
                    serde_json::Value::Number(value) => value.to_string(),
                    serde_json::Value::Bool(value) => value.to_string(),
                    serde_json::Value::Null => continue,
                    _ => bail!(
                        "Application configuration option `{name}` must have a scalar value or null."
                    ),
                };
                args.push(value);
            }
        }
        self.parse_vect(args, SettingSource::Configuration)?;
        Ok(self)
    }

    /// Checks if the given option is supported by this application.
    ///
    /// # Arguments
    ///
    /// * `option` - Option name without the leading dashes.
    ///
    /// # Returns
    ///
    /// `true` if the option is supported, `false` otherwise.
    fn contains_option(&self, option: String) -> bool {
        // OSX Option
        if cfg!(target_os = "macos") && option.starts_with("-psn_") {
            return true;
        }

        self.get_options().iter().any(|opt| opt.name == option)
    }
}

/// Builds the command-line help message in English.
///
/// One line is emitted per option definition: the option name left aligned in
/// a field as wide as the longest option name, followed by the description.
///
/// # Arguments
///
/// * `options` - Option providers contributing to the message; their
///   [`get_options`](ApplicationOptionsProvider::get_options) results are
///   concatenated in the given order.
///
/// # Returns
///
/// The formatted help message, starting with the `Usage` line.
pub fn get_application_help_message(options: Vec<Box<dyn ApplicationOptionsProvider>>) -> String {
    let options: Vec<ApplicationOptionDefinition> = options
        .iter()
        .flat_map(|provider| provider.get_options())
        .collect();

    let name_width = options
        .iter()
        .map(|definition| definition.name.chars().count())
        .max()
        .unwrap_or(0);

    let option_lines: Vec<String> = options
        .iter()
        .map(|definition| {
            format!(
                "  {:<width$}  {}",
                definition.name,
                definition.description,
                width = name_width
            )
        })
        .collect();

    format!(
        "Usage: {} [OPTIONS]\n\nOptions:\n{}",
        program_name(),
        option_lines.join("\n")
    )
}
