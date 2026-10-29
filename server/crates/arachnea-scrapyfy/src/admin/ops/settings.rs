//! Administration settings and password operations.

use super::*;

/// Builds the `settings` reply.
///
/// # Arguments
///
/// * `state` - Shared administration state providing settings, sessions, stores and runtime hooks.
/// * `context` - Incoming controller context used for access checks and request metadata.
/// * `_input` - Unused input accepted for compatibility with the administration handler signature.
pub(crate) async fn op_settings(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    _input: AdminEmptyRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;

    let effective = state.settings();
    let stored = state.adapter().persisted_settings().map_err(admin_err)?;
    let persisted_scraper = ScraperAdminSettings::from_configuration(&stored);
    let cli = state.cli_scraper_settings();
    let effective_scraper = persisted_scraper.clone().merged_overriding(cli);
    let source_for = |cli_set: bool, persisted_set: bool| {
        if cli_set {
            SettingSource::CommandLine
        } else if persisted_set {
            SettingSource::Configuration
        } else {
            SettingSource::Default
        }
    };
    Ok(AdminReply::ok(&SettingsResponse {
        server_port: effective
            .server_port
            .expect("runtime administration settings must have an effective server port"),
        server_port_source: effective.server_port_source,
        network_mode: effective.network_mode.unwrap_or_default().to_string(),
        network_mode_source: effective.network_mode_source,
        entrypoint_root: effective.entrypoint_root.clone(),
        entrypoint_root_source: effective.entrypoint_root_source,
        password_configured: stored.password_hash.is_some(),
        public_http_warning: effective.network_mode == Some(ServerNetworkMode::Public),
        current_country: effective_scraper.current_country.clone(),
        current_country_source: source_for(
            cli.current_country.is_some(),
            persisted_scraper.current_country.is_some(),
        ),
        cache_max_disk_bytes: effective_scraper
            .cache_max_disk_bytes
            .unwrap_or(DEFAULT_CACHE_MAX_DISK_KIB),
        cache_max_disk_bytes_source: source_for(
            cli.cache_max_disk_bytes.is_some(),
            persisted_scraper.cache_max_disk_bytes.is_some(),
        ),
        cache_max_memory_bytes: effective_scraper
            .cache_max_memory_bytes
            .unwrap_or(DEFAULT_CACHE_MAX_MEMORY_KIB),
        cache_max_memory_bytes_source: source_for(
            cli.cache_max_memory_bytes.is_some(),
            persisted_scraper.cache_max_memory_bytes.is_some(),
        ),
        cache_block_size_bytes: effective_scraper
            .cache_block_size_bytes
            .unwrap_or(DEFAULT_CACHE_BLOCK_SIZE_KIB),
        cache_block_size_bytes_source: source_for(
            cli.cache_block_size_bytes.is_some(),
            persisted_scraper.cache_block_size_bytes.is_some(),
        ),
    }))
}

