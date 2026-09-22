//! PORTING A6/A7: observing/dropping a Promise does not own the I/O task.
//! A retained I/O executor also supports the existing synchronous tool-validation
//! carrier without depending on the blocked caller's Tokio scheduler.
use futures::future::BoxFuture;
use std::{future::Future, io, sync::LazyLock};
static IO_RUNTIME: LazyLock<io::Result<tokio::runtime::Runtime>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("fs-operations")
        .enable_all()
        .build()
});
pub(super) fn start<T: Send + 'static>(
    future: impl Future<Output = io::Result<T>> + Send + 'static,
) -> BoxFuture<'static, io::Result<T>> {
    let (send, receive) = futures::channel::oneshot::channel();
    match IO_RUNTIME.as_ref() {
        Ok(runtime) => {
            runtime.spawn(async move {
                let _ = send.send(future.await);
            });
        }
        Err(error) => {
            let _ = send.send(Err(io::Error::new(error.kind(), error.to_string())));
        }
    }
    Box::pin(async move { receive.await.map_err(io::Error::other)? })
}
