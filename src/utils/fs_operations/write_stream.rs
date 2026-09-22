//! Carrier of Node `fs.createWriteStream`: open starts at construction; errors
//! are delivered asynchronously through the stream, with ordered writes.
use std::{
    future::Future,
    io,
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::AsyncWrite;

pub(super) struct NativeWriteStream {
    opening: Option<futures::channel::oneshot::Receiver<io::Result<std::fs::File>>>,
    file: Option<tokio::fs::File>,
}
impl NativeWriteStream {
    pub(super) fn new(path: &Path) -> Self {
        let path = path.to_owned();
        let (send, receive) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = send.send(
                std::fs::File::create(&path)
                    .map_err(|error| super::error::native(error, "open", &path, None)),
            );
        });
        Self {
            opening: Some(receive),
            file: None,
        }
    }
    fn ready(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<&mut tokio::fs::File>> {
        if let Some(opening) = self.opening.as_mut() {
            let result = std::task::ready!(Pin::new(opening).poll(cx));
            self.opening = None;
            self.file = Some(tokio::fs::File::from_std(
                result.map_err(io::Error::other)??,
            ));
        }
        Poll::Ready(
            self.file
                .as_mut()
                .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "stream is closed")),
        )
    }
}
impl AsyncWrite for NativeWriteStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(std::task::ready!(self.ready(cx))?)
            .poll_write(cx, data)
            .map_err(|error| super::error::native(error, "write", Path::new(""), None))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(std::task::ready!(self.ready(cx))?)
            .poll_flush(cx)
            .map_err(|error| super::error::native(error, "write", Path::new(""), None))
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        std::task::ready!(Pin::new(std::task::ready!(self.ready(cx))?).poll_shutdown(cx))
            .map_err(|error| super::error::native(error, "write", Path::new(""), None))?;
        self.file.take();
        Poll::Ready(Ok(()))
    }
}
