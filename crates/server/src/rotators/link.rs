use std::{
    io::BufReader,
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use sdrmm_wire::{RotatorAxes, RotatorLink, SkyPoint};

use super::{gs232::Gs232, rotctld::Rotctld};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) trait Link: Send {
    fn position(&mut self) -> Result<SkyPoint, LinkError>;
    fn point(&mut self, aim: SkyPoint) -> Result<(), LinkError>;
    fn stop(&mut self) -> Result<(), LinkError>;
}

#[derive(Debug, thiserror::Error)]
pub(super) enum LinkError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Serial(#[from] serialport::Error),
    #[error("rotator refused, code {0}")]
    Refused(i32),
    #[error("unreadable reply \"{0}\"")]
    Garbled(String),
    #[error("{0} resolves to nothing")]
    Unresolved(String),
}

pub(super) fn open(link: &RotatorLink, axes: RotatorAxes) -> Result<Box<dyn Link>, LinkError> {
    match link {
        RotatorLink::Gs232 { device, baud } => {
            let port = serialport::new(device, *baud).timeout(IO_TIMEOUT).open()?;
            Ok(Box::new(Gs232::new(port, axes)))
        }
        RotatorLink::Rotctld { address } => {
            let target = address
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| LinkError::Unresolved(address.clone()))?;
            let stream = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)?;
            stream.set_read_timeout(Some(IO_TIMEOUT))?;
            stream.set_write_timeout(Some(IO_TIMEOUT))?;
            stream.set_nodelay(true)?;
            let reader = BufReader::new(stream.try_clone()?);
            Ok(Box::new(Rotctld::new(reader, stream)))
        }
    }
}
