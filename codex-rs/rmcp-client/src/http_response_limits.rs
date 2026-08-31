use std::io;

pub(crate) const MAX_MCP_HTTP_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct SseEventSizeLimit {
    maximum_bytes: Option<usize>,
    pub(super) retained_bytes: usize,
    pub(super) line_bytes: usize,
    pub(super) line_is_comment: bool,
    pub(super) previous_was_carriage_return: bool,
    pub(super) failed: bool,
}

impl SseEventSizeLimit {
    pub(crate) fn new(maximum_bytes: Option<usize>) -> Self {
        Self {
            maximum_bytes,
            retained_bytes: 0,
            line_bytes: 0,
            line_is_comment: false,
            previous_was_carriage_return: false,
            failed: false,
        }
    }

    pub(crate) fn observe(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized MCP SSE event was already rejected",
            ));
        }
        let Some(maximum_bytes) = self.maximum_bytes else {
            return Ok(());
        };

        for &byte in bytes {
            if self.previous_was_carriage_return {
                self.previous_was_carriage_return = false;
                if byte == b'\n' {
                    continue;
                }
            }

            match byte {
                b'\r' => {
                    self.finish_line(maximum_bytes)?;
                    self.previous_was_carriage_return = true;
                }
                b'\n' => self.finish_line(maximum_bytes)?,
                _ => {
                    if self.line_bytes == 0 {
                        self.line_is_comment = byte == b':';
                    }
                    self.line_bytes = self.line_bytes.saturating_add(1);
                    self.check_limit(maximum_bytes)?;
                }
            }
        }
        Ok(())
    }

    fn finish_line(&mut self, maximum_bytes: usize) -> io::Result<()> {
        if self.line_bytes == 0 {
            self.retained_bytes = 0;
        } else if !self.line_is_comment {
            self.retained_bytes = self
                .retained_bytes
                .saturating_add(self.line_bytes)
                .saturating_add(1);
        }

        self.line_bytes = 0;
        self.line_is_comment = false;
        self.check_limit(maximum_bytes)
    }

    fn check_limit(&mut self, maximum_bytes: usize) -> io::Result<()> {
        if self.retained_bytes.saturating_add(self.line_bytes) > maximum_bytes {
            self.failed = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("MCP response body exceeds {maximum_bytes} bytes"),
            ));
        }
        Ok(())
    }
}
