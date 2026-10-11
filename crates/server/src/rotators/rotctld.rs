use std::io::{BufRead, Read, Write};

use sdrmm_wire::{FLIP_ELEVATION_DEG, MAX_ROTATOR_AZIMUTH_DEG, SkyPoint};

use super::link::{Link, LinkError};

const MAX_REPLY: u64 = 256;

pub(super) struct Rotctld<R, W> {
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> Rotctld<R, W> {
    pub(super) const fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    fn send(&mut self, command: &str) -> Result<(), LinkError> {
        self.writer.write_all(command.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    fn reply(&mut self) -> Result<String, LinkError> {
        let mut line = String::new();
        if (&mut self.reader).take(MAX_REPLY).read_line(&mut line)? == 0 {
            return Err(LinkError::Io(std::io::ErrorKind::UnexpectedEof.into()));
        }
        Ok(line.trim().to_owned())
    }

    fn angle(&mut self) -> Result<f64, LinkError> {
        let line = self.reply()?;
        if let Some(code) = report(&line) {
            return Err(LinkError::Refused(code));
        }
        line.parse().map_err(|_| LinkError::Garbled(line))
    }

    fn accepted(&mut self) -> Result<(), LinkError> {
        let line = self.reply()?;
        match report(&line) {
            Some(0) => Ok(()),
            Some(code) => Err(LinkError::Refused(code)),
            None => Err(LinkError::Garbled(line)),
        }
    }
}

impl<R: BufRead + Send, W: Write + Send> Link for Rotctld<R, W> {
    fn position(&mut self) -> Result<SkyPoint, LinkError> {
        self.send("p\n")?;
        Ok(SkyPoint {
            azimuth_deg: self.angle()?,
            elevation_deg: self.angle()?,
        })
    }

    fn point(&mut self, aim: SkyPoint) -> Result<(), LinkError> {
        self.send(&format!(
            "P {:.1} {:.1}\n",
            aim.azimuth_deg.clamp(0.0, MAX_ROTATOR_AZIMUTH_DEG),
            aim.elevation_deg.clamp(0.0, FLIP_ELEVATION_DEG)
        ))?;
        self.accepted()
    }

    fn stop(&mut self) -> Result<(), LinkError> {
        self.send("S\n")?;
        self.accepted()
    }
}

fn report(line: &str) -> Option<i32> {
    line.strip_prefix("RPRT")?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn rotctld(replies: &str) -> Rotctld<Cursor<Vec<u8>>, Vec<u8>> {
        Rotctld::new(Cursor::new(replies.as_bytes().to_vec()), Vec::new())
    }

    fn sky(azimuth_deg: f64, elevation_deg: f64) -> SkyPoint {
        SkyPoint {
            azimuth_deg,
            elevation_deg,
        }
    }

    #[test]
    fn the_position_is_two_lines() {
        let mut rotator = rotctld("182.500000\n12.000000\n");
        assert_eq!(rotator.position().expect("reads"), sky(182.5, 12.0));
        assert_eq!(rotator.writer, b"p\n");
    }

    #[test]
    fn pointing_and_stopping_wait_for_the_report() {
        let mut rotator = rotctld("RPRT 0\nRPRT 0\n");
        rotator.point(sky(420.0, 150.0)).expect("accepted");
        rotator.stop().expect("accepted");
        assert_eq!(rotator.writer, b"P 420.0 150.0\nS\n");
    }

    #[test]
    fn a_refusal_carries_the_hamlib_code() {
        assert!(matches!(
            rotctld("RPRT -1\n").point(sky(10.0, 10.0)),
            Err(LinkError::Refused(-1))
        ));
        assert!(matches!(
            rotctld("RPRT -8\n").position(),
            Err(LinkError::Refused(-8))
        ));
        assert!(matches!(rotctld("").position(), Err(LinkError::Io(_))));
    }
}
