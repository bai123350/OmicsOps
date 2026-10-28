//! Standalone offline child used only by claude_code_contracts.
use std::io::{Read, Write};

fn main() {
    let mut request = Vec::new();
    std::io::stdin().read_to_end(&mut request).unwrap();
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(&[b'x'; 65_537]).unwrap();
    stderr.flush().unwrap();
}
