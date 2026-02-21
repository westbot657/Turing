use std::{marker::PhantomData, sync::Arc, task::Poll};

use parking_lot::RwLock;
use tokio::io::AsyncWrite;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};

use crate::ExternalFunctions;

pub struct OutputWriter<Ext: ExternalFunctions + Send> {
    inner: Arc<RwLock<Vec<u8>>>,
    is_err: bool,
    _ext: PhantomData<Ext>,
}

impl<Ext: ExternalFunctions + Send> std::io::Write for OutputWriter<Ext> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write().extend(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        // Move the inner buffer out so we avoid an extra copy when converting
        // bytes -> String. Taking the write lock lets us swap the Vec<u8>.
        let vec = {
            let mut guard = self.inner.write();
            std::mem::take(&mut *guard)
        };
        if !vec.is_empty() {
            let s = String::from_utf8_lossy(&vec).into_owned();
            if self.is_err {
                Ext::log_critical(s)
            } else {
                Ext::log_info(s);
            }
        }
        Ok(())
    }
}

impl<Ext: ExternalFunctions + Send> AsyncWrite for OutputWriter<Ext> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        self.inner.write().extend(buf);
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        // Move the inner buffer out so we avoid an extra copy when converting
        // bytes -> String. Taking the write lock lets us swap the Vec<u8>.
        let vec = {
            let mut guard = self.inner.write();
            std::mem::take(&mut *guard)
        };
        if !vec.is_empty() {
            let s = String::from_utf8_lossy(&vec).into_owned();
            if self.is_err {
                Ext::log_critical(s);
            } else {
                Ext::log_info(s);
            }
        }
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

pub struct WriterInit<Ext: ExternalFunctions>(
    pub Arc<RwLock<Vec<u8>>>,
    pub bool,
    pub PhantomData<Ext>,
);

impl<Ext: ExternalFunctions> IsTerminal for WriterInit<Ext> {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl<Ext: ExternalFunctions + Send + Sync + 'static> StdoutStream for WriterInit<Ext> {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(OutputWriter::<Ext> {
            inner: self.0.clone(),
            is_err: self.1,
            _ext: PhantomData,
        })
    }
}