/// Builds the `update-settings` reply.
///
/// # Arguments
///
/// * `state` - Shared administration state providing settings, sessions, stores and runtime hooks.
/// * `context` - Incoming controller context used for access checks and request metadata.
/// * `input` - Deserialized input for the administration operation.
pub(crate) async fn op_update_settings(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: UpdateSettingsRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let effective_mode = state.mode();
    let mut persisted = state.adapter().persisted_settings().map_err(admin_err)?;

    if let Some(port) = input.server_port {
        if port == 0 {
            return Err(AdminError::bad_request("Server port cannot be zero."));
        }
        persisted.server_port = Some(port);
    }
    if let Some(root) = input.entrypoint_root {
        let root = root.trim().to_string();
        persisted.entrypoint_root = if root.is_empty() { None } else { Some(root) };
    }
    if let Some(network) = input.network_mode {
        let network = network.trim().to_lowercase();
        if !matches!(network.as_str(), "local" | "private" | "public") {
            return Err(AdminError::bad_request(format!(
                "Invalid network mode {network}; expected local, private or public."
            )));
        }
        if network == "local" && effective_mode == ApplicationMode::Server {
            let loopback = context
                .remote_addr()
                .map(|addr| addr.ip().is_loopback())
                .unwrap_or(false);
            if !loopback || !application::is_running_elevated() {
                return Err(AdminError::forbidden(
                    "Network mode local requires a loopback client and elevated privileges.",
                ));
            }
        }
        persisted.network_mode = Some(network.parse().expect("validated network mode must parse"));
    }

    // Scraper-specific overrides: current country and cache sizing. Empty
    // values clear the persisted override (runtime default applies). Values
    // pinned by the command line cannot be changed or cleared through the API.
    let cli = state.cli_scraper_settings();
    let mut scraper_settings = ScraperAdminSettings::from_configuration(&persisted);
    let mut scraper_input_changed = false;
    if let Some(country) = &input.current_country {
        scraper_input_changed = true;
        if cli.current_country.is_some() {
            return Err(AdminError::bad_request(
                "The current country is pinned by the command line and cannot be changed.",
            ));
        }
        let country = country.trim().to_uppercase();
        scraper_settings.current_country = if country.is_empty() {
            None
        } else {
            Some(country)
        };
    }
    if let Some(bytes) = input.cache_max_disk_bytes {
        scraper_input_changed = true;
        if cli.cache_max_disk_bytes.is_some() {
            return Err(AdminError::bad_request(
                "The disk cache size is pinned by the command line and cannot be changed.",
            ));
        }
        scraper_settings.cache_max_disk_bytes = if bytes == 0 { None } else { Some(bytes) };
    }
    if let Some(bytes) = input.cache_max_memory_bytes {
        scraper_input_changed = true;
        if cli.cache_max_memory_bytes.is_some() {
            return Err(AdminError::bad_request(
                "The memory cache size is pinned by the command line and cannot be changed.",
            ));
        }
        scraper_settings.cache_max_memory_bytes = if bytes == 0 { None } else { Some(bytes) };
    }
    if let Some(bytes) = input.cache_block_size_bytes {
        scraper_input_changed = true;
        if cli.cache_block_size_bytes.is_some() {
            return Err(AdminError::bad_request(
                "The cache block size is pinned by the command line and cannot be changed.",
            ));
        }
        scraper_settings.cache_block_size_bytes = if bytes == 0 { None } else { Some(bytes) };
    }
    // Validate the persisted overrides and the effective (CLI-over-persisted)
    // configuration: both must be internally consistent.
    scraper_settings.validate().map_err(admin_err)?;
    let effective_scraper = scraper_settings.clone().merged_overriding(cli);
    effective_scraper.validate().map_err(admin_err)?;
    scraper_settings.apply_to_configuration(&mut persisted);

    state
        .adapter()
        .save_persisted_settings(&persisted)
        .map_err(admin_err)?;

    // Hot-apply the scraper runtime overrides (current country and cache
    // sizing) when they were changed. The adapter reloads the runtime with the
    // effective merged values; the command-line-pinned ones stay authoritative.
    if scraper_input_changed {
        state
            .adapter()
            .apply_scraper_settings(&effective_scraper)
            .await
            .map_err(admin_err)?;
    }

    // Hot application path: in server mode with a REST handle, the port,
    // network mode and entrypoint root are applied to the running server
    // without restarting the process. The response is sent first; the
    // application runs APPLY_SETTINGS_DELAY later so the response flows on
    // the listener still alive. The definitive outcome is observable via the
    // `settings` operation and the application log.
    if effective_mode == ApplicationMode::Server {
        if let Some(target) = state.adapter().target_server_settings() {
            // Only advertise an URL change when the port or the entrypoint
            // root will actually move, so the UI never redirects to a URL that
            // stays pinned by a command-line override.
            let current = state.settings();
            let target_port = target
                .server_port
                .expect("REST server target settings must have an effective server port");
            let port_changes = target_port
                != current
                    .server_port
                    .expect("runtime administration settings must have an effective server port");
            let current_root = current
                .entrypoint_root
                .as_deref()
                .filter(|root| !root.is_empty());
            let root_changes = target.entrypoint_root.as_deref() != current_root;
            let admin_url = if port_changes || root_changes {
                hot_apply_admin_url(&context, target.entrypoint_root.as_deref(), target_port)
            } else {
                None
            };
            let adapter = Arc::clone(state.adapter());
            let state_for_apply = Arc::clone(&state);
            std::thread::spawn(move || {
                std::thread::sleep(APPLY_SETTINGS_DELAY);
                let report = adapter.apply_server_settings();
                state_for_apply.update_effective_server_settings(
                    report.server_port,
                    report
                        .network_mode
                        .parse()
                        .expect("REST server report must contain a valid network mode"),
                    report.entrypoint_root.clone(),
                );
                match &report.apply_error {
                    Some(error) => tracing::error!(target: "arachnea_scrapyfy::admin::ops",
                        error = %error,
                        "hot application of server settings failed"
                    ),
                    None => tracing::info!(target: "arachnea_scrapyfy::admin::ops",
                        port = report.server_port,
                        network = %report.network_mode,
                        root = ?report.entrypoint_root,
                        "server settings applied to the running server"
                    ),
                }
            });

            return Ok(AdminReply::ok(&UpdateSettingsResponse {
                restart_required: false,
                applied: true,
                apply_error: None,
                admin_url,
            }));
        }
    }

    Ok(AdminReply::ok(&UpdateSettingsResponse {
        restart_required: true,
        applied: false,
        apply_error: None,
        admin_url: None,
    }))
}

