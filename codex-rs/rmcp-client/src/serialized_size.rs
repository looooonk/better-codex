use std::io;
use std::io::Write;

use serde::Serialize;

pub(crate) fn serialized_size_exceeds(
    value: &impl Serialize,
    max_bytes: usize,
) -> serde_json::Result<bool> {
    let mut writer = CappedWriter::new(max_bytes.saturating_add(1));
    match serde_json::to_writer(&mut writer, value) {
        Ok(()) => Ok(writer.written > max_bytes),
        Err(error) => {
            if writer.remaining == 0 && error.io_error_kind() == Some(io::ErrorKind::WriteZero) {
                Ok(true)
            } else {
                Err(error)
            }
        }
    }
}

struct CappedWriter {
    remaining: usize,
    written: usize,
}

impl CappedWriter {
    fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            written: 0,
        }
    }
}

impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }

        let written = bytes.len().min(self.remaining);
        self.remaining -= written;
        self.written += written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "serialized_size_tests.rs"]
mod tests;
