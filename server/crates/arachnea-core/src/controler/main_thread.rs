//! Main-thread dispatch primitives shared by controller backends.

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
};

/// Identifier returned for a registered main-thread event handler.
///
/// This struct uniquely identifies a registered event handler and can be used
/// to unregister the handler or dispatch events to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MainThreadHandlerId(u64);

impl MainThreadHandlerId {
    /// Returns the numeric identifier value.
    ///
    /// # Returns
    /// The underlying u64 value of the handler identifier.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Context passed to tasks and handlers running on the process main thread.
///
/// This struct provides a type-safe resource container that can be accessed
/// by main-thread tasks and event handlers.
#[derive(Default)]
pub struct MainThreadContext {
    /// Internal map of type-erased resources.
    resources: HashMap<TypeId, Box<dyn Any + Send + 'static>>,
}

impl MainThreadContext {
    /// Creates an empty main-thread context.
    ///
    /// # Returns
    /// A new, empty MainThreadContext instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a typed resource in the context.
    ///
    /// # Type Parameters
    /// * `T` - The type of resource to insert, must implement `Any + Send + 'static`.
    ///
    /// # Arguments
    /// * `resource` - Resource made available to main-thread tasks.
    pub fn insert_resource<T>(&mut self, resource: T)
    where
        T: Any + Send + 'static,
    {
        self.resources.insert(TypeId::of::<T>(), Box::new(resource));
    }

    /// Borrows a typed resource from the context.
    ///
    /// # Type Parameters
    /// * `T` - The type of resource to borrow, must implement `Any + Send + 'static`.
    ///
    /// # Returns
    /// `Some(&T)` if a resource of the requested type exists.
    /// `None` if no resource of the requested type exists.
    pub fn resource<T>(&self) -> Option<&T>
    where
        T: Any + Send + 'static,
    {
        self.resources.get(&TypeId::of::<T>())?.downcast_ref::<T>()
    }
}

/// Task executed on the process main thread.
///
/// This type represents a one-shot task that will be executed on the main thread
/// with access to the main thread context.
pub type MainThreadTask = Box<dyn FnOnce(&mut MainThreadContext) + Send + 'static>;

/// Handler executed on the process main thread when a matching event is dispatched.
///
/// This type represents an event handler that can be called multiple times
/// with different events.
pub type MainThreadHandler =
    Box<dyn FnMut(&mut MainThreadContext, MainThreadEvent) + Send + 'static>;

/// Event payload delivered to one registered main-thread handler.
///
/// This struct represents an event that can be dispatched to a specific handler
/// on the main thread.
pub struct MainThreadEvent {
    /// The stable event name used by the caller.
    name: String,
    /// Optional typed payload consumed by the handler.
    payload: Option<Box<dyn Any + Send + 'static>>,
}

impl MainThreadEvent {
    /// Creates an event without a typed payload.
    ///
    /// # Arguments
    /// * `name` - Stable event name used by the caller.
    ///
    /// # Returns
    /// A new MainThreadEvent with the given name and no payload.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            payload: None,
        }
    }

    /// Creates an event with a typed payload.
    ///
    /// # Type Parameters
    /// * `T` - The type of payload, must implement `Any + Send + 'static`.
    ///
    /// # Arguments
    /// * `name` - Stable event name used by the caller.
    /// * `payload` - Payload consumed by the handler.
    ///
    /// # Returns
    /// A new MainThreadEvent with the given name and typed payload.
    pub fn with_payload<T>(name: impl Into<String>, payload: T) -> Self
    where
        T: Any + Send + 'static,
    {
        Self {
            name: name.into(),
            payload: Some(Box::new(payload)),
        }
    }

    /// Returns the event name.
    ///
    /// # Returns
    /// A reference to the event name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrows the payload as a concrete type when it matches.
    ///
    /// # Type Parameters
    /// * `T` - The type to borrow the payload as, must implement `Any + Send + 'static`.
    ///
    /// # Returns
    /// `Some(&T)` if the payload matches the requested type.
    /// `None` if there is no payload or it doesn't match the requested type.
    pub fn payload_ref<T>(&self) -> Option<&T>
    where
        T: Any + Send + 'static,
    {
        self.payload.as_ref()?.downcast_ref::<T>()
    }

    /// Consumes the event and returns the payload as a concrete type when it matches.
    ///
    /// # Type Parameters
    /// * `T` - The type to extract the payload as, must implement `Any + Send + 'static`.
    ///
    /// # Returns
    /// `Ok(T)` if the payload matches the requested type.
    /// `Err(Self)` if there is no payload or it doesn't match the requested type.
    pub fn into_payload<T>(self) -> Result<T, Self>
    where
        T: Any + Send + 'static,
    {
        let Some(payload) = self.payload else {
            return Err(self);
        };
        match payload.downcast::<T>() {
            Ok(payload) => Ok(*payload),
            Err(payload) => Err(Self {
                name: self.name,
                payload: Some(payload),
            }),
        }
    }
}

