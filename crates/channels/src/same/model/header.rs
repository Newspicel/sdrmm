use super::{MAX_SAME_LOCATIONS, SameAlert, SameBody, SameLocation, codes};

pub const END_OF_MESSAGE: &str = "NNNN";
pub const START_OF_HEADER: &str = "ZCZC";
pub const MAX_HEADER_LEN: usize = 252;

const STATION_LEN: usize = 8;
const LOCATION_LEN: usize = 6;

struct Cursor<'a> {
    rest: &'a str,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Option<&'a str> {
        let taken = self.rest.get(..len)?;
        self.rest = &self.rest[len..];
        Some(taken)
    }

    fn digits(&mut self, len: usize) -> Option<u32> {
        let taken = self.take(len)?;
        taken
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then_some(())?;
        taken.parse().ok()
    }

    fn eat(&mut self, delimiter: char) -> Option<()> {
        self.rest = self.rest.strip_prefix(delimiter)?;
        Some(())
    }

    fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }

    fn uppercase(&mut self, len: usize) -> Option<&'a str> {
        let taken = self.take(len)?;
        taken
            .bytes()
            .all(|byte| byte.is_ascii_uppercase())
            .then_some(taken)
    }
}

#[must_use]
pub fn parse(text: &str) -> Option<SameBody> {
    if text == END_OF_MESSAGE {
        return Some(SameBody::EndOfMessage);
    }
    alert(text).map(SameBody::Alert)
}

fn alert(text: &str) -> Option<SameAlert> {
    if text.len() > MAX_HEADER_LEN {
        return None;
    }
    let mut cursor = Cursor {
        rest: text.strip_prefix(START_OF_HEADER)?,
    };
    cursor.eat('-')?;
    let originator = cursor.uppercase(3)?;
    cursor.eat('-')?;
    let event = cursor.uppercase(3)?;
    cursor.eat('-')?;
    let locations = locations(&mut cursor)?;
    let valid_minutes = valid_minutes(&mut cursor)?;
    cursor.eat('-')?;
    let (issued_day, issued_hour, issued_minute) = issued(&mut cursor)?;
    cursor.eat('-')?;
    let station = station(&mut cursor)?;
    cursor.rest.is_empty().then_some(SameAlert {
        originator: originator.to_owned(),
        originator_name: codes::originator(originator).map(str::to_owned),
        event: event.to_owned(),
        event_name: codes::event(event).map(str::to_owned),
        locations,
        valid_minutes,
        issued_day,
        issued_hour,
        issued_minute,
        station: station.to_owned(),
    })
}

fn locations(cursor: &mut Cursor<'_>) -> Option<Vec<SameLocation>> {
    let mut locations = Vec::new();
    loop {
        locations.push(location(cursor.digits(LOCATION_LEN)?));
        match cursor.peek()? {
            '-' if locations.len() < MAX_SAME_LOCATIONS => cursor.eat('-')?,
            '+' => return Some(locations),
            _ => return None,
        }
    }
}

fn location(code: u32) -> SameLocation {
    let part = (code / 100_000) as u8;
    let state = (code / 1_000 % 100) as u8;
    let county = (code % 1_000) as u16;
    SameLocation {
        part,
        state,
        county,
        name: codes::place(part, state, county),
    }
}

fn valid_minutes(cursor: &mut Cursor<'_>) -> Option<u32> {
    cursor.eat('+')?;
    let hours = cursor.digits(2)?;
    let minutes = cursor.digits(2)?;
    (minutes < 60).then_some(hours * 60 + minutes)
}

fn issued(cursor: &mut Cursor<'_>) -> Option<(u16, u8, u8)> {
    let day = cursor.digits(3)?;
    let hour = cursor.digits(2)?;
    let minute = cursor.digits(2)?;
    ((1..=366).contains(&day) && hour < 24 && minute < 60).then_some((
        day as u16,
        hour as u8,
        minute as u8,
    ))
}

fn station<'a>(cursor: &mut Cursor<'a>) -> Option<&'a str> {
    let end = cursor.rest.find('-')?;
    let station = cursor.take(end.min(STATION_LEN))?.trim_end();
    cursor.eat('-')?;
    (!station.is_empty() && station.bytes().all(|byte| byte.is_ascii_graphic())).then_some(station)
}
