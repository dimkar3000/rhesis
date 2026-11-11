use std::error::Error as StdError;
use std::fmt;

/// Frame-level failure with enough context for a clean reconnect decision
#[derive(Debug)]
pub enum FrameError {
    /// Socket/pipe failure unrelated to the data (keep the session).
    Io(std::io::Error),
    /// postcard failed to serialize (should be impossible for these enum
    /// shapes; treated as a bug, session ends).
    Encode(postcard::Error),
    /// postcard deserialization or shape mismatch (garbage/racing versions).
    Decode(postcard::Error),
    /// Header announced, or payload produced, more than [`FRAME_CAP`] bytes.
    TooLarge { length: usize },
    /// The stream ended mid-frame (peer died between bytes, not frames).
    Truncated,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "io error on the frame stream: {e}"),
            FrameError::Encode(e) => write!(f, "postcard encoding failed: {e}"),
            FrameError::Decode(e) => write!(f, "postcard decoding failed: {e}"),
            FrameError::TooLarge { length } => {
                write!(f, "frame of {length} bytes exceeds the cap")
            }
            FrameError::Truncated => write!(f, "frame stream ended mid-frame"),
        }
    }
}

impl StdError for FrameError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            FrameError::Io(e) => Some(e),
            FrameError::Encode(e) | FrameError::Decode(e) => Some(e),
            FrameError::TooLarge { .. } | FrameError::Truncated => None,
        }
    }
}