/// Errors produced by main-thread dispatch operations.
///
/// This enum represents all possible errors that can occur when working with
/// the main-thread dispatch system.
#[derive(Debug)]
pub enum MainThreadDispatchError {
    /// A global dispatcher was already installed.
    ///
    /// This error occurs when attempting to install a second global dispatcher.
    DispatcherAlreadyInstalled,
    /// The dispatcher queue is no longer accepting tasks.
    ///
    /// This error occurs when the dispatcher has been dropped and can no longer
    /// accept new tasks or events.
    DispatcherUnavailable,
    /// The requested handler does not exist.
    ///
    /// This error occurs when attempting to unregister or dispatch to a handler
    /// that was never registered or has already been unregistered.
    HandlerNotFound(MainThreadHandlerId),
    /// A dispatcher-specific failure occurred.
    ///
    /// This error represents any other failure that may occur during dispatch
    /// operations.
    DispatchFailed(String),
}

impl fmt::Display for MainThreadDispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DispatcherAlreadyInstalled => {
                f.write_str("main-thread dispatcher is already installed")
            }
            Self::DispatcherUnavailable => f.write_str("main-thread dispatcher is unavailable"),
            Self::HandlerNotFound(id) => {
                write!(f, "main-thread handler {} was not found", id.value())
            }
            Self::DispatchFailed(error) => write!(f, "main-thread dispatch failed: {error}"),
        }
    }
}

impl std::error::Error for MainThreadDispatchError {}

/// Dispatches tasks and handler events onto the process main thread.
///
/// This trait defines the interface for dispatching tasks and events to the
/// main thread from any thread in the application.
pub trait MainThreadDispatcher: Send + Sync {
    /// Queues a one-shot task to run on the process main thread.
    ///
    /// # Arguments
    /// * `task` - Task to run on the main thread.
    ///
    /// # Returns
    /// `Ok(())` if the task was successfully queued.
    /// `Err(MainThreadDispatchError)` when the backend cannot accept the task.
    fn dispatch_main_thread_task(
        &self,
        task: MainThreadTask,
    ) -> Result<(), MainThreadDispatchError>;

    /// Registers an event handler that is invoked on the process main thread.
    ///
    /// # Arguments
    /// * `handler` - Handler receiving events dispatched to the returned id.
    ///
    /// # Returns
    /// `Ok(MainThreadHandlerId)` containing the handler identifier.
    /// `Err(MainThreadDispatchError)` when the handler store cannot be updated.
    fn register_main_thread_handler(
        &self,
        handler: MainThreadHandler,
    ) -> Result<MainThreadHandlerId, MainThreadDispatchError>;

    /// Removes a registered main-thread event handler.
    ///
    /// # Arguments
    /// * `id` - Handler id returned by [`Self::register_main_thread_handler`].
    ///
    /// # Returns
    /// `Ok(())` if the handler was successfully removed.
    /// `Err(MainThreadDispatchError)` when the handler does not exist.
    fn unregister_main_thread_handler(
        &self,
        id: MainThreadHandlerId,
    ) -> Result<(), MainThreadDispatchError>;

    /// Queues an event for one registered handler on the process main thread.
    ///
    /// # Arguments
    /// * `id` - Registered handler id.
    /// * `event` - Event delivered to the handler.
    ///
    /// # Returns
    /// `Ok(())` if the event was successfully queued.
    /// `Err(MainThreadDispatchError)` when the handler is absent or the backend cannot queue the event.
    fn dispatch_main_thread_event(
        &self,
        id: MainThreadHandlerId,
        event: MainThreadEvent,
    ) -> Result<(), MainThreadDispatchError>;
}

static GLOBAL_MAIN_THREAD_DISPATCHER: OnceLock<Arc<dyn MainThreadDispatcher>> = OnceLock::new();