/// Builds the target administration URL of a hot settings application.
///
/// The host name comes from the request `Host` header so the URL keeps the
/// origin the administrator is talking to; the target port replaces the port
/// of the header, and the target root prefixes the admin path. Returns `None`
/// when the header is missing.
///
/// # Arguments
///
/// * `context` - Incoming controller context used for access checks and request metadata.
/// * `entrypoint_root` - Effective root path or entry-point prefix of the running controller.
/// * `target_port` - Effective controller port used when rebuilding the public administration URL.
pub(super) fn hot_apply_admin_url(
    context: &RequestControlerContext,
    entrypoint_root: Option<&str>,
    target_port: u16,
) -> Option<String> {
    let host = context.header("host")?;
    let hostname = match host.rsplit_once(':') {
        // Host header forms: `hostname:port`, `[ipv6]:port` or bare `ipv6`.
        Some((lhs, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => lhs,
        _ => host,
    };
    let root = entrypoint_root.unwrap_or("").trim_matches('/');
    if root.is_empty() {
        Some(format!("http://{hostname}:{target_port}/admin/"))
    } else {
        Some(format!("http://{hostname}:{target_port}/{root}/admin/"))
    }
}

/// Builds the `set-admin-password` reply.
///
/// # Arguments
///
/// * `state` - Shared administration state providing settings, sessions, stores and runtime hooks.
/// * `context` - Incoming controller context used for access checks and request metadata.
/// * `input` - Deserialized input for the administration operation.
pub(crate) async fn op_set_admin_password(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: SetAdminPasswordRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let mut persisted = state.adapter().persisted_settings().map_err(admin_err)?;
    if let Some(existing) = persisted.password_hash.as_deref() {
        let current = input
            .current_password
            .as_deref()
            .ok_or_else(|| AdminError::unauthorized("Current password is incorrect."))?;
        if !verify_admin_password(current, existing) {
            return Err(AdminError::unauthorized("Current password is incorrect."));
        }
    }
    if input.new_password.chars().count() < 8 {
        return Err(AdminError::bad_request(
            "New administrator password must be at least 8 characters.",
        ));
    }

    persisted.password_hash = Some(hash_admin_password(&input.new_password).map_err(admin_err)?);
    state
        .adapter()
        .save_persisted_settings(&persisted)
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&AdminEmptyRequest {}))
}
