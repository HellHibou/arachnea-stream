//! Native proxy configuration installed before a solver WebView navigates.

use url::Url;

use crate::ArachneaHttpError;

/// A proxy endpoint and its credentials, deliberately without a Debug implementation.
#[derive(Clone)]
pub(super) struct WebviewProxy {
    endpoint: Url,
    credentials: Option<(String, String)>,
}

impl WebviewProxy {
    /// Parses credentials separately from the native endpoint and validates platform support.
    ///
    /// # Arguments
    /// - `value`: Effective proxy URL, including any opaque loopback routing credentials.
    ///
    /// # Errors
    /// Returns a redacted configuration error for unsupported endpoints or platforms.
    pub(super) fn parse(value: &str) -> Result<Self, ArachneaHttpError> {
        let mut endpoint = Url::parse(value).map_err(|_| invalid("invalid proxy URL"))?;
        if !matches!(endpoint.scheme(), "http" | "socks5")
            || endpoint.host_str().is_none()
            || endpoint.port_or_known_default().is_none()
        {
            return Err(invalid(
                "the WebView proxy requires an HTTP or SOCKS5 endpoint with a port",
            ));
        }
        let credentials = if endpoint.username().is_empty() {
            if endpoint.password().is_some() {
                return Err(invalid("proxy credentials require a username"));
            }
            None
        } else {
            let username = urlencoding::decode(endpoint.username())
                .map_err(|_| invalid("invalid proxy username encoding"))?
                .into_owned();
            let password = urlencoding::decode(
                endpoint
                    .password()
                    .ok_or_else(|| invalid("proxy credentials require a password"))?,
            )
            .map_err(|_| invalid("invalid proxy password encoding"))?
            .into_owned();
            if username.contains('\0') || password.contains('\0') {
                return Err(invalid("proxy credentials cannot contain null characters"));
            }
            Some((username, password))
        };
        #[cfg(target_os = "macos")]
        if endpoint.scheme() == "http"
            && credentials
                .as_ref()
                .is_some_and(|(username, _)| username == "arachnea-context")
            && endpoint
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback())
        {
            endpoint = Url::parse(&format!(
                "socks5://{}:{}",
                endpoint.host().unwrap(),
                endpoint.port_or_known_default().unwrap()
            ))
            .map_err(|_| invalid("invalid internal proxy endpoint"))?;
        }
        #[cfg(target_os = "macos")]
        if endpoint.scheme() == "http" {
            return Err(invalid(
                "the macOS WebView requires a SOCKS5 proxy for complete HTTP and HTTPS routing",
            ));
        }
        #[cfg(target_os = "windows")]
        if endpoint.scheme() == "socks5" && credentials.is_some() {
            return Err(invalid(
                "authenticated SOCKS5 is unavailable through WebView2; use an HTTP proxy",
            ));
        }
        endpoint
            .set_username("")
            .map_err(|_| invalid("invalid proxy endpoint"))?;
        endpoint
            .set_password(None)
            .map_err(|_| invalid("invalid proxy endpoint"))?;
        #[cfg(target_os = "macos")]
        if objc2_foundation::NSProcessInfo::processInfo()
            .operatingSystemVersion()
            .majorVersion
            < 14
        {
            return Err(invalid("WebView proxy routing requires macOS 14 or later"));
        }
        #[cfg(any(target_os = "ios", target_os = "android"))]
        return Err(invalid(
            "the native WebView proxy adapter is unavailable on this platform",
        ));
        Ok(Self {
            endpoint,
            credentials,
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn endpoint(&self) -> Url {
        self.endpoint.clone()
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn wry_config(&self) -> wry::ProxyConfig {
        let endpoint = wry::ProxyEndpoint {
            host: self
                .endpoint
                .host_str()
                .expect("validated proxy host")
                .to_string(),
            port: self
                .endpoint
                .port_or_known_default()
                .expect("validated proxy port")
                .to_string(),
        };
        if self.endpoint.scheme() == "socks5" {
            wry::ProxyConfig::Socks5(endpoint)
        } else {
            wry::ProxyConfig::Http(endpoint)
        }
    }

    /// Installs native authentication before the standalone WebView's first navigation.
    pub(super) fn configure_wry(&self, webview: &wry::WebView) -> Result<(), ArachneaHttpError> {
        #[cfg(target_os = "macos")]
        {
            let _ = webview;
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        {
            use wry::WebViewExtWindows;
            return self.configure_windows(webview.webview());
        }
        #[cfg(target_os = "linux")]
        {
            use wry::WebViewExtUnix;
            self.configure_linux(&webview.webview());
            return Ok(());
        }
        #[allow(unreachable_code)]
        Err(invalid(
            "the native WebView proxy adapter is unavailable on this platform",
        ))
    }

    /// Installs native authentication in Tauri's main-thread WebView callback.
    pub(super) fn configure_tauri(
        &self,
        webview: tauri::webview::PlatformWebview,
    ) -> Result<(), ArachneaHttpError> {
        #[cfg(target_os = "macos")]
        {
            let _ = webview;
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        return self.configure_windows(
            unsafe { webview.controller().CoreWebView2() }
                .map_err(|_| invalid("cannot access WebView2 proxy authentication"))?,
        );
        #[cfg(target_os = "linux")]
        {
            self.configure_linux(&webview.inner());
            return Ok(());
        }
        #[allow(unreachable_code)]
        Err(invalid(
            "the native WebView proxy adapter is unavailable on this platform",
        ))
    }

    /// Creates an isolated store with proxy settings before WebKit starts its network session.
    #[cfg(target_os = "macos")]
    pub(super) fn macos_configuration(
        &self,
    ) -> Result<objc2::rc::Retained<objc2_web_kit::WKWebViewConfiguration>, ArachneaHttpError> {
        use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};
        let main_thread = objc2::MainThreadMarker::new()
            .ok_or_else(|| invalid("proxy configuration requires the WebView thread"))?;
        unsafe {
            let configuration = WKWebViewConfiguration::new(main_thread);
            let store = WKWebsiteDataStore::nonPersistentDataStore(main_thread);
            self.configure_macos_store(&store)?;
            configuration.setWebsiteDataStore(&store);
            Ok(configuration)
        }
    }

    /// Sets credentials on the Network.framework proxy object, never on origin requests.
    #[cfg(target_os = "macos")]
    unsafe fn configure_macos_store(
        &self,
        store: &objc2::runtime::AnyObject,
    ) -> Result<(), ArachneaHttpError> {
        use objc2::{msg_send, rc::Retained, runtime::AnyObject};
        use objc2_foundation::NSArray;
        use std::ffi::CString;

        let api = native_proxy_api()?;
        let host = CString::new(self.endpoint.host_str().unwrap())
            .map_err(|_| invalid("invalid proxy host"))?;
        let port =
            CString::new(self.endpoint.port_or_known_default().unwrap().to_string()).unwrap();
        let endpoint = Retained::from_raw((api.endpoint)(host.as_ptr(), port.as_ptr()))
            .ok_or_else(|| invalid("cannot create native proxy endpoint"))?;
        let endpoint_ptr = Retained::as_ptr(&endpoint) as *mut AnyObject;
        let config = Retained::from_raw((api.socks)(endpoint_ptr))
            .ok_or_else(|| invalid("cannot create native proxy configuration"))?;
        let config_ptr = Retained::as_ptr(&config) as *mut AnyObject;
        (api.failover)(config_ptr, false);
        if let Some((username, password)) = &self.credentials {
            let username = CString::new(username.as_str()).unwrap();
            let password = CString::new(password.as_str()).unwrap();
            (api.credentials)(config_ptr, username.as_ptr(), password.as_ptr());
        }
        let proxies = NSArray::arrayWithObject(&*config);
        let _: () = msg_send![store, setProxyConfigurations: &*proxies];
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn configure_windows(
        &self,
        webview: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    ) -> Result<(), ArachneaHttpError> {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_10;
        use webview2_com::{take_pwstr, BasicAuthenticationRequestedEventHandler};
        use windows_core::{Interface, HSTRING, PWSTR};
        let Some((username, password)) = self.credentials.clone() else {
            return Ok(());
        };
        let endpoint = self.endpoint.clone();
        let core: ICoreWebView2_10 = webview
            .cast()
            .map_err(|_| invalid("WebView2 runtime does not support proxy authentication"))?;
        let handler = BasicAuthenticationRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Ok(());
            };
            unsafe {
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                let matches_proxy = Url::parse(&uri).ok().is_some_and(|uri| {
                    uri.scheme() == endpoint.scheme()
                        && uri.host_str() == endpoint.host_str()
                        && uri.port_or_known_default() == endpoint.port_or_known_default()
                });
                if matches_proxy {
                    let response = args.Response()?;
                    response.SetUserName(&HSTRING::from(&username))?;
                    response.SetPassword(&HSTRING::from(&password))?;
                }
            }
            Ok(())
        }));
        let mut token = 0;
        unsafe { core.add_BasicAuthenticationRequested(&handler, &mut token) }
            .map_err(|_| invalid("cannot install WebView2 proxy authentication"))?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn configure_linux(&self, webview: &webkit2gtk::WebView) {
        use webkit2gtk::{prelude::*, Credential, CredentialPersistence};
        let Some((username, password)) = self.credentials.clone() else {
            return;
        };
        let endpoint = self.endpoint.clone();
        webview.connect_authenticate(move |_, request| {
            if !request.is_for_proxy()
                || request.host().as_deref() != endpoint.host_str()
                || request.port() != u32::from(endpoint.port_or_known_default().unwrap())
            {
                return false;
            }
            if request.is_retry() {
                request.cancel();
            } else {
                use webkit2gtk::glib::translate::{ToGlibPtr, ToGlibPtrMut};
                let mut credential =
                    Credential::new(&username, &password, CredentialPersistence::None);
                unsafe {
                    webkit2gtk::ffi::webkit_authentication_request_authenticate(
                        request.to_glib_none().0,
                        credential.to_glib_none_mut().0,
                    );
                }
            }
            true
        });
    }
}

fn invalid(message: &str) -> ArachneaHttpError {
    ArachneaHttpError::InvalidConfiguration(format!("Tauri/Wry proxy: {message}"))
}

/// Resolve modern Network.framework symbols lazily so direct mode still launches on older macOS.
#[cfg(target_os = "macos")]
struct NativeProxyApi {
    endpoint: unsafe extern "C" fn(
        *const std::ffi::c_char,
        *const std::ffi::c_char,
    ) -> *mut objc2::runtime::AnyObject,
    socks: unsafe extern "C" fn(*mut objc2::runtime::AnyObject) -> *mut objc2::runtime::AnyObject,
    credentials: unsafe extern "C" fn(
        *mut objc2::runtime::AnyObject,
        *const std::ffi::c_char,
        *const std::ffi::c_char,
    ),
    failover: unsafe extern "C" fn(*mut objc2::runtime::AnyObject, bool),
    _library: libloading::Library,
}

#[cfg(target_os = "macos")]
fn native_proxy_api() -> Result<&'static NativeProxyApi, ArachneaHttpError> {
    static API: std::sync::OnceLock<Result<NativeProxyApi, ()>> = std::sync::OnceLock::new();
    API.get_or_init(|| {
        // These signatures follow Apple's Network.framework C API; keep the library loaded.
        let load = || unsafe {
            let library =
                libloading::Library::new("/System/Library/Frameworks/Network.framework/Network")?;
            Ok::<_, libloading::Error>(NativeProxyApi {
                endpoint: *library.get(b"nw_endpoint_create_host\0")?,
                socks: *library.get(b"nw_proxy_config_create_socksv5\0")?,
                credentials: *library.get(b"nw_proxy_config_set_username_and_password\0")?,
                failover: *library.get(b"nw_proxy_config_set_failover_allowed\0")?,
                _library: library,
            })
        };
        load().map_err(|_| ())
    })
    .as_ref()
    .map_err(|_| invalid("native proxy configuration API is unavailable"))
}