/// Installs the process-wide main-thread dispatcher.
///
/// # Arguments
/// * `dispatcher` - Dispatcher owned by the selected controller backend.
///
/// # Errors
/// Returns an error when another dispatcher was already installed.
pub fn install_global_main_thread_dispatcher(
    dispatcher: Arc<dyn MainThreadDispatcher>,
) -> Result<(), MainThreadDispatchError> {
    GLOBAL_MAIN_THREAD_DISPATCHER
        .set(dispatcher)
        .map_err(|_| MainThreadDispatchError::DispatcherAlreadyInstalled)
}

/// Returns the process-wide main-thread dispatcher when one was installed.
pub fn global_main_thread_dispatcher() -> Option<Arc<dyn MainThreadDispatcher>> {
    GLOBAL_MAIN_THREAD_DISPATCHER.get().cloned()
}

#[derive(Default)]
pub(super) struct MainThreadHandlerStore {
    next_id: AtomicU64,
    handlers: Mutex<HashMap<MainThreadHandlerId, MainThreadHandler>>,
}

impl MainThreadHandlerStore {
    pub(super) fn register(
        &self,
        handler: MainThreadHandler,
    ) -> Result<MainThreadHandlerId, MainThreadDispatchError> {
        let id = MainThreadHandlerId(self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        let mut handlers = self.handlers.lock().map_err(|_| {
            MainThreadDispatchError::DispatchFailed("handler store poisoned".into())
        })?;
        handlers.insert(id, handler);
        Ok(id)
    }

    pub(super) fn unregister(
        &self,
        id: MainThreadHandlerId,
    ) -> Result<(), MainThreadDispatchError> {
        let mut handlers = self.handlers.lock().map_err(|_| {
            MainThreadDispatchError::DispatchFailed("handler store poisoned".into())
        })?;
        handlers
            .remove(&id)
            .map(|_| ())
            .ok_or(MainThreadDispatchError::HandlerNotFound(id))
    }

    pub(super) fn dispatch_event(
        &self,
        context: &mut MainThreadContext,
        id: MainThreadHandlerId,
        event: MainThreadEvent,
    ) -> Result<(), MainThreadDispatchError> {
        let mut handlers = self.handlers.lock().map_err(|_| {
            MainThreadDispatchError::DispatchFailed("handler store poisoned".into())
        })?;
        let handler = handlers
            .get_mut(&id)
            .ok_or(MainThreadDispatchError::HandlerNotFound(id))?;
        handler(context, event);
        Ok(())
    }

    pub(super) fn ensure_registered(
        &self,
        id: MainThreadHandlerId,
    ) -> Result<(), MainThreadDispatchError> {
        let handlers = self.handlers.lock().map_err(|_| {
            MainThreadDispatchError::DispatchFailed("handler store poisoned".into())
        })?;
        handlers
            .contains_key(&id)
            .then_some(())
            .ok_or(MainThreadDispatchError::HandlerNotFound(id))
    }
}

enum MainThreadCommand {
    Run(MainThreadTask),
}

/// Dispatcher backed by a command queue consumed by the process main thread.
pub struct QueuedMainThreadDispatcher {
    sender: mpsc::Sender<MainThreadCommand>,
    handlers: Arc<MainThreadHandlerStore>,
}

impl QueuedMainThreadDispatcher {
    /// Creates a queued dispatcher and the loop that must run on the main thread.
    pub fn new_pair() -> (Arc<Self>, MainThreadDispatchLoop) {
        let (sender, receiver) = mpsc::channel();
        let handlers = Arc::new(MainThreadHandlerStore::default());
        (
            Arc::new(Self {
                sender,
                handlers: Arc::clone(&handlers),
            }),
            MainThreadDispatchLoop {
                receiver,
                context: MainThreadContext::new(),
            },
        )
    }
}

impl MainThreadDispatcher for QueuedMainThreadDispatcher {
    fn dispatch_main_thread_task(
        &self,
        task: MainThreadTask,
    ) -> Result<(), MainThreadDispatchError> {
        self.sender
            .send(MainThreadCommand::Run(task))
            .map_err(|_| MainThreadDispatchError::DispatcherUnavailable)
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

/// Main-thread command loop used by controller backends that do not own a UI loop.
pub struct MainThreadDispatchLoop {
    receiver: mpsc::Receiver<MainThreadCommand>,
    context: MainThreadContext,
}

impl MainThreadDispatchLoop {
    /// Runs queued tasks until all dispatcher handles are dropped.
    pub fn run(mut self) {
        while let Ok(command) = self.receiver.recv() {
            match command {
                MainThreadCommand::Run(task) => task(&mut self.context),
            }
        }
    }
}
