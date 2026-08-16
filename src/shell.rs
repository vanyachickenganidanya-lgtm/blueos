pub struct Shell {
    buffer: [u8; 80],
    length: usize,
}

impl Shell {
    pub const fn new() -> Self {
        Self {
            buffer: [0; 80],
            length: 0,
        }
    }

    /// Feeds one ASCII byte. Returns true when a complete command is ready.
    pub fn feed(&mut self, byte: u8) -> FeedResult {
        match byte {
            b'\r' | b'\n' => FeedResult::Ready,
            8 | 127 => {
                if self.length > 0 {
                    self.length -= 1;
                    FeedResult::Backspace
                } else {
                    FeedResult::Ignored
                }
            }
            b' '..=b'~' if self.length < self.buffer.len() => {
                self.buffer[self.length] = byte;
                self.length += 1;
                FeedResult::Echo(byte)
            }
            _ => FeedResult::Ignored,
        }
    }

    pub fn line(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.buffer[..self.length]) }
    }

    pub fn reset(&mut self) {
        self.length = 0;
    }
}

pub enum FeedResult {
    Echo(u8),
    Backspace,
    Ready,
    Ignored,
}
