//! Tauri main-thread tasks, queued startup work and event dispatch.

use crate::controler::{
    main_thread::MainThreadHandlerStore, MainThreadContext, MainThreadDispatchError,
    MainThreadDispatcher, MainThreadEvent, MainThreadHandler, MainThreadHandlerId, MainThreadTask,
};
use std::sync::{Arc, Mutex};

/// Main thread dispatcher implementation for Tauri applications.
pub(super) struct TauriMainThreadDispatcher {
    /// The Tauri application handle (set during setup).
    app_handle: Mutex<Option<::tauri::AppHandle<::tauri::Wry>>>,
    /// Tasks queued before the app handle is available.
    pending_tasks: Mutex<Vec<MainThreadTask>>,
    /// Registered event handlers.
    handlers: Arc<MainThreadHandlerStore>,
    /// Shared main thread context.
    context: Arc<Mutex<MainThreadContext>>,
}

impl TauriMainThreadDispatcher {
    /// Creates a new Tauri main thread dispatcher.
    ///
    /// # Returns
    /// A new Arc-wrapped TauriMainThreadDispatcher instance.
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            app_handle: Mutex::new(None),
            pending_tasks: Mutex::new(Vec::new()),
            handlers: Arc::new(MainThreadHandlerStore::default()),
            context: Arc::new(Mutex::new(MainThreadContext::new())),
        })
    }

    /// Attaches the Tauri app handle to this dispatcher.
    ///
    /// This method must be called during Tauri setup to enable main thread
    /// dispatching. It also processes any tasks that were queued before
    /// the app handle was available.
    ///
    /// # Arguments
    /// * `app_handle` - The Tauri application handle.
    ///
    /// # Returns
    /// `Ok(())` if the app handle was successfully attached.
    /// `Err(MainThreadDispatchError)` if there was an error.
    pub(super) fn attach_app_handle(
        &self,
        app_handle: ::tauri::AppHandle<::tauri::Wry>,
    ) -> Result<(), MainThreadDispatchError> {
        {
            let mut handle_slot = self.app_handle.lock().map_err(|_| {
                MainThreadDispatchError::DispatchFailed("Tauri app handle store poisoned".into())
            })?;
            *handle_slot = Some(app_handle.clone());
        }
        {
            let mut context = self.context.lock().map_err(|_| {
                MainThreadDispatchError::DispatchFailed("Tauri main-thread context poisoned".into())
            })?;
            context.insert_resource(app_handle.clone());
        }

        let pending_tasks = {
            let mut pending_tasks = self.pending_tasks.lock().map_err(|_| {
                MainThreadDispatchError::DispatchFailed("Tauri pending task queue poisoned".into())
            })?;
            pending_tasks.drain(..).collect::<Vec<_>>()
        };

        for task in pending_tasks {
            let context = Arc::clone(&self.context);
            app_handle
                .run_on_main_thread(move || {
                    if let Ok(mut context) = context.lock() {
                        task(&mut context);
                    }
                })
                .map_err(|err| MainThreadDispatchError::DispatchFailed(err.to_string()))?;
        }
        Ok(())
    }
}

impl MainThreadDispatcher for TauriMainThreadDispatcher {
    fn dispatch_main_thread_task(
        &self,
        task: MainThreadTask,
    ) -> Result<(), MainThreadDispatchError> {
        let app_handle = self
            .app_handle
            .lock()
            .map_err(|_| {
                MainThreadDispatchError::DispatchFailed("Tauri app handle store poisoned".into())
            })?
            .clone();

        if let Some(app_handle) = app_handle {
            let context = Arc::clone(&self.context);
            app_handle
                .run_on_main_thread(move || {
                    if let Ok(mut context) = context.lock() {
                        task(&mut context);
                    }
                })
                .map_err(|err| MainThreadDispatchError::DispatchFailed(err.to_string()))
        } else {
            self.pending_tasks
                .lock()
                .map_err(|_| {
                    MainThreadDispatchError::DispatchFailed(
                        "Tauri pending task queue poisoned".into(),
                    )
                })?
                .push(task);
            Ok(())
        }
    }

    fn register_main_thread_handler(
        &self,
        handler: MainThreadHandler,
    ) -> Result<MainThreadHandlerId, MainThreadDispatchError> {
        self.handlers.register(handler)
    }

    fn unregister_main_thread_handler(
        &self,
        id: MainThreadHandlerId,
    ) -> Result<(), MainThreadDispatchError> {
        self.handlers.unregister(id)
    }

    fn dispatch_main_thread_event(
        &self,
        id: MainThreadHandlerId,
        event: MainThreadEvent,
    ) -> Result<(), MainThreadDispatchError> {
        self.handlers.ensure_registered(id)?;
        let handlers = Arc::clone(&self.handlers);
        self.dispatch_main_thread_task(Box::new(move |context| {
            let _ = handlers.dispatch_event(context, id, event);
        }))
    }
}
