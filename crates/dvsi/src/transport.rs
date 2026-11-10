use std::{
    io::{self, Read, Write},
    time::Duration,
};

/// A byte pipe to a chip.
pub trait Transport: Send {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// What arrived within `timeout`; empty when nothing did.
    fn receive(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize>;
    fn discard_input(&mut self) -> io::Result<()>;
}

/// Opens a [`Transport`] to the port at a baud rate.
pub trait Opener: Send + Sync {
    fn open(&self, port: &str, baud: u32) -> Result<Box<dyn Transport>, String>;
}

const OPEN_TIMEOUT: Duration = Duration::from_millis(50);

pub struct SerialOpener;

impl Opener for SerialOpener {
    fn open(&self, port: &str, baud: u32) -> Result<Box<dyn Transport>, String> {
        sdrmm_serial::new(port, baud)
            .timeout(OPEN_TIMEOUT)
            .flow_control(sdrmm_serial::FlowControl::None)
            .open()
            .map(|port| Box::new(SerialTransport { port }) as Box<dyn Transport>)
            .map_err(|error| error.to_string())
    }
}

struct SerialTransport {
    port: Box<dyn sdrmm_serial::SerialPort>,
}

impl Transport for SerialTransport {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.port.write_all(bytes)?;
        self.port.flush()
    }

    fn receive(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.port.set_timeout(timeout)?;
        match self.port.read(buffer) {
            Err(error) if error.kind() == io::ErrorKind::TimedOut => Ok(0),
            other => other,
        }
    }

    fn discard_input(&mut self) -> io::Result<()> {
        self.port
            .clear(sdrmm_serial::ClearBuffer::Input)
            .map_err(io::Error::other)
    }
}
