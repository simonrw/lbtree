use std::sync::{Arc, Mutex};

/// Trait for writing output, allows abstraction for testing
pub trait OutputWriter: Send + Sync {
    fn write_line(&self, content: &str);
}

/// Standard output writer using println!
pub struct StdoutWriter;

impl OutputWriter for StdoutWriter {
    fn write_line(&self, content: &str) {
        println!("{}", content);
    }
}

/// Buffer writer for capturing output in tests
pub struct BufferWriter {
    buffer: Arc<Mutex<String>>,
}

impl BufferWriter {
    pub fn new() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn get_output(&self) -> String {
        self.buffer.lock().unwrap().clone()
    }
}

impl OutputWriter for BufferWriter {
    fn write_line(&self, content: &str) {
        let mut buf = self.buffer.lock().unwrap();
        buf.push_str(content);
        buf.push('\n');
    }
}

/// Trait for presenting AWS resources in a tree structure
pub trait Present: std::fmt::Debug + Send + Sync + 'static {
    /// Get the string representation of this resource
    fn content(&self) -> String;

    /// Get the indentation level for this resource
    fn indent(&self) -> usize;

    /// Present this resource using the provided output writer
    fn present(&self, writer: &dyn OutputWriter) {
        let prefix = " ".repeat(self.indent()) + "-> ";
        writer.write_line(&format!("{}{}", prefix, self.content()));
    }
}
